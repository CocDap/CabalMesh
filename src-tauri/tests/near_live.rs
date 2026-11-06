//! The settlement flow against the live Tidepool verifier.
//!
//! Ignored by default: it spends testnet gas and needs funded accounts. Create
//! them with `node scripts/near-intents-spike/probe.mjs setup`, then run
//! `cargo test --test near_live -- --ignored --test-threads=1`.
//!
//! Each `Device` is what one phone holds: its own ledger, diff book and key.
//! Messages travel as the same JSON the mesh floods; only the radio is missing.

use cabal_core::{
    Action, Chain, Condition, ExecutionMode, IntentDraft, IntentId, IntentStatus, PrivacyLevel, TokenAmount, UsdPrice,
};
use cabal_near::{AccountId, MultiPayload, NearKeypair};
use cabalmesh_lib::intents::{Intent, Ledger};
use cabalmesh_lib::near_bridge::{NearBridge, NearError};
use cabalmesh_lib::near_settlement::{self as settlement, DiffBook, SettlementComplete};
use cabalmesh_lib::network_config::{NearConfig, Network};
use std::path::Path;

/// 0.1 wNEAR. Small enough that the faucet's drip lasts many runs.
const AMOUNT: u128 = 100_000_000_000_000_000_000_000;
const PRICE_CENTS: u64 = 404;
/// 0.1 wNEAR at $4.04.
const QUOTE: u128 = 404_000;

fn near() -> NearConfig {
    Network::Testnet.near()
}

fn now() -> u64 {
    std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).unwrap().as_millis() as u64
}

fn key(name: &str) -> NearKeypair {
    let path = Path::new(env!("CARGO_MANIFEST_DIR")).join("../scripts/near-intents-spike/.keys.json");
    let text = std::fs::read_to_string(&path).expect("run probe.mjs setup first, it writes the keys this test uses");
    let keys: serde_json::Value = serde_json::from_str(&text).unwrap();
    let seed = keys[name]["seed"].as_str().unwrap();
    let bytes: Vec<u8> = (0..seed.len())
        .step_by(2)
        .map(|i| u8::from_str_radix(&seed[i..i + 2], 16).unwrap())
        .collect();
    NearKeypair::from_secret_bytes(&bytes.try_into().unwrap())
}

fn bridge(name: &str) -> NearBridge {
    let near = near();
    NearBridge::new(near.rpc_url.to_owned(), near.verifier, key(name), reqwest::Client::new()).unwrap()
}

struct Device {
    name: &'static str,
    keypair: NearKeypair,
    ledger: Ledger,
    book: DiffBook,
    _dir: tempfile::TempDir,
}

impl Device {
    fn new(name: &'static str) -> Self {
        let dir = tempfile::tempdir().unwrap();
        Self {
            name,
            keypair: key(name),
            ledger: Ledger::open(cabal_store::DocumentStore::new(dir.path().join("intents.json"))).unwrap(),
            book: DiffBook::open(cabal_store::DocumentStore::new(dir.path().join("diffs.json"))).unwrap(),
            _dir: dir,
        }
    }

    fn bridge(&self) -> NearBridge {
        bridge(self.name)
    }

    fn post(&self, action: Action) -> Intent {
        let draft = IntentDraft {
            chain: Chain::NearTestnet,
            action,
            asset: "wNEAR".into(),
            condition: Condition::Any,
            amount: TokenAmount::from_raw(AMOUNT, 24),
            mode: ExecutionMode::Shark,
            privacy: PrivacyLevel::Low,
        };
        let intent = self.ledger.create(draft, now()).unwrap();
        self.ledger.advance(&intent.id, IntentStatus::Broadcast { route_len: 1 }, now()).unwrap();
        intent
    }

    /// Agrees on a deal, then signs this side: the moment funds are committed.
    fn sign(&self, intent: &IntentId, deal: &str, salt: [u8; 4]) -> settlement::SignedDiff {
        let near = near();
        let verifier = AccountId::try_from(near.verifier).unwrap();
        self.ledger.propose_deal(intent, deal, UsdPrice::from_cents(PRICE_CENTS), "target");
        let current = self.ledger.get(intent).unwrap();
        let signed =
            settlement::sign_own_half(&self.ledger, &self.book, &current, &self.keypair, &verifier, &near, Some(salt), now())
                .expect("the deal prices to something");
        self.ledger.advance(intent, IntentStatus::Waiting, now()).unwrap();
        signed
    }

    fn receive(&self, diff: &settlement::SignedDiff) -> bool {
        let near = near();
        settlement::receive_diff(&self.ledger, &self.book, Chain::NearTestnet, near.verifier, &serde_json::to_string(diff).unwrap())
    }

    fn status(&self, intent: &IntentId) -> IntentStatus {
        self.ledger.get(intent).unwrap().status
    }

    fn log_has(&self, intent: &IntentId, text: &str) -> bool {
        self.ledger.get(intent).unwrap().log.iter().any(|entry| entry.text.contains(text))
    }
}

async fn balances(bridge: &NearBridge) -> (u128, u128) {
    let near = near();
    (
        bridge.verifier_balance(near.base.contract).await.unwrap(),
        bridge.verifier_balance(near.quote.contract).await.unwrap(),
    )
}

/// Run after deploying the rewards contract: a real pair through the real thing.
///
/// ```bash
/// CABALMESH_REWARDS_ID=rewards.you.testnet cargo test --test near_live \\
///   the_deployed_rewards_contract_pays_the_gateway -- --ignored --nocapture
/// ```
#[tokio::test]
#[ignore = "needs a deployed contract, CABALMESH_REWARDS_ID and spike keys"]
async fn the_deployed_rewards_contract_pays_the_gateway() {
    let rewards: &'static str = std::env::var("CABALMESH_REWARDS_ID")
        .expect("set CABALMESH_REWARDS_ID to the deployed contract")
        .leak();
    let near = NearConfig {
        rewards: Some(rewards),
        ..near()
    };
    let fee = near.gateway_fee().expect("a valid account id");
    let verifier = AccountId::try_from(near.verifier).unwrap();
    let (a, b) = (key("A"), key("B"));
    let gateway = bridge("G").with_gateway_fee(Some(fee.clone()));
    let salt = gateway.current_salt().await.unwrap();
    let deadline = now() + 3_600_000;

    let sign = |keypair: &NearKeypair, side| {
        settlement::sign_half(
            keypair,
            &verifier,
            &near,
            side,
            AMOUNT,
            QUOTE,
            deadline,
            settlement::fresh_nonce(Some(salt), deadline),
        )
    };
    let pair = [sign(&a, Action::Sell), sign(&b, Action::Buy)];

    let before = gateway.verifier_balance(near.quote.contract).await.unwrap();
    let hash = gateway
        .submit_pair(&pair)
        .await
        .expect("the contract takes the pair");
    println!("settled through the contract: {}", hash.0);
    tokio::time::sleep(std::time::Duration::from_secs(5)).await;

    assert_eq!(
        gateway.verifier_balance(near.quote.contract).await.unwrap(),
        before + 2 * fee.amount,
        "the gateway is paid both fees"
    );
    let needle = pair[0]
        .hash()
        .map(|hash| bs58::encode(hash).into_string())
        .unwrap();
    assert!(
        gateway
            .tx_logged(&hash.0, gateway.account_id().as_str(), &needle)
            .await
            .unwrap(),
        "a peer can prove this settlement from the verifier's own receipt"
    );
}

/// Two devices, a gateway that holds neither token, and a device with no gas.
#[tokio::test]
#[ignore = "spends testnet gas and needs scripts/near-intents-spike/.keys.json"]
async fn two_devices_settle_through_a_third_party_gateway() {
    let (a, b) = (Device::new("A"), Device::new("B"));
    let gateway = bridge("G");
    let salt = gateway.current_salt().await.unwrap();

    let (a_before, b_before) = (balances(&a.bridge()).await, balances(&b.bridge()).await);
    assert!(a_before.0 >= AMOUNT, "A needs wNEAR in the verifier: {a_before:?}; use the faucet");
    assert!(b_before.1 >= QUOTE, "B needs USDC in the verifier: {b_before:?}; use the faucet");

    // The deal, then one signature each, flooded across.
    let (sell, buy) = (a.post(Action::Sell), b.post(Action::Buy));
    let deal = settlement::deal_id(sell.id.as_str(), buy.id.as_str());
    let (sell_half, buy_half) = (a.sign(&sell.id, &deal, salt), b.sign(&buy.id, &deal, salt));
    assert!(a.receive(&buy_half), "A must accept B's signature");
    assert!(b.receive(&sell_half), "B must accept A's signature");

    // B has no NEAR at all. It must hold the pair, not fail the deal.
    let submitted = settlement::submit_held(&b.ledger, &b.book, &b.bridge()).await;
    assert!(submitted.is_empty(), "B cannot pay gas: {submitted:?}");
    assert!(b.book.held().contains(&deal), "the pair stays held for a gateway");
    assert!(matches!(b.status(&buy.id), IntentStatus::Waiting), "{:?}", b.status(&buy.id));
    assert!(b.log_has(&buy.id, "NO NEAR FOR GAS"));

    // Anyone with gas can carry it.
    let submitted = settlement::submit_held(&a.ledger, &a.book, &gateway).await;
    assert_eq!(submitted.len(), 1, "the gateway submits the pair exactly once");
    let report: &SettlementComplete = &submitted[0];
    println!("settled: {} by {}", report.tx_hash, report.gateway);
    assert!(matches!(a.status(&sell.id), IntentStatus::Settled { .. }), "{:?}", a.status(&sell.id));

    // B believes the report only because its own hash is in that transaction.
    settlement::receive_report(&b.ledger, &serde_json::to_string(report).unwrap()).expect("B has this deal");
    settlement::confirm_reports(&b.ledger, &b.book, &b.bridge()).await;
    assert!(
        matches!(b.status(&buy.id), IntentStatus::Settled { .. }),
        "B must find its intent hash in the settlement logs: {:?}",
        b.status(&buy.id)
    );

    // Both legs moved, by exactly the deal.
    let (a_after, b_after) = (balances(&a.bridge()).await, balances(&b.bridge()).await);
    assert_eq!(a_after, (a_before.0 - AMOUNT, a_before.1 + QUOTE));
    assert_eq!(b_after, (b_before.0 + AMOUNT, b_before.1 - QUOTE));

    // A device that was offline when the report went out still finds out, from
    // its own spent nonce.
    let missed = Device::new("B");
    let same_side = missed.post(Action::Buy);
    missed.ledger.propose_deal(&same_side.id, &deal, UsdPrice::from_cents(PRICE_CENTS), "target");
    let signed_until = b.ledger.get(&buy.id).unwrap().deal.unwrap().signed_until_ms.unwrap();
    missed.ledger.mark_signed(&same_side.id, signed_until);
    missed.ledger.advance(&same_side.id, IntentStatus::Waiting, now()).unwrap();
    missed.book.insert(&deal, Action::Buy, buy_half.payload.clone());
    settlement::confirm_reports(&missed.ledger, &missed.book, &missed.bridge()).await;
    assert!(
        matches!(missed.status(&same_side.id), IntentStatus::Settled { .. }),
        "{:?}",
        missed.status(&same_side.id)
    );

    // The same signatures cannot move funds twice.
    let pair: [MultiPayload; 2] = [sell_half.payload.clone(), buy_half.payload.clone()];
    let replay = gateway.execute_intents(&pair).await.expect_err("a replay must fail");
    assert!(replay.is_already_settled(), "{replay}");

    // A real transaction that is not about this deal convinces nobody.
    let other = b.post(Action::Buy);
    let other_deal = settlement::deal_id(other.id.as_str(), "someone-else");
    let _ = b.sign(&other.id, &other_deal, salt);
    let false_report = SettlementComplete {
        deal_id: other_deal,
        tx_hash: report.tx_hash.clone(),
        gateway: report.gateway.clone(),
    };
    settlement::receive_report(&b.ledger, &serde_json::to_string(&false_report).unwrap()).unwrap();
    settlement::confirm_reports(&b.ledger, &b.book, &b.bridge()).await;
    let after = b.ledger.get(&other.id).unwrap();
    assert!(matches!(after.status, IntentStatus::Waiting), "{:?}", after.status);
    settlement::confirm_reports(&b.ledger, &b.book, &b.bridge()).await;
    assert!(matches!(b.status(&other.id), IntentStatus::Waiting), "an unspent nonce proves nothing");
    assert!(after.deal.unwrap().reported.is_none(), "the false report is dropped");
}

/// A pair that does not net to zero costs no gas, moves nothing, and tells the
/// user their funds are safe.
#[tokio::test]
#[ignore = "calls testnet and needs scripts/near-intents-spike/.keys.json"]
async fn a_mismatched_pair_is_rejected_without_moving_funds() {
    let (a, b) = (Device::new("A"), Device::new("B"));
    let gateway = bridge("G");
    let salt = gateway.current_salt().await.unwrap();
    let before = (balances(&a.bridge()).await, balances(&b.bridge()).await);

    let sell = a.post(Action::Sell);
    let deal = settlement::deal_id(sell.id.as_str(), "peer");
    let _ = a.sign(&sell.id, &deal, salt);

    // B agreed to the same deal at a lower price than A did.
    let near = near();
    let verifier = AccountId::try_from(near.verifier).unwrap();
    let cheap = settlement::sign_half(
        &b.keypair,
        &verifier,
        &near,
        Action::Buy,
        AMOUNT,
        QUOTE / 2,
        settlement::deadline_from(now()),
        settlement::fresh_nonce(Some(salt), settlement::deadline_from(now())),
    );
    assert!(a.receive(&settlement::SignedDiff {
        deal_id: deal,
        chain: Chain::NearTestnet,
        side: "buy".into(),
        payload: cheap,
    }));

    assert!(settlement::submit_held(&a.ledger, &a.book, &gateway).await.is_empty());
    assert!(matches!(a.status(&sell.id), IntentStatus::Failed { .. }), "{:?}", a.status(&sell.id));
    assert!(a.log_has(&sell.id, "FUNDS REMAIN IN INTENTS BALANCE"));
    assert_eq!((balances(&a.bridge()).await, balances(&b.bridge()).await), before);
}

/// A signature made under an old salt is refused with a reason the app can act on.
#[tokio::test]
#[ignore = "calls testnet and needs scripts/near-intents-spike/.keys.json"]
async fn a_stale_salt_is_refused_as_invalid_salt() {
    let near = near();
    let verifier = AccountId::try_from(near.verifier).unwrap();
    let deadline = settlement::deadline_from(now());
    let sign = |name: &str, side, seed: [u8; 4]| {
        settlement::sign_half(&key(name), &verifier, &near, side, AMOUNT, QUOTE, deadline, settlement::fresh_nonce(Some(seed), deadline))
    };
    let stale = [0xde, 0xad, 0xbe, 0xef];
    let error = bridge("G")
        .simulate(&[sign("A", Action::Sell, stale), sign("B", Action::Buy, stale)])
        .await
        .expect_err("a made-up salt must be refused");
    assert!(error.is_invalid_salt(), "{error}");
    assert!(matches!(error, NearError::Rejected(_)), "{error}");
}

/// The wallet round trip the Vault screen drives, signed by the Rust encoder.
#[tokio::test]
#[ignore = "spends testnet gas and needs scripts/near-intents-spike/.keys.json"]
async fn withdraw_then_deposit_returns_the_balance() {
    let a = bridge("A");
    let contract = near().quote.contract;
    const ONE_USDC: u128 = 1_000_000;

    let intents = a.verifier_balance(contract).await.unwrap();
    let wallet = a.wallet_balance(contract).await.unwrap();
    assert!(intents >= ONE_USDC, "A needs USDC in the verifier: {intents}");

    let out = a.withdraw(contract, ONE_USDC, now()).await.unwrap();
    println!("withdraw: {}", out.0);
    assert_eq!(a.verifier_balance(contract).await.unwrap(), intents - ONE_USDC);
    assert_eq!(a.wallet_balance(contract).await.unwrap(), wallet + ONE_USDC);

    let back = a.deposit(contract, ONE_USDC).await.unwrap();
    println!("deposit: {}", back.0);
    assert_eq!(a.verifier_balance(contract).await.unwrap(), intents);
    assert_eq!(a.wallet_balance(contract).await.unwrap(), wallet);
}
