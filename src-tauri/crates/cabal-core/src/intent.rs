//! The intent model: what the user asks for, and where it is in its life.

use crate::ids::ProofHash;
use crate::money::{TokenAmount, UsdPrice};
use serde::{Deserialize, Serialize};

#[cfg(feature = "ts-rs")]
use ts_rs::TS;

/// What the user is trying to do — the `I WANT TO` segmented control.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[cfg_attr(feature = "ts-rs", derive(TS), ts(export, export_to = "../../src/types/bindings.ts"))]
#[serde(rename_all = "UPPERCASE")]
pub enum Action {
    Buy,
    Sell,
    Swap,
    Stake,
}

impl Action {
    /// Every variant, in the order the segmented control shows them.
    pub const ALL: [Self; 4] = [Self::Buy, Self::Sell, Self::Swap, Self::Stake];
}

/// Execution strategy — the `MODE` selector.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[cfg_attr(feature = "ts-rs", derive(TS), ts(export, export_to = "../../src/types/bindings.ts"))]
#[serde(rename_all = "SCREAMING_SNAKE_CASE")]
pub enum ExecutionMode {
    Shark,
    Ghost,
    Patient,
}

impl ExecutionMode {
    pub const ALL: [Self; 3] = [Self::Shark, Self::Ghost, Self::Patient];

    /// The label shown in the selector, e.g. `SHARK MODE`.
    #[must_use]
    pub const fn label(self) -> &'static str {
        match self {
            Self::Shark => "SHARK MODE",
            Self::Ghost => "GHOST MODE",
            Self::Patient => "PATIENT MODE",
        }
    }

    /// The copy shown beneath the selector. Lives here rather than in the
    /// frontend so the mode and its description cannot drift apart.
    #[must_use]
    pub const fn description(self) -> &'static str {
        match self {
            Self::Shark => "Aggressive execution. Best price. Higher risk.",
            Self::Ghost => "Maximum privacy. Longer route. Slower fill.",
            Self::Patient => "Waits for the condition. No slippage tolerance.",
        }
    }
}

/// How much routing privacy to buy, at the cost of speed.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
#[cfg_attr(feature = "ts-rs", derive(TS), ts(export, export_to = "../../src/types/bindings.ts"))]
#[serde(rename_all = "UPPERCASE")]
pub enum PrivacyLevel {
    Low,
    Medium,
    High,
}

impl PrivacyLevel {
    pub const ALL: [Self; 3] = [Self::Low, Self::Medium, Self::High];
}

/// The `CONDITION` row.
///
/// `Any` carries no price, which is the point: the type makes a priceless
/// `Under` unconstructable, so no downstream code has to handle a
/// "price-under-nothing" case that should never exist.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "ts-rs", derive(TS), ts(export, export_to = "../../src/types/bindings.ts"))]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum Condition {
    Under { price: UsdPrice },
    Above { price: UsdPrice },
    Any,
}

impl Condition {
    /// The price this condition tests against, if it tests one.
    #[must_use]
    pub const fn price(&self) -> Option<UsdPrice> {
        match self {
            Self::Under { price } | Self::Above { price } => Some(*price),
            Self::Any => None,
        }
    }

    /// Whether `candidate` satisfies the condition.
    #[must_use]
    pub fn is_satisfied_by(&self, candidate: UsdPrice) -> bool {
        match self {
            Self::Under { price } => candidate < *price,
            Self::Above { price } => candidate > *price,
            Self::Any => true,
        }
    }
}

/// Why an intent failed. A variant rather than a message, so the UI can render
/// on-voice copy instead of echoing an internal string.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "ts-rs", derive(TS), ts(export, export_to = "../../src/types/bindings.ts"))]
#[serde(rename_all = "snake_case")]
#[non_exhaustive]
pub enum FailureReason {
    /// No route through the mesh satisfied the condition before expiry.
    NoRoute,
    /// A node on the chosen route failed liveness and was slashed.
    NodeFailure,
    /// The condition never became true.
    ConditionUnmet,
    /// Settlement was attempted and rejected on-chain.
    SettlementRejected,
    /// Balance no longer covers the intent.
    InsufficientBalance,
}

/// What a settlement proof hash names.
///
/// Both are base58 hashes of the same length, so the hash alone cannot say
/// which it is, and the proof screen must not call an intent hash a
/// transaction hash.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
#[cfg_attr(feature = "ts-rs", derive(TS), ts(export, export_to = "../../src/types/bindings.ts"))]
#[serde(rename_all = "SCREAMING_SNAKE_CASE")]
pub enum ProofKind {
    /// The transaction that executed the intent, from this device's own
    /// submission or a gateway's report checked against the chain. The
    /// default, so proofs stored before this field existed keep their label.
    #[default]
    Transaction,
    /// This intent's own hash, found spent on chain. The settlement is proven
    /// but the transaction that spent it is not known to this device.
    Intent,
}

/// Where an intent is in its life.
///
/// Matched exhaustively everywhere — no catch-all arm — so adding a variant
/// surfaces every place that has to decide what it means, rather than silently
/// falling through to a default.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[cfg_attr(feature = "ts-rs", derive(TS), ts(export, export_to = "../../src/types/bindings.ts"))]
#[serde(tag = "status", rename_all = "SCREAMING_SNAKE_CASE")]
pub enum IntentStatus {
    /// Composed but not yet broadcast. Nothing has left the device.
    Draft,
    /// Dispatched to the mesh.
    Broadcast { route_len: u8 },
    /// Nodes are bidding.
    Negotiating { bids: u8, best: Option<UsdPrice> },
    /// Accepted, searching for a path.
    FindingRoute,
    /// Waiting for the condition to become true.
    Waiting,
    /// Settled on-chain. Terminal.
    Settled {
        proof: ProofHash,
        /// What `proof` names.
        #[serde(default)]
        proof_kind: ProofKind,
        filled_at: UsdPrice,
        elapsed_ms: u32,
    },
    /// Failed. Terminal.
    Failed { reason: FailureReason },
    /// Cancelled by the user. Terminal. Nothing written.
    Cancelled,
}

impl IntentStatus {
    /// Whether this is an end state. Terminal states accept no successor.
    #[must_use]
    pub const fn is_terminal(&self) -> bool {
        matches!(self, Self::Settled { .. } | Self::Failed { .. } | Self::Cancelled)
    }

    /// Whether the intent is live on the mesh and can still be cancelled.
    #[must_use]
    pub const fn is_active(&self) -> bool {
        matches!(
            self,
            Self::Broadcast { .. } | Self::Negotiating { .. } | Self::FindingRoute | Self::Waiting
        )
    }

    /// Whether `next` is a legal successor of `self`.
    ///
    /// The rules, in one place so they cannot be reimplemented differently by
    /// each caller:
    ///
    /// - A terminal state accepts nothing. Once settled, always settled.
    /// - Nothing returns to `Draft`; broadcasting is irreversible.
    /// - A draft may only be broadcast or cancelled — it cannot jump straight
    ///   to settled, which would mean settling something never sent.
    /// - Any live state may fail or be cancelled.
    #[must_use]
    pub fn can_transition_to(&self, next: &Self) -> bool {
        use IntentStatus as S;

        if self.is_terminal() {
            return false;
        }
        if matches!(next, S::Draft) {
            return false;
        }
        if matches!(next, S::Failed { .. } | S::Cancelled) {
            return true;
        }

        match (self, next) {
            (S::Draft, S::Broadcast { .. }) => true,
            (S::Draft, _) => false,

            (S::Broadcast { .. }, S::Negotiating { .. } | S::FindingRoute | S::Waiting) => true,
            (S::Negotiating { .. }, S::Negotiating { .. } | S::FindingRoute | S::Waiting) => true,
            (S::FindingRoute, S::Negotiating { .. } | S::Waiting | S::Settled { .. }) => true,
            (S::Waiting, S::Negotiating { .. } | S::FindingRoute | S::Settled { .. }) => true,

            // Settlement has to be routed first — otherwise there is nothing
            // to settle through.
            (S::Broadcast { .. } | S::Negotiating { .. }, S::Settled { .. }) => false,

            _ => false,
        }
    }
}

/// Which chain an intent settles on.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[cfg_attr(feature = "ts-rs", derive(TS), ts(export, export_to = "../../src/types/bindings.ts"))]
#[serde(rename_all = "kebab-case")]
pub enum Chain {
    NearTestnet,
    NearMainnet,
}

impl Chain {
    #[must_use]
    pub const fn is_testnet(self) -> bool {
        matches!(self, Self::NearTestnet)
    }

    #[must_use]
    pub const fn label(self) -> &'static str {
        match self {
            Self::NearTestnet => "NEAR testnet",
            Self::NearMainnet => "NEAR",
        }
    }

    /// Whether intents on `self` and `other` can trade with each other.
    #[must_use]
    pub const fn matches(self, other: Self) -> bool {
        match (self, other) {
            (Self::NearTestnet, Self::NearTestnet) | (Self::NearMainnet, Self::NearMainnet) => true,
            _ => false,
        }
    }
}

/// A composed but unsent intent — what the `new` screen produces.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct IntentDraft {
    pub chain: Chain,
    pub action: Action,
    pub asset: Box<str>,
    pub condition: Condition,
    pub amount: TokenAmount,
    pub mode: ExecutionMode,
    pub privacy: PrivacyLevel,
}

/// What an intent tells the mesh. Has no price: the user's limit stays on the device,
/// or a counterparty's agent would simply offer exactly that.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct PublicIntent {
    pub id: String,
    pub chain: Chain,
    pub asset: Box<str>,
    /// Only `Buy` or `Sell` ever trade with another intent.
    pub action: Action,
    pub amount: TokenAmount,
}

impl PublicIntent {
    #[must_use]
    pub fn of(id: String, draft: &IntentDraft) -> Self {
        Self { id, chain: draft.chain, asset: draft.asset.clone(), action: draft.action, amount: draft.amount }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn public_intent_has_no_price_field() {
        let draft = IntentDraft {
            chain: Chain::NearTestnet,
            action: Action::Sell,
            asset: "wNEAR".into(),
            condition: Condition::Above { price: UsdPrice::from_cents(39_017) },
            amount: TokenAmount::parse("1", 24).unwrap(),
            mode: ExecutionMode::Shark,
            privacy: PrivacyLevel::Low,
        };
        let wire = serde_json::to_string(&PublicIntent::of("a".into(), &draft)).unwrap();
        assert!(!wire.contains("condition"), "{wire}");
        assert!(!wire.contains("39017"), "{wire}");
    }

    #[test]
    fn a_24_decimal_amount_survives_the_wire() {
        // 1 wNEAR is 10^24, past what a JSON number holds in a u64 or f64.
        let draft = IntentDraft {
            chain: Chain::NearTestnet,
            action: Action::Sell,
            asset: "wNEAR".into(),
            condition: Condition::Any,
            amount: TokenAmount::parse("1234567.000000000000000000000001", 24).unwrap(),
            mode: ExecutionMode::Shark,
            privacy: PrivacyLevel::Low,
        };
        let back: IntentDraft = serde_json::from_str(&serde_json::to_string(&draft).unwrap()).unwrap();
        assert_eq!(back, draft);
    }

    #[test]
    fn an_intent_must_name_a_chain_this_build_knows() {
        assert!(serde_json::from_str::<Chain>(r#""avalanche-fuji""#).is_err());
        let no_chain = r#"{"action":"BUY","asset":"wNEAR","condition":{"kind":"any"},"amount":{"raw":1,"decimals":24},"mode":"SHARK","privacy":"LOW"}"#;
        assert!(serde_json::from_str::<IntentDraft>(no_chain).is_err());
    }

    #[test]
    fn chain_round_trips_as_kebab_case() {
        assert_eq!(serde_json::to_string(&Chain::NearTestnet).unwrap(), r#""near-testnet""#);
        let back: Chain = serde_json::from_str(r#""near-mainnet""#).unwrap();
        assert_eq!(back, Chain::NearMainnet);
    }

    #[test]
    fn only_the_same_near_chain_matches() {
        assert!(Chain::NearTestnet.matches(Chain::NearTestnet));
        assert!(!Chain::NearTestnet.matches(Chain::NearMainnet));
    }

    fn settled() -> IntentStatus {
        IntentStatus::Settled {
            proof: ProofHash::new("G1s5UDrB6f2phA7RdNvY"),
            proof_kind: ProofKind::Transaction,
            filled_at: UsdPrice::from_cents(9421),
            elapsed_ms: 11_400,
        }
    }

    #[test]
    fn a_settlement_stored_before_proof_kind_reads_as_a_transaction() {
        let stored = r#"{"status":"SETTLED","proof":"G1s5","filled_at":9421,"elapsed_ms":11400}"#;
        let IntentStatus::Settled { proof_kind, .. } = serde_json::from_str(stored).unwrap() else {
            panic!("not settled");
        };
        assert_eq!(proof_kind, ProofKind::Transaction);
    }

    #[test]
    fn an_intent_hash_proof_round_trips() {
        let status = IntentStatus::Settled {
            proof: ProofHash::new("CRDH"),
            proof_kind: ProofKind::Intent,
            filled_at: UsdPrice::from_cents(370),
            elapsed_ms: 1,
        };
        let json = serde_json::to_string(&status).unwrap();
        assert!(json.contains(r#""proof_kind":"INTENT""#), "{json}");
        assert_eq!(serde_json::from_str::<IntentStatus>(&json).unwrap(), status);
    }

    #[test]
    fn terminal_states_accept_nothing() {
        for terminal in [
            settled(),
            IntentStatus::Failed { reason: FailureReason::NoRoute },
            IntentStatus::Cancelled,
        ] {
            assert!(!terminal.can_transition_to(&IntentStatus::Waiting));
            assert!(!terminal.can_transition_to(&IntentStatus::Cancelled));
            assert!(!terminal.can_transition_to(&settled()));
        }
    }

    #[test]
    fn nothing_returns_to_draft() {
        assert!(!IntentStatus::Waiting.can_transition_to(&IntentStatus::Draft));
        assert!(!IntentStatus::Broadcast { route_len: 3 }.can_transition_to(&IntentStatus::Draft));
    }

    #[test]
    fn a_draft_cannot_settle_without_being_broadcast() {
        assert!(!IntentStatus::Draft.can_transition_to(&settled()));
    }

    #[test]
    fn settlement_requires_a_route() {
        // Settling straight from broadcast would mean settling through a route
        // that was never found.
        assert!(!IntentStatus::Broadcast { route_len: 3 }.can_transition_to(&settled()));
        assert!(IntentStatus::FindingRoute.can_transition_to(&settled()));
        assert!(IntentStatus::Waiting.can_transition_to(&settled()));
    }

    #[test]
    fn any_live_state_can_fail_or_be_cancelled() {
        for live in [
            IntentStatus::Draft,
            IntentStatus::Broadcast { route_len: 3 },
            IntentStatus::Negotiating { bids: 3, best: None },
            IntentStatus::FindingRoute,
            IntentStatus::Waiting,
        ] {
            assert!(live.can_transition_to(&IntentStatus::Cancelled), "{live:?}");
            assert!(
                live.can_transition_to(&IntentStatus::Failed { reason: FailureReason::NodeFailure }),
                "{live:?}"
            );
        }
    }

    #[test]
    fn active_and_terminal_are_mutually_exclusive() {
        for status in [
            IntentStatus::Draft,
            IntentStatus::Broadcast { route_len: 1 },
            IntentStatus::Negotiating { bids: 0, best: None },
            IntentStatus::FindingRoute,
            IntentStatus::Waiting,
            settled(),
            IntentStatus::Failed { reason: FailureReason::NoRoute },
            IntentStatus::Cancelled,
        ] {
            assert!(!(status.is_active() && status.is_terminal()), "{status:?}");
        }
    }

    #[test]
    fn conditions_test_the_price_they_carry() {
        let ninety_five = UsdPrice::from_cents(9500);
        let filled = UsdPrice::from_cents(9421);

        assert!(Condition::Under { price: ninety_five }.is_satisfied_by(filled));
        assert!(!Condition::Above { price: ninety_five }.is_satisfied_by(filled));
        assert!(Condition::Any.is_satisfied_by(filled));
    }

    #[test]
    fn any_condition_carries_no_price() {
        assert_eq!(Condition::Any.price(), None);
    }

    #[test]
    fn status_serializes_with_the_tag_the_ui_switches_on() {
        let json = serde_json::to_string(&IntentStatus::FindingRoute).unwrap();
        assert_eq!(json, r#"{"status":"FINDING_ROUTE"}"#);
    }

    #[test]
    fn mode_labels_match_the_prototype() {
        assert_eq!(ExecutionMode::Shark.label(), "SHARK MODE");
        assert_eq!(
            ExecutionMode::Ghost.description(),
            "Maximum privacy. Longer route. Slower fill."
        );
    }
}
