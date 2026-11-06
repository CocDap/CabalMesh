//! First-run named testnet wallets. The helper receives public keys only.
//! A name is activated only after RPC key ownership and Intents registration succeed.

use crate::{
    identity::IdentityStore,
    network_config::{Network, NetworkConfig},
    state::AppState,
};
use cabal_near::{AccountId, NearKeypair};
use serde::Serialize;
use serde_json::{json, Value};
use std::time::Duration;
use tauri::State;
use tokio::sync::Mutex;

const TESTNET_HELPER: &str = "https://helper.nearprotocol.com/account";
const TIMEOUT: Duration = Duration::from_secs(20);

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, thiserror::Error)]
#[serde(rename_all = "snake_case")]
pub enum SetupError {
    #[error("services not ready")]
    NotReady,
    #[error("vault locked")]
    Locked,
    #[error("invalid name")]
    InvalidName,
    #[error("name already taken")]
    NameTaken,
    #[error("network unavailable")]
    Offline,
    #[error("account creation service unavailable")]
    HelperUnavailable,
    #[error("helper rate limited")]
    RateLimited,
    #[error("account creation not confirmed")]
    CreationFailed,
    #[error("intents key registration not confirmed")]
    IntentsUnavailable,
    #[error("setup storage failed")]
    Storage,
    #[error("wallet changed during setup")]
    WalletChanged,
    #[error("finish pending registration first")]
    PendingAccount,
    #[error("existing on-chain account must be preserved")]
    ExistingAccount,
    #[error("named account creation is testnet only")]
    TestnetOnly,
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct SetupStatus {
    pub required: bool,
    pub testnet: bool,
    pub account_id: String,
    pub pending_account_id: Option<String>,
}

#[derive(Debug, Clone, Copy, Serialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum NameStatus {
    Available,
    Owned,
    Taken,
}

/// Accept a single testnet name, never a nested subaccount or another network.
pub fn testnet_account(name: &str) -> Result<AccountId, SetupError> {
    let short = name.strip_suffix(".testnet").unwrap_or(name);
    if short.is_empty() || short.len() > 56 || short.contains('.') || short.trim() != short {
        return Err(SetupError::InvalidName);
    }
    AccountId::try_from(format!("{short}.testnet")).map_err(|_| SetupError::InvalidName)
}

fn status(identity: &IdentityStore, network: Network) -> Result<SetupStatus, SetupError> {
    let key = identity.primary_keypair().ok_or(SetupError::Locked)?;
    let record = identity.identities.first().ok_or(SetupError::Locked)?;
    let testnet = network == Network::Testnet;
    Ok(SetupStatus {
        required: testnet
            && (record.wallet_setup_required || record.pending_testnet_account.is_some()),
        testnet,
        account_id: key.account_id().to_string(),
        pending_account_id: record
            .pending_testnet_account
            .as_ref()
            .map(ToString::to_string),
    })
}

struct Registrar {
    http: reqwest::Client,
    rpc: String,
    helper: String,
}

impl Registrar {
    async fn query(&self, params: Value) -> Result<Option<Value>, SetupError> {
        let response = self.http.post(&self.rpc).timeout(TIMEOUT)
            .json(&json!({ "jsonrpc": "2.0", "id": "wallet-setup", "method": "query", "params": params }))
            .send().await.map_err(|_| SetupError::Offline)?;
        if !response.status().is_success() {
            return Err(SetupError::Offline);
        }
        let body: Value = response.json().await.map_err(|_| SetupError::Offline)?;
        if let Some(error) = body.get("error") {
            return match error.pointer("/cause/name").and_then(Value::as_str) {
                Some("UNKNOWN_ACCOUNT" | "UNKNOWN_ACCESS_KEY") => Ok(None),
                _ => Err(SetupError::Offline),
            };
        }
        body.get("result")
            .cloned()
            .map(Some)
            .ok_or(SetupError::Offline)
    }

    async fn exists(&self, account: &AccountId) -> Result<bool, SetupError> {
        Ok(self.query(json!({ "request_type": "view_account", "finality": "final", "account_id": account })).await?.is_some())
    }

    async fn check(
        &self,
        account: &AccountId,
        key: &NearKeypair,
    ) -> Result<NameStatus, SetupError> {
        if !self.exists(account).await? {
            return Ok(NameStatus::Available);
        }
        let access = self
            .query(json!({
                "request_type": "view_access_key", "finality": "final",
                "account_id": account, "public_key": key.public_key_string(),
            }))
            .await?;
        Ok(
            if access
                .as_ref()
                .and_then(|value| value.get("permission"))
                .and_then(Value::as_str)
                == Some("FullAccess")
            {
                NameStatus::Owned
            } else {
                NameStatus::Taken
            },
        )
    }

    async fn create(&self, account: &AccountId, key: &NearKeypair) -> Result<(), SetupError> {
        match self.check(account, key).await? {
            NameStatus::Owned => return Ok(()),
            NameStatus::Taken => return Err(SetupError::NameTaken),
            NameStatus::Available => {}
        }
        let response = self
            .http
            .post(&self.helper)
            .timeout(TIMEOUT)
            .json(&creation_request(account, key))
            .send()
            .await;
        let helper_error = match response {
            Ok(response) if response.status().as_u16() == 429 => Some(SetupError::RateLimited),
            Ok(response) if response.status().is_success() => None,
            Ok(response) => {
                tracing::warn!(status = %response.status(), "testnet account helper rejected request");
                Some(if response.status().is_server_error() {
                    SetupError::HelperUnavailable
                } else {
                    SetupError::CreationFailed
                })
            }
            Err(error) => {
                tracing::warn!(error = ?error, "testnet account helper connection failed");
                Some(SetupError::HelperUnavailable)
            }
        };
        // A helper timeout may happen after it created the account. Verify, don't re-key.
        for attempt in 0..4 {
            match self.check(account, key).await? {
                NameStatus::Owned => return Ok(()),
                NameStatus::Taken => return Err(SetupError::NameTaken),
                NameStatus::Available => {}
            }
            if attempt < 3 {
                tokio::time::sleep(Duration::from_millis(400)).await;
            }
        }
        Err(helper_error.unwrap_or(SetupError::CreationFailed))
    }
}

fn creation_request(account: &AccountId, key: &NearKeypair) -> Value {
    json!({ "newAccountId": account, "newAccountPublicKey": key.public_key_string() })
}

fn registrar(http: reqwest::Client, config: &NetworkConfig) -> Result<Registrar, SetupError> {
    if config.network != Network::Testnet {
        return Err(SetupError::TestnetOnly);
    }
    Ok(Registrar {
        http,
        rpc: config.rpc_url(),
        helper: TESTNET_HELPER.into(),
    })
}

/// Restoring a named wallet must prove the supplied key, never trust a typed name.
pub(crate) async fn verify_imported_account(
    http: reqwest::Client,
    config: &NetworkConfig,
    name: &str,
    key: &NearKeypair,
) -> Result<AccountId, SetupError> {
    let account = testnet_account(name)?;
    if registrar(http, config)?.check(&account, key).await? != NameStatus::Owned {
        return Err(SetupError::NameTaken);
    }
    Ok(account)
}

#[tauri::command]
pub async fn wallet_setup_status(state: State<'_, AppState>) -> Result<SetupStatus, SetupError> {
    let services = state.services().map_err(|_| SetupError::NotReady)?;
    let identity = services.identity.lock().await;
    status(&identity, NetworkConfig::current().network)
}

#[tauri::command]
pub async fn wallet_check_name(
    name: String,
    state: State<'_, AppState>,
) -> Result<NameStatus, SetupError> {
    let services = state.services().map_err(|_| SetupError::NotReady)?;
    let account = testnet_account(&name)?;
    let key = services
        .identity
        .lock()
        .await
        .primary_keypair()
        .ok_or(SetupError::Locked)?;
    registrar(services.http.clone(), &NetworkConfig::current())?
        .check(&account, &key)
        .await
}

#[tauri::command]
pub async fn wallet_create_testnet_account(
    name: String,
    state: State<'_, AppState>,
) -> Result<String, SetupError> {
    let services = state.services().map_err(|_| SetupError::NotReady)?;
    let config = NetworkConfig::current();
    let registrar = registrar(services.http.clone(), &config)?;
    create_wallet(&services.identity, &config, registrar, &name).await
}

async fn create_wallet(
    identity_store: &Mutex<IdentityStore>,
    config: &NetworkConfig,
    registrar: Registrar,
    name: &str,
) -> Result<String, SetupError> {
    let account = testnet_account(&name)?;
    let key = {
        let identity = identity_store.lock().await;
        let record = identity.identities.first().ok_or(SetupError::Locked)?;
        if record
            .pending_testnet_account
            .as_ref()
            .is_some_and(|pending| pending != &account)
        {
            return Err(SetupError::PendingAccount);
        }
        if record
            .testnet_account
            .as_ref()
            .is_some_and(|current| current != &account)
        {
            return Err(SetupError::ExistingAccount);
        }
        identity.primary_keypair().ok_or(SetupError::Locked)?
    };
    // Existing funded/activated implicit wallets are not silently switched to a new account.
    if key.account_id() != account && registrar.exists(&key.account_id()).await? {
        return Err(SetupError::ExistingAccount);
    }
    if registrar.check(&account, &key).await? == NameStatus::Taken {
        return Err(SetupError::NameTaken);
    }
    {
        let mut identity = identity_store.lock().await;
        ensure_same_key(&identity, &key)?;
        if identity
            .identities
            .first()
            .and_then(|record| record.pending_testnet_account.as_ref())
            .is_some_and(|pending| pending != &account)
        {
            return Err(SetupError::PendingAccount);
        }
        identity
            .set_testnet_registration(account.clone(), false)
            .map_err(|_| SetupError::Storage)?;
    }
    if let Err(error) = registrar.create(&account, &key).await {
        if error == SetupError::NameTaken {
            let mut identity = identity_store.lock().await;
            ensure_same_key(&identity, &key)?;
            identity
                .clear_pending_testnet_registration()
                .map_err(|_| SetupError::Storage)?;
        }
        return Err(error);
    }
    let named = key.clone().with_account_id(account.clone());
    let bridge = crate::near_bridge::NearBridge::new(
        config.rpc_url(),
        &config.verifier(),
        named,
        registrar.http.clone(),
    )
    .map_err(|_| SetupError::IntentsUnavailable)?;
    bridge
        .register_signing_key()
        .await
        .map_err(|_| SetupError::IntentsUnavailable)?;
    {
        let mut identity = identity_store.lock().await;
        ensure_same_key(&identity, &key)?;
        identity
            .set_testnet_registration(account.clone(), true)
            .map_err(|_| SetupError::Storage)?;
    }
    Ok(account.to_string())
}

fn ensure_same_key(identity: &IdentityStore, expected: &NearKeypair) -> Result<(), SetupError> {
    if identity
        .primary_keypair()
        .is_some_and(|key| key.public_key_bytes() == expected.public_key_bytes())
    {
        Ok(())
    } else {
        Err(SetupError::WalletChanged)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::near_bridge::testing::{result, MockRpc};
    use std::sync::{
        atomic::{AtomicBool, Ordering},
        Arc,
    };

    #[test]
    fn helper_request_contains_only_the_name_and_public_key() {
        let key = NearKeypair::from_secret_bytes(&[7; 32]);
        let body = creation_request(&testnet_account("alice").unwrap(), &key);
        assert_eq!(body.as_object().unwrap().len(), 2);
        assert_eq!(body["newAccountId"], "alice.testnet");
        assert_eq!(body["newAccountPublicKey"], key.public_key_string());
        assert!(!body.to_string().contains(&hex::encode(key.secret_bytes())));
    }

    #[tokio::test]
    async fn first_setup_creates_verifies_and_persists_a_named_wallet() {
        let created = Arc::new(AtomicBool::new(false));
        let state = created.clone();
        let rpc = MockRpc::serve(move |method, params| {
            if method.is_empty() {
                state.store(true, Ordering::SeqCst);
                return json!({ "status": "helper response is not trusted by itself" });
            }
            if params["request_type"] == "view_account" {
                if params["account_id"] == "alice.testnet" && state.load(Ordering::SeqCst) {
                    return result(json!({ "amount": "1000000000000000000000000" }));
                }
                return json!({ "error": { "cause": { "name": "UNKNOWN_ACCOUNT" } } });
            }
            if params["request_type"] == "view_access_key" {
                return result(json!({ "permission": "FullAccess" }));
            }
            if params["method_name"] == "has_public_key" {
                return crate::near_bridge::testing::view_result(&json!(true));
            }
            panic!("unexpected RPC request: {method} {params}");
        })
        .await;
        let dir = tempfile::tempdir().unwrap();
        let identity = Mutex::new(IdentityStore::open(dir.path()));
        let public_key = identity
            .lock()
            .await
            .primary_keypair()
            .unwrap()
            .public_key_bytes();
        let config = NetworkConfig {
            near_rpc_url: Some(rpc.url.clone()),
            ..Default::default()
        };
        let registrar = Registrar {
            http: reqwest::Client::new(),
            rpc: rpc.url.clone(),
            helper: rpc.url.clone(),
        };
        assert_eq!(
            create_wallet(&identity, &config, registrar, "alice")
                .await
                .unwrap(),
            "alice.testnet"
        );
        let reopened = IdentityStore::open(dir.path());
        assert_eq!(
            reopened.primary_account().unwrap().as_str(),
            "alice.testnet"
        );
        assert_eq!(
            reopened.primary_keypair().unwrap().public_key_bytes(),
            public_key
        );
        assert!(!status(&reopened, Network::Testnet).unwrap().required);
        assert_eq!(
            rpc.calls
                .lock()
                .unwrap()
                .iter()
                .filter(|(method, _)| method.is_empty())
                .count(),
            1
        );
    }

    #[tokio::test]
    async fn unconfirmed_creation_keeps_the_original_key_and_pending_name() {
        let rpc = MockRpc::serve(|method, _| {
            if method.is_empty() {
                return json!({ "error": "helper failed" });
            }
            json!({ "error": { "cause": { "name": "UNKNOWN_ACCOUNT" } } })
        })
        .await;
        let dir = tempfile::tempdir().unwrap();
        let identity = Mutex::new(IdentityStore::open(dir.path()));
        let original = identity.lock().await.primary_account();
        let config = NetworkConfig {
            near_rpc_url: Some(rpc.url.clone()),
            ..Default::default()
        };
        let registrar = Registrar {
            http: reqwest::Client::new(),
            rpc: rpc.url.clone(),
            helper: rpc.url,
        };
        assert_eq!(
            create_wallet(&identity, &config, registrar, "alice").await,
            Err(SetupError::CreationFailed)
        );
        let reopened = IdentityStore::open(dir.path());
        assert_eq!(reopened.primary_account(), original);
        assert_eq!(
            reopened.identities[0]
                .pending_testnet_account
                .as_ref()
                .unwrap()
                .as_str(),
            "alice.testnet"
        );
        assert!(status(&reopened, Network::Testnet).unwrap().required);
    }

    #[tokio::test]
    #[ignore = "creates a disposable funded testnet account and registers its Intents key"]
    async fn live_named_testnet_onboarding() {
        let dir = tempfile::tempdir().unwrap();
        let identity = Mutex::new(IdentityStore::open(dir.path()));
        let key = identity.lock().await.primary_keypair().unwrap();
        let name = format!("cabal-check-{}", &hex::encode(key.public_key_bytes())[..16]);
        let config = NetworkConfig::default();
        let registrar = registrar(reqwest::Client::new(), &config).unwrap();
        let account = create_wallet(&identity, &config, registrar, &name)
            .await
            .unwrap();
        println!("Verified disposable testnet account: {account}");
        assert_eq!(
            IdentityStore::open(dir.path())
                .primary_account()
                .unwrap()
                .as_str(),
            account
        );
    }

    #[test]
    fn names_are_direct_testnet_subaccounts() {
        for name in ["alice", "alice.testnet", "a-b_2"] {
            assert!(testnet_account(name).is_ok());
        }
        for name in [
            "",
            "ALICE",
            " alice",
            "alice.near",
            "a.b",
            "a__b",
            "-alice",
            "alice-",
        ] {
            assert_eq!(
                testnet_account(name),
                Err(SetupError::InvalidName),
                "{name}"
            );
        }
        assert!(testnet_account(&"a".repeat(56)).is_ok());
        assert!(testnet_account(&"a".repeat(57)).is_err());
    }

    #[tokio::test]
    async fn only_a_full_access_key_proves_ownership() {
        let rpc = MockRpc::serve(|_, params| match params["request_type"].as_str() {
            Some("view_account") => result(json!({ "amount": "1" })),
            _ => result(json!({ "permission": { "FunctionCall": {} } })),
        })
        .await;
        let registrar = Registrar {
            http: reqwest::Client::new(),
            rpc: rpc.url,
            helper: "http://unused".into(),
        };
        let account = testnet_account("alice").unwrap();
        assert_eq!(
            registrar
                .check(&account, &NearKeypair::generate())
                .await
                .unwrap(),
            NameStatus::Taken
        );
        assert_eq!(
            registrar.create(&account, &NearKeypair::generate()).await,
            Err(SetupError::NameTaken)
        );
    }

    #[tokio::test]
    async fn network_error_is_never_available() {
        let rpc =
            MockRpc::serve(|_, _| json!({ "error": { "cause": { "name": "INTERNAL_ERROR" } } }))
                .await;
        let registrar = Registrar {
            http: reqwest::Client::new(),
            rpc: rpc.url,
            helper: "http://unused".into(),
        };
        assert_eq!(
            registrar
                .check(&testnet_account("alice").unwrap(), &NearKeypair::generate())
                .await,
            Err(SetupError::Offline)
        );
    }

    #[tokio::test]
    async fn retry_owned_account_never_calls_helper() {
        let rpc = MockRpc::serve(|_, params| match params["request_type"].as_str() {
            Some("view_account") => result(json!({ "amount": "1" })),
            _ => result(json!({ "permission": "FullAccess" })),
        })
        .await;
        let registrar = Registrar {
            http: reqwest::Client::new(),
            rpc: rpc.url,
            helper: "http://127.0.0.1:1".into(),
        };
        assert!(registrar
            .create(&testnet_account("alice").unwrap(), &NearKeypair::generate())
            .await
            .is_ok());
    }

    #[tokio::test]
    async fn helper_connection_failure_is_distinct_from_rpc_failure() {
        let rpc = MockRpc::serve(|_, _| {
            json!({ "error": { "cause": { "name": "UNKNOWN_ACCOUNT" } } })
        }).await;
        let unavailable = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let helper = format!("http://{}", unavailable.local_addr().unwrap());
        drop(unavailable);
        let registrar = Registrar {
            http: reqwest::Client::new(),
            rpc: rpc.url,
            helper,
        };
        let error = registrar.create(&testnet_account("alice").unwrap(), &NearKeypair::generate())
            .await.unwrap_err();
        assert_eq!(serde_json::to_value(error).unwrap(), "helper_unavailable");
    }
}
