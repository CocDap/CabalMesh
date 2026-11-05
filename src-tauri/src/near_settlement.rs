//! Exchanging signed diffs over the mesh, and settling them through Intents.
//!
//! # Who does what
//!
//! Each side signs its half of a deal and floods it. Every node holds what it
//! hears in a [`DiffBook`], and any node holding both halves can submit them:
//! the verifier applies a pair only if it nets to zero, so a gateway can delay a
//! deal and cannot change one.
//!
//! # Nothing from the mesh is believed
//!
//! A signature is checked before a diff is held. A report that a deal settled
//! is checked against the chain before an intent says so, because the proof
//! screen renders that hash as fact.

use crate::bindings::LogTone;
use crate::intents::{line, now_ms, Intent, Ledger};
use crate::near_bridge::{NearBridge, NearError, TxHash, INTENT_LIFETIME_MS};
use crate::network_config::NearConfig;
use crate::state::AppState;
use cabal_core::{Action, Chain, FailureReason, IntentId, IntentStatus, ProofHash, UsdPrice};
use cabal_near::{
    buyer_diff, quote_amount, seller_diff, token_diff_payload, verify, versioned_nonce, AccountId, FeeRule, MultiPayload,
    NearKeypair,
};
use serde::{Deserialize, Serialize};
use std::collections::{BTreeMap, HashSet};
use std::sync::{Arc, Mutex, PoisonError};
use std::time::Duration;

/// How often held pairs are retried while the app runs.
///
/// `online` only says whether libp2p has peers, so a device with internet and
/// no mesh peers would otherwise never be told it can reach the chain.
pub const RETRY_INTERVAL: Duration = Duration::from_secs(30);

/// Deals remembered at once. Anyone on the mesh can make this device hold a
/// signed diff, so it has to have a limit.
const MAX_DEALS: usize = 256;

/// The two intents of a deal name it, in an order both sides compute alike.
#[must_use]
pub fn deal_id(a: &str, b: &str) -> String {
    if a <= b {
        format!("{a}:{b}")
    } else {
        format!("{b}:{a}")
    }
}

const fn side_label(side: Action) -> Option<&'static str> {
    match side {
        Action::Sell => Some("sell"),
        Action::Buy => Some("buy"),
        Action::Swap | Action::Stake => None,
    }
}

/// What a device floods once it has signed.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct SignedDiff {
    pub deal_id: String,
    pub chain: Chain,
    /// `sell` or `buy`.
    pub side: String,
    pub payload: MultiPayload,
}

/// What a gateway floods once a pair is on-chain.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct SettlementComplete {
    pub deal_id: String,
    pub tx_hash: String,
    /// The sender, which the RPC needs to look the transaction up.
    pub gateway: String,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
struct Entry {
    sell: Option<MultiPayload>,
    buy: Option<MultiPayload>,
    closed: bool,
    arrived: u64,
}

#[derive(Debug, Default, Serialize, Deserialize)]
struct BookFile {
    deals: BTreeMap<String, Entry>,
    next: u64,
}

/// Every signed diff this device holds, persisted so a pair picked up over
/// Bluetooth survives until the device can reach the chain.
#[derive(Clone)]
pub struct DiffBook {
    file: Arc<Mutex<BookFile>>,
    store: Arc<cabal_store::JsonStore>,
    /// Being submitted right now. Not persisted: a crash mid-submission should
    /// retry, and the verifier refuses a real duplicate.
    in_flight: Arc<Mutex<HashSet<String>>>,
    /// Deals whose lack of gas has been told to the user already.
    noted: Arc<Mutex<HashSet<String>>>,
}

impl DiffBook {
    #[must_use]
    pub fn open(store: cabal_store::JsonStore) -> Self {
        Self {
            file: Arc::new(Mutex::new(store.load_or(BookFile::default()))),
            store: Arc::new(store),
            in_flight: Arc::default(),
            noted: Arc::default(),
        }
    }

    fn persist(&self, file: &BookFile) {
        if let Err(error) = self.store.save(file) {
            tracing::error!(target: "cabalmesh::settlement", %error, "could not persist signed diffs");
        }
    }

    /// Holds one half. Returns the pair, ordered `[sell, buy]`, at the one
    /// moment it completes. The first valid diff for a side stays.
    pub fn insert(&self, deal_id: &str, side: Action, payload: MultiPayload) -> Option<[MultiPayload; 2]> {
        self.place(deal_id, side, payload, false)
    }

    /// Holds this device's own signature, replacing whatever a peer put in its
    /// slot: deal ids are public, so one could have got there first.
    fn hold_own(&self, deal_id: &str, side: Action, payload: MultiPayload) {
        let _ = self.place(deal_id, side, payload, true);
    }

    fn place(&self, deal_id: &str, side: Action, payload: MultiPayload, replace: bool) -> Option<[MultiPayload; 2]> {
        side_label(side)?;
        let mut file = self.file.lock().unwrap_or_else(PoisonError::into_inner);
        let arrived = file.next;
        file.next += 1;
        let entry = file.deals.entry(deal_id.to_owned()).or_insert_with(|| Entry { arrived, ..Entry::default() });
        let slot = if side == Action::Sell { &mut entry.sell } else { &mut entry.buy };
        if slot.is_some() && !replace {
            return None;
        }
        *slot = Some(payload);
        let pair = match (&entry.sell, &entry.buy) {
            (Some(sell), Some(buy)) if !entry.closed => Some([sell.clone(), buy.clone()]),
            _ => None,
        };
        while file.deals.len() > MAX_DEALS {
            let Some(oldest) = file.deals.iter().min_by_key(|(_, entry)| entry.arrived).map(|(id, _)| id.clone()) else {
                break;
            };
            file.deals.remove(&oldest);
        }
        self.persist(&file);
        pair
    }

    /// Takes a complete, unsubmitted pair for submission, or `None` if there is
    /// none or someone is already on it.
    fn claim(&self, deal_id: &str) -> Option<[MultiPayload; 2]> {
        let file = self.file.lock().unwrap_or_else(PoisonError::into_inner);
        let entry = file.deals.get(deal_id)?;
        if entry.closed {
            return None;
        }
        let pair = [entry.sell.clone()?, entry.buy.clone()?];
        self.in_flight.lock().unwrap_or_else(PoisonError::into_inner).insert(deal_id.to_owned()).then_some(pair)
    }

    fn release(&self, deal_id: &str) {
        self.in_flight.lock().unwrap_or_else(PoisonError::into_inner).remove(deal_id);
    }

    /// Nothing more is to be sent for this deal: it landed, or it never can.
    pub fn mark_submitted(&self, deal_id: &str) {
        {
            let mut file = self.file.lock().unwrap_or_else(PoisonError::into_inner);
            file.deals.entry(deal_id.to_owned()).or_default().closed = true;
            self.persist(&file);
        }
        self.release(deal_id);
    }

    /// Deals with both halves and nothing sent.
    #[must_use]
    pub fn held(&self) -> Vec<String> {
        let file = self.file.lock().unwrap_or_else(PoisonError::into_inner);
        file.deals
            .iter()
            .filter(|(_, entry)| !entry.closed && entry.sell.is_some() && entry.buy.is_some())
            .map(|(id, _)| id.clone())
            .collect()
    }

    /// One side's diff, for the hash a settlement report must log.
    #[must_use]
    pub fn half(&self, deal_id: &str, side: Action) -> Option<MultiPayload> {
        let file = self.file.lock().unwrap_or_else(PoisonError::into_inner);
        let entry = file.deals.get(deal_id)?;
        if side == Action::Sell { entry.sell.clone() } else { entry.buy.clone() }
    }

    fn first_note(&self, deal_id: &str) -> bool {
        self.noted.lock().unwrap_or_else(PoisonError::into_inner).insert(deal_id.to_owned())
    }
}

/// Where the last verifier salt is remembered.
#[must_use]
pub fn salt_store() -> cabal_store::JsonStore {
    cabal_store::JsonStore::new(crate::app_paths::in_data_dir("near_salt.json"))
}

/// The verifier salt a signature carries, remembered for signing offline.
#[must_use]
pub fn cached_salt(store: &cabal_store::JsonStore) -> Option<[u8; 4]> {
    let hex_salt: String = store.load().ok()?;
    hex::decode(hex_salt).ok()?.try_into().ok()
}

pub fn remember_salt(store: &cabal_store::JsonStore, salt: [u8; 4]) {
    if let Err(error) = store.save(&hex::encode(salt)) {
        tracing::warn!(target: "cabalmesh::settlement", %error, "could not persist the verifier salt");
    }
}

/// What the deal costs in quote, or `None` if it rounds to nothing or does not
/// fit. A zero leg would be a gift, not a trade.
#[must_use]
pub fn quote_for(near: &NearConfig, base_raw: u128, price: UsdPrice) -> Option<u128> {
    quote_amount(base_raw, near.base.decimals, price.cents(), near.quote.decimals).filter(|quote| *quote > 0)
}

/// Signs this side of a deal. Pure, so a device with no network can do it.
#[must_use]
#[allow(clippy::too_many_arguments)]
pub fn sign_half(
    keypair: &NearKeypair,
    verifier: &AccountId,
    near: &NearConfig,
    side: Action,
    base_raw: u128,
    quote_raw: u128,
    deadline_ms: u64,
    nonce: [u8; 32],
) -> MultiPayload {
    let (base, quote) = (near.base.token_id(), near.quote.token_id());
    let diff = if side == Action::Sell {
        seller_diff(&base, &quote, base_raw, quote_raw, &FeeRule::NONE)
    } else {
        buyer_diff(&base, &quote, base_raw, quote_raw, &FeeRule::NONE)
    };
    token_diff_payload(
        keypair,
        verifier,
        &diff,
        near.gateway_fee().as_ref(),
        deadline_ms,
        nonce,
    )
}

/// Signs `intent`'s side of its deal, holds it, and fixes the deal.
///
/// Returns what to flood. `None` if there is no deal, it prices to nothing in
/// the quote, or this device already signed it: two signatures for one side could
/// each settle against a different half, so a second tap produces nothing.
#[must_use]
#[allow(clippy::too_many_arguments)]
pub fn sign_own_half(
    ledger: &Ledger,
    book: &DiffBook,
    intent: &Intent,
    keypair: &NearKeypair,
    verifier: &AccountId,
    near: &NearConfig,
    salt: Option<[u8; 4]>,
    now: u64,
) -> Option<SignedDiff> {
    let terms = intent.deal.as_ref()?;
    let side = side_label(intent.draft.action)?;
    let base_raw = intent.draft.amount.raw();
    let quote_raw = quote_for(near, base_raw, terms.price)?;
    let deadline = deadline_from(now);

    // Marked and held before it can leave: whatever happens to the flood, this
    // device knows a signature it cannot recall exists.
    if !ledger.mark_signed(&intent.id, deadline) {
        return None;
    }
    let nonce = fresh_nonce(salt, deadline);
    let payload = sign_half(keypair, verifier, near, intent.draft.action, base_raw, quote_raw, deadline, nonce);
    book.hold_own(&terms.id, intent.draft.action, payload.clone());

    Some(SignedDiff { deal_id: terms.id.clone(), chain: intent.draft.chain, side: side.to_owned(), payload })
}

/// The nonce for a new signature. With no salt ever seen, 32 random bytes: the
/// verifier takes that as an unversioned nonce.
#[must_use]
pub fn fresh_nonce(salt: Option<[u8; 4]>, deadline_ms: u64) -> [u8; 32] {
    salt.map_or_else(rand::random, |salt| versioned_nonce(salt, deadline_ms, rand::random()))
}

/// When a signature made now stops being valid.
#[must_use]
pub const fn deadline_from(now: u64) -> u64 {
    now + INTENT_LIFETIME_MS
}

fn intent_of_deal(ledger: &Ledger, deal: &str) -> Option<Intent> {
    ledger.all().into_iter().find(|intent| intent.deal.as_ref().is_some_and(|d| d.id == deal))
}

/// Holds a diff from the mesh, if it is genuine and for this network. Returns
/// whether it was held.
pub fn receive_diff(ledger: &Ledger, book: &DiffBook, chain: Chain, verifier: &str, details: &str) -> bool {
    let Ok(message) = serde_json::from_str::<SignedDiff>(details) else {
        return false;
    };
    let side = match message.side.as_str() {
        "sell" => Action::Sell,
        "buy" => Action::Buy,
        _ => return false,
    };
    if !message.chain.matches(chain) || message.payload.payload.recipient != verifier {
        return false;
    }
    if !verify(&message.payload) {
        tracing::warn!(target: "cabalmesh::settlement", deal = %message.deal_id, "dropped a diff with a bad signature");
        return false;
    }

    // The signature is the only trustworthy source of who the other side is.
    if let (Some(own), Ok(signer)) = (intent_of_deal(ledger, &message.deal_id), message.payload.signer_id()) {
        if own.draft.action != side && own.counterparty.is_none() {
            ledger.set_counterparty(&own.id, signer);
            ledger.record(&own.id, line("COUNTERPARTY SIGNED.", LogTone::Ok));
        }
    }

    book.insert(&message.deal_id, side, message.payload);
    true
}

/// Finishes this device's intent for a deal, if it has one.
fn mark_settled(ledger: &Ledger, deal: &str, tx: &str) {
    let Some(intent) = intent_of_deal(ledger, deal) else {
        return;
    };
    let Some(terms) = &intent.deal else {
        return;
    };
    let elapsed_ms = u32::try_from(intent.elapsed_ms(now_ms())).unwrap_or(u32::MAX);
    let settled = IntentStatus::Settled {
        proof: ProofHash::new(tx),
        filled_at: terms.price,
        elapsed_ms,
    };
    if ledger.advance(&intent.id, settled, now_ms()).is_ok() {
        ledger.record(&intent.id, line(format!("PROOF {tx}"), LogTone::Loud));
    }
}

/// Fails this device's intent for a deal, if it has one.
fn mark_failed(ledger: &Ledger, deal: &str, error: &NearError) {
    let Some(intent) = intent_of_deal(ledger, deal) else {
        return;
    };
    if error.is_invalid_salt() {
        ledger.record(&intent.id, line("SALT ROTATED, SIGN AGAIN.", LogTone::Err));
    } else {
        ledger.record(&intent.id, line("VERIFIER REJECTED THE PAIR.", LogTone::Err));
    }
    ledger.record(&intent.id, line("FUNDS REMAIN IN INTENTS BALANCE.", LogTone::Dim));
    let _ = ledger.advance(&intent.id, IntentStatus::Failed { reason: FailureReason::SettlementRejected }, now_ms());
}

/// A pair that reached the chain, for flooding.
pub type Submitted = SettlementComplete;

/// Submits every held pair. What cannot go out now stays held.
pub async fn submit_held(ledger: &Ledger, book: &DiffBook, bridge: &NearBridge) -> Vec<Submitted> {
    let mut submitted = Vec::new();
    for deal in book.held() {
        let Some(pair) = book.claim(&deal) else {
            continue;
        };
        match bridge.submit_pair(&pair).await {
            Ok(TxHash(tx)) => {
                book.mark_submitted(&deal);
                mark_settled(ledger, &deal, &tx);
                submitted.push(SettlementComplete {
                    deal_id: deal,
                    tx_hash: tx,
                    gateway: bridge.account_id().to_string(),
                });
            }
            Err(error) if error.is_already_settled() => {
                // Someone else got there first; their report carries the hash.
                book.mark_submitted(&deal);
                if let Some(intent) = intent_of_deal(ledger, &deal) {
                    ledger.record(&intent.id, line("ANOTHER GATEWAY SUBMITTED FIRST.", LogTone::Dim));
                }
            }
            Err(NearError::AccountNotFunded { .. }) => {
                book.release(&deal);
                if book.first_note(&deal) {
                    if let Some(intent) = intent_of_deal(ledger, &deal) {
                        ledger.record(&intent.id, line("NO NEAR FOR GAS. HELD FOR A GATEWAY.", LogTone::Dim));
                    }
                }
            }
            Err(error @ NearError::Rejected(_)) => {
                tracing::warn!(target: "cabalmesh::settlement", %deal, %error, "verifier rejected a pair");
                book.mark_submitted(&deal);
                mark_failed(ledger, &deal, &error);
            }
            Err(error) => {
                tracing::debug!(target: "cabalmesh::settlement", %deal, %error, "could not submit; holding");
                book.release(&deal);
            }
        }
    }
    submitted
}

/// Takes a report that a deal settled. Stored until it is checked against the
/// chain, so a device with no route keeps its claim for later.
pub fn receive_report(ledger: &Ledger, details: &str) -> Option<IntentId> {
    let report = serde_json::from_str::<SettlementComplete>(details).ok()?;
    let intent = intent_of_deal(ledger, &report.deal_id)?;
    if !matches!(intent.status, IntentStatus::Waiting | IntentStatus::FindingRoute) {
        return None;
    }
    ledger.set_reported_tx(&intent.id, report.tx_hash, report.gateway);
    Some(intent.id)
}

/// Asks the chain how every signed deal ended: by the report if one arrived,
/// otherwise by whether this device's signature has been spent. A device offline
/// when the gateway flooded its report would never hear it again.
pub async fn confirm_reports(ledger: &Ledger, book: &DiffBook, bridge: &NearBridge) {
    for intent in ledger.all() {
        let Some(terms) = &intent.deal else {
            continue;
        };
        // Only once signed: before that, the slot may hold a peer's forgery.
        if terms.signed_until_ms.is_none() || !intent.status.is_active() {
            continue;
        }
        let Some(own) = book.half(&terms.id, intent.draft.action) else {
            continue;
        };
        let Ok(hash) = own.hash() else {
            continue;
        };
        let needle = bs58::encode(hash).into_string();
        let Some(reported) = &terms.reported else {
            let Ok(signer) = own.signer_id() else {
                continue;
            };
            match bridge.nonce_used(&signer, &own.payload.nonce).await {
                Ok(true) => {
                    book.mark_submitted(&terms.id);
                    ledger.record(&intent.id, line("ANOTHER GATEWAY SETTLED THIS. PROOF IS YOUR INTENT HASH.", LogTone::Dim));
                    mark_settled(ledger, &terms.id, &needle);
                }
                Ok(false) => {}
                Err(error) => tracing::debug!(target: "cabalmesh::settlement", %error, "could not check a nonce"),
            }
            continue;
        };
        match bridge.tx_logged(&reported.tx_hash, &reported.gateway, &needle).await {
            Ok(true) => {
                book.mark_submitted(&terms.id);
                mark_settled(ledger, &terms.id, &reported.tx_hash);
            }
            Ok(false) => {
                tracing::warn!(target: "cabalmesh::settlement", deal = %terms.id, "a settlement report did not check out");
                ledger.clear_reported_tx(&intent.id);
            }
            // Keep it: no route, or the node has not seen the transaction yet.
            Err(error) => tracing::debug!(target: "cabalmesh::settlement", %error, "could not check a report"),
        }
    }
}

/// Floods a settlement message over whichever plane reaches someone.
pub async fn flood(services: &crate::state::Services, kind: &str, body: String) {
    let intent = crate::mesh::PrivacyIntent {
        id: format!("{kind}-{}-{}", now_ms(), rand::random::<u32>()),
        intent_type: "settlement".into(),
        payload: body,
        encrypted: false,
        relay_path: vec!["origin_node".into()],
        relay_fee: None,
    };
    if crate::commands::publish_over_mesh(services, &intent).await.is_err() {
        let _ = crate::commands::publish_over_ble(services, &intent).await;
    }
}

/// Everything a retry needs, or `None` if this device cannot act as a gateway
/// now: the vault is locked, or the user switched to offline mode, which
/// promises nothing leaves the device.
pub(crate) async fn gateway(state: &AppState) -> Option<(crate::state::Services, NearBridge)> {
    let services = state.services().ok()?;
    if let Some(mesh) = &services.mesh {
        if mesh.snapshot().await.ok()?.offline {
            return None;
        }
    }
    let bridge = services.near().await.ok()?;
    Some((services, bridge))
}

/// Submits whatever is held and checks whatever was reported.
pub async fn retry(state: &AppState) {
    let Some((services, bridge)) = gateway(state).await else {
        return;
    };
    let book = state.diff_book();
    for report in submit_held(state.intents(), book, &bridge).await {
        flood(&services, "settled", tagged("SettlementComplete", &report).to_string()).await;
    }
    confirm_reports(state.intents(), book, &bridge).await;
    for message in halves_to_resend(state.intents(), book, now_ms()) {
        flood(&services, "diff", tagged("SignedDiff", &message).to_string()).await;
    }
}

/// This device's signed halves whose other half has not arrived.
///
/// A flood reaches whoever is listening at that moment, and the peer that needs
/// this one may have been out of range. Nothing else would ever send it again.
fn halves_to_resend(ledger: &Ledger, book: &DiffBook, now: u64) -> Vec<SignedDiff> {
    ledger
        .all()
        .into_iter()
        .filter(|intent| intent.status.is_active())
        .filter_map(|intent| {
            let terms = intent.deal.as_ref()?;
            let side = side_label(intent.draft.action)?;
            if terms.signed_until_ms? <= now {
                return None;
            }
            let other = if intent.draft.action == Action::Sell { Action::Buy } else { Action::Sell };
            if book.half(&terms.id, other).is_some() {
                return None;
            }
            Some(SignedDiff {
                deal_id: terms.id.clone(),
                chain: intent.draft.chain,
                side: side.to_owned(),
                payload: book.half(&terms.id, intent.draft.action)?,
            })
        })
        .collect()
}

/// Acts on a settlement message from either plane. Applied in the background:
/// this talks to the chain, and the mesh loop must not wait on it.
pub fn dispatch(state: &AppState, event: &crate::mesh::MeshEvent) {
    use crate::mesh::MeshEvent;
    let state = state.clone();
    match event {
        MeshEvent::SignedDiff { details } => {
            let details = details.clone();
            tokio::spawn(async move { on_signed_diff(&state, &details).await });
        }
        MeshEvent::SettlementComplete { details } => {
            let details = details.clone();
            tokio::spawn(async move { on_settlement_complete(&state, &details).await });
        }
        _ => {}
    }
}

/// A message body with its `type`, which is how the mesh tells them apart.
pub fn tagged(kind: &str, message: &impl Serialize) -> serde_json::Value {
    let mut value = serde_json::to_value(message).unwrap_or_default();
    if let Some(fields) = value.as_object_mut() {
        fields.insert("type".into(), kind.into());
    }
    value
}

/// A `SignedDiff` arrived from the mesh.
pub async fn on_signed_diff(state: &AppState, details: &str) {
    let config = crate::network_config::NetworkConfig::current();
    if receive_diff(state.intents(), state.diff_book(), config.network.chain(), &config.verifier(), details) {
        retry(state).await;
    }
}

/// A `SettlementComplete` arrived from the mesh.
pub async fn on_settlement_complete(state: &AppState, details: &str) {
    if receive_report(state.intents(), details).is_some() {
        retry(state).await;
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::near_bridge::testing::{healthy, result, MockRpc};
    use cabal_core::{Condition, ExecutionMode, IntentDraft, PrivacyLevel, TokenAmount};
    use serde_json::json;
    use std::sync::atomic::{AtomicU32, Ordering};

    const WNEAR: u128 = 1_000_000_000_000_000_000_000_000;
    const VERIFIER: &str = "verifier.tidepool-sandbox.testnet";

    fn near() -> NearConfig {
        crate::network_config::Network::Testnet.near()
    }

    fn half(seed: u8, side: Action) -> MultiPayload {
        sign_half(
            &NearKeypair::from_secret_bytes(&[seed; 32]),
            &AccountId::try_from(VERIFIER).unwrap(),
            &near(),
            side,
            WNEAR,
            4_040_000,
            4_102_444_800_000,
            [seed; 32],
        )
    }

    struct Fixture {
        ledger: Ledger,
        book: DiffBook,
        _dir: tempfile::TempDir,
    }

    fn fixture() -> Fixture {
        let dir = tempfile::tempdir().unwrap();
        Fixture {
            ledger: Ledger::open(cabal_store::JsonStore::new(dir.path().join("intents.json"))),
            book: DiffBook::open(cabal_store::JsonStore::new(dir.path().join("diffs.json"))),
            _dir: dir,
        }
    }

    fn bridge(url: &str) -> NearBridge {
        NearBridge::new(
            url.to_owned(),
            VERIFIER,
            NearKeypair::from_secret_bytes(&[9; 32]),
            reqwest::Client::new(),
        )
        .unwrap()
    }

    /// An intent that signed its half of `deal`, waiting for the other.
    fn waiting_intent(fx: &Fixture, deal: &str, side: Action) -> IntentId {
        let draft = IntentDraft {
            chain: Chain::NearTestnet,
            action: side,
            asset: "wNEAR".into(),
            condition: Condition::Any,
            amount: TokenAmount::from_raw(WNEAR, 24),
            mode: ExecutionMode::Shark,
            privacy: PrivacyLevel::Low,
        };
        let intent = fx.ledger.create(draft, now_ms());
        fx.ledger.advance(&intent.id, IntentStatus::Broadcast { route_len: 1 }, now_ms()).unwrap();
        fx.ledger.propose_deal(&intent.id, deal, UsdPrice::from_cents(404), "target");
        fx.ledger.mark_signed(&intent.id, deadline_from(now_ms()));
        fx.ledger.advance(&intent.id, IntentStatus::Waiting, now_ms()).unwrap();
        intent.id
    }

    fn diff_message(deal: &str, side: &str, payload: MultiPayload) -> String {
        serde_json::to_string(&SignedDiff {
            deal_id: deal.into(),
            chain: Chain::NearTestnet,
            side: side.into(),
            payload,
        })
        .unwrap()
    }

    #[test]
    fn a_half_signs_the_fee_only_when_the_network_pays_gateways() {
        let keypair = NearKeypair::from_secret_bytes(&[4; 32]);
        let verifier = AccountId::try_from(VERIFIER).unwrap();
        let paying = NearConfig {
            rewards: Some("rewards.test"),
            fee_per_side: 10_000,
            ..near()
        };
        let fee = paying.gateway_fee().unwrap();

        let half = sign_half(
            &keypair,
            &verifier,
            &paying,
            Action::Sell,
            1,
            1,
            1_000,
            [0; 32],
        );
        assert!(fee.paid_by(&half));
        let half = sign_half(
            &keypair,
            &verifier,
            &near(),
            Action::Sell,
            1,
            1,
            1_000,
            [0; 32],
        );
        assert!(!fee.paid_by(&half));
    }

    #[test]
    fn deal_ids_are_the_same_from_both_ends() {
        assert_eq!(deal_id("B", "A"), deal_id("A", "B"));
    }

    #[test]
    fn diff_book_returns_pair_once() {
        let fx = fixture();
        assert!(fx.book.insert("d", Action::Sell, half(1, Action::Sell)).is_none());
        let [sell, buy] = fx.book.insert("d", Action::Buy, half(2, Action::Buy)).unwrap();
        assert_eq!(sell, half(1, Action::Sell));
        assert_eq!(buy, half(2, Action::Buy));
        assert!(fx.book.insert("d", Action::Buy, half(2, Action::Buy)).is_none());
    }

    #[test]
    fn diff_book_refuses_a_side_that_is_not_a_trade() {
        let fx = fixture();
        assert!(fx.book.insert("d", Action::Swap, half(1, Action::Sell)).is_none());
        assert!(fx.book.held().is_empty());
    }

    #[test]
    fn diff_book_is_bounded() {
        let fx = fixture();
        for i in 0..(MAX_DEALS + 8) {
            fx.book.insert(&format!("d{i}"), Action::Sell, half(1, Action::Sell));
        }
        assert_eq!(fx.book.file.lock().unwrap().deals.len(), MAX_DEALS);
        assert!(!fx.book.file.lock().unwrap().deals.contains_key("d0"), "the oldest goes first");
    }

    #[test]
    fn held_pairs_survive_a_restart() {
        let dir = tempfile::tempdir().unwrap();
        let open = || DiffBook::open(cabal_store::JsonStore::new(dir.path().join("diffs.json")));
        let book = open();
        book.insert("d", Action::Sell, half(1, Action::Sell));
        book.insert("d", Action::Buy, half(2, Action::Buy));
        assert_eq!(open().held(), vec!["d".to_owned()]);
    }

    #[test]
    fn diff_book_rejects_bad_signature() {
        let fx = fixture();
        let mut forged = half(1, Action::Sell);
        forged.payload.message = forged.payload.message.replace("-1000000000000000000000000", "-1");
        let held = receive_diff(&fx.ledger, &fx.book, Chain::NearTestnet, VERIFIER, &diff_message("d", "sell", forged));
        assert!(!held);
        assert!(fx.book.half("d", Action::Sell).is_none());
    }

    #[test]
    fn a_diff_for_another_chain_or_verifier_is_not_held() {
        let fx = fixture();
        let message = diff_message("d", "sell", half(1, Action::Sell));
        assert!(!receive_diff(&fx.ledger, &fx.book, Chain::NearMainnet, VERIFIER, &message));
        assert!(!receive_diff(&fx.ledger, &fx.book, Chain::NearTestnet, "other.testnet", &message));
        assert!(receive_diff(&fx.ledger, &fx.book, Chain::NearTestnet, VERIFIER, &message));
    }

    #[test]
    fn a_genuine_diff_names_the_counterparty() {
        let fx = fixture();
        let id = waiting_intent(&fx, "d", Action::Buy);
        let sell = half(1, Action::Sell);
        let signer = sell.signer_id().unwrap();
        assert!(receive_diff(&fx.ledger, &fx.book, Chain::NearTestnet, VERIFIER, &diff_message("d", "sell", sell)));
        assert_eq!(fx.ledger.get(&id).unwrap().counterparty.as_deref(), Some(signer.as_str()));
    }

    #[tokio::test]
    async fn offline_device_holds_pair_until_online() {
        let fx = fixture();
        let id = waiting_intent(&fx, "d", Action::Sell);
        fx.book.insert("d", Action::Sell, half(1, Action::Sell));
        fx.book.insert("d", Action::Buy, half(2, Action::Buy));

        // Nothing is listening on this port: no route to the chain.
        let unreachable = bridge("http://127.0.0.1:9");
        assert!(submit_held(&fx.ledger, &fx.book, &unreachable).await.is_empty());
        assert_eq!(fx.book.held(), vec!["d".to_owned()], "still held");
        assert!(matches!(fx.ledger.get(&id).unwrap().status, IntentStatus::Waiting));

        let rpc = MockRpc::serve(|method, params| {
            healthy(method, params, |view| (view == "simulate_intents").then(|| json!({})))
        })
        .await;
        let online = bridge(&rpc.url);
        let submitted = submit_held(&fx.ledger, &fx.book, &online).await;
        assert_eq!(submitted.len(), 1);
        assert_eq!(submitted[0].tx_hash, "TXHASH");

        // Coming online twice must not pay for the same deal twice.
        assert!(submit_held(&fx.ledger, &fx.book, &online).await.is_empty());
        assert_eq!(rpc.order().iter().filter(|m| *m == "send_tx").count(), 1);

        match fx.ledger.get(&id).unwrap().status {
            IntentStatus::Settled { proof, filled_at, .. } => {
                assert_eq!(proof, ProofHash::new("TXHASH"));
                assert_eq!(filled_at, UsdPrice::from_cents(404));
            }
            other => panic!("expected Settled, got {other:?}"),
        }
    }

    #[tokio::test]
    async fn a_simultaneous_retry_does_not_submit_twice() {
        let fx = fixture();
        fx.book.insert("d", Action::Sell, half(1, Action::Sell));
        fx.book.insert("d", Action::Buy, half(2, Action::Buy));
        let rpc = MockRpc::serve(|method, params| {
            healthy(method, params, |view| (view == "simulate_intents").then(|| json!({})))
        })
        .await;
        let gateway = bridge(&rpc.url);
        let (a, b) = tokio::join!(
            submit_held(&fx.ledger, &fx.book, &gateway),
            submit_held(&fx.ledger, &fx.book, &gateway)
        );
        assert_eq!(a.len() + b.len(), 1);
        assert_eq!(rpc.order().iter().filter(|m| *m == "send_tx").count(), 1);
    }

    #[tokio::test]
    async fn submit_failure_marks_intent_failed() {
        let fx = fixture();
        let id = waiting_intent(&fx, "d", Action::Sell);
        fx.book.insert("d", Action::Sell, half(1, Action::Sell));
        fx.book.insert("d", Action::Buy, half(2, Action::Buy));

        let rpc = MockRpc::serve(|method, params| {
            if params["method_name"] == "simulate_intents" {
                result(json!({ "error": "Smart contract panicked: insufficient balance", "logs": [] }))
            } else {
                healthy(method, params, |_| None)
            }
        })
        .await;
        assert!(submit_held(&fx.ledger, &fx.book, &bridge(&rpc.url)).await.is_empty());

        let intent = fx.ledger.get(&id).unwrap();
        assert!(matches!(intent.status, IntentStatus::Failed { reason: FailureReason::SettlementRejected }));
        assert!(intent.log.iter().any(|l| l.text.contains("FUNDS REMAIN IN INTENTS BALANCE")));
        assert!(fx.book.held().is_empty(), "a rejected pair is not retried");
    }

    #[tokio::test]
    async fn a_rotated_salt_says_to_sign_again() {
        let fx = fixture();
        let id = waiting_intent(&fx, "d", Action::Sell);
        fx.book.insert("d", Action::Sell, half(1, Action::Sell));
        fx.book.insert("d", Action::Buy, half(2, Action::Buy));

        let rpc = MockRpc::serve(|method, params| {
            if params["method_name"] == "simulate_intents" {
                result(json!({ "error": "Smart contract panicked: invalid salt", "logs": [] }))
            } else {
                healthy(method, params, |_| None)
            }
        })
        .await;
        submit_held(&fx.ledger, &fx.book, &bridge(&rpc.url)).await;

        let intent = fx.ledger.get(&id).unwrap();
        assert!(matches!(intent.status, IntentStatus::Failed { .. }));
        assert!(intent.log.iter().any(|l| l.text.contains("SALT ROTATED, SIGN AGAIN")));
    }

    #[tokio::test]
    async fn an_unfunded_gateway_holds_the_pair_and_says_so_once() {
        let fx = fixture();
        let id = waiting_intent(&fx, "d", Action::Sell);
        fx.book.insert("d", Action::Sell, half(1, Action::Sell));
        fx.book.insert("d", Action::Buy, half(2, Action::Buy));

        let rpc = MockRpc::serve(|method, params| {
            if params["request_type"] == "view_access_key" {
                result(json!({ "error": "access key ed25519:x does not exist while viewing" }))
            } else {
                healthy(method, params, |view| (view == "simulate_intents").then(|| json!({})))
            }
        })
        .await;
        let gateway = bridge(&rpc.url);
        submit_held(&fx.ledger, &fx.book, &gateway).await;
        submit_held(&fx.ledger, &fx.book, &gateway).await;

        let intent = fx.ledger.get(&id).unwrap();
        assert!(matches!(intent.status, IntentStatus::Waiting), "not this deal's failure");
        assert_eq!(intent.log.iter().filter(|l| l.text.contains("NO NEAR FOR GAS")).count(), 1);
        assert_eq!(fx.book.held(), vec!["d".to_owned()]);
    }

    #[tokio::test]
    async fn another_gateway_winning_is_not_a_failure() {
        let fx = fixture();
        let id = waiting_intent(&fx, "d", Action::Sell);
        fx.book.insert("d", Action::Sell, half(1, Action::Sell));
        fx.book.insert("d", Action::Buy, half(2, Action::Buy));

        let rpc = MockRpc::serve(|method, params| {
            if params["method_name"] == "simulate_intents" {
                result(json!({ "error": "Smart contract panicked: nonce was already used", "logs": [] }))
            } else {
                healthy(method, params, |_| None)
            }
        })
        .await;
        submit_held(&fx.ledger, &fx.book, &bridge(&rpc.url)).await;

        assert!(matches!(fx.ledger.get(&id).unwrap().status, IntentStatus::Waiting));
        assert!(fx.book.held().is_empty());
    }

    #[tokio::test]
    async fn a_settlement_report_is_only_believed_once_the_chain_agrees() {
        let fx = fixture();
        let id = waiting_intent(&fx, "d", Action::Sell);
        let own = half(1, Action::Sell);
        let needle = bs58::encode(own.hash().unwrap()).into_string();
        fx.book.insert("d", Action::Sell, own);

        let report = |hash: &str| {
            serde_json::to_string(&SettlementComplete {
                deal_id: "d".into(),
                tx_hash: hash.into(),
                gateway: "gw.testnet".into(),
            })
            .unwrap()
        };
        let calls = Arc::new(AtomicU32::new(0));
        let counted = calls.clone();
        let rpc = MockRpc::serve(move |_, params| {
            counted.fetch_add(1, Ordering::Relaxed);
            let logs = if params["tx_hash"] == "REAL" { vec![needle.clone()] } else { vec![] };
            result(json!({
                "status": { "SuccessValue": "" },
                "transaction": { "receiver_id": VERIFIER },
                "receipts_outcome": [{
                    "outcome": { "executor_id": VERIFIER, "status": { "SuccessValue": "" }, "logs": logs },
                }],
            }))
        })
        .await;
        let gateway = bridge(&rpc.url);

        // A forged report changes nothing, and is dropped once checked.
        receive_report(&fx.ledger, &report("FORGED")).unwrap();
        assert!(matches!(fx.ledger.get(&id).unwrap().status, IntentStatus::Waiting));
        confirm_reports(&fx.ledger, &fx.book, &gateway).await;
        assert!(matches!(fx.ledger.get(&id).unwrap().status, IntentStatus::Waiting));
        assert!(fx.ledger.get(&id).unwrap().deal.unwrap().reported.is_none());

        // No route: kept for later.
        receive_report(&fx.ledger, &report("REAL")).unwrap();
        confirm_reports(&fx.ledger, &fx.book, &bridge("http://127.0.0.1:9")).await;
        assert!(fx.ledger.get(&id).unwrap().deal.unwrap().reported.is_some());

        confirm_reports(&fx.ledger, &fx.book, &gateway).await;
        assert!(matches!(fx.ledger.get(&id).unwrap().status, IntentStatus::Settled { .. }));
    }

    #[test]
    fn a_report_about_an_unsigned_intent_is_ignored() {
        let fx = fixture();
        let draft = IntentDraft {
            chain: Chain::NearTestnet,
            action: Action::Sell,
            asset: "wNEAR".into(),
            condition: Condition::Any,
            amount: TokenAmount::from_raw(WNEAR, 24),
            mode: ExecutionMode::Shark,
            privacy: PrivacyLevel::Low,
        };
        let intent = fx.ledger.create(draft, now_ms());
        fx.ledger.advance(&intent.id, IntentStatus::Broadcast { route_len: 1 }, now_ms()).unwrap();
        fx.ledger.propose_deal(&intent.id, "d", UsdPrice::from_cents(404), "target");
        let report = serde_json::to_string(&SettlementComplete {
            deal_id: "d".into(),
            tx_hash: "X".into(),
            gateway: "g".into(),
        })
        .unwrap();
        assert!(receive_report(&fx.ledger, &report).is_none());
    }

    #[test]
    fn an_unanswered_half_is_sent_again_until_it_expires() {
        let fx = fixture();
        waiting_intent(&fx, "d", Action::Sell);
        fx.book.insert("d", Action::Sell, half(1, Action::Sell));
        let now = now_ms();

        let resend = halves_to_resend(&fx.ledger, &fx.book, now);
        assert_eq!(resend.len(), 1);
        assert_eq!(resend[0].side, "sell");
        assert!(halves_to_resend(&fx.ledger, &fx.book, deadline_from(now) + 1_000).is_empty(), "expired");

        fx.book.insert("d", Action::Buy, half(2, Action::Buy));
        assert!(halves_to_resend(&fx.ledger, &fx.book, now).is_empty(), "the other half arrived");
    }

    #[test]
    fn signing_needs_no_network_and_both_halves_net_to_zero() {
        let quote = quote_for(&near(), WNEAR, UsdPrice::from_cents(404)).unwrap();
        assert_eq!(quote, 4_040_000);
        let sell = half(1, Action::Sell);
        let buy = half(2, Action::Buy);
        assert!(verify(&sell) && verify(&buy));
        assert!(sell.payload.message.contains(r#""nep141:wnear.tidepool-sandbox.testnet":"-1000000000000000000000000""#));
        assert!(buy.payload.message.contains(r#""nep141:wnear.tidepool-sandbox.testnet":"1000000000000000000000000""#));
    }

    /// An intent with an agreed deal, not signed yet.
    fn dealt_intent(fx: &Fixture, deal: &str, side: Action) -> Intent {
        let draft = IntentDraft {
            chain: Chain::NearTestnet,
            action: side,
            asset: "wNEAR".into(),
            condition: Condition::Any,
            amount: TokenAmount::from_raw(WNEAR, 24),
            mode: ExecutionMode::Shark,
            privacy: PrivacyLevel::Low,
        };
        let intent = fx.ledger.create(draft, now_ms());
        fx.ledger.propose_deal(&intent.id, deal, UsdPrice::from_cents(404), "target");
        fx.ledger.get(&intent.id).unwrap()
    }

    fn sign(fx: &Fixture, intent: &Intent) -> Option<SignedDiff> {
        let verifier = AccountId::try_from(VERIFIER).unwrap();
        let keypair = NearKeypair::from_secret_bytes(&[1; 32]);
        sign_own_half(&fx.ledger, &fx.book, intent, &keypair, &verifier, &near(), Some([1, 2, 3, 4]), now_ms())
    }

    /// The salt read can take seconds with no network, and the button is still
    /// there. Two signatures for one side could each settle against a different
    /// half, so the second tap must produce nothing.
    #[test]
    fn a_second_tap_cannot_sign_a_second_time() {
        let fx = fixture();
        let intent = dealt_intent(&fx, "d", Action::Sell);
        let first = sign(&fx, &intent).expect("the first tap signs");
        assert!(sign(&fx, &intent).is_none(), "both taps read the intent before either signed");
        assert_eq!(fx.book.half("d", Action::Sell), Some(first.payload));
    }

    /// Deal ids are public, so a peer can fill this device's own slot first.
    /// What this device signs is what it must hold and flood.
    #[test]
    fn an_own_signature_replaces_a_forgery_that_arrived_first() {
        let fx = fixture();
        let intent = dealt_intent(&fx, "d", Action::Sell);
        fx.book.insert("d", Action::Sell, half(3, Action::Sell));
        let signed = sign(&fx, &intent).unwrap();
        assert_eq!(fx.book.half("d", Action::Sell), Some(signed.payload));
    }

    fn nonce_rpc(used: bool) -> impl Fn(&str, &serde_json::Value) -> serde_json::Value + Send + Sync + 'static {
        move |method, params| healthy(method, params, |name| (name == "is_nonce_used").then(|| json!(used)))
    }

    /// A device offline when the gateway flooded its report never hears it. The
    /// chain still knows its signature was spent.
    #[tokio::test]
    async fn a_lost_report_settles_once_the_chain_shows_the_nonce_was_used() {
        let fx = fixture();
        let id = waiting_intent(&fx, "d", Action::Sell);
        let own = half(1, Action::Sell);
        let hash = bs58::encode(own.hash().unwrap()).into_string();
        fx.book.insert("d", Action::Sell, own);

        let unused = MockRpc::serve(nonce_rpc(false)).await;
        confirm_reports(&fx.ledger, &fx.book, &bridge(&unused.url)).await;
        assert!(matches!(fx.ledger.get(&id).unwrap().status, IntentStatus::Waiting));

        let used = MockRpc::serve(nonce_rpc(true)).await;
        confirm_reports(&fx.ledger, &fx.book, &bridge(&used.url)).await;
        let IntentStatus::Settled { proof, .. } = fx.ledger.get(&id).unwrap().status else {
            panic!("the signature was spent, so the deal settled");
        };
        assert_eq!(proof.to_string(), hash, "no tx hash reached this device, so the proof is its intent hash");
        assert!(fx.book.held().is_empty());
    }

    #[tokio::test]
    async fn a_half_this_device_never_signed_proves_nothing_about_its_deal() {
        let fx = fixture();
        let intent = dealt_intent(&fx, "d", Action::Sell);
        fx.ledger.advance(&intent.id, IntentStatus::Broadcast { route_len: 1 }, now_ms()).unwrap();
        fx.book.insert("d", Action::Sell, half(3, Action::Sell));

        let used = MockRpc::serve(nonce_rpc(true)).await;
        confirm_reports(&fx.ledger, &fx.book, &bridge(&used.url)).await;
        assert!(matches!(fx.ledger.get(&intent.id).unwrap().status, IntentStatus::Broadcast { .. }));
    }

    #[test]
    fn a_price_too_small_to_quote_is_refused() {
        assert_eq!(quote_for(&near(), 1, UsdPrice::from_cents(1)), None);
    }

    #[test]
    fn fresh_nonces_carry_the_salt_when_there_is_one() {
        let nonce = fresh_nonce(Some([0xfc, 0x4d, 0x10, 0xf1]), 1_000);
        assert_eq!(&nonce[5..9], &[0xfc, 0x4d, 0x10, 0xf1]);
        assert_ne!(fresh_nonce(None, 1_000), fresh_nonce(None, 1_000));
    }
}
