//! The intent ledger — every intent this device has composed, and where it is.
//!
//! # Why the log lives here rather than in the streaming task
//!
//! Ticket 34's load-bearing rule is that **cancelling the settlement log must
//! not abort the settlement.** That is a correctness property with money
//! attached, and the only way to hold it reliably is structurally: the
//! settlement task writes into the ledger, and a subscription is a *reader*
//! that happens to be attached at the time. Dropping every reader changes
//! nothing about the writer.
//!
//! So lines are appended to the intent itself and fanned out to whoever is
//! listening, rather than being pushed straight down a channel. That also means
//! a subscriber arriving late replays what it missed instead of joining a
//! stream mid-sentence, which is what navigating back to a settling intent
//! does.
//!
//! # Why it persists
//!
//! An intent queued while offline has to survive the app being killed — that is
//! the entire promise of queue-then-drain. Writes go through [`cabal_store`],
//! which is crash-safe, after every mutation rather than on a timer: the
//! mutations are rare and small, and a timer would mean the one crash that
//! matters is the one that loses the queue.
//!
//! # Transitions are checked, not trusted
//!
//! [`IntentStatus::can_transition_to`] owns the rules. [`Ledger::advance`]
//! refuses anything it rejects rather than overwriting, so no caller can settle
//! something that was never broadcast by writing the field directly.

use crate::bindings::{LogLine, LogTone};
use crate::error::AppError;
use cabal_core::{IntentDraft, IntentId, IntentStatus, NodeId, UsdPrice};
use serde::{Deserialize, Serialize};
use std::sync::{Arc, Mutex, PoisonError};
use tokio::sync::broadcast;

/// How many lines a slow subscriber may fall behind before it starts missing
/// them.
///
/// Overflow is survivable rather than fatal: the retained log on the intent is
/// the source of truth, so a subscriber that lags has still missed only its
/// live view, not the record.
const FANOUT_CAPACITY: usize = 256;

/// A report that a deal settled, not yet checked against the chain.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ReportedTx {
    pub tx_hash: String,
    /// The transaction's sender, which the RPC needs to look it up.
    pub gateway: String,
}

/// What this intent agreed with another. The amount is the draft's: a deal only
/// forms between intents of equal size.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Deal {
    pub id: String,
    pub price: UsdPrice,
    /// The other intent. Both sides reach a deal with the same price through
    /// `negotiation`, so this no longer decides anything, but ledgers already on disk
    /// carry it.
    pub proposal_target: String,
    /// Set at signing. A signature cannot be recalled, so until this passes the
    /// other side or any gateway can still settle it.
    pub signed_until_ms: Option<u64>,
    pub reported: Option<ReportedTx>,
}

/// What a proposal did to an intent's deal.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum DealChange {
    New,
    Repriced,
    /// Another deal holds this intent, or it has already signed.
    Ignored,
}

/// A composed intent and everything observed about it since.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Intent {
    pub id: IntentId,
    pub draft: IntentDraft,
    pub status: IntentStatus,
    /// Unix milliseconds when it was composed.
    pub created_ms: u64,
    /// Unix milliseconds when it reached a terminal state.
    pub finished_ms: Option<u64>,
    /// Peers the intent actually travelled through. Empty until a route is
    /// found — never padded, because the proof screen renders it as fact.
    pub route: Vec<NodeId>,
    /// Verification lines, retained so a subscriber arriving late replays
    /// rather than joining mid-stream.
    pub log: Vec<LogLine>,
    #[serde(default)]
    pub deal: Option<Deal>,
    /// The other side's NEAR account, for display. Learned from its signature, the
    /// only trustworthy source. A `token_diff` names no counterparty, so settling
    /// does not depend on this.
    pub counterparty: Option<String>,
}

impl Intent {
    /// Milliseconds from composition to now, or to the terminal state if it
    /// reached one.
    #[must_use]
    pub fn elapsed_ms(&self, now_ms: u64) -> u64 {
        self.finished_ms.unwrap_or(now_ms).saturating_sub(self.created_ms)
    }
}

/// What the ledger keeps on disk.
///
/// A struct rather than a bare `Vec` so a later field — a schema version, a
/// cursor — can be added without the file becoming unreadable.
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
struct Persisted {
    #[serde(default)]
    intents: Vec<Intent>,
}

/// Every intent this device has composed.
///
/// Cheap to clone; every clone shares one ledger.
#[derive(Clone)]
pub struct Ledger {
    inner: Arc<Mutex<Vec<Intent>>>,
    store: cabal_store::Database,
    /// One channel for all intents. Subscribers filter by identifier — with a
    /// handful of live intents that is cheaper than a map of channels and has
    /// no lifetime problem when an intent finishes.
    fanout: broadcast::Sender<(IntentId, LogLine)>,
}

impl Ledger {
    /// Opens the ledger, adopting whatever was persisted.
    ///
    /// Imports legacy records once. Invalid data fails without overwriting it.
    pub fn open(legacy: cabal_store::DocumentStore) -> Result<Self, cabal_store::StoreError> {
        let store = legacy.database()?;
        if !store.migrated("ledger:intents")? {
            let persisted: Persisted = if legacy.path().exists() {
                cabal_store::JsonStore::new(legacy.path()).load()?
            } else { legacy.load_optional()?.unwrap_or_default() };
            let rows = persisted.intents.iter().map(|intent| intent_row(intent, store.path())).collect::<Result<Vec<_>, _>>()?;
            store.import_intents("ledger:intents", &rows)?;
        }
        let intents = store.intent_payloads()?.into_iter().map(|raw| {
            serde_json::from_str(&raw).map_err(|source| cabal_store::StoreError::Decode { path: store.path().to_path_buf(), source })
        }).collect::<Result<Vec<Intent>, _>>()?;
        let (fanout, _) = broadcast::channel(FANOUT_CAPACITY);

        tracing::info!(
            target: "cabalmesh::intents",
            count = intents.len(),
            "ledger opened"
        );

        Ok(Self {
            inner: Arc::new(Mutex::new(intents)),
            store,
            fanout,
        })
    }

    fn list(&self) -> std::sync::MutexGuard<'_, Vec<Intent>> {
        self.inner.lock().unwrap_or_else(PoisonError::into_inner)
    }

    /// Commit one changed intent before it becomes visible or can be published.
    fn persist(&self, intent: &Intent, create: bool) -> Result<(), AppError> {
        let row = intent_row(intent, self.store.path()).map_err(AppError::local_storage)?;
        self.store.save_intent(&row, create).map_err(AppError::local_storage)
    }

    fn update<R>(&self, id: &IntentId, change: impl FnOnce(&mut Intent) -> Result<R, AppError>) -> Result<Option<R>, AppError> {
        let mut intents = self.list();
        let Some(intent) = intents.iter_mut().find(|intent| &intent.id == id) else { return Ok(None); };
        let before = intent.clone();
        match change(intent).and_then(|value| self.persist(intent, false).map(|()| value)) {
            Ok(value) => Ok(Some(value)),
            Err(error) => { *intent = before; Err(error) }
        }
    }

    /// Composes a new intent in [`IntentStatus::Draft`].
    ///
    /// Draft means nothing has left the device. Broadcasting is a separate
    /// step, so a compose that succeeds and a publish that fails are
    /// distinguishable states rather than one ambiguous one.
    pub fn create(&self, draft: IntentDraft, now_ms: u64) -> Result<Intent, AppError> {
        let intent = Intent {
            id: IntentId::new(format!("{now_ms:016X}{:016X}", rand::random::<u64>())),
            draft,
            status: IntentStatus::Draft,
            created_ms: now_ms,
            finished_ms: None,
            route: Vec::new(),
            log: Vec::new(),
            deal: None,
            counterparty: None,
        };

        let mut intents = self.list();
        // Newest first: every screen that reads this shows recent work at the
        // top, and sorting per read would be the same order computed again and
        // again.
        self.persist(&intent, true)?;
        intents.insert(0, intent.clone());

        Ok(intent)
    }

    /// Every intent, newest first.
    #[must_use]
    pub fn all(&self) -> Vec<Intent> {
        self.list().clone()
    }

    /// One intent by identifier.
    #[must_use]
    pub fn get(&self, id: &IntentId) -> Option<Intent> {
        self.list().iter().find(|intent| &intent.id == id).cloned()
    }

    /// Moves an intent to `next`, if the domain permits it.
    ///
    /// # Errors
    ///
    /// [`AppError::InvalidIntent`] when the identifier is unknown or the
    /// transition is illegal. Both are caller bugs rather than user errors, and
    /// both are better as a refusal than as a silently overwritten status.
    pub fn advance(&self, id: &IntentId, next: IntentStatus, now_ms: u64) -> Result<Intent, AppError> {
        self.update(id, |intent| {
            if !intent.status.can_transition_to(&next) {
                return Err(AppError::InvalidIntent { field: "status", reason: crate::error::InvalidReason::OutOfRange });
            }
            if next.is_terminal() { intent.finished_ms = Some(now_ms); }
            intent.status = next;
            Ok(intent.clone())
        })?.ok_or(AppError::InvalidIntent { field: "id", reason: crate::error::InvalidReason::Missing })
    }

    pub fn set_route(&self, id: &IntentId, route: Vec<NodeId>) {
        let _ = self.update(id, |intent| { intent.route = route; Ok(()) });
    }

    pub fn set_counterparty(&self, id: &IntentId, address: String) {
        let _ = self.update(id, |intent| { intent.counterparty = Some(address); Ok(()) });
    }

    pub fn propose_deal(&self, id: &IntentId, deal_id: &str, price: UsdPrice, target: &str) -> DealChange {
        self.update(id, |intent| {
            Ok(match &mut intent.deal {
                None => {
                    intent.deal = Some(Deal { id: deal_id.to_owned(), price, proposal_target: target.to_owned(), signed_until_ms: None, reported: None });
                    DealChange::New
                }
                Some(deal) if deal.id == deal_id && deal.signed_until_ms.is_none() && target < deal.proposal_target.as_str() => {
                    deal.price = price;
                    deal.proposal_target = target.to_owned();
                    DealChange::Repriced
                }
                Some(_) => DealChange::Ignored,
            })
        }).ok().flatten().unwrap_or(DealChange::Ignored)
    }

    /// Only report a signed deal once its lock has been committed to SQLite.
    pub fn mark_signed(&self, id: &IntentId, until_ms: u64) -> bool {
        self.update(id, |intent| {
            let Some(deal) = intent.deal.as_mut() else { return Ok(false); };
            if deal.signed_until_ms.is_some() { return Ok(false); }
            deal.signed_until_ms = Some(until_ms);
            Ok(true)
        }).ok().flatten().unwrap_or(false)
    }

    pub fn set_reported_tx(&self, id: &IntentId, tx_hash: String, gateway: String) {
        self.update_deal(id, |deal| deal.reported = Some(ReportedTx { tx_hash, gateway }));
    }

    pub fn clear_reported_tx(&self, id: &IntentId) {
        self.update_deal(id, |deal| deal.reported = None);
    }

    fn update_deal(&self, id: &IntentId, change: impl FnOnce(&mut Deal)) {
        let _ = self.update(id, |intent| {
            if let Some(deal) = intent.deal.as_mut() { change(deal); }
            Ok(())
        });
    }

    /// Persist before notifying subscribers, so a visible log line survives restart.
    pub fn record(&self, id: &IntentId, line: LogLine) {
        if matches!(self.update(id, |intent| { intent.log.push(line.clone()); Ok(()) }), Ok(Some(()))) {
            let _ = self.fanout.send((id.clone(), line));
        }
    }

    /// The lines already recorded, plus a receiver for the ones still coming.
    ///
    /// Returned together and under one lock so nothing can be appended between
    /// the replay and the subscription — the gap that would otherwise drop
    /// exactly the line a user navigated back to see.
    #[must_use]
    pub fn watch(&self, id: &IntentId) -> (Vec<LogLine>, broadcast::Receiver<(IntentId, LogLine)>) {
        let intents = self.list();
        let receiver = self.fanout.subscribe();
        let replay = intents
            .iter()
            .find(|intent| &intent.id == id)
            .map(|intent| intent.log.clone())
            .unwrap_or_default();
        (replay, receiver)
    }

    /// How many intents are held. Primarily for tests and diagnostics.
    #[must_use]
    pub fn len(&self) -> usize {
        self.list().len()
    }

    /// Whether nothing has been composed.
    #[must_use]
    pub fn is_empty(&self) -> bool {
        self.len() == 0
    }
}

fn intent_row(intent: &Intent, path: &std::path::Path) -> Result<cabal_store::IntentRow, cabal_store::StoreError> {
    let status = match &intent.status {
        IntentStatus::Draft => "DRAFT",
        IntentStatus::Broadcast { .. } => "BROADCAST",
        IntentStatus::Negotiating { .. } => "NEGOTIATING",
        IntentStatus::FindingRoute => "FINDING_ROUTE",
        IntentStatus::Waiting => "WAITING",
        IntentStatus::Settled { .. } => "SETTLED",
        IntentStatus::Failed { .. } => "FAILED",
        IntentStatus::Cancelled => "CANCELLED",
    };
    Ok(cabal_store::IntentRow {
        id: intent.id.to_string(),
        network: match intent.draft.chain { cabal_core::Chain::NearTestnet => "near-testnet", cabal_core::Chain::NearMainnet => "near-mainnet" }.into(),
        created_ms: i64::try_from(intent.created_ms).unwrap_or(i64::MAX),
        status: status.into(),
        action: format!("{:?}", intent.draft.action).to_uppercase(),
        asset: intent.draft.asset.to_string(),
        amount_raw: intent.draft.amount.raw().to_string(),
        decimals: intent.draft.amount.decimals(),
        payload: serde_json::to_string(intent).map_err(|source| cabal_store::StoreError::Encode { path: path.to_path_buf(), source })?,
    })
}

/// Intents this device has seen from other peers — over the IP mesh directly,
/// or bridged in from BLE. The mirror of [`Ledger`]: that tracks what this
/// device composed, this tracks the opposite direction, so a device that only
/// ever relays or receives is not blank on both counts forever.
///
/// Deduped by [`crate::mesh::PrivacyIntent::id`] rather than counted per
/// delivery: the BLE fallback resends an unconfirmed broadcast a few seconds
/// apart, and without dedup a single intent would inflate the count with its
/// own retries.
#[derive(Clone)]
pub struct ReceivedLog {
    inner: Arc<Mutex<std::collections::HashSet<String>>>,
    store: Arc<cabal_store::DocumentStore>,
}

impl ReceivedLog {
    /// Opens the log, adopting whatever was persisted.
    pub fn open(store: cabal_store::DocumentStore) -> Result<Self, cabal_store::StoreError> {
        let seen = store.load_optional()?.unwrap_or_default();
        Ok(Self {
            inner: Arc::new(Mutex::new(seen)),
            store: Arc::new(store),
        })
    }

    /// Records a sighting, deduped by id. Returns whether this id was new —
    /// callers that trigger one-shot work off a sighting (negotiation, in
    /// particular) use this instead of a second dedup set of their own, so a
    /// BLE resend of the same intent can never fire that work twice.
    ///
    /// A blank id — an old build's payload, or a malformed one — is never
    /// recorded: an empty string would dedupe every such intent into one slot
    /// instead of counting none of them.
    pub fn record(&self, id: &str) -> bool {
        if id.is_empty() {
            return false;
        }
        let mut seen = self.inner.lock().unwrap_or_else(PoisonError::into_inner);
        if !seen.insert(id.to_string()) {
            return false;
        }
        if let Err(error) = self.store.save(&*seen) {
            seen.remove(id);
            tracing::error!(target: "cabalmesh::intents", %error, "could not persist the received log");
            return false;
        }
        true
    }

    /// Distinct intents seen from other peers, ever.
    #[must_use]
    pub fn count(&self) -> usize {
        self.inner.lock().unwrap_or_else(PoisonError::into_inner).len()
    }
}

/// Counts a sighting of another peer's intent.
///
/// Unrecognised events are ignored rather than logged as errors: this topic carries
/// traffic for the whole mesh, and most of it is not about any intent this device composed.
pub fn apply_mesh_event(received: &ReceivedLog, event: &crate::mesh::MeshEvent) {
    if let crate::mesh::MeshEvent::IntentReceived { intent } = event {
        received.record(&intent.id);
    }
}

/// Wall-clock milliseconds.
///
/// Wall clock rather than a monotonic instant because these are persisted and
/// compared across process restarts, where a monotonic reading means nothing.
#[must_use]
pub fn now_ms() -> u64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map_or(0, |since| u64::try_from(since.as_millis()).unwrap_or(u64::MAX))
}

/// Formats a duration the way the board writes it: `2M 14S`, `11.4S`, `1H 3M`.
///
/// Sub-minute durations keep a decimal because settlement timings are the
/// figures the product points at, and `11S` where the truth is `11.4S` is the
/// kind of rounding the brand's exactness rule exists to forbid.
#[must_use]
pub fn format_elapsed(ms: u64) -> String {
    let total_seconds = ms / 1_000;
    let hours = total_seconds / 3_600;
    let minutes = (total_seconds % 3_600) / 60;

    if hours > 0 {
        format!("{hours}H {minutes}M")
    } else if minutes > 0 {
        format!("{minutes}M {}S", total_seconds % 60)
    } else {
        format!("{:.1}S", ms as f64 / 1_000.0)
    }
}

/// A line at the given tone, for settlement logs.
#[must_use]
pub fn line(text: impl Into<Box<str>>, tone: LogTone) -> LogLine {
    LogLine::new(text, tone)
}

#[cfg(test)]
mod tests {
    use super::*;
    use cabal_core::{Action, Condition, ExecutionMode, PrivacyLevel, ProofHash, TokenAmount, UsdPrice};

    fn draft() -> IntentDraft {
        IntentDraft {
            chain: cabal_core::Chain::NearTestnet,
            action: Action::Buy,
            asset: "wNEAR".into(),
            condition: Condition::Under { price: UsdPrice::from_cents(9500) },
            amount: TokenAmount::parse("1.5", 24).unwrap(),
            mode: ExecutionMode::Shark,
            privacy: PrivacyLevel::High,
        }
    }

    fn ledger() -> (Ledger, tempfile::TempDir) {
        let dir = tempfile::tempdir().unwrap();
        let store = cabal_store::DocumentStore::new(dir.path().join("intents.json"));
        (Ledger::open(store).unwrap(), dir)
    }

    fn settled() -> IntentStatus {
        IntentStatus::Settled {
            proof: ProofHash::new("0xa4f2c9e1b70d5533"),
            filled_at: UsdPrice::from_cents(9421),
            elapsed_ms: 11_400,
        }
    }

    #[test]
    fn a_new_intent_starts_as_a_draft() {
        // Draft means nothing has left the device, which is what makes a failed
        // publish distinguishable from one that never happened.
        let (ledger, _dir) = ledger();
        let intent = ledger.create(draft(), 1_000).unwrap();
        assert_eq!(intent.status, IntentStatus::Draft);
        assert!(intent.log.is_empty());
        assert!(intent.route.is_empty());
    }

    #[test]
    fn identifiers_do_not_collide_within_a_millisecond() {
        let (ledger, _dir) = ledger();
        let first = ledger.create(draft(), 1_000).unwrap();
        let second = ledger.create(draft(), 1_000).unwrap();
        assert_ne!(first.id, second.id);
    }

    #[test]
    fn the_newest_intent_is_first() {
        let (ledger, _dir) = ledger();
        let older = ledger.create(draft(), 1_000).unwrap();
        let newer = ledger.create(draft(), 2_000).unwrap();

        let all = ledger.all();
        assert_eq!(all[0].id, newer.id);
        assert_eq!(all[1].id, older.id);
    }

    #[test]
    fn illegal_transitions_are_refused_rather_than_written() {
        // Settling a draft would mean settling something never broadcast. The
        // domain already says so; this asserts the ledger asks.
        let (ledger, _dir) = ledger();
        let intent = ledger.create(draft(), 1_000).unwrap();

        assert!(ledger.advance(&intent.id, settled(), 2_000).is_err());
        assert_eq!(ledger.get(&intent.id).unwrap().status, IntentStatus::Draft);
    }

    #[test]
    fn a_terminal_state_records_when_it_finished() {
        let (ledger, _dir) = ledger();
        let intent = ledger.create(draft(), 1_000).unwrap();
        ledger.advance(&intent.id, IntentStatus::Broadcast { route_len: 3 }, 1_100).unwrap();
        ledger.advance(&intent.id, IntentStatus::Cancelled, 5_000).unwrap();

        let cancelled = ledger.get(&intent.id).unwrap();
        assert_eq!(cancelled.finished_ms, Some(5_000));
        // Elapsed freezes at the terminal state rather than counting forever.
        assert_eq!(cancelled.elapsed_ms(9_999_999), 4_000);
    }

    #[test]
    fn an_unknown_identifier_is_refused_rather_than_ignored() {
        let (ledger, _dir) = ledger();
        assert!(ledger
            .advance(&IntentId::new("NOPE"), IntentStatus::Cancelled, 1)
            .is_err());
    }

    #[test]
    fn the_ledger_survives_being_reopened() {
        // The whole promise of queue-then-drain: an intent composed offline has
        // to still be there after the app is killed.
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("intents.json");

        let id = {
            let ledger = Ledger::open(cabal_store::DocumentStore::new(&path)).unwrap();
            let intent = ledger.create(draft(), 1_000).unwrap();
            ledger.record(&intent.id, line("QUEUED LOCALLY.", LogTone::Dim));
            intent.id
        };

        let reopened = Ledger::open(cabal_store::DocumentStore::new(&path)).unwrap();
        let intent = reopened.get(&id).expect("the queued intent survived");
        assert_eq!(intent.status, IntentStatus::Draft);
        assert_eq!(intent.log.len(), 1);
        assert_eq!(&*intent.log[0].text, "QUEUED LOCALLY.");
    }

    #[test]
    fn a_late_subscriber_replays_what_it_missed() {
        // Navigating back to a settling intent must not join mid-sentence.
        let (ledger, _dir) = ledger();
        let intent = ledger.create(draft(), 1_000).unwrap();
        ledger.record(&intent.id, line("ROUTE FOUND.", LogTone::Ok));
        ledger.record(&intent.id, line("SIGNED.", LogTone::Ok));

        let (replay, _receiver) = ledger.watch(&intent.id);
        assert_eq!(replay.len(), 2);
        assert_eq!(&*replay[1].text, "SIGNED.");
    }

    #[test]
    fn recording_with_nobody_listening_is_not_a_failure() {
        // This is the normal case for any intent nobody has open, and mistaking
        // it for an error is exactly how a settlement gets aborted by a
        // navigation.
        let (ledger, _dir) = ledger();
        let intent = ledger.create(draft(), 1_000).unwrap();

        ledger.record(&intent.id, line("SUBMITTED.", LogTone::Out));

        assert_eq!(ledger.get(&intent.id).unwrap().log.len(), 1);
    }

    #[tokio::test]
    async fn dropping_every_subscriber_does_not_stop_the_writer() {
        // Ticket 34's rule, asserted directly: the settlement keeps writing
        // after the last reader is gone, and the record is complete.
        let (ledger, _dir) = ledger();
        let intent = ledger.create(draft(), 1_000).unwrap();

        let (_replay, receiver) = ledger.watch(&intent.id);
        ledger.record(&intent.id, line("SUBMITTED.", LogTone::Out));

        drop(receiver);

        ledger.record(&intent.id, line("RECEIPT MINED.", LogTone::Ok));
        ledger.record(&intent.id, line("PROOF WRITTEN.", LogTone::Loud));

        let after = ledger.get(&intent.id).unwrap();
        assert_eq!(after.log.len(), 3);
        assert_eq!(&*after.log[2].text, "PROOF WRITTEN.");
    }

    #[tokio::test]
    async fn a_live_subscriber_receives_lines_for_its_own_intent_only() {
        let (ledger, _dir) = ledger();
        let mine = ledger.create(draft(), 1_000).unwrap();
        let theirs = ledger.create(draft(), 1_000).unwrap();

        let (_replay, mut receiver) = ledger.watch(&mine.id);
        ledger.record(&theirs.id, line("NOT MINE.", LogTone::Dim));
        ledger.record(&mine.id, line("MINE.", LogTone::Ok));

        // The channel carries both; filtering by identifier is the subscriber's
        // job, and this asserts it has what it needs to do it.
        let (first_id, first) = receiver.recv().await.unwrap();
        assert_eq!(first_id, theirs.id);
        assert_eq!(&*first.text, "NOT MINE.");

        let (second_id, second) = receiver.recv().await.unwrap();
        assert_eq!(second_id, mine.id);
        assert_eq!(&*second.text, "MINE.");
    }

    #[test]
    fn elapsed_matches_the_boards_format() {
        assert_eq!(format_elapsed(11_400), "11.4S");
        assert_eq!(format_elapsed(134_000), "2M 14S");
        assert_eq!(format_elapsed(3_780_000), "1H 3M");
        // Zero renders as a figure, not as an em dash: a settlement that took
        // no measurable time still took a measured time.
        assert_eq!(format_elapsed(0), "0.0S");
    }

    #[test]
    fn a_route_is_recorded_rather_than_assumed() {
        let (ledger, _dir) = ledger();
        let intent = ledger.create(draft(), 1_000).unwrap();
        assert!(ledger.get(&intent.id).unwrap().route.is_empty());

        ledger.set_route(&intent.id, vec![NodeId::new("7F3A"), NodeId::new("8C2E")]);
        assert_eq!(ledger.get(&intent.id).unwrap().route.len(), 2);
    }

    #[test]
    fn the_proposal_to_the_smaller_intent_wins_on_both_ends() {
        let (ledger, _dir) = ledger();
        let intent = ledger.create(draft(), 1_000).unwrap();
        let price = UsdPrice::from_cents;

        assert_eq!(ledger.propose_deal(&intent.id, "a:b", price(400), "b"), DealChange::New);
        assert_eq!(ledger.propose_deal(&intent.id, "a:b", price(410), "c"), DealChange::Ignored);
        assert_eq!(ledger.propose_deal(&intent.id, "a:b", price(420), "a"), DealChange::Repriced);
        assert_eq!(ledger.get(&intent.id).unwrap().deal.unwrap().price, price(420));
    }

    #[test]
    fn a_signed_deal_cannot_be_repriced_or_replaced() {
        let (ledger, _dir) = ledger();
        let intent = ledger.create(draft(), 1_000).unwrap();
        let price = UsdPrice::from_cents;

        ledger.propose_deal(&intent.id, "a:b", price(400), "b");
        assert_eq!(ledger.propose_deal(&intent.id, "z:y", price(100), "a"), DealChange::Ignored, "one deal per intent");
        ledger.mark_signed(&intent.id, 9_000);
        assert_eq!(ledger.propose_deal(&intent.id, "a:b", price(420), "a"), DealChange::Ignored);
        assert_eq!(ledger.get(&intent.id).unwrap().deal.unwrap().price, price(400));
    }

    #[test]
    fn legacy_intents_without_a_chain_are_not_silently_discarded() {
        let dir = tempfile::tempdir().unwrap();
        let store = || cabal_store::DocumentStore::new(dir.path().join("intents.json"));
        let (seed, _seed_dir) = ledger();
        let intent = seed.create(draft(), 1_000).unwrap();
        // As text: a raw amount is a u128, which `serde_json::Value` cannot hold.
        let text = serde_json::to_string(&intent).unwrap().replace(r#""chain":"near-testnet","#, "");
        std::fs::write(dir.path().join("intents.json"), format!(r#"{{"intents":[{text}]}}"#)).unwrap();

        assert!(Ledger::open(store()).is_err());
        assert!(dir.path().join("intents.json").exists());
    }

    #[test]
    fn legacy_history_is_imported_once_with_its_logs_and_deals() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("intents.json");
        let (source, _source_dir) = ledger();
        let original = source.create(draft(), 1_000).unwrap();
        source.propose_deal(&original.id, "a:b", UsdPrice::from_cents(400), "b");
        assert!(source.mark_signed(&original.id, 9_000));
        source.record(&original.id, line("SIGNED OFFLINE.", LogTone::Ok));
        source.set_counterparty(&original.id, "peer.testnet".into());
        let expected = source.get(&original.id).unwrap();
        cabal_store::JsonStore::new(&path).save(&Persisted { intents: vec![expected.clone()] }).unwrap();
        let backup = std::fs::read(&path).unwrap();
        let imported = Ledger::open(cabal_store::DocumentStore::new(&path)).unwrap();
        assert_eq!(serde_json::to_string(&imported.get(&original.id).unwrap()).unwrap(), serde_json::to_string(&expected).unwrap());
        imported.advance(&original.id, IntentStatus::Cancelled, 2_000).unwrap();
        let next = imported.create(draft(), 1_000).unwrap();
        assert_ne!(next.id, original.id);
        let reopened = Ledger::open(cabal_store::DocumentStore::new(&path)).unwrap();
        assert_eq!(reopened.len(), 2);
        assert_eq!(reopened.get(&original.id).unwrap().status, IntentStatus::Cancelled);
        assert_eq!(std::fs::read(&path).unwrap(), backup);
        let connection = rusqlite::Connection::open(imported.store.path()).unwrap();
        let (amount, storage): (String,String) = connection.query_row("SELECT amount_raw,typeof(amount_raw) FROM intents WHERE id=?1", [original.id.as_str()], |row|Ok((row.get(0)?,row.get(1)?))).unwrap();
        assert_eq!(amount,"1500000000000000000000000");
        assert_eq!(storage,"text");
    }

    #[test]
    fn failed_database_writes_do_not_create_or_advance_an_intent() {
        let (ledger, _dir) = ledger();
        let connection = rusqlite::Connection::open(ledger.store.path()).unwrap();
        connection.execute_batch("CREATE TRIGGER fail_create BEFORE INSERT ON intents BEGIN SELECT RAISE(ABORT,'test write failure'); END;").unwrap();
        assert!(matches!(ledger.create(draft(), 1_000), Err(AppError::LocalStorage)));
        assert!(ledger.is_empty());
        connection.execute_batch("DROP TRIGGER fail_create").unwrap();
        let intent = ledger.create(draft(), 1_000).unwrap();
        ledger.propose_deal(&intent.id,"deal",UsdPrice::from_cents(400),"b");
        connection.execute_batch("CREATE TRIGGER fail_update BEFORE UPDATE ON intents BEGIN SELECT RAISE(ABORT,'test write failure'); END;").unwrap();
        assert!(matches!(ledger.advance(&intent.id, IntentStatus::Broadcast { route_len: 1 }, 1_001),Err(AppError::LocalStorage)));
        assert_eq!(ledger.get(&intent.id).unwrap().status, IntentStatus::Draft);
        assert!(!ledger.mark_signed(&intent.id,9_000));
        assert_eq!(ledger.get(&intent.id).unwrap().deal.unwrap().signed_until_ms,None);
        ledger.record(&intent.id,line("MUST NOT APPEAR.",LogTone::Ok));
        assert!(ledger.get(&intent.id).unwrap().log.is_empty());
        let saved: String = connection.query_row("SELECT payload_json FROM intents WHERE id=?1",[intent.id.as_str()],|row|row.get(0)).unwrap();
        let saved: Intent = serde_json::from_str(&saved).unwrap();
        assert_eq!(saved.status,IntentStatus::Draft);
        assert!(saved.log.is_empty());
    }
}
