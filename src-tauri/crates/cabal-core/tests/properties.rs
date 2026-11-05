//! Property tests for the domain invariants.
//!
//! These are the rules that matter and that example-based tests miss: they
//! hold for *every* status pair and *every* amount, not the handful someone
//! thought to write down.
//!
//! They run here rather than in the app crate precisely because `cabal-core`
//! has no I/O — thousands of cases execute in milliseconds instead of behind a
//! cross-compile.

use cabal_core::intent::{Condition, FailureReason, IntentStatus};
use cabal_core::money::{TokenAmount, UsdPrice};
use cabal_core::ProofHash;
use proptest::prelude::*;

// ---------------------------------------------------------------------------
// Strategies
// ---------------------------------------------------------------------------

fn any_status() -> impl Strategy<Value = IntentStatus> {
    prop_oneof![
        Just(IntentStatus::Draft),
        any::<u8>().prop_map(|route_len| IntentStatus::Broadcast { route_len }),
        (any::<u8>(), any::<Option<u64>>()).prop_map(|(bids, best)| IntentStatus::Negotiating {
            bids,
            best: best.map(UsdPrice::from_cents),
        }),
        Just(IntentStatus::FindingRoute),
        Just(IntentStatus::Waiting),
        (any::<u64>(), any::<u32>()).prop_map(|(cents, elapsed_ms)| IntentStatus::Settled {
            proof: ProofHash::new("0xa4f2c9e1b70d5533"),
            filled_at: UsdPrice::from_cents(cents),
            elapsed_ms,
        }),
        Just(IntentStatus::Failed { reason: FailureReason::NoRoute }),
        Just(IntentStatus::Cancelled),
    ]
}

/// A decimal string that is always parseable at `decimals` precision.
fn amount_string(decimals: u8) -> impl Strategy<Value = String> {
    (0u64..1_000_000_000, 0u32..1_000_000).prop_map(move |(whole, frac)| {
        if decimals == 0 {
            return whole.to_string();
        }
        let places = usize::from(decimals).min(6);
        let fraction = format!("{frac:0>6}")[..places].to_string();
        format!("{whole}.{fraction}")
    })
}

// ---------------------------------------------------------------------------
// Intent lifecycle
// ---------------------------------------------------------------------------

proptest! {
    /// The invariant the whole state machine rests on. If a terminal state
    /// could ever be left, a settled intent could be re-settled — and that is
    /// money moving twice.
    #[test]
    fn terminal_states_never_transition(from in any_status(), to in any_status()) {
        if from.is_terminal() {
            prop_assert!(!from.can_transition_to(&to));
        }
    }

    /// Broadcasting is irreversible: once an intent has left the device there
    /// is no honest way to claim it never did.
    #[test]
    fn nothing_ever_returns_to_draft(from in any_status()) {
        prop_assert!(!from.can_transition_to(&IntentStatus::Draft));
    }

    /// No self-loop is meaningful except re-negotiation, where the bid count
    /// genuinely changes.
    #[test]
    fn only_negotiating_may_repeat(status in any_status()) {
        let repeats = status.can_transition_to(&status);
        let is_negotiating = matches!(status, IntentStatus::Negotiating { .. });
        prop_assert_eq!(repeats, is_negotiating);
    }

    /// A live intent can always be abandoned. If this ever failed, the UI
    /// would show a cancel button that silently does nothing.
    #[test]
    fn every_live_state_can_be_cancelled(status in any_status()) {
        if status.is_active() || matches!(status, IntentStatus::Draft) {
            prop_assert!(status.can_transition_to(&IntentStatus::Cancelled));
        }
    }

    /// Active and terminal partition the space; nothing is both, and the two
    /// predicates are used to drive different UI affordances.
    #[test]
    fn active_and_terminal_are_disjoint(status in any_status()) {
        prop_assert!(!(status.is_active() && status.is_terminal()));
    }

    /// Settlement requires a route. Reaching settled from broadcast or
    /// negotiating would mean settling through a path never established.
    #[test]
    fn settlement_requires_routing(cents in any::<u64>()) {
        let settled = IntentStatus::Settled {
            proof: ProofHash::new("0x0"),
            filled_at: UsdPrice::from_cents(cents),
            elapsed_ms: 0,
        };
        // Bound to locals rather than inlined: prop_assert! stringifies its
        // expression into a format string, and a struct literal's braces
        // break that.
        let draft = IntentStatus::Draft;
        let broadcast = IntentStatus::Broadcast { route_len: 3 };
        let routing = IntentStatus::FindingRoute;
        let waiting = IntentStatus::Waiting;

        prop_assert!(!draft.can_transition_to(&settled));
        prop_assert!(!broadcast.can_transition_to(&settled));
        prop_assert!(routing.can_transition_to(&settled));
        prop_assert!(waiting.can_transition_to(&settled));
    }

    /// Every status survives a serde round trip. The frontend switches on the
    /// serialized tag, so a variant that does not round-trip is a variant the
    /// UI cannot see.
    #[test]
    fn status_round_trips_through_json(status in any_status()) {
        let json = serde_json::to_string(&status).unwrap();
        let back: IntentStatus = serde_json::from_str(&json).unwrap();
        prop_assert_eq!(status, back);
    }
}

// ---------------------------------------------------------------------------
// Money
// ---------------------------------------------------------------------------

proptest! {
    /// Parsing and rendering are inverses. A value that does not survive the
    /// round trip is a value the user typed and the app silently changed.
    #[test]
    fn amounts_round_trip_through_display(decimals in 0u8..=18, s in amount_string(18)) {
        let truncated = truncate_to(&s, decimals);
        let parsed = TokenAmount::parse(&truncated, decimals);
        prop_assume!(parsed.is_ok());
        let amount = parsed.unwrap();

        let reparsed = TokenAmount::parse(&amount.to_plain_string(), decimals).unwrap();
        prop_assert_eq!(amount.raw(), reparsed.raw());
    }

    /// Thousands separators are display sugar and must not change the value —
    /// users paste back exactly what the UI showed them.
    #[test]
    fn separators_never_change_the_value(decimals in 0u8..=18, s in amount_string(18)) {
        let truncated = truncate_to(&s, decimals);
        let parsed = TokenAmount::parse(&truncated, decimals);
        prop_assume!(parsed.is_ok());
        let amount = parsed.unwrap();

        let with_separators = amount.to_string();
        let reparsed = TokenAmount::parse(&with_separators, decimals).unwrap();
        prop_assert_eq!(amount.raw(), reparsed.raw());
    }

    /// Parsing never panics, whatever arrives from the webview. Every input is
    /// either a value or a typed error.
    #[test]
    fn parsing_arbitrary_input_never_panics(s in ".*", decimals in 0u8..=40) {
        let _ = TokenAmount::parse(&s, decimals);
    }

    /// Addition either produces a correct sum or refuses. It never wraps —
    /// wrapping here would turn an overflow into a plausible-looking balance.
    #[test]
    fn addition_overflows_rather_than_wrapping(a in any::<u128>(), b in any::<u128>()) {
        let lhs = TokenAmount::from_raw(a, 18);
        let rhs = TokenAmount::from_raw(b, 18);
        match lhs.checked_add(rhs) {
            Ok(sum) => prop_assert_eq!(sum.raw(), a.checked_add(b).unwrap()),
            Err(_) => prop_assert!(a.checked_add(b).is_none()),
        }
    }

    /// Mixing assets is always refused, whatever the values.
    #[test]
    fn adding_different_assets_always_fails(a in any::<u128>(), b in any::<u128>()) {
        let wnear = TokenAmount::from_raw(a, 24);
        let usdc = TokenAmount::from_raw(b, 6);
        prop_assert!(wnear.checked_add(usdc).is_err());
    }

    /// USD prices render at exactly two decimal places, always.
    #[test]
    fn usd_always_renders_two_decimal_places(cents in any::<u64>()) {
        let rendered = UsdPrice::from_cents(cents).to_string();
        let fraction = rendered.rsplit_once('.').expect("must have a decimal point").1;
        prop_assert_eq!(fraction.len(), 2);
        prop_assert!(rendered.starts_with('$'));
    }

    /// Prices survive their own formatting, `$` and separators included.
    #[test]
    fn usd_round_trips_through_display(cents in 0u64..1_000_000_000_000) {
        let price = UsdPrice::from_cents(cents);
        prop_assert_eq!(UsdPrice::parse(&price.to_string()).unwrap(), price);
    }
}

/// Cuts a decimal string down to at most `decimals` places.
fn truncate_to(s: &str, decimals: u8) -> String {
    match s.split_once('.') {
        None => s.to_string(),
        Some((whole, _)) if decimals == 0 => whole.to_string(),
        Some((whole, fraction)) => {
            let keep = usize::from(decimals).min(fraction.len());
            format!("{whole}.{}", &fraction[..keep])
        }
    }
}

// ---------------------------------------------------------------------------
// Conditions
// ---------------------------------------------------------------------------

proptest! {
    /// `Under` and `Above` never both hold, and never both fail except at the
    /// exact boundary — where neither strict comparison is true.
    #[test]
    fn under_and_above_are_complementary_except_at_the_boundary(
        threshold in any::<u64>(),
        candidate in any::<u64>(),
    ) {
        let price = UsdPrice::from_cents(threshold);
        let value = UsdPrice::from_cents(candidate);

        let under = Condition::Under { price }.is_satisfied_by(value);
        let above = Condition::Above { price }.is_satisfied_by(value);

        prop_assert!(!(under && above));
        if threshold != candidate {
            prop_assert!(under || above);
        } else {
            prop_assert!(!under && !above);
        }
    }

    /// `Any` accepts everything — that is the whole meaning of the variant.
    #[test]
    fn any_condition_accepts_every_price(candidate in any::<u64>()) {
        prop_assert!(Condition::Any.is_satisfied_by(UsdPrice::from_cents(candidate)));
    }
}
