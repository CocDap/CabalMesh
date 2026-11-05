//! Local, autonomous price negotiation between buy and sell intents.
//!
//! # What runs where
//!
//! Every node runs its own copy of this: there is no matchmaker service, only
//! each device's own [`crate::intents::Ledger`] and whatever local Ollama
//! model it has configured. When an intent arrives from the mesh, this module
//! checks it against this device's *own* open intents for the opposite side
//! of the same asset. If the price ranges overlap, the local model proposes a
//! settlement price, and this device broadcasts a `DealAccepted` — the exact
//! wire message [`crate::intents::apply_mesh_event`] already knows how to
//! read, it just never had a sender before this.
//!
//! # Why the model is never trusted on the number
//!
//! An LLM decides *which* price within a range reads as fair, never *whether* a
//! price is acceptable. The
//! proposed price is checked against the buyer's ceiling and the seller's
//! floor before anything is broadcast, and an unreachable or malformed
//! response falls back to a deterministic split rather than blocking the
//! match — the trade is real even when the model is not running.
//!
//! # What "confirmation" means on each side
//!
//! The buyer's confirmation is the existing `DealAccepted` handler in
//! `intents.rs`: it already sets a counterparty and moves the intent to
//! `Negotiating` the moment such a message arrives, which is what this module
//! now causes to happen. The seller's confirmation — this device, when it is
//! the one that matched — is set directly on its own ledger entry here,
//! since it needs no round trip to learn about a decision it just made.

use crate::intents::{line, now_ms};
use crate::mesh::PrivacyIntent;
use crate::state::AppState;
use cabal_core::{Action, Condition, IntentDraft, IntentStatus, UsdPrice};
use serde::Deserialize;
use std::time::Duration;

/// How long to wait on the local model before falling back to a plain split
/// of the range. Short on purpose: this runs unattended in the background for
/// every intent the mesh delivers, and a slow or hung Ollama must not stall
/// the mesh event loop it competes with — see the `tokio::spawn` at each call
/// site in `lib.rs`.
const NEGOTIATION_TIMEOUT: Duration = Duration::from_secs(20);

/// Considers one intent freshly received from the mesh: does it complete a
/// trade with anything this device is itself offering, and if so, at what
/// price?
///
/// Called at most once per distinct intent id — the call sites in `lib.rs`
/// gate it behind [`crate::intents::ReceivedLog::record`]'s "was this new"
/// result, so a BLE resend of the same intent can never propose (and
/// broadcast) the same deal twice.
pub async fn consider(state: &AppState, incoming: &PrivacyIntent) {
    // Privacy is the transport's job elsewhere in this codebase (see
    // `commands::publish`'s comment on `encrypted`), but this module can only
    // match on what it can actually read.
    if incoming.encrypted {
        return;
    }
    let Ok(theirs) = serde_json::from_str::<IntentDraft>(&incoming.payload) else {
        return;
    };
    // Swap and Stake have no natural counter-side in this model — only Buy
    // and Sell describe two parties trading the same asset.
    if !matches!(theirs.action, Action::Buy | Action::Sell) {
        return;
    }

    let Ok(services) = state.services() else {
        return;
    };
    let Some(our_account) = services.identity.lock().await.primary_account() else {
        // The vault is locked: this device could not sign what it proposed.
        return;
    };

    let ledger = state.intents();
    let Some((own_id, own_draft, own_status, floor, ceiling)) =
        ledger.all().into_iter().find_map(|own| {
            if !own.status.is_active() {
                return None;
            }
            if own.draft.asset != theirs.asset || !own.draft.chain.matches(theirs.chain) {
                return None;
            }
            // No partial fills: each side signs its whole amount, and a pair
            // of unequal sizes could never net to zero.
            if own.draft.amount != theirs.amount {
                return None;
            }
            // One deal at a time. Another deal may already hold this intent,
            // and a signed diff cannot be recalled.
            if own.deal.is_some() {
                return None;
            }
            if !opposite_sides(own.draft.action, theirs.action) {
                return None;
            }
            price_bounds(&own.draft, &theirs)
                .map(|(floor, ceiling)| (own.id, own.draft, own.status, floor, ceiling))
        })
    else {
        return;
    };

    let ollama_url = crate::ollama_config::url();
    let (price, method) = negotiate_price(&ollama_url, &own_draft, &theirs, floor, ceiling).await;
    tracing::info!(
        target: "cabalmesh::negotiation",
        asset = %own_draft.asset,
        %price,
        method,
        "matched a received intent against an open one"
    );

    let deal = serde_json::json!({
        "type": "DealAccepted",
        "intentId": incoming.id,
        "ownIntentId": own_id.to_string(),
        "chain": own_draft.chain,
        "address": our_account.as_str(),
        // Plain, not `$4.04`: `UsdPrice::parse` reads this back on the far side.
        "price": format!("{}.{:02}", price.cents() / 100, price.cents() % 100),
        "amount": own_draft.amount.raw().to_string(),
    })
    .to_string();
    let deal_intent = PrivacyIntent {
        // Unused by the receiving side (it keys off `intentId` inside the
        // payload), but every `PrivacyIntent` carries one, and a settlement
        // message is exactly as identifiable as any other.
        id: format!("deal-{}-{}", incoming.id, now_ms()),
        intent_type: "settlement".into(),
        payload: deal,
        encrypted: false,
        relay_path: vec!["origin_node".into()],
        relay_fee: None,
    };
    if crate::commands::publish_over_mesh(&services, &deal_intent).await.is_err() {
        let _ = crate::commands::publish_over_ble(&services, &deal_intent).await;
    }

    // Our own confirmation: the counterparty's `DealAccepted` message above
    // is what gives *them* one, via the handler already in
    // `intents::apply_mesh_event`. This device made the decision itself, so
    // it records the same fact locally rather than waiting to hear it back.
    let deal_id = crate::near_settlement::deal_id(&own_id.to_string(), &incoming.id);
    if ledger.propose_deal(&own_id, &deal_id, price, &incoming.id) == crate::intents::DealChange::Ignored {
        return;
    }

    let bids = match own_status {
        IntentStatus::Negotiating { bids, .. } => bids.saturating_add(1),
        _ => 1,
    };
    if ledger
        .advance(&own_id, IntentStatus::Negotiating { bids, best: Some(price) }, now_ms())
        .is_ok()
    {
        ledger.record(
            &own_id,
            line(
                format!(
                    "AI MATCHED WITH {} {} AT {price} (CONFIRM TO SETTLE).",
                    action_label(theirs.action),
                    theirs.asset,
                ),
                crate::bindings::LogTone::Ok,
            ),
        );
    }
}

/// Whether `price` honours the bound this user stated.
///
/// Checked on a peer's proposal before it becomes a deal: a peer can send any
/// number, and only this device can enforce what its user asked for.
#[must_use]
pub fn price_acceptable(draft: &IntentDraft, price: UsdPrice) -> bool {
    match draft.action {
        Action::Buy => buyer_ceiling(draft).is_none_or(|ceiling| price <= ceiling),
        Action::Sell => seller_floor(draft).is_none_or(|floor| price >= floor),
        Action::Swap | Action::Stake => false,
    }
}

/// Whether two actions describe opposite sides of the same trade.
const fn opposite_sides(a: Action, b: Action) -> bool {
    matches!((a, b), (Action::Buy, Action::Sell) | (Action::Sell, Action::Buy))
}

fn action_label(action: Action) -> &'static str {
    match action {
        Action::Buy => "BUY",
        Action::Sell => "SELL",
        Action::Swap => "SWAP",
        Action::Stake => "STAKE",
    }
}

/// The buyer's price ceiling, if its condition sets one. A buyer who picked
/// `Above` or `Any` has stated no upper limit for matching purposes — only
/// `Under` is a cap.
fn buyer_ceiling(draft: &IntentDraft) -> Option<UsdPrice> {
    match draft.condition {
        Condition::Under { price } => Some(price),
        Condition::Above { .. } | Condition::Any => None,
    }
}

/// The seller's price floor, if its condition sets one. Only `Above` is a
/// floor, by the same rule as `buyer_ceiling`.
fn seller_floor(draft: &IntentDraft) -> Option<UsdPrice> {
    match draft.condition {
        Condition::Above { price } => Some(price),
        Condition::Under { .. } | Condition::Any => None,
    }
}

/// The `[floor, ceiling]` a settlement price must land in for `own` and
/// `theirs` — one buying, one selling — to be a real match.
///
/// `None` when there is no overlap (the seller wants more than the buyer will
/// pay) or when neither side stated a bound at all: two `Any`-priced intents
/// have nothing for a fair price to anchor to, so they are left for a human
/// to negotiate rather than matched on a price this code invented outright.
fn price_bounds(own: &IntentDraft, theirs: &IntentDraft) -> Option<(Option<UsdPrice>, Option<UsdPrice>)> {
    let (buyer, seller) = if own.action == Action::Buy { (own, theirs) } else { (theirs, own) };
    let ceiling = buyer_ceiling(buyer);
    let floor = seller_floor(seller);

    if ceiling.is_none() && floor.is_none() {
        return None;
    }
    if let (Some(floor), Some(ceiling)) = (floor, ceiling) {
        if floor > ceiling {
            return None;
        }
    }
    Some((floor, ceiling))
}

/// Settles on one price within `[floor, ceiling]` (either bound may be
/// absent, never both — see [`price_bounds`]): asks the local model for a
/// fair number, and falls back to a deterministic one whenever the model is
/// unreachable, slow, or answers with something outside the range this
/// device already verified independently.
async fn negotiate_price(
    ollama_url: &str,
    own: &IntentDraft,
    theirs: &IntentDraft,
    floor: Option<UsdPrice>,
    ceiling: Option<UsdPrice>,
) -> (UsdPrice, &'static str) {
    match tokio::time::timeout(NEGOTIATION_TIMEOUT, ask_model(ollama_url, own, theirs, floor, ceiling)).await {
        Ok(Some(price)) if within(price, floor, ceiling) => (price, "ai"),
        _ => (deterministic_settlement(floor, ceiling), "fallback"),
    }
}

fn within(price: UsdPrice, floor: Option<UsdPrice>, ceiling: Option<UsdPrice>) -> bool {
    floor.is_none_or(|f| price >= f) && ceiling.is_none_or(|c| price <= c)
}

/// A price within range with no model involved: the midpoint when both sides
/// gave a bound, or whichever single bound exists otherwise — meeting the one
/// side that actually named a number.
fn deterministic_settlement(floor: Option<UsdPrice>, ceiling: Option<UsdPrice>) -> UsdPrice {
    match (floor, ceiling) {
        (Some(floor), Some(ceiling)) => {
            UsdPrice::from_cents(floor.cents() + (ceiling.cents() - floor.cents()) / 2)
        }
        (Some(floor), None) => floor,
        (None, Some(ceiling)) => ceiling,
        (None, None) => unreachable!("price_bounds never returns with both bounds absent"),
    }
}

#[derive(Debug, Deserialize)]
struct PriceResponse {
    price: f64,
}

#[derive(Debug, serde::Serialize)]
struct OllamaRequest {
    model: &'static str,
    prompt: String,
    stream: bool,
    system: String,
    format: &'static str,
}

#[derive(Debug, Deserialize)]
struct OllamaResponse {
    response: String,
}

/// Asks the local model to propose one fair settlement price for the trade.
/// `None` on any failure — network, timeout, or a response that does not
/// parse — which the caller treats identically to "the model disagreed with
/// itself": fall back rather than error, since a background match is not a
/// user action waiting on a result.
async fn ask_model(
    ollama_url: &str,
    own: &IntentDraft,
    theirs: &IntentDraft,
    floor: Option<UsdPrice>,
    ceiling: Option<UsdPrice>,
) -> Option<UsdPrice> {
    // Both sides trade the same asset by construction — `price_bounds`
    // already checked `own.asset == theirs.asset` before this was called.
    let buyer_asset = if own.action == Action::Buy { own } else { theirs };
    let system = format!(
        r#"You are settling a peer-to-peer trade between a buyer and a seller of {asset}. Propose one fair price in US dollars.
Buyer will pay at most: {ceiling}
Seller will accept at least: {floor}
The price you propose MUST be within that range.
Respond ONLY with JSON in this exact format: {{"price": <number>}}"#,
        asset = buyer_asset.asset,
        ceiling = ceiling.map_or("no stated limit".to_string(), |c| c.to_string()),
        floor = floor.map_or("no stated limit".to_string(), |f| f.to_string()),
    );

    let request = OllamaRequest {
        model: crate::ollama_config::INTENT_MODEL,
        prompt: "Propose the settlement price now.".to_string(),
        stream: false,
        system,
        format: "json",
    };

    let response = reqwest::Client::new()
        .post(format!("{ollama_url}/api/generate"))
        .json(&request)
        .send()
        .await
        .ok()?
        .error_for_status()
        .ok()?;
    let body: OllamaResponse = response.json().await.ok()?;
    let parsed: PriceResponse =
        serde_json::from_str(crate::llm_json::extract_json_object(&body.response)).ok()?;

    if !parsed.price.is_finite() || parsed.price < 0.0 {
        return None;
    }
    Some(UsdPrice::from_cents((parsed.price * 100.0).round() as u64))
}

#[cfg(test)]
mod tests {
    use super::*;
    use cabal_core::{ExecutionMode, PrivacyLevel, TokenAmount};

    fn draft(action: Action, asset: &str, condition: Condition) -> IntentDraft {
        IntentDraft {
            chain: cabal_core::Chain::NearTestnet,
            action,
            asset: asset.into(),
            condition,
            amount: TokenAmount::parse("1", 24).unwrap(),
            mode: ExecutionMode::Shark,
            privacy: PrivacyLevel::Low,
        }
    }

    fn usd(cents: u64) -> UsdPrice {
        UsdPrice::from_cents(cents)
    }

    #[test]
    fn a_buyer_ceiling_above_a_seller_floor_overlaps() {
        let buy = draft(Action::Buy, "wNEAR", Condition::Under { price: usd(1200) });
        let sell = draft(Action::Sell, "wNEAR", Condition::Above { price: usd(900) });
        assert_eq!(price_bounds(&buy, &sell), Some((Some(usd(900)), Some(usd(1200)))));
    }

    #[test]
    fn a_seller_asking_more_than_the_buyer_will_pay_does_not_match() {
        let buy = draft(Action::Buy, "wNEAR", Condition::Under { price: usd(700) });
        let sell = draft(Action::Sell, "wNEAR", Condition::Above { price: usd(900) });
        assert_eq!(price_bounds(&buy, &sell), None);
    }

    #[test]
    fn two_any_priced_intents_have_no_anchor_to_match_on() {
        let buy = draft(Action::Buy, "wNEAR", Condition::Any);
        let sell = draft(Action::Sell, "wNEAR", Condition::Any);
        assert_eq!(price_bounds(&buy, &sell), None);
    }

    #[test]
    fn an_unconstrained_buyer_matches_the_sellers_floor() {
        let buy = draft(Action::Buy, "wNEAR", Condition::Any);
        let sell = draft(Action::Sell, "wNEAR", Condition::Above { price: usd(900) });
        assert_eq!(price_bounds(&buy, &sell), Some((Some(usd(900)), None)));
        assert_eq!(deterministic_settlement(Some(usd(900)), None), usd(900));
    }

    #[test]
    fn same_side_actions_never_match() {
        let buy_one = draft(Action::Buy, "wNEAR", Condition::Under { price: usd(1200) });
        let buy_two = draft(Action::Buy, "wNEAR", Condition::Above { price: usd(900) });
        assert!(!opposite_sides(buy_one.action, buy_two.action));
    }

    #[test]
    fn the_midpoint_sits_evenly_between_both_bounds() {
        assert_eq!(deterministic_settlement(Some(usd(900)), Some(usd(1200))), usd(1050));
    }

    #[tokio::test]
    async fn an_unreachable_model_still_settles_within_range() {
        // A bound-then-dropped listener refuses the connection immediately,
        // independent of whatever Ollama the developer running this test may
        // or may not have listening on the real default URL — see the same
        // pattern in `intent_chat.rs`'s `unreachable_model_is_reported...`.
        let listener = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
        let address = listener.local_addr().unwrap();
        drop(listener);

        let buy = draft(Action::Buy, "wNEAR", Condition::Under { price: usd(1200) });
        let sell = draft(Action::Sell, "wNEAR", Condition::Above { price: usd(900) });
        let (price, method) =
            negotiate_price(&format!("http://{address}"), &buy, &sell, Some(usd(900)), Some(usd(1200))).await;
        assert_eq!(method, "fallback");
        assert!(within(price, Some(usd(900)), Some(usd(1200))));
    }
}
