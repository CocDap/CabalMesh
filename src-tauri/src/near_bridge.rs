//! JSON-RPC I/O for NEAR: balances, deposit, withdraw and `execute_intents`.
//!
//! Everything chain-shaped that can be computed without a network lives in
//! `cabal-near`; this file is the part that talks.

use cabal_near::{
    ft_withdraw_payload, sign_function_calls, versioned_nonce, AccountId, FunctionCall, MultiPayload, NearKeypair,
    GAS_EXECUTE_INTENTS, GAS_FT_TRANSFER_CALL,
};
use base64::{engine::general_purpose::STANDARD, Engine};
use serde_json::{json, Value};
use std::time::Duration;

/// Signed intents expire this long after signing.
pub const INTENT_LIFETIME_MS: u64 = 3_600_000;

/// `wait_until: FINAL` takes a few blocks, and a busy node takes longer.
const REQUEST_TIMEOUT: Duration = Duration::from_secs(60);

/// Attempts for a request a public node answered with 429 or 5xx.
const RPC_ATTEMPTS: u32 = 3;

const RPC_RETRY_DELAY: Duration = Duration::from_millis(500);

/// A transaction hash, as returned by the RPC.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TxHash(pub String);

#[derive(Debug, thiserror::Error)]
pub enum NearError {
    /// The account has no native NEAR on-chain, so it cannot pay for gas.
    #[error("account {account} has no NEAR for gas")]
    AccountNotFunded { account: String },
    #[error("RPC error: {0}")]
    Rpc(String),
    /// The verifier or the chain refused. The reason comes from the contract.
    #[error("rejected: {0}")]
    Rejected(String),
    #[error("network error: {0}")]
    Network(#[from] reqwest::Error),
}

impl NearError {
    /// Whether the verifier refused because this exact pair was already
    /// applied, which means another gateway got there first.
    #[must_use]
    pub fn is_already_settled(&self) -> bool {
        matches!(self, Self::Rejected(reason) if reason.contains("nonce was already used"))
    }

    /// Whether the signed nonce names a salt the verifier no longer accepts.
    #[must_use]
    pub fn is_invalid_salt(&self) -> bool {
        matches!(self, Self::Rejected(reason) if reason.contains("invalid salt"))
    }
}

/// `fetch`-shaped failure markers an RPC node uses for a key or account that
/// does not exist. The name differs by node version and by request type.
fn names_missing_account(text: &str) -> bool {
    text.contains("UNKNOWN_ACCOUNT")
        || text.contains("UNKNOWN_ACCESS_KEY")
        || text.contains("does not exist")
}

#[derive(Clone)]
pub struct NearBridge {
    rpc_url: String,
    verifier: AccountId,
    keypair: NearKeypair,
    http: reqwest::Client,
}

impl NearBridge {
    /// # Errors
    ///
    /// [`NearError::Rpc`] if the verifier id is not a valid account id.
    pub fn new(rpc_url: String, verifier: &str, keypair: NearKeypair, http: reqwest::Client) -> Result<Self, NearError> {
        let verifier = AccountId::try_from(verifier).map_err(|_| NearError::Rpc("invalid verifier id".into()))?;
        Ok(Self { rpc_url, verifier, keypair, http })
    }

    #[must_use]
    pub fn account_id(&self) -> AccountId {
        self.keypair.account_id()
    }

    #[must_use]
    pub const fn verifier(&self) -> &AccountId {
        &self.verifier
    }

    async fn rpc(&self, method: &str, params: Value) -> Result<Value, NearError> {
        let body = json!({ "jsonrpc": "2.0", "id": "cabalmesh", "method": method, "params": params });
        let mut attempt = 0;
        let response = loop {
            attempt += 1;
            let response = self.http.post(&self.rpc_url).timeout(REQUEST_TIMEOUT).json(&body).send().await?;
            let status = response.status();
            if status.as_u16() != 429 && !status.is_server_error() || attempt >= RPC_ATTEMPTS {
                break response;
            }
            tokio::time::sleep(RPC_RETRY_DELAY * attempt).await;
        };
        let status = response.status();
        let value: Value = response.json().await?;
        if let Some(error) = value.get("error") {
            return Err(NearError::Rpc(error.to_string()));
        }
        if !status.is_success() {
            return Err(NearError::Rpc(format!("HTTP {status}")));
        }
        value.get("result").cloned().ok_or_else(|| NearError::Rpc("response had no result".into()))
    }

    /// A contract view. A contract panic arrives as [`NearError::Rejected`].
    async fn view(&self, contract: &str, method: &str, args: &Value) -> Result<Value, NearError> {
        let params = json!({
            "request_type": "call_function",
            "finality": "final",
            "account_id": contract,
            "method_name": method,
            "args_base64": STANDARD.encode(args.to_string()),
        });
        let result = self.rpc("query", params).await.map_err(|error| match error {
            // Newer nodes report a panic as a top-level error.
            NearError::Rpc(text) if text.contains("wasm execution failed") || text.contains("CONTRACT_EXECUTION_ERROR") => {
                NearError::Rejected(text)
            }
            other => other,
        })?;
        if let Some(error) = result.get("error").and_then(Value::as_str) {
            return Err(NearError::Rejected(error.to_owned()));
        }
        let bytes: Vec<u8> = result
            .get("result")
            .and_then(Value::as_array)
            .map(|array| array.iter().filter_map(|byte| byte.as_u64().and_then(|b| u8::try_from(b).ok())).collect())
            .ok_or_else(|| NearError::Rpc("view had no result".into()))?;
        serde_json::from_slice(&bytes).map_err(|_| NearError::Rpc("view result was not JSON".into()))
    }

    /// This account's balance inside the verifier, in the token's smallest unit.
    ///
    /// # Errors
    ///
    /// Any RPC failure.
    pub async fn verifier_balance(&self, token: &str) -> Result<u128, NearError> {
        let args = json!({ "account_id": self.account_id().as_str(), "token_id": format!("nep141:{token}") });
        parse_amount(&self.view(self.verifier.as_str(), "mt_balance_of", &args).await?)
    }

    /// This account's balance in its own wallet, outside the verifier.
    ///
    /// # Errors
    ///
    /// Any RPC failure.
    pub async fn wallet_balance(&self, token: &str) -> Result<u128, NearError> {
        let args = json!({ "account_id": self.account_id().as_str() });
        parse_amount(&self.view(token, "ft_balance_of", &args).await?)
    }

    /// Native NEAR, in yocto. A never-funded implicit account reads as zero
    /// rather than as an error: having none is a fact, not a failure.
    ///
    /// # Errors
    ///
    /// Any RPC failure other than the account not existing.
    pub async fn native_balance(&self) -> Result<u128, NearError> {
        let params = json!({
            "request_type": "view_account",
            "finality": "final",
            "account_id": self.account_id().as_str(),
        });
        let result = match self.rpc("query", params).await {
            Ok(result) => result,
            Err(NearError::Rpc(text)) if names_missing_account(&text) => return Ok(0),
            Err(error) => return Err(error),
        };
        if let Some(error) = result.get("error").and_then(Value::as_str) {
            return if names_missing_account(error) { Ok(0) } else { Err(NearError::Rpc(error.to_owned())) };
        }
        parse_amount(result.get("amount").unwrap_or(&Value::Null))
    }

    /// The salt a new nonce must carry.
    ///
    /// # Errors
    ///
    /// Any RPC failure, or a salt that is not 4 hex bytes.
    pub async fn current_salt(&self) -> Result<[u8; 4], NearError> {
        let salt = self.view(self.verifier.as_str(), "current_salt", &json!({})).await?;
        salt.as_str()
            .and_then(|hex_salt| hex::decode(hex_salt).ok())
            .and_then(|bytes| <[u8; 4]>::try_from(bytes).ok())
            .ok_or_else(|| NearError::Rpc("current_salt was not 4 hex bytes".into()))
    }

    /// Dry-runs intents. A panic and an unbalanced set both become
    /// [`NearError::Rejected`]: an unbalanced set does not panic, it returns
    /// `invariant_violated`.
    ///
    /// # Errors
    ///
    /// [`NearError::Rejected`] with the verifier's reason, or any RPC failure.
    pub async fn simulate(&self, payloads: &[MultiPayload]) -> Result<(), NearError> {
        let result = self
            .view(self.verifier.as_str(), "simulate_intents", &json!({ "signed": payloads }))
            .await?;
        match result.get("invariant_violated") {
            None | Some(Value::Null) => Ok(()),
            Some(violation) => Err(NearError::Rejected(violation.to_string())),
        }
    }

    /// Submits intents, always dry-running them first so a doomed pair costs
    /// no gas.
    ///
    /// # Errors
    ///
    /// [`NearError::Rejected`] from the dry run or the chain,
    /// [`NearError::AccountNotFunded`] if this account cannot pay for gas.
    pub async fn execute_intents(&self, payloads: &[MultiPayload]) -> Result<TxHash, NearError> {
        self.simulate(payloads).await?;
        let call = FunctionCall {
            method_name: "execute_intents".into(),
            args: json!({ "signed": payloads }).to_string().into_bytes(),
            gas: GAS_EXECUTE_INTENTS,
            deposit: 0,
        };
        self.send(self.verifier.clone(), call).await
    }

    /// Moves `amount` of `token` from the wallet into the verifier.
    ///
    /// # Errors
    ///
    /// [`NearError::AccountNotFunded`] if this account cannot pay for gas,
    /// [`NearError::Rejected`] if the token refuses.
    pub async fn deposit(&self, token: &str, amount: u128) -> Result<TxHash, NearError> {
        let receiver = AccountId::try_from(token).map_err(|_| NearError::Rpc("invalid token id".into()))?;
        let call = FunctionCall {
            method_name: "ft_transfer_call".into(),
            args: json!({ "receiver_id": self.verifier.as_str(), "amount": amount.to_string(), "msg": "" })
                .to_string()
                .into_bytes(),
            gas: GAS_FT_TRANSFER_CALL,
            deposit: 1,
        };
        self.send(receiver, call).await
    }

    /// Moves `amount` of `token` from the verifier back to the wallet.
    ///
    /// # Errors
    ///
    /// As [`NearBridge::execute_intents`].
    pub async fn withdraw(&self, token: &str, amount: u128, now_ms: u64) -> Result<TxHash, NearError> {
        let deadline = now_ms + INTENT_LIFETIME_MS;
        let nonce = versioned_nonce(self.current_salt().await?, deadline, rand::random());
        let payload = ft_withdraw_payload(&self.keypair, &self.verifier, token, amount, deadline, nonce);
        self.execute_intents(&[payload]).await
    }

    /// Whether `account` has spent `nonce` (base64) on this verifier. A nonce is
    /// only spent by a transaction that succeeded.
    ///
    /// # Errors
    ///
    /// Any RPC failure.
    pub async fn nonce_used(&self, account: &str, nonce: &str) -> Result<bool, NearError> {
        let args = json!({ "account_id": account, "nonce": nonce });
        self.view(self.verifier.as_str(), "is_nonce_used", &args)
            .await?
            .as_bool()
            .ok_or_else(|| NearError::Rpc("is_nonce_used did not return a bool".into()))
    }

    /// Whether a transaction finished and logged `needle`.
    ///
    /// How a report from the mesh is checked before anything is believed: a
    /// peer can claim any hash, so the claim has to name a real, successful
    /// verifier call that logged this device's own intent hash.
    ///
    /// # Errors
    ///
    /// Any RPC failure, including a transaction the node does not know yet.
    pub async fn tx_logged(&self, hash: &str, sender: &str, needle: &str) -> Result<bool, NearError> {
        let params = json!({ "tx_hash": hash, "sender_account_id": sender, "wait_until": "FINAL" });
        let outcome = self.rpc("tx", params).await?;
        if outcome.pointer("/status/SuccessValue").is_none()
            || outcome.pointer("/transaction/receiver_id").and_then(Value::as_str) != Some(self.verifier.as_str())
        {
            return Ok(false);
        }
        let logged = outcome
            .get("receipts_outcome")
            .and_then(Value::as_array)
            .is_some_and(|receipts| {
                receipts.iter().any(|receipt| {
                    receipt
                        .pointer("/outcome/logs")
                        .and_then(Value::as_array)
                        .is_some_and(|logs| logs.iter().any(|log| log.as_str().is_some_and(|l| l.contains(needle))))
                })
            });
        Ok(logged)
    }

    /// Reads the access key nonce and a recent block, signs, sends, and waits
    /// for finality.
    async fn send(&self, receiver: AccountId, call: FunctionCall) -> Result<TxHash, NearError> {
        let account = self.account_id();
        let not_funded = || NearError::AccountNotFunded { account: account.to_string() };

        let params = json!({
            "request_type": "view_access_key",
            "finality": "final",
            "account_id": account.as_str(),
            "public_key": self.keypair.public_key_string(),
        });
        let key = match self.rpc("query", params).await {
            Ok(key) => key,
            Err(NearError::Rpc(text)) if names_missing_account(&text) => return Err(not_funded()),
            Err(error) => return Err(error),
        };
        if let Some(error) = key.get("error").and_then(Value::as_str) {
            return Err(if names_missing_account(error) { not_funded() } else { NearError::Rpc(error.to_owned()) });
        }
        let nonce = key
            .get("nonce")
            .and_then(Value::as_u64)
            .ok_or_else(|| NearError::Rpc("access key had no nonce".into()))?
            + 1;

        let block = self.rpc("block", json!({ "finality": "final" })).await?;
        let block_hash: [u8; 32] = block
            .pointer("/header/hash")
            .and_then(Value::as_str)
            .and_then(|hash| bs58::decode(hash).into_vec().ok())
            .and_then(|bytes| bytes.try_into().ok())
            .ok_or_else(|| NearError::Rpc("block had no hash".into()))?;

        let signed = sign_function_calls(&self.keypair, &receiver, nonce, block_hash, &[call]);
        let params = json!({ "signed_tx_base64": signed, "wait_until": "FINAL" });
        let outcome = self.rpc("send_tx", params).await.map_err(|error| match error {
            // Zero balance reaches the node as a transaction-level refusal.
            NearError::Rpc(text) if text.contains("NotEnoughBalance") || text.contains("LackBalanceForState") => not_funded(),
            other => other,
        })?;

        if let Some(failure) = outcome.pointer("/status/Failure") {
            return Err(NearError::Rejected(failure.to_string()));
        }
        outcome
            .pointer("/transaction/hash")
            .and_then(Value::as_str)
            .map(|hash| TxHash(hash.to_owned()))
            .ok_or_else(|| NearError::Rpc("send_tx returned no hash".into()))
    }
}

fn parse_amount(value: &Value) -> Result<u128, NearError> {
    value
        .as_str()
        .and_then(|amount| amount.parse().ok())
        .ok_or_else(|| NearError::Rpc("expected a decimal amount".into()))
}

#[cfg(test)]
pub mod testing {
    //! A scripted JSON-RPC node on a local port.

    use serde_json::{json, Value};
    use std::sync::{Arc, Mutex};
    use tokio::io::{AsyncReadExt, AsyncWriteExt};
    use tokio::net::TcpListener;

    type Script = dyn Fn(&str, &Value) -> Value + Send + Sync;

    pub struct MockRpc {
        pub url: String,
        /// `(method, params)` in arrival order.
        pub calls: Arc<Mutex<Vec<(String, Value)>>>,
    }

    impl MockRpc {
        /// `script` returns the whole JSON-RPC body for each request.
        pub async fn serve(script: impl Fn(&str, &Value) -> Value + Send + Sync + 'static) -> Self {
            let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
            let url = format!("http://{}", listener.local_addr().unwrap());
            let calls = Arc::new(Mutex::new(Vec::new()));
            let recorded = calls.clone();
            let script: Arc<Script> = Arc::new(script);
            tokio::spawn(async move {
                while let Ok((mut stream, _)) = listener.accept().await {
                    let recorded = recorded.clone();
                    let script = script.clone();
                    tokio::spawn(async move {
                        let mut request = Vec::new();
                        let mut chunk = [0u8; 8192];
                        loop {
                            let read = stream.read(&mut chunk).await.unwrap_or(0);
                            if read == 0 {
                                break;
                            }
                            request.extend_from_slice(&chunk[..read]);
                            if is_complete(&request) {
                                break;
                            }
                        }
                        let text = String::from_utf8_lossy(&request);
                        let body = text.split_once("\r\n\r\n").map_or("{}", |(_, body)| body);
                        let parsed: Value = serde_json::from_str(body).unwrap_or(Value::Null);
                        let method = parsed["method"].as_str().unwrap_or("").to_owned();
                        let params = parsed["params"].clone();
                        recorded.lock().unwrap().push((method.clone(), params.clone()));
                        let reply = script(&method, &params).to_string();
                        let response = format!(
                            "HTTP/1.1 200 OK\r\nContent-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{reply}",
                            reply.len()
                        );
                        let _ = stream.write_all(response.as_bytes()).await;
                    });
                }
            });
            Self { url, calls }
        }

        /// RPC method names, with the contract method for views.
        pub fn order(&self) -> Vec<String> {
            self.calls
                .lock()
                .unwrap()
                .iter()
                .map(|(method, params)| params["method_name"].as_str().unwrap_or(method).to_owned())
                .collect()
        }
    }

    fn is_complete(request: &[u8]) -> bool {
        let Some(end) = request.windows(4).position(|window| window == b"\r\n\r\n") else {
            return false;
        };
        let headers = String::from_utf8_lossy(&request[..end]).to_ascii_lowercase();
        let length = headers
            .lines()
            .find_map(|line| line.strip_prefix("content-length:").and_then(|v| v.trim().parse::<usize>().ok()))
            .unwrap_or(0);
        request.len() >= end + 4 + length
    }

    /// A view result: the JSON value as the byte array the RPC returns.
    pub fn view_result(value: &Value) -> Value {
        json!({ "jsonrpc": "2.0", "id": "x", "result": { "result": value.to_string().into_bytes(), "logs": [] } })
    }

    pub fn result(value: Value) -> Value {
        json!({ "jsonrpc": "2.0", "id": "x", "result": value })
    }

    /// Everything a healthy node answers for a send, for any view `views` skips.
    pub fn healthy(method: &str, params: &Value, views: impl Fn(&str) -> Option<Value>) -> Value {
        match method {
            "query" if params["request_type"] == "view_access_key" => result(json!({ "nonce": 41, "permissions": "FullAccess" })),
            "query" if params["request_type"] == "view_account" => result(json!({ "amount": "5000" })),
            "query" => view_result(&views(params["method_name"].as_str().unwrap_or("")).unwrap_or(json!({}))),
            "block" => result(json!({ "header": { "hash": "HjLq1b8YYsVzjd4rZYBTPMPYGiTBZNESAsFBAWbiXePK" } })),
            "send_tx" => result(json!({ "status": { "SuccessValue": "" }, "transaction": { "hash": "TXHASH" } })),
            _ => json!({ "jsonrpc": "2.0", "id": "x", "error": { "message": "unexpected" } }),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::testing::{healthy, result, MockRpc};
    use super::*;

    fn bridge(url: &str) -> NearBridge {
        NearBridge::new(
            url.to_owned(),
            "verifier.test",
            NearKeypair::from_secret_bytes(&[1; 32]),
            reqwest::Client::new(),
        )
        .unwrap()
    }

    fn payload() -> MultiPayload {
        let keypair = NearKeypair::from_secret_bytes(&[2; 32]);
        ft_withdraw_payload(&keypair, &AccountId::try_from("verifier.test").unwrap(), "usdc.test", 1, 1_000, [0; 32])
    }

    #[tokio::test]
    async fn unfunded_account_maps_to_account_not_funded() {
        let rpc = MockRpc::serve(|method, params| {
            if method == "query" && params["request_type"] == "view_access_key" {
                json!({ "jsonrpc": "2.0", "id": "x", "error": { "cause": { "name": "UNKNOWN_ACCOUNT" }, "name": "HANDLER_ERROR" } })
            } else {
                healthy(method, params, |_| None)
            }
        })
        .await;
        let bridge = bridge(&rpc.url);
        match bridge.deposit("usdc.test", 1).await {
            Err(NearError::AccountNotFunded { account }) => assert_eq!(account, bridge.account_id().as_str()),
            other => panic!("expected AccountNotFunded, got {other:?}"),
        }
    }

    #[tokio::test]
    async fn the_older_missing_key_shape_is_also_not_funded() {
        let rpc = MockRpc::serve(|method, params| {
            if method == "query" && params["request_type"] == "view_access_key" {
                result(json!({ "error": "access key ed25519:abc does not exist while viewing", "logs": [] }))
            } else {
                healthy(method, params, |_| None)
            }
        })
        .await;
        assert!(matches!(bridge(&rpc.url).deposit("usdc.test", 1).await, Err(NearError::AccountNotFunded { .. })));
    }

    #[tokio::test]
    async fn native_balance_of_a_never_funded_account_is_zero() {
        let rpc = MockRpc::serve(|_, _| {
            json!({ "jsonrpc": "2.0", "id": "x", "error": { "cause": { "name": "UNKNOWN_ACCOUNT" } } })
        })
        .await;
        assert_eq!(bridge(&rpc.url).native_balance().await.unwrap(), 0);
    }

    #[tokio::test]
    async fn deposit_sends_ft_transfer_call_to_verifier() {
        let rpc = MockRpc::serve(|method, params| healthy(method, params, |_| None)).await;
        let hash = bridge(&rpc.url).deposit("usdc.test", 7).await.unwrap();
        assert_eq!(hash, TxHash("TXHASH".into()));

        let calls = rpc.calls.lock().unwrap();
        let (_, params) = calls.iter().find(|(method, _)| method == "send_tx").unwrap();
        let tx = STANDARD.decode(params["signed_tx_base64"].as_str().unwrap()).unwrap();
        let text = String::from_utf8_lossy(&tx);
        assert!(text.contains("usdc.test"), "the transaction goes to the token");
        assert!(text.contains("ft_transfer_call"));
        assert!(text.contains(r#""receiver_id":"verifier.test""#), "the token sends to the verifier");
        assert!(text.contains(r#""amount":"7""#));
        assert_eq!(params["wait_until"], "FINAL");
    }

    #[tokio::test]
    async fn the_next_transaction_nonce_is_one_past_the_access_key() {
        let rpc = MockRpc::serve(|method, params| healthy(method, params, |_| None)).await;
        bridge(&rpc.url).deposit("usdc.test", 7).await.unwrap();

        // Signer 64-char id, key byte, 32-byte key, then the nonce.
        let calls = rpc.calls.lock().unwrap();
        let (_, params) = calls.iter().find(|(method, _)| method == "send_tx").unwrap();
        let tx = STANDARD.decode(params["signed_tx_base64"].as_str().unwrap()).unwrap();
        let nonce = u64::from_le_bytes(tx[4 + 64 + 1 + 32..4 + 64 + 1 + 32 + 8].try_into().unwrap());
        assert_eq!(nonce, 42);
    }

    #[tokio::test]
    async fn execute_intents_simulates_first() {
        let rpc = MockRpc::serve(|method, params| {
            healthy(method, params, |view| (view == "simulate_intents").then(|| json!({ "intents_executed": [] })))
        })
        .await;
        bridge(&rpc.url).execute_intents(&[payload()]).await.unwrap();
        let order = rpc.order();
        let simulate = order.iter().position(|m| m == "simulate_intents").unwrap();
        let send = order.iter().position(|m| m == "send_tx").unwrap();
        assert!(simulate < send, "{order:?}");
    }

    #[tokio::test]
    async fn simulate_rejection_is_not_submitted() {
        let rpc = MockRpc::serve(|method, params| {
            if params["method_name"] == "simulate_intents" {
                result(json!({ "error": "wasm execution failed with error: Smart contract panicked: invalid salt", "logs": [] }))
            } else {
                healthy(method, params, |_| None)
            }
        })
        .await;
        let error = bridge(&rpc.url).execute_intents(&[payload()]).await.unwrap_err();
        assert!(error.is_invalid_salt(), "{error:?}");
        assert!(!rpc.order().contains(&"send_tx".to_owned()));
    }

    #[tokio::test]
    async fn unmatched_deltas_is_rejected() {
        let rpc = MockRpc::serve(|method, params| {
            healthy(method, params, |view| {
                (view == "simulate_intents").then(|| {
                    json!({ "invariant_violated": { "error": "unmatched_deltas", "unmatched_deltas": { "nep141:usdc.test": "1" } } })
                })
            })
        })
        .await;
        match bridge(&rpc.url).execute_intents(&[payload()]).await {
            Err(NearError::Rejected(reason)) => assert!(reason.contains("unmatched_deltas")),
            other => panic!("expected Rejected, got {other:?}"),
        }
        assert!(!rpc.order().contains(&"send_tx".to_owned()));
    }

    #[tokio::test]
    async fn a_chain_failure_is_rejected_with_its_reason() {
        let rpc = MockRpc::serve(|method, params| {
            if method == "send_tx" {
                result(json!({ "status": { "Failure": { "ActionError": "nonce was already used" } }, "transaction": { "hash": "T" } }))
            } else {
                healthy(method, params, |view| (view == "simulate_intents").then(|| json!({})))
            }
        })
        .await;
        let error = bridge(&rpc.url).execute_intents(&[payload()]).await.unwrap_err();
        assert!(error.is_already_settled(), "{error:?}");
    }

    #[tokio::test]
    async fn balances_read_the_right_views() {
        let rpc = MockRpc::serve(|method, params| {
            healthy(method, params, |view| match view {
                "mt_balance_of" => Some(json!("11")),
                "ft_balance_of" => Some(json!("22")),
                "current_salt" => Some(json!("fc4d10f1")),
                _ => None,
            })
        })
        .await;
        let bridge = bridge(&rpc.url);
        assert_eq!(bridge.verifier_balance("usdc.test").await.unwrap(), 11);
        assert_eq!(bridge.wallet_balance("usdc.test").await.unwrap(), 22);
        assert_eq!(bridge.native_balance().await.unwrap(), 5000);
        assert_eq!(bridge.current_salt().await.unwrap(), [0xfc, 0x4d, 0x10, 0xf1]);
    }

    #[tokio::test]
    async fn withdraw_signs_an_ft_withdraw_to_self() {
        let rpc = MockRpc::serve(|method, params| {
            healthy(method, params, |view| match view {
                "current_salt" => Some(json!("fc4d10f1")),
                "simulate_intents" => Some(json!({})),
                _ => None,
            })
        })
        .await;
        let bridge = bridge(&rpc.url);
        bridge.withdraw("usdc.test", 5, 1_000).await.unwrap();
        let calls = rpc.calls.lock().unwrap();
        let (_, params) = calls.iter().find(|(_, p)| p["method_name"] == "simulate_intents").unwrap();
        let args: Value =
            serde_json::from_slice(&STANDARD.decode(params["args_base64"].as_str().unwrap()).unwrap()).unwrap();
        let message = args["signed"][0]["payload"]["message"].as_str().unwrap();
        assert!(message.contains(r#""intent":"ft_withdraw""#));
        assert!(message.contains(&format!(r#""receiver_id":"{}""#, bridge.account_id())));
    }

    #[tokio::test]
    async fn a_reported_transaction_must_be_a_verifier_call_that_logged_the_hash() {
        let rpc = MockRpc::serve(|_, params| {
            let receiver = if params["tx_hash"] == "ELSEWHERE" { "other.test" } else { "verifier.test" };
            let status = if params["tx_hash"] == "FAILED" { json!({ "Failure": {} }) } else { json!({ "SuccessValue": "" }) };
            result(json!({
                "status": status,
                "transaction": { "receiver_id": receiver },
                "receipts_outcome": [{ "outcome": { "logs": ["EVENT_JSON:{\"intent_hash\":\"MYHASH\"}"] } }],
            }))
        })
        .await;
        let bridge = bridge(&rpc.url);
        assert!(bridge.tx_logged("GOOD", "gw", "MYHASH").await.unwrap());
        assert!(!bridge.tx_logged("GOOD", "gw", "OTHERHASH").await.unwrap());
        assert!(!bridge.tx_logged("FAILED", "gw", "MYHASH").await.unwrap());
        assert!(!bridge.tx_logged("ELSEWHERE", "gw", "MYHASH").await.unwrap());
    }

    /// Run with `CABALMESH_NEAR_LIVE_SECRET=<hex of 32 bytes> cargo test --lib live_ -- --ignored`.
    #[tokio::test]
    #[ignore = "needs testnet and a funded key"]
    async fn live_deposit_and_balance() {
        let secret: [u8; 32] =
            hex::decode(std::env::var("CABALMESH_NEAR_LIVE_SECRET").unwrap()).unwrap().try_into().unwrap();
        let near = crate::network_config::Network::Testnet.near();
        let bridge = NearBridge::new(
            near.rpc_url.to_owned(),
            near.verifier,
            NearKeypair::from_secret_bytes(&secret),
            reqwest::Client::new(),
        )
        .unwrap();
        let before = bridge.verifier_balance(near.quote.contract).await.unwrap();
        bridge.deposit(near.quote.contract, 1).await.unwrap();
        assert_eq!(bridge.verifier_balance(near.quote.contract).await.unwrap(), before + 1);
    }
}
