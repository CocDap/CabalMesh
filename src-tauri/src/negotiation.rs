//! Agents negotiating a price over the mesh.
//!
//! # What runs where
//!
//! Every node runs its own copy of this: there is no matchmaker service. An intent
//! tells the mesh its chain, asset, side and size, and never its price limit. Code
//! filters on those before any model is involved, and whether the prices overlap is
//! found out through offers.
//!
//! # Who starts
//!
//! The seller opens, and only one device hears each intent as it is published: the one
//! that was already listening. So a seller that hears a buyer sends its offer, and a
//! buyer that hears a seller sends an `Invite`, addressed to that intent, so the seller
//! learns it has a counterparty.
//!
//! # Nothing here can spend
//!
//! [`cabal_negotiation::Negotiation`] decides what may be sent, and enforces the user's
//! limit on every price, whatever the model said. An agreement only forms a deal on the
//! ledger. Signing is `settle_intent`, which only the user's button calls.

use crate::agent::{self, Decision, Said};
use crate::agent_tools::ToolContext;
use crate::bindings::LogTone;
use crate::intents::{line, now_ms, Intent, Ledger};
use crate::llm_client::LlmClient;
use crate::mesh::{MeshEvent, PrivacyIntent};
use crate::near_settlement::deal_id;
use crate::price_feed::PriceFeed;
use crate::state::AppState;
use cabal_core::{Action, Condition, IntentDraft, IntentId, IntentStatus, PublicIntent, UsdPrice};
use cabal_negotiation::{Bound, EndReason, Msg, Negotiation, Outcome, Reject, Side, MAX_ROUNDS};
use serde::{Deserialize, Serialize};
use std::collections::{HashMap, VecDeque};
use std::sync::{Arc, Mutex, PoisonError};
use std::time::Duration;

pub const TICK_INTERVAL: Duration = Duration::from_secs(5);
/// How many other intents are remembered for `list_mesh_offers`.
const MAX_REMEMBERED_OFFERS: usize = 64;
/// How long a finished negotiation is kept, so a resent `Offer` cannot restart it.
const KEEP_FINISHED_MS: u64 = 60 * 60 * 1000;

/// One negotiation message on the wire. Sent in the clear, and carries no limit.
#[derive(Debug, Clone, Serialize, Deserialize)]
struct Wire {
    #[serde(flatten)]
    msg: Msg,
    to_intent: String,
    from: PublicIntent,
}

/// A buyer telling a seller it exists. See the module docs.
///
/// A struct with a one-variant `type`, not `#[serde(tag = "type")]` on an enum: that
/// buffers the whole message first, and a buffered `u128` amount does not survive.
#[derive(Debug, Clone, Serialize, Deserialize)]
struct Invite {
    #[serde(rename = "type")]
    kind: InviteKind,
    to_intent: String,
    from: PublicIntent,
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize)]
enum InviteKind {
    Invite,
}

struct Active {
    negotiation: Negotiation,
    own_id: IntentId,
    theirs: PublicIntent,
    history: Vec<Said>,
    finished_ms: Option<u64>,
}

/// This device's negotiations, and what it has seen on the mesh.
#[derive(Default)]
pub struct Agents {
    active: Mutex<HashMap<String, Active>>,
    offers: Mutex<VecDeque<PublicIntent>>,
}

impl Agents {
    fn lock(&self) -> std::sync::MutexGuard<'_, HashMap<String, Active>> {
        self.active.lock().unwrap_or_else(PoisonError::into_inner)
    }

    fn remember(&self, offer: PublicIntent) {
        let mut offers = self.offers.lock().unwrap_or_else(PoisonError::into_inner);
        offers.push_back(offer);
        while offers.len() > MAX_REMEMBERED_OFFERS {
            offers.pop_front();
        }
    }

    fn remembered(&self) -> Vec<PublicIntent> {
        self.offers
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
            .iter()
            .cloned()
            .collect()
    }

    /// One negotiation at a time per intent: a deal could only come from one of them.
    fn is_busy(&self, own_id: &IntentId) -> bool {
        self.lock()
            .values()
            .any(|active| &active.own_id == own_id && !active.negotiation.is_over())
    }

    /// Starts a negotiation, unless this pair has already had one.
    fn start(&self, own: &Intent, theirs: PublicIntent, now: u64) -> Option<String> {
        let bound = bound_of(&own.draft)?;
        let neg_id = deal_id(&own.id.to_string(), &theirs.id);
        let mut active = self.lock();
        if active.contains_key(&neg_id) {
            return None;
        }
        active.insert(
            neg_id.clone(),
            Active {
                negotiation: Negotiation::start(neg_id.clone(), bound, now),
                own_id: own.id.clone(),
                theirs,
                history: Vec::new(),
                finished_ms: None,
            },
        );
        Some(neg_id)
    }

    /// Ends what has run out of time, and forgets what is long finished.
    fn tick(&self, ledger: &Ledger, now: u64) {
        let mut ended = Vec::new();
        self.lock().retain(|_, active| {
            if let Outcome::Ended { reason } = active.negotiation.tick(now) {
                ended.push((active.own_id.clone(), reason));
            }
            if active.negotiation.is_over() && active.finished_ms.is_none() {
                active.finished_ms = Some(now);
            }
            active
                .finished_ms
                .is_none_or(|finished| now.saturating_sub(finished) < KEEP_FINISHED_MS)
        });
        for (own_id, reason) in ended {
            record_end(ledger, &own_id, reason);
        }
    }
}

/// Everything one negotiation step needs. A struct so that two of these can talk to
/// each other in a test, with no mesh, window or vault.
pub(crate) struct Node {
    ledger: Ledger,
    agents: Arc<Agents>,
    feed: Arc<PriceFeed>,
    client: LlmClient,
    /// Tradable balances, as shown to the model: of the base, and of the quote.
    base_balance: Option<String>,
    quote_balance: Option<String>,
    clock: fn() -> u64,
}

/// Considers one intent freshly received from the mesh.
///
/// Called at most once per distinct intent id: the call sites in `lib.rs` gate it behind
/// [`crate::intents::ReceivedLog::record`], so a BLE resend cannot run it twice.
pub async fn consider(state: &AppState, incoming: &PrivacyIntent) {
    // Privacy is the transport's job elsewhere (see `commands::publish`'s comment on
    // `encrypted`), but this can only match on what it can read.
    if incoming.encrypted {
        return;
    }
    let Ok(theirs) = serde_json::from_str::<PublicIntent>(&incoming.payload) else {
        return;
    };
    if theirs.id != incoming.id {
        return;
    }
    let Some((node, services)) = node(state).await else {
        return;
    };
    publish(&services, on_intent(&node, theirs).await).await;
}

/// Acts on a negotiation message from either plane. In the background: a turn can take
/// 30 s, and the mesh loop must not wait on it.
pub fn dispatch(state: &AppState, event: &MeshEvent) {
    let MeshEvent::Negotiation { details } = event else {
        return;
    };
    let state = state.clone();
    let details = details.clone();
    tokio::spawn(async move {
        if let Some((node, services)) = node(&state).await {
            publish(&services, on_message(&node, &details).await).await;
        }
    });
}

/// Ends negotiations that have run out of time.
pub fn tick(state: &AppState) {
    if let Ok(services) = state.services() {
        services.agents.tick(state.intents(), now_ms());
    }
}

/// `None` while the vault is locked: this device could not sign what it agreed.
async fn node(state: &AppState) -> Option<(Node, crate::state::Services)> {
    let services = state.services().ok()?;
    services.identity.lock().await.primary_account()?;
    let near = crate::network_config::NetworkConfig::current().near();
    let balances = crate::commands::cached_balances(state).await;
    let balance = |spec: &crate::network_config::AssetSpec| {
        balances
            .in_intents_raw(spec.symbol)
            .map(|raw| cabal_core::TokenAmount::from_raw(raw, spec.decimals).to_string())
    };
    let node = Node {
        ledger: state.intents().clone(),
        agents: services.agents.clone(),
        feed: services.price_feed.clone(),
        client: services.llm().await,
        base_balance: balance(&near.base),
        quote_balance: balance(&near.quote),
        clock: now_ms,
    };
    Some((node, services))
}

async fn publish(services: &crate::state::Services, payloads: Vec<String>) {
    for payload in payloads {
        let message = PrivacyIntent {
            // Unused by the receiving side, but every `PrivacyIntent` carries one.
            id: format!("neg-{}-{}", now_ms(), payload.len()),
            intent_type: "settlement".into(),
            payload,
            encrypted: false,
            relay_path: vec!["origin_node".into()],
            relay_fee: None,
        };
        if crate::commands::publish_over_mesh(services, &message)
            .await
            .is_err()
        {
            let _ = crate::commands::publish_over_ble(services, &message).await;
        }
    }
}

/// The filter that runs before any model call, on what an intent makes public.
fn can_trade(own: &Intent, theirs: &PublicIntent) -> bool {
    own.status.is_active()
        // One deal at a time. A signed diff cannot be recalled.
        && own.deal.is_none()
        && own.id.to_string() != theirs.id
        && &*own.draft.asset == &*theirs.asset
        && own.draft.chain.matches(theirs.chain)
        // No partial fills: each side signs its whole amount, and a pair of unequal
        // sizes could never net to zero.
        && own.draft.amount == theirs.amount
        && opposite_sides(own.draft.action, theirs.action)
}

async fn on_intent(node: &Node, theirs: PublicIntent) -> Vec<String> {
    if Side::try_from(theirs.action).is_err() {
        return Vec::new();
    }
    node.agents.remember(theirs.clone());

    let own = node
        .ledger
        .all()
        .into_iter()
        .find(|own| can_trade(own, &theirs) && !node.agents.is_busy(&own.id));
    let Some(own) = own else {
        return Vec::new();
    };

    if own.draft.action == Action::Buy {
        let invite = Invite {
            kind: InviteKind::Invite,
            to_intent: theirs.id,
            from: PublicIntent::of(own.id.to_string(), &own.draft),
        };
        return serde_json::to_string(&invite).ok().into_iter().collect();
    }
    open_as_seller(node, &own, theirs).await
}

async fn open_as_seller(node: &Node, own: &Intent, theirs: PublicIntent) -> Vec<String> {
    match node.agents.start(own, theirs, (node.clock)()) {
        Some(neg_id) => take_turn(node, &neg_id).await,
        None => Vec::new(),
    }
}

async fn on_message(node: &Node, payload: &str) -> Vec<String> {
    if let Ok(Invite {
        to_intent, from, ..
    }) = serde_json::from_str(payload)
    {
        let own = node.ledger.get(&IntentId::new(to_intent));
        return match own {
            Some(own)
                if own.draft.action == Action::Sell
                    && can_trade(&own, &from)
                    && !node.agents.is_busy(&own.id) =>
            {
                open_as_seller(node, &own, from).await
            }
            _ => Vec::new(),
        };
    }
    let Ok(wire) = serde_json::from_str::<Wire>(payload) else {
        return Vec::new();
    };
    on_wire(node, wire).await
}

async fn on_wire(node: &Node, wire: Wire) -> Vec<String> {
    let (Msg::Offer { neg_id, .. } | Msg::Counter { neg_id, .. } | Msg::Accept { neg_id, .. }) =
        &wire.msg;
    let neg_id = neg_id.clone();
    // Every peer sees every message on this topic, so most are about somebody else.
    let Some(own) = node.ledger.get(&IntentId::new(wire.to_intent.clone())) else {
        return Vec::new();
    };
    if !own.status.is_active() || neg_id != deal_id(&wire.to_intent, &wire.from.id) {
        return Vec::new();
    }
    let known = node.agents.lock().contains_key(&neg_id);
    if !known {
        // Only an opening starts one. Anything later would sit here blocking this pair.
        let opens =
            matches!(wire.msg, Msg::Offer { round: 1, .. }) && own.draft.action == Action::Buy;
        if !opens
            || !can_trade(&own, &wire.from)
            || node.agents.is_busy(&own.id)
            || node
                .agents
                .start(&own, wire.from.clone(), (node.clock)())
                .is_none()
        {
            return Vec::new();
        }
    }

    let now = (node.clock)();
    let (outcome, heard) = {
        let mut active = node.agents.lock();
        let Some(entry) = active.get_mut(&neg_id) else {
            return Vec::new();
        };
        let waiting = !entry.negotiation.is_our_turn();
        let outcome = entry.negotiation.receive(&wire.msg, now);
        let heard = waiting && entry.negotiation.is_our_turn();
        if heard {
            entry.history.push(Said {
                by_us: false,
                msg: wire.msg.clone(),
            });
        }
        (outcome, heard)
    };

    match outcome {
        Outcome::Agreed { price_cents, reply } => {
            agree(node, &own.id, &neg_id, price_cents);
            let reply = reply.and_then(|msg| encode(&own, &wire.from.id, msg));
            reply.into_iter().collect()
        }
        Outcome::Ended { reason } => {
            record_end(&node.ledger, &own.id, reason);
            Vec::new()
        }
        Outcome::Continue if heard => {
            let (round, price_cents) = said_values(&wire.msg);
            node.ledger.record(
                &own.id,
                line(
                    format!(
                        "PEER SAID {} (ROUND {round} OF {MAX_ROUNDS}).",
                        UsdPrice::from_cents(price_cents)
                    ),
                    LogTone::Info,
                ),
            );
            take_turn(node, &neg_id).await
        }
        Outcome::Continue => Vec::new(),
    }
}

/// Runs this side's move, if it has one to make, and returns what to send.
async fn take_turn(node: &Node, neg_id: &str) -> Vec<String> {
    // Only called when the turn passes to this side, which happens once per message: a
    // resent message finds it already here, and cannot start a second model call.
    let turn = {
        let active = node.agents.lock();
        active
            .get(neg_id)
            .filter(|entry| entry.negotiation.is_our_turn())
            .map(|entry| {
                (
                    entry.own_id.clone(),
                    entry.theirs.id.clone(),
                    entry.negotiation.next_round(),
                    entry.history.clone(),
                )
            })
    };
    let Some((own_id, their_id, round, history)) = turn else {
        return Vec::new();
    };

    let decision = match node.ledger.get(&own_id) {
        Some(own) if own.status.is_active() && round > MAX_ROUNDS => {
            last_move(node, &own, neg_id).await
        }
        Some(own) if own.status.is_active() => decide(node, &own, round, &history).await,
        _ => Decision::Nothing,
    };

    // The user may have cancelled while the model was thinking.
    let own = node
        .ledger
        .get(&own_id)
        .filter(|own| own.status.is_active());
    let mut active = node.agents.lock();
    let Some(entry) = active.get_mut(neg_id) else {
        return Vec::new();
    };
    let Some(own) = own else {
        return Vec::new();
    };

    let price = match decision {
        Decision::Propose { price_cents } => price_cents,
        Decision::ModelUnavailable { market_cents } => {
            node.ledger.record(
                &own_id,
                line(
                    "NO MODEL AVAILABLE. USING THE BUILT-IN PRICE.",
                    LogTone::Dim,
                ),
            );
            entry.negotiation.fallback_price(market_cents)
        }
        Decision::SuggestOnly { note } => {
            node.ledger.record(&own_id, line(note, LogTone::Info));
            return Vec::new();
        }
        Decision::Nothing => {
            node.ledger.record(
                &own_id,
                line("AGENT MADE NO OFFER THIS ROUND.", LogTone::Dim),
            );
            return Vec::new();
        }
    };

    match entry.negotiation.propose(price, (node.clock)()) {
        Ok(msg) => {
            entry.history.push(Said {
                by_us: true,
                msg: msg.clone(),
            });
            drop(active);
            record_sent(&node.ledger, &own_id, &msg);
            encode(&own, &their_id, msg).into_iter().collect()
        }
        Err(Reject::OutsideBound) => {
            drop(active);
            node.ledger.record(
                &own_id,
                line(
                    "AGENT'S PRICE WAS OUTSIDE YOUR LIMIT. NOT SENT.",
                    LogTone::Info,
                ),
            );
            Vec::new()
        }
        Err(Reject::Ended) => {
            let reason = entry
                .negotiation
                .ended()
                .unwrap_or(EndReason::RoundsExhausted);
            drop(active);
            record_end(&node.ledger, &own_id, reason);
            Vec::new()
        }
        Err(Reject::NotOurTurn) => Vec::new(),
    }
}

/// The seller's last move, after the buyer's final price. All it can do is take that
/// price or end with no trade, and a price inside the user's limit is one they asked for,
/// so no model is asked. They still confirm before anything is signed.
///
/// A fresh market price is still required: with no limit set, it is what says how far
/// from market is too far.
async fn last_move(node: &Node, own: &Intent, neg_id: &str) -> Decision {
    let quote = node
        .feed
        .quote(&own.draft.asset, (node.clock)())
        .await
        .filter(|quote| !quote.stale);
    let price = quote.and_then(|quote| {
        Some(
            node.agents
                .lock()
                .get(neg_id)?
                .negotiation
                .fallback_price(quote.usd_cents),
        )
    });
    price.map_or(Decision::Nothing, |price_cents| Decision::Propose {
        price_cents,
    })
}

async fn decide(node: &Node, own: &Intent, round: u8, history: &[Said]) -> Decision {
    let Some(bound) = bound_of(&own.draft) else {
        return Decision::Nothing;
    };
    let ctx = ToolContext {
        chain: own.draft.chain,
        asset: own.draft.asset.to_string(),
        own_intent: own.id.clone(),
        bound,
        amount: own.draft.amount.to_string(),
        balance: match bound.side {
            Side::Sell => node.base_balance.clone(),
            Side::Buy => node.quote_balance.clone(),
        },
        round,
        mesh_offers: node.agents.remembered(),
        feed: node.feed.clone(),
        now_ms: (node.clock)(),
        proposed: Mutex::new(None),
    };
    agent::decide(&node.client, &ctx, history).await
}

fn encode(own: &Intent, their_id: &str, msg: Msg) -> Option<String> {
    let wire = Wire {
        msg,
        to_intent: their_id.to_owned(),
        from: PublicIntent::of(own.id.to_string(), &own.draft),
    };
    serde_json::to_string(&wire).ok()
}

/// Forms the deal. Still only a deal: the user signs it or nobody does.
fn agree(node: &Node, own_id: &IntentId, neg_id: &str, price_cents: u64) {
    let Some(their_id) = node
        .agents
        .lock()
        .get(neg_id)
        .map(|entry| entry.theirs.id.clone())
    else {
        return;
    };
    let price = UsdPrice::from_cents(price_cents);
    if node.ledger.propose_deal(own_id, neg_id, price, &their_id)
        == crate::intents::DealChange::Ignored
    {
        return;
    }
    let bids = match node.ledger.get(own_id).map(|own| own.status) {
        Some(IntentStatus::Negotiating { bids, .. }) => bids.saturating_add(1),
        _ => 1,
    };
    if node
        .ledger
        .advance(
            own_id,
            IntentStatus::Negotiating {
                bids,
                best: Some(price),
            },
            (node.clock)(),
        )
        .is_ok()
    {
        node.ledger.record(
            own_id,
            line(
                format!("AGENTS AGREED AT {price} (CONFIRM TO SETTLE)."),
                LogTone::Ok,
            ),
        );
    }
}

const fn said_values(msg: &Msg) -> (u8, u64) {
    match msg {
        Msg::Offer {
            round, price_cents, ..
        }
        | Msg::Counter {
            round, price_cents, ..
        }
        | Msg::Accept {
            round, price_cents, ..
        } => (*round, *price_cents),
    }
}

fn record_sent(ledger: &Ledger, own_id: &IntentId, msg: &Msg) {
    let (round, price_cents) = said_values(msg);
    let price = UsdPrice::from_cents(price_cents);
    let text = match msg {
        Msg::Accept { .. } => format!("AGENT ACCEPTED {price}. WAITING FOR THE OTHER SIDE."),
        Msg::Offer { .. } | Msg::Counter { .. } => {
            format!("AGENT OFFERED {price} (ROUND {round} OF {MAX_ROUNDS}).")
        }
    };
    ledger.record(own_id, line(text, LogTone::Out));
}

fn record_end(ledger: &Ledger, own_id: &IntentId, reason: EndReason) {
    let text = match reason {
        EndReason::RoundsExhausted => "NO DEAL AFTER 3 ROUNDS. INTENT STILL OPEN.",
        EndReason::TimedOut => "NEGOTIATION TIMED OUT. INTENT STILL OPEN.",
        EndReason::Unmatched => "SIDES ACCEPTED DIFFERENT PRICES. INTENT STILL OPEN.",
    };
    ledger.record(own_id, line(text, LogTone::Dim));
}

/// What the user's condition allows. `None` for anything but a buy or a sell.
///
/// Only `Under` caps a buyer and only `Above` floors a seller. Either side picking
/// anything else has stated no limit.
fn bound_of(draft: &IntentDraft) -> Option<Bound> {
    let side = Side::try_from(draft.action).ok()?;
    let limit = match (side, draft.condition) {
        (Side::Buy, Condition::Under { price }) | (Side::Sell, Condition::Above { price }) => {
            Some(price.cents())
        }
        _ => None,
    };
    Some(Bound {
        side,
        limit_cents: limit,
    })
}

/// Whether two actions describe opposite sides of the same trade.
const fn opposite_sides(a: Action, b: Action) -> bool {
    matches!(
        (a, b),
        (Action::Buy, Action::Sell) | (Action::Sell, Action::Buy)
    )
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::llm_budget::LlmBudget;
    use crate::llm_client::mock;
    use crate::llm_config::LlmSettings;
    use crate::price_feed::tests::{loaded_feed, NOON_MS};
    use cabal_core::{Chain, ExecutionMode, PrivacyLevel, TokenAmount};
    use cabal_negotiation::ROUND_TIMEOUT_MS;
    use serde_json::json;

    const FLOOR: u64 = 391;
    const CEILING: u64 = 423;

    fn noon() -> u64 {
        NOON_MS + 1
    }

    fn draft(action: Action, condition: Condition) -> IntentDraft {
        IntentDraft {
            chain: Chain::NearTestnet,
            action,
            asset: "wNEAR".into(),
            condition,
            amount: TokenAmount::parse("1", 24).unwrap(),
            mode: ExecutionMode::Shark,
            privacy: PrivacyLevel::Low,
        }
    }

    fn above(cents: u64) -> Condition {
        Condition::Above {
            price: UsdPrice::from_cents(cents),
        }
    }

    fn under(cents: u64) -> Condition {
        Condition::Under {
            price: UsdPrice::from_cents(cents),
        }
    }

    struct Side_ {
        node: Node,
        own: IntentId,
        _dir: tempfile::TempDir,
    }

    /// A device with one open intent, a price from noon, and `client` for its model.
    async fn device(draft: IntentDraft, client: LlmClient) -> Side_ {
        let dir = tempfile::tempdir().unwrap();
        let ledger = Ledger::open(cabal_store::DocumentStore::new(dir.path().join("intents.json"))).unwrap();
        // An id comes from the clock and a per-ledger serial, so two fresh ledgers need
        // two different times to be two different devices.
        static CREATED: std::sync::atomic::AtomicU64 = std::sync::atomic::AtomicU64::new(NOON_MS);
        let created = CREATED.fetch_add(1, std::sync::atomic::Ordering::SeqCst);
        let own = ledger.create(draft, created).unwrap();
        ledger
            .advance(&own.id, IntentStatus::Broadcast { route_len: 1 }, NOON_MS)
            .unwrap();
        let feed = Arc::new(loaded_feed(4.04, &dir).await);
        let node = Node {
            ledger,
            agents: Arc::new(Agents::default()),
            feed,
            client,
            base_balance: Some("40".into()),
            quote_balance: Some("100".into()),
            clock: noon,
        };
        Side_ {
            node,
            own: own.id,
            _dir: dir,
        }
    }

    /// A client whose model is out of reach, so the built-in schedule plays.
    fn no_model() -> LlmClient {
        mock::client(&mock::refused_url(), None)
    }

    fn public_of(side: &Side_, id: &str) -> PublicIntent {
        let own = side.node.ledger.get(&side.own).unwrap();
        PublicIntent::of(id.to_string(), &own.draft)
    }

    fn public(side: &Side_) -> PublicIntent {
        public_of(side, &side.own.to_string())
    }

    /// Passes messages between two devices until neither has anything to say, and
    /// returns everything that crossed the wire.
    async fn exchange(a: &Side_, b: &Side_, first: Vec<String>, first_to_b: bool) -> Vec<String> {
        let mut wire = Vec::new();
        let mut queue: VecDeque<(String, bool)> = first
            .into_iter()
            .map(|payload| (payload, first_to_b))
            .collect();
        while let Some((payload, to_b)) = queue.pop_front() {
            wire.push(payload.clone());
            let (receiver, next_to_b) = if to_b { (b, false) } else { (a, true) };
            for reply in on_message(&receiver.node, &payload).await {
                queue.push_back((reply, next_to_b));
            }
        }
        wire
    }

    fn deal_price(side: &Side_) -> Option<UsdPrice> {
        side.node
            .ledger
            .get(&side.own)
            .unwrap()
            .deal
            .map(|deal| deal.price)
    }

    fn log_of(side: &Side_) -> String {
        side.node
            .ledger
            .get(&side.own)
            .unwrap()
            .log
            .iter()
            .map(|line| format!("{line:?}"))
            .collect()
    }

    /// A message with its ids gone: their digits could be anything.
    fn without_ids(message: &str) -> String {
        let mut said: serde_json::Value = serde_json::from_str(message).unwrap();
        for field in ["neg_id", "to_intent"] {
            said.as_object_mut().unwrap().remove(field);
        }
        said["from"].as_object_mut().unwrap().remove("id");
        said.to_string()
    }

    /// A seller that has opened against a buyer, and the buyer, and the opening offer.
    async fn opened(
        seller_client: LlmClient,
        buyer_client: LlmClient,
    ) -> (Side_, Side_, Vec<String>) {
        let seller = device(draft(Action::Sell, above(FLOOR)), seller_client).await;
        let buyer = device(draft(Action::Buy, under(CEILING)), buyer_client).await;
        let offer = on_intent(&seller.node, public(&buyer)).await;
        assert_eq!(offer.len(), 1, "the seller opens");
        (seller, buyer, offer)
    }

    #[tokio::test]
    async fn filter_skips_model_for_other_chain_asset_or_same_side() {
        // A live listener that must see nothing.
        let silent = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
        silent.set_nonblocking(true).unwrap();
        let client = mock::client(&format!("http://{}/v1", silent.local_addr().unwrap()), None);
        let seller = device(draft(Action::Sell, above(FLOOR)), client).await;

        let good = PublicIntent {
            id: "peer".into(),
            chain: Chain::NearTestnet,
            asset: "wNEAR".into(),
            action: Action::Buy,
            amount: TokenAmount::parse("1", 24).unwrap(),
        };
        let cases = [
            PublicIntent {
                chain: Chain::NearMainnet,
                ..good.clone()
            },
            PublicIntent {
                asset: "USDT".into(),
                ..good.clone()
            },
            PublicIntent {
                action: Action::Sell,
                ..good.clone()
            },
            PublicIntent {
                action: Action::Swap,
                ..good.clone()
            },
            PublicIntent {
                amount: TokenAmount::parse("2", 24).unwrap(),
                ..good.clone()
            },
            PublicIntent {
                id: seller.own.to_string(),
                ..good.clone()
            },
        ];
        for case in cases {
            assert!(
                on_intent(&seller.node, case.clone()).await.is_empty(),
                "{case:?}"
            );
        }
        assert!(
            silent.accept().is_err(),
            "no model call should have been made"
        );
    }

    #[tokio::test]
    async fn two_nodes_agree_without_a_model_and_never_send_a_limit() {
        let (seller, buyer, offer) = opened(no_model(), no_model()).await;
        let wire = exchange(&seller, &buyer, offer, true).await;

        assert_eq!(
            deal_price(&seller),
            Some(UsdPrice::from_cents(404)),
            "{wire:#?}"
        );
        assert_eq!(deal_price(&buyer), deal_price(&seller));
        assert_eq!(
            seller
                .node
                .ledger
                .get(&seller.own)
                .unwrap()
                .deal
                .unwrap()
                .id,
            buyer.node.ledger.get(&buyer.own).unwrap().deal.unwrap().id,
        );
        assert!(
            log_of(&seller).contains("AGENTS AGREED AT $4.04 (CONFIRM TO SETTLE)."),
            "{}",
            log_of(&seller)
        );
        assert!(log_of(&buyer).contains("AGENTS AGREED AT $4.04 (CONFIRM TO SETTLE)."));

        for message in &wire {
            let bare = without_ids(message);
            assert!(!bare.contains(&FLOOR.to_string()), "floor leaked: {bare}");
            assert!(
                !bare.contains(&CEILING.to_string()),
                "ceiling leaked: {bare}"
            );
        }
    }

    #[tokio::test]
    async fn a_buyer_who_hears_a_seller_invites_it_to_open() {
        let seller = device(draft(Action::Sell, above(FLOOR)), no_model()).await;
        let buyer = device(draft(Action::Buy, under(CEILING)), no_model()).await;

        let invite = on_intent(&buyer.node, public(&seller)).await;
        assert!(invite[0].contains(r#""type":"Invite""#), "{invite:?}");

        exchange(&seller, &buyer, invite, false).await;
        assert_eq!(deal_price(&seller), Some(UsdPrice::from_cents(404)));
        assert_eq!(deal_price(&buyer), deal_price(&seller));
    }

    #[tokio::test]
    async fn agreement_requires_user_confirmation() {
        let (seller, buyer, offer) = opened(no_model(), no_model()).await;
        let wire = exchange(&seller, &buyer, offer, true).await;

        for message in &wire {
            let kind = serde_json::from_str::<serde_json::Value>(message).unwrap()["type"].clone();
            assert!(
                ["Offer", "Counter", "Accept"].contains(&kind.as_str().unwrap()),
                "{message}"
            );
        }
        for side in [&seller, &buyer] {
            let own = side.node.ledger.get(&side.own).unwrap();
            assert_eq!(
                own.deal.unwrap().signed_until_ms,
                None,
                "nothing signs until the user says so"
            );
            assert!(matches!(
                own.status,
                IntentStatus::Negotiating { best: Some(_), .. }
            ));
        }
    }

    /// Only a signature can say who the other side is. A name from an unauthenticated
    /// message would also stop `near_settlement` recording the real one.
    #[tokio::test]
    async fn no_counterparty_is_named_until_it_signs() {
        let (seller, buyer, offer) = opened(no_model(), no_model()).await;
        exchange(&seller, &buyer, offer, true).await;

        assert!(deal_price(&seller).is_some());
        for side in [&seller, &buyer] {
            assert_eq!(side.node.ledger.get(&side.own).unwrap().counterparty, None);
        }
    }

    #[tokio::test]
    async fn limits_that_do_not_overlap_form_no_deal() {
        let seller = device(draft(Action::Sell, above(450)), no_model()).await;
        let buyer = device(draft(Action::Buy, under(380)), no_model()).await;
        let offer = on_intent(&seller.node, public(&buyer)).await;
        exchange(&seller, &buyer, offer, true).await;

        assert_eq!(deal_price(&seller), None);
        assert_eq!(deal_price(&buyer), None);
        assert!(
            log_of(&seller).contains("NO DEAL AFTER 3 ROUNDS"),
            "{}",
            log_of(&seller)
        );
    }

    #[tokio::test]
    async fn a_tool_calling_seller_trades_with_a_device_with_no_model() {
        let propose = json!({ "choices": [{ "message": { "role": "assistant", "content": null, "tool_calls": [
            { "id": "p", "type": "function", "function": { "name": "propose_offer", "arguments": r#"{"price_usd":4.1}"# } }
        ] } }] });
        let (url, calls) = mock::serve_forever(propose.to_string());
        let (seller, buyer, offer) = opened(mock::client(&url, None), no_model()).await;

        exchange(&seller, &buyer, offer, true).await;

        assert_eq!(deal_price(&seller), Some(UsdPrice::from_cents(410)));
        assert_eq!(deal_price(&buyer), Some(UsdPrice::from_cents(410)));
        assert!(calls.load(std::sync::atomic::Ordering::SeqCst) >= 1);
    }

    fn always_proposes(price: &str) -> (String, Arc<std::sync::atomic::AtomicUsize>) {
        let body = json!({ "choices": [{ "message": { "role": "assistant", "content": null, "tool_calls": [
            { "id": "p", "type": "function", "function": { "name": "propose_offer", "arguments": format!(r#"{{"price_usd":{price}}}"#) } }
        ] } }] });
        mock::serve_forever(body.to_string())
    }

    /// Two real llama3.1 did this: $4.00 and $3.95 three times, with both prices inside
    /// both limits, and no trade.
    #[tokio::test]
    async fn stubborn_models_still_trade_if_the_last_price_fits() {
        let (seller_url, seller_calls) = always_proposes("4.00");
        let (buyer_url, _) = always_proposes("3.95");
        let (seller, buyer, offer) = opened(
            mock::client(&seller_url, None),
            mock::client(&buyer_url, None),
        )
        .await;

        exchange(&seller, &buyer, offer, true).await;

        assert_eq!(
            deal_price(&seller),
            Some(UsdPrice::from_cents(395)),
            "{}",
            log_of(&seller)
        );
        assert_eq!(deal_price(&buyer), deal_price(&seller));
        let asked = seller_calls.load(std::sync::atomic::Ordering::SeqCst);
        assert_eq!(asked, 3, "the last move needs no model");
    }

    #[tokio::test]
    async fn the_last_price_is_not_taken_if_it_is_past_the_limit() {
        let (seller_url, _) = always_proposes("4.00");
        let (buyer_url, _) = always_proposes("3.50");
        let (seller, buyer, offer) = opened(
            mock::client(&seller_url, None),
            mock::client(&buyer_url, None),
        )
        .await;

        exchange(&seller, &buyer, offer, true).await;

        assert_eq!(deal_price(&seller), None);
        assert!(
            log_of(&seller).contains("NO DEAL AFTER 3 ROUNDS"),
            "{}",
            log_of(&seller)
        );
    }

    #[tokio::test]
    async fn the_last_move_with_no_market_price_sends_nothing() {
        // Stale by the end: an intent with no limit would otherwise take any price at all.
        fn much_later() -> u64 {
            NOON_MS + crate::price_feed::STALE_AFTER_MS + 1
        }
        let (url, _) = always_proposes("4.00");
        let (buyer_url, _) = always_proposes("0.01");
        let mut seller = device(
            draft(Action::Sell, Condition::Any),
            mock::client(&url, None),
        )
        .await;
        let buyer = device(
            draft(Action::Buy, Condition::Any),
            mock::client(&buyer_url, None),
        )
        .await;
        let offer = on_intent(&seller.node, public(&buyer)).await;
        seller.node.clock = much_later;

        exchange(&seller, &buyer, offer, true).await;

        assert_eq!(deal_price(&seller), None);
    }

    #[tokio::test]
    async fn a_model_price_past_the_limit_is_never_sent() {
        // A model with no tools gets no chance to retry, so this reaches the state machine.
        let settings = |url: &str| LlmSettings {
            base_url: url.into(),
            tools_supported: Some(false),
            ..LlmSettings::default()
        };
        let (url, _calls) = mock::serve_forever(mock::reply(r#"{"price_usd": 3.00}"#).1);
        let client = LlmClient::new(
            settings(&url),
            None,
            reqwest::Client::new(),
            Arc::new(LlmBudget::new(100)),
        );
        let seller = device(draft(Action::Sell, above(FLOOR)), client).await;
        let buyer = device(draft(Action::Buy, under(CEILING)), no_model()).await;

        assert!(on_intent(&seller.node, public(&buyer)).await.is_empty());
        assert!(
            log_of(&seller).contains("OUTSIDE YOUR LIMIT"),
            "{}",
            log_of(&seller)
        );
    }

    #[tokio::test]
    async fn flood_is_capped_by_budget() {
        // Every intent waits on its own negotiation, so nothing shares one.
        let (url, calls) = mock::serve_forever(mock::reply("no comment").1);
        let client = mock::client_with_budget(&url, None, 30);
        let seller = device(draft(Action::Sell, above(FLOOR)), client).await;
        for _ in 0..99 {
            let own = seller
                .node
                .ledger
                .create(draft(Action::Sell, above(FLOOR)), NOON_MS).unwrap();
            seller
                .node
                .ledger
                .advance(&own.id, IntentStatus::Broadcast { route_len: 1 }, NOON_MS)
                .unwrap();
        }

        for i in 0..100 {
            let flood = PublicIntent {
                id: format!("flood-{i}"),
                chain: Chain::NearTestnet,
                asset: "wNEAR".into(),
                action: Action::Buy,
                amount: TokenAmount::parse("1", 24).unwrap(),
            };
            on_intent(&seller.node, flood).await;
        }

        assert_eq!(calls.load(std::sync::atomic::Ordering::SeqCst), 30);
        assert_eq!(
            seller.node.agents.lock().len(),
            100,
            "the test really did start 100"
        );
    }

    #[tokio::test]
    async fn timeout_returns_intent_to_open() {
        let (seller, buyer, _offer) = opened(no_model(), no_model()).await;
        assert!(seller.node.agents.is_busy(&seller.own));

        seller
            .node
            .agents
            .tick(&seller.node.ledger, noon() + ROUND_TIMEOUT_MS + 1);

        let own = seller.node.ledger.get(&seller.own).unwrap();
        assert!(own.status.is_active());
        assert_eq!(own.deal, None);
        assert!(log_of(&seller).contains("TIMED OUT"), "{}", log_of(&seller));
        assert!(!seller.node.agents.is_busy(&seller.own));

        // Open means open: another peer can start again.
        let next = public_of(&buyer, "someone-else");
        assert_eq!(on_intent(&seller.node, next).await.len(), 1);
    }

    #[tokio::test]
    async fn finished_negotiations_are_forgotten_eventually() {
        let (seller, _buyer, _offer) = opened(no_model(), no_model()).await;
        let agents = &seller.node.agents;
        agents.tick(&seller.node.ledger, noon() + ROUND_TIMEOUT_MS + 1);
        assert_eq!(
            agents.lock().len(),
            1,
            "kept, so a resent message cannot restart it"
        );
        agents.tick(
            &seller.node.ledger,
            noon() + ROUND_TIMEOUT_MS + 1 + KEEP_FINISHED_MS,
        );
        assert_eq!(agents.lock().len(), 0);
    }

    #[tokio::test]
    async fn a_resent_offer_after_the_end_does_not_restart_it() {
        let (seller, buyer, offer) = opened(no_model(), no_model()).await;
        assert_eq!(
            on_message(&buyer.node, &offer[0]).await.len(),
            1,
            "the buyer answers"
        );

        buyer
            .node
            .agents
            .tick(&buyer.node.ledger, noon() + ROUND_TIMEOUT_MS + 1);
        assert!(on_message(&buyer.node, &offer[0]).await.is_empty());
        assert_eq!(seller.node.ledger.get(&seller.own).unwrap().deal, None);
    }

    #[tokio::test]
    async fn a_resent_message_does_not_start_a_second_turn() {
        let (_seller, buyer, offer) = opened(no_model(), no_model()).await;
        assert_eq!(on_message(&buyer.node, &offer[0]).await.len(), 1);
        assert!(on_message(&buyer.node, &offer[0]).await.is_empty());
    }

    fn forged(to: &Side_, from: PublicIntent, msg: Msg) -> String {
        let wire = Wire {
            msg,
            to_intent: to.own.to_string(),
            from,
        };
        serde_json::to_string(&wire).unwrap()
    }

    #[tokio::test]
    async fn accept_for_a_price_nobody_offered_forms_no_deal() {
        let (seller, buyer, offer) = opened(no_model(), no_model()).await;
        on_message(&buyer.node, &offer[0]).await;
        let neg_id = deal_id(&buyer.own.to_string(), &seller.own.to_string());

        let forgery = forged(
            &buyer,
            public(&seller),
            Msg::Accept {
                neg_id,
                round: 1,
                price_cents: 100,
            },
        );
        assert!(on_message(&buyer.node, &forgery).await.is_empty());
        assert_eq!(deal_price(&buyer), None);
    }

    #[tokio::test]
    async fn the_old_direct_acceptance_no_longer_forms_a_deal() {
        let (_seller, buyer, _offer) = opened(no_model(), no_model()).await;
        let old = json!({ "type": "DealAccepted", "intentId": buyer.own.to_string(), "ownIntentId": "x",
            "chain": "near-testnet", "address": "mallory", "price": "1.00", "amount": "1000000000000000000000000" });
        assert!(on_message(&buyer.node, &old.to_string()).await.is_empty());
        assert_eq!(deal_price(&buyer), None);
    }

    #[tokio::test]
    async fn an_offer_with_a_wrong_negotiation_id_is_ignored() {
        let (seller, buyer, _offer) = opened(no_model(), no_model()).await;
        let forgery = forged(
            &buyer,
            public(&seller),
            Msg::Offer {
                neg_id: "not:theirs".into(),
                round: 1,
                price_cents: 410,
            },
        );
        assert!(on_message(&buyer.node, &forgery).await.is_empty());
        assert!(buyer.node.agents.lock().is_empty());
    }

    #[tokio::test]
    async fn a_later_round_cannot_open_a_negotiation() {
        let (seller, buyer, _offer) = opened(no_model(), no_model()).await;
        let neg_id = deal_id(&buyer.own.to_string(), &seller.own.to_string());
        let message = forged(
            &buyer,
            public(&seller),
            Msg::Offer {
                neg_id,
                round: 2,
                price_cents: 410,
            },
        );
        assert!(on_message(&buyer.node, &message).await.is_empty());
        assert!(buyer.node.agents.lock().is_empty());
    }

    #[tokio::test]
    async fn an_offer_for_a_different_size_is_ignored() {
        let (seller, buyer, _offer) = opened(no_model(), no_model()).await;
        let mut from = public(&seller);
        from.amount = TokenAmount::parse("2", 24).unwrap();
        let neg_id = deal_id(&buyer.own.to_string(), &from.id);
        let message = forged(
            &buyer,
            from,
            Msg::Offer {
                neg_id,
                round: 1,
                price_cents: 410,
            },
        );
        assert!(on_message(&buyer.node, &message).await.is_empty());
        assert!(buyer.node.agents.lock().is_empty());
    }

    #[tokio::test]
    async fn crossed_accepts_leave_the_intent_open() {
        let (seller, buyer, offer) = opened(no_model(), no_model()).await;
        let neg_id = deal_id(&buyer.own.to_string(), &seller.own.to_string());
        // A seller's 4.13 opening. Have the buyer accept it, then hear a different price.
        let opening: serde_json::Value = serde_json::from_str(&offer[0]).unwrap();
        let (round, price) = (
            opening["round"].as_u64().unwrap(),
            opening["price_cents"].as_u64().unwrap(),
        );
        {
            let mut active = buyer.node.agents.lock();
            let own = buyer.node.ledger.get(&buyer.own).unwrap();
            drop(active);
            buyer
                .node
                .agents
                .start(&own, public(&seller), noon())
                .unwrap();
            active = buyer.node.agents.lock();
            let entry = active.get_mut(&neg_id).unwrap();
            entry.negotiation.receive(
                &Msg::Offer {
                    neg_id: neg_id.clone(),
                    round: 1,
                    price_cents: price,
                },
                noon(),
            );
            entry.negotiation.propose(price, noon()).unwrap();
        }

        let round = u8::try_from(round).unwrap();
        let crossed = forged(
            &buyer,
            public(&seller),
            Msg::Accept {
                neg_id,
                round,
                price_cents: price - 1,
            },
        );
        assert!(on_message(&buyer.node, &crossed).await.is_empty());

        assert_eq!(deal_price(&buyer), None);
        assert!(
            log_of(&buyer).contains("DIFFERENT PRICES"),
            "{}",
            log_of(&buyer)
        );
        assert!(buyer
            .node
            .ledger
            .get(&buyer.own)
            .unwrap()
            .status
            .is_active());
    }

    #[tokio::test]
    async fn a_cancelled_intent_stops_negotiating() {
        let (_seller, buyer, offer) = opened(no_model(), no_model()).await;
        buyer
            .node
            .ledger
            .advance(&buyer.own, IntentStatus::Cancelled, noon())
            .unwrap();
        assert!(on_message(&buyer.node, &offer[0]).await.is_empty());
    }

    #[tokio::test]
    async fn a_stale_price_sends_nothing_and_says_so() {
        fn much_later() -> u64 {
            NOON_MS + crate::price_feed::STALE_AFTER_MS + 1
        }
        let mut seller = device(draft(Action::Sell, above(FLOOR)), no_model()).await;
        seller.node.clock = much_later;
        let buyer = device(draft(Action::Buy, under(CEILING)), no_model()).await;

        assert!(on_intent(&seller.node, public(&buyer)).await.is_empty());
        assert!(
            log_of(&seller).contains("AGENT WILL NOT OFFER"),
            "{}",
            log_of(&seller)
        );
    }

    #[tokio::test]
    async fn one_negotiation_at_a_time_per_intent() {
        let (seller, buyer, _offer) = opened(no_model(), no_model()).await;
        assert!(on_intent(&seller.node, public_of(&buyer, "another-buyer"))
            .await
            .is_empty());
    }

    /// `cargo test --lib two_real_models -- --ignored --nocapture`
    #[tokio::test]
    #[ignore = "needs Ollama running with the default model"]
    async fn two_real_models_negotiate() {
        let real = || {
            LlmClient::new(
                LlmSettings::default(),
                None,
                reqwest::Client::new(),
                Arc::new(LlmBudget::new(100)),
            )
        };
        let (seller, buyer, offer) = opened(real(), real()).await;
        let wire = exchange(&seller, &buyer, offer, true).await;

        for message in &wire {
            let said: serde_json::Value = serde_json::from_str(message).unwrap();
            println!(
                "{} round {} at {}",
                said["type"], said["round"], said["price_cents"]
            );
        }
        println!(
            "deal: seller {:?} buyer {:?}",
            deal_price(&seller),
            deal_price(&buyer)
        );
        assert_eq!(deal_price(&seller), deal_price(&buyer));
        if let Some(price) = deal_price(&seller) {
            assert!((FLOOR..=CEILING).contains(&price.cents()), "{price}");
        }
    }

    #[test]
    fn only_a_buy_under_or_a_sell_above_sets_a_limit() {
        let limit = |action, condition| bound_of(&draft(action, condition)).unwrap().limit_cents;
        assert_eq!(limit(Action::Buy, under(420)), Some(420));
        assert_eq!(limit(Action::Sell, above(390)), Some(390));
        assert_eq!(limit(Action::Buy, above(390)), None);
        assert_eq!(limit(Action::Sell, under(420)), None);
        assert_eq!(limit(Action::Buy, Condition::Any), None);
        assert!(bound_of(&draft(Action::Swap, Condition::Any)).is_none());
    }
}
