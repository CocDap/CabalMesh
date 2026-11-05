//! The contract against the real verifier code, on a local sandbox.
//!
//! The verifier wasm is the one deployed on testnet, fetched once and kept in
//! `rewards/res/` (gitignored). The first run needs network; later runs do not.

use anyhow::{Context, Result};
use cabal_near::{
    token_diff_payload, versioned_nonce, GatewayFee, MultiPayload, NearKeypair, TokenDiff,
};
use near_workspaces::network::Sandbox;
use near_workspaces::result::ExecutionFinalResult;
use near_workspaces::types::Gas;
use near_workspaces::{Account, Contract, Worker};
use serde_json::{json, Value};
use std::path::PathBuf;
use tokio::sync::OnceCell;

const TESTNET_VERIFIER: &str = "verifier.tidepool-sandbox.testnet";
const FEE: u128 = 10_000;
const BASE: u128 = 1_000;
const QUOTE: u128 = 4_000_000;
const FAR_FUTURE_MS: u64 = 4_000_000_000_000;

struct Fixture {
    worker: Worker<Sandbox>,
    verifier: Contract,
    usdc: Contract,
    wnear: Contract,
    rewards: Contract,
    seller: NearKeypair,
    buyer: NearKeypair,
    salt: [u8; 4],
}

/// Built once per run. Tests run in parallel, and two `cargo near` builds of one
/// crate overwrite each other's output while `wasm-opt` is reading it.
async fn built_wasm() -> Result<&'static (Vec<u8>, Vec<u8>)> {
    static WASM: OnceCell<(Vec<u8>, Vec<u8>)> = OnceCell::const_new();
    WASM.get_or_try_init(|| async {
        let token = near_workspaces::compile_project("../test-token").await?;
        let rewards = near_workspaces::compile_project("./").await?;
        Ok::<_, anyhow::Error>((token, rewards))
    })
    .await
}

async fn verifier_wasm() -> Result<Vec<u8>> {
    let path = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("res/verifier.wasm");
    if let Ok(wasm) = std::fs::read(&path) {
        return Ok(wasm);
    }
    let id = TESTNET_VERIFIER.parse()?;
    let wasm = near_workspaces::testnet()
        .await?
        .view_code(&id)
        .await
        .context("fetching the verifier")?;
    std::fs::create_dir_all(path.parent().context("no parent")?)?;
    std::fs::write(&path, &wasm)?;
    Ok(wasm)
}

fn token_id(token: &Contract) -> String {
    format!("nep141:{}", token.id())
}

impl Fixture {
    /// The seller holds only wNEAR and the buyer only USDC, enough for the trade
    /// and the buyer's fee. So the seller's fee can only come out of the trade.
    async fn new() -> Result<Self> {
        let worker = near_workspaces::sandbox().await?;
        let verifier = worker.dev_deploy(&verifier_wasm().await?).await?;
        let (token, rewards_wasm) = built_wasm().await?;
        let usdc = worker.dev_deploy(token).await?;
        let wnear = worker.dev_deploy(token).await?;

        verifier
            .call("new")
            .args_json(json!({
                "config": {
                    "wnear_id": wnear.id(),
                    "fees": { "fee": 0, "fee_collector": verifier.id() },
                    "roles": { "super_admins": [], "admins": {}, "grantees": {} },
                },
            }))
            .max_gas()
            .transact()
            .await?
            .into_result()
            .context("initialising the verifier")?;

        let rewards = worker.dev_deploy(rewards_wasm).await?;
        rewards
            .call("new")
            .args_json(json!({
                "verifier": verifier.id(),
                "fee_token": token_id(&usdc),
                "fee_per_side": FEE.to_string(),
            }))
            .transact()
            .await?
            .into_result()
            .context("initialising the contract")?;

        let (seller, buyer) = (
            NearKeypair::from_secret_bytes(&[1; 32]),
            NearKeypair::from_secret_bytes(&[2; 32]),
        );
        let salt = verifier.view("current_salt").await?.json::<String>()?;
        let salt = <[u8; 4]>::try_from(hex::decode(salt)?)
            .map_err(|_| anyhow::anyhow!("salt is not 4 bytes"))?;
        let fixture = Self {
            worker,
            verifier,
            usdc,
            wnear,
            rewards,
            seller,
            buyer,
            salt,
        };
        fixture
            .credit(&fixture.wnear, fixture.seller.account_id().as_str(), BASE)
            .await?;
        fixture
            .credit(
                &fixture.usdc,
                fixture.buyer.account_id().as_str(),
                QUOTE + FEE,
            )
            .await?;
        Ok(fixture)
    }

    async fn credit(&self, token: &Contract, receiver: &str, amount: u128) -> Result<()> {
        token
            .call("credit")
            .args_json(json!({ "verifier": self.verifier.id(), "receiver": receiver, "amount": amount.to_string() }))
            .max_gas()
            .transact()
            .await?
            .into_result()?;
        Ok(())
    }

    fn fee(&self) -> GatewayFee {
        GatewayFee {
            to: self.rewards.id().as_str().try_into().unwrap(),
            token: token_id(&self.usdc),
            amount: FEE,
        }
    }

    fn sign(
        &self,
        keypair: &NearKeypair,
        usdc: i128,
        wnear: i128,
        fee: Option<&GatewayFee>,
        tail: u8,
    ) -> Value {
        let diff: TokenDiff = [(token_id(&self.usdc), usdc), (token_id(&self.wnear), wnear)]
            .into_iter()
            .collect();
        let nonce = versioned_nonce(self.salt, FAR_FUTURE_MS, [tail; 15]);
        let verifier = self.verifier.id().as_str().try_into().unwrap();
        let payload = token_diff_payload(keypair, &verifier, &diff, fee, FAR_FUTURE_MS, nonce);
        serde_json::to_value(payload).unwrap()
    }

    fn pair(&self) -> [Value; 2] {
        let fee = self.fee();
        [
            self.sign(&self.seller, QUOTE as i128, -(BASE as i128), Some(&fee), 1),
            self.sign(&self.buyer, -(QUOTE as i128), BASE as i128, Some(&fee), 2),
        ]
    }

    async fn submit(&self, gateway: &Account, payloads: &[Value]) -> Result<ExecutionFinalResult> {
        Ok(gateway
            .call(self.rewards.id(), "submit")
            .args_json(json!({ "payloads": payloads }))
            .max_gas()
            .transact()
            .await?)
    }

    async fn balance(&self, account: &str, token: &Contract) -> Result<u128> {
        let amount: String = self
            .verifier
            .view("mt_balance_of")
            .args_json(json!({ "account_id": account, "token_id": token_id(token) }))
            .await?
            .json()?;
        Ok(amount.parse()?)
    }

    async fn nonce_used(&self, keypair: &NearKeypair, payload: &Value) -> Result<bool> {
        Ok(self
            .verifier
            .view("is_nonce_used")
            .args_json(json!({ "account_id": keypair.account_id().as_str(), "nonce": payload["payload"]["nonce"] }))
            .await?
            .json()?)
    }

    async fn stats(&self) -> Result<Value> {
        Ok(self.rewards.view("stats").await?.json()?)
    }
}

#[tokio::test]
async fn gateway_is_paid_for_a_valid_pair() -> Result<()> {
    let fx = Fixture::new().await?;
    let gateway = fx.worker.dev_create_account().await?;
    let outcome = fx.submit(&gateway, &fx.pair()).await?;
    assert!(outcome.is_success(), "{:#?}", outcome.failures());

    assert_eq!(
        fx.balance(gateway.id().as_str(), &fx.usdc).await?,
        2 * FEE,
        "both fees reach the gateway"
    );
    assert_eq!(
        fx.balance(fx.rewards.id().as_str(), &fx.usdc).await?,
        0,
        "the contract keeps nothing"
    );
    let seller = fx.seller.account_id();
    assert_eq!(
        fx.balance(seller.as_str(), &fx.usdc).await?,
        QUOTE - FEE,
        "the seller's fee came from the trade"
    );
    assert_eq!(fx.balance(seller.as_str(), &fx.wnear).await?, 0);
    let buyer = fx.buyer.account_id();
    assert_eq!(fx.balance(buyer.as_str(), &fx.usdc).await?, 0);
    assert_eq!(fx.balance(buyer.as_str(), &fx.wnear).await?, BASE);

    let stats = fx.stats().await?;
    assert_eq!(
        stats,
        json!({ "submissions": 1, "paid": (2 * FEE).to_string() })
    );
    assert_eq!(
        fx.rewards
            .view("pending_of")
            .args_json(json!({ "account_id": gateway.id() }))
            .await?
            .json::<String>()?,
        "0"
    );
    Ok(())
}

/// What the app relies on to believe a report: the verifier's own receipt logs
/// the trader's intent, though the transaction went to the contract.
#[tokio::test]
async fn the_verifier_itself_logs_the_settlement() -> Result<()> {
    let fx = Fixture::new().await?;
    let gateway = fx.worker.dev_create_account().await?;
    let outcome = fx.submit(&gateway, &fx.pair()).await?;
    let seller = fx.seller.account_id();
    assert!(
        outcome.receipt_outcomes().iter().any(|receipt| {
            receipt.executor_id == *fx.verifier.id()
                && receipt.is_success()
                && receipt
                    .logs
                    .iter()
                    .any(|log| log.contains("intents_executed") && log.contains(seller.as_str()))
        }),
        "{:#?}",
        outcome.receipt_outcomes()
    );
    Ok(())
}

#[tokio::test]
async fn a_failed_execution_pays_nothing() -> Result<()> {
    let fx = Fixture::new().await?;
    let gateway = fx.worker.dev_create_account().await?;
    let mut pair = fx.pair();
    let forged = fx.sign(
        &fx.buyer,
        -(QUOTE as i128),
        BASE as i128,
        Some(&fx.fee()),
        3,
    );
    pair[1]["signature"] = forged["signature"].clone();

    let outcome = fx.submit(&gateway, &pair).await?;
    assert!(
        outcome.is_failure(),
        "a rejected pair must read as a failed transaction"
    );
    assert_eq!(fx.balance(gateway.id().as_str(), &fx.usdc).await?, 0);
    assert_eq!(fx.balance(fx.rewards.id().as_str(), &fx.usdc).await?, 0);
    assert_eq!(
        fx.balance(fx.buyer.account_id().as_str(), &fx.usdc).await?,
        QUOTE + FEE,
        "the fee reverted"
    );
    assert_eq!(fx.stats().await?, json!({ "submissions": 0, "paid": "0" }));
    Ok(())
}

#[tokio::test]
async fn a_double_submission_pays_once() -> Result<()> {
    let fx = Fixture::new().await?;
    let (first, second) = (
        fx.worker.dev_create_account().await?,
        fx.worker.dev_create_account().await?,
    );
    let pair = fx.pair();
    let (a, b) = tokio::join!(fx.submit(&first, &pair), fx.submit(&second, &pair));
    let (a, b) = (a?, b?);
    assert_ne!(a.is_success(), b.is_success(), "exactly one wins");

    let paid = fx.balance(first.id().as_str(), &fx.usdc).await?
        + fx.balance(second.id().as_str(), &fx.usdc).await?;
    assert_eq!(paid, 2 * FEE);
    assert_eq!(
        fx.stats().await?,
        json!({ "submissions": 1, "paid": (2 * FEE).to_string() })
    );
    Ok(())
}

#[tokio::test]
async fn a_payload_without_the_fee_never_reaches_the_verifier() -> Result<()> {
    let fx = Fixture::new().await?;
    let gateway = fx.worker.dev_create_account().await?;
    let seller = fx.sign(&fx.seller, QUOTE as i128, -(BASE as i128), None, 1);
    let buyer = fx.sign(&fx.buyer, -(QUOTE as i128), BASE as i128, None, 2);

    let outcome = fx.submit(&gateway, &[seller.clone(), buyer]).await?;
    assert!(outcome.is_failure());
    assert!(!fx.nonce_used(&fx.seller, &seller).await?);
    assert!(outcome
        .receipt_outcomes()
        .iter()
        .all(|receipt| receipt.executor_id != *fx.verifier.id()));
    Ok(())
}

/// With too little gas the trade could stand with nothing left to pay for it.
#[tokio::test]
async fn too_little_gas_is_refused_before_forwarding() -> Result<()> {
    let fx = Fixture::new().await?;
    let gateway = fx.worker.dev_create_account().await?;
    let pair = fx.pair();
    let outcome = gateway
        .call(fx.rewards.id(), "submit")
        .args_json(json!({ "payloads": pair }))
        .gas(Gas::from_tgas(150))
        .transact()
        .await?;
    assert!(outcome.is_failure());
    assert!(!fx.nonce_used(&fx.seller, &pair[0]).await?);
    Ok(())
}

/// A transfer the verifier can refund after it runs must not earn a payout out
/// of fees other traders already paid.
#[tokio::test]
async fn a_refundable_fee_cannot_drain_the_contract() -> Result<()> {
    let fx = Fixture::new().await?;
    let attacker = fx.worker.dev_create_account().await?;
    fx.credit(&fx.usdc, fx.rewards.id().as_str(), 5 * FEE)
        .await?;

    // Signed properly, so only the contract's own check can stop it.
    let mut pair = fx.pair();
    let message = pair[0]["payload"]["message"]
        .as_str()
        .context("no message")?;
    let mut message: Value = serde_json::from_str(message)?;
    message["intents"][1]["notification"] = json!({ "msg": "refund me" });
    let nonce = versioned_nonce(fx.salt, FAR_FUTURE_MS, [9; 15]);
    let signed = MultiPayload::sign(
        &fx.seller,
        message.to_string(),
        nonce,
        fx.verifier.id().as_str(),
    );
    pair[0] = serde_json::to_value(signed)?;

    let outcome = fx.submit(&attacker, &pair).await?;
    assert!(outcome.is_failure());
    assert_eq!(fx.balance(attacker.id().as_str(), &fx.usdc).await?, 0);
    assert_eq!(
        fx.balance(fx.rewards.id().as_str(), &fx.usdc).await?,
        5 * FEE
    );
    Ok(())
}
