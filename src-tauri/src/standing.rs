//! This installation's private settlement history and trend.
//!
//! # What this replaces, and why the name changed
//!
//! Ticket 03 shipped a **mock** reputation score, derived from the peer
//! identifier so it would at least be stable. It was a number the product could
//! not back, and it was recorded as such at the time. Ticket 39 replaces it.
//!
//! The field is now called what it measures. `REPUTATION SCORE 87.6` was a
//! score with no scale, no inputs and no way for a user to ask why it was that
//! number. `INTENTS SETTLED 14 (+55.6%)` is a count anyone can verify by
//! looking at their own history, which is the only kind of trust signal a
//! product built on proving things can honestly show about itself.
//!
//! Renaming rather than redefining matters: keeping the label `REPUTATION
//! SCORE` over a settled-intent count would be the same dishonesty in a
//! different place — a figure whose name promises more than its definition
//! delivers.
//!
//! # Where the numbers come from
//!
//! The device-local ledger, which is the same place the intents list reads
//! from. There is no second local accounting to drift from the first.
//!
//! This is intentionally named [`LocalStanding`]. It is valid for the owner's
//! home/profile screens, but it is not public seller standing and must never be
//! sent to a peer as evidence about another account. Public standing, when it
//! exists, comes from settlement events on-chain, never from this local count.
//!
//! - **The headline is lifetime**, not windowed. "How many have I settled" has
//!   one obvious answer and it is not "in the last week".
//! - **The delta compares the last seven days against the seven before it.**
//!   A real prior window, not a fabricated trend.
//!
//! # When there is no delta
//!
//! A node that settled nothing in the prior window has **no baseline**, and a
//! percentage change from zero is not a large number — it is undefined. The
//! delta is absent in that case, which `StatTile::plain` already renders as no
//! delta at all rather than as `+0.0%`.
//!
//! Zero settled intents is a different thing: that is a measured zero, and it
//! renders as `0`. The em dash was right when nothing was measured; it would be
//! wrong here, because this is measured and the answer is none.

use crate::intents::{Intent, Ledger};
use cabal_core::IntentStatus;

/// Seven days, the comparison window.
///
/// Long enough to survive a quiet weekend, short enough that the delta still
/// describes recent behaviour rather than the whole history the headline
/// already covers.
const WINDOW_MS: u64 = 7 * 24 * 60 * 60 * 1_000;

/// This installation's private settlement record.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct LocalStanding {
    /// Lifetime settled intents.
    pub settled: u32,
    /// Change against the previous seven days, as a percentage. `None` when
    /// there is no prior window to compare against.
    pub delta_percent: Option<f64>,
}

impl LocalStanding {
    /// Computes private local standing from the ledger.
    #[must_use]
    pub fn of(ledger: &Ledger, now_ms: u64) -> Self {
        Self::from_intents(&ledger.all(), now_ms)
    }

    /// The same, from intents already in hand.
    ///
    /// Split out so it is testable without a store, and so a caller that
    /// already holds the list does not read it twice.
    #[must_use]
    pub fn from_intents(intents: &[Intent], now_ms: u64) -> Self {
        let settled_at = |intent: &Intent| match intent.status {
            // `finished_ms` is set by the ledger on every terminal transition,
            // so a settled intent always has one. Matching on both rather than
            // unwrapping keeps a malformed persisted entry out of the counts
            // instead of panicking on it.
            IntentStatus::Settled { .. } => intent.finished_ms,
            _ => None,
        };

        let recent_start = now_ms.saturating_sub(WINDOW_MS);
        let prior_start = now_ms.saturating_sub(WINDOW_MS * 2);

        let mut settled = 0_u32;
        let mut recent = 0_u32;
        let mut prior = 0_u32;

        for intent in intents {
            let Some(at) = settled_at(intent) else { continue };
            settled = settled.saturating_add(1);

            if at >= recent_start {
                recent = recent.saturating_add(1);
            } else if at >= prior_start {
                prior = prior.saturating_add(1);
            }
        }

        Self {
            settled,
            // No prior window means no baseline. A percentage change from zero
            // is undefined, not infinite, and rendering one would be the
            // fabricated trend this whole exercise exists to remove.
            delta_percent: (prior > 0).then(|| {
                (f64::from(recent) - f64::from(prior)) / f64::from(prior) * 100.0
            }),
        }
    }

    /// `14` — the count alone, for the home tile that carries its delta in a
    /// separate field.
    #[must_use]
    pub fn value(&self) -> String {
        crate::bindings::separated(u64::from(self.settled))
    }

    /// `14 (+55.6%)`, or `14` with no baseline — the shape the profile row
    /// renders.
    #[must_use]
    pub fn combined(&self) -> String {
        match self.delta_percent {
            Some(delta) => format!("{} ({delta:+.1}%)", self.value()),
            None => self.value(),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use cabal_core::{
        Action, Condition, ExecutionMode, IntentDraft, IntentId, PrivacyLevel, ProofHash,
        TokenAmount, UsdPrice,
    };

    const DAY: u64 = 24 * 60 * 60 * 1_000;
    /// A fixed "now" so the windows are arithmetic rather than wall-clock.
    ///
    /// Far enough from zero that a test can place an intent hundreds of days
    /// back without the subtraction underflowing.
    const NOW: u64 = 1_000 * DAY;

    fn intent(status: IntentStatus, finished_ms: Option<u64>) -> Intent {
        Intent {
            id: IntentId::new("TEST"),
            draft: IntentDraft {
                chain: cabal_core::Chain::NearTestnet,
                action: Action::Buy,
                asset: "wNEAR".into(),
                condition: Condition::Any,
                amount: TokenAmount::parse("1", 18).unwrap(),
                mode: ExecutionMode::Shark,
                privacy: PrivacyLevel::High,
            },
            status,
            created_ms: 0,
            finished_ms,
            route: Vec::new(),
            log: Vec::new(),
            deal: None,
            counterparty: None,
        }
    }

    fn settled(days_ago: u64) -> Intent {
        intent(
            IntentStatus::Settled {
                proof: ProofHash::new("0xabc"),
                proof_kind: cabal_core::ProofKind::Transaction,
                filled_at: UsdPrice::from_cents(9421),
                elapsed_ms: 11_400,
            },
            Some(NOW - days_ago * DAY),
        )
    }

    #[test]
    fn a_node_that_has_settled_nothing_reads_zero_not_an_em_dash() {
        // Measured, and the answer is none. The em dash was right when there
        // was no source at all; it would be wrong now.
        let standing = LocalStanding::from_intents(&[], NOW);
        assert_eq!(standing.settled, 0);
        assert_eq!(standing.value(), "0");
        assert_eq!(standing.combined(), "0");
    }

    #[test]
    fn only_settled_intents_count() {
        // Cancelled and failed intents are history, not achievement. Counting
        // them would make the figure grow by giving up.
        let intents = [
            settled(1),
            intent(IntentStatus::Cancelled, Some(NOW - DAY)),
            intent(
                IntentStatus::Failed { reason: cabal_core::FailureReason::NoRoute },
                Some(NOW - DAY),
            ),
            intent(IntentStatus::Draft, None),
            intent(IntentStatus::Waiting, None),
        ];
        assert_eq!(LocalStanding::from_intents(&intents, NOW).settled, 1);
    }

    #[test]
    fn the_headline_is_lifetime_not_windowed() {
        // "How many have I settled" has one obvious answer, and it is not
        // "in the last week".
        let intents = [settled(1), settled(40), settled(300)];
        assert_eq!(LocalStanding::from_intents(&intents, NOW).settled, 3);
    }

    #[test]
    fn the_delta_compares_two_real_windows() {
        // Three in the last seven days against two in the seven before:
        // +50.0%, computed from counts that both exist.
        let intents = [
            settled(1),
            settled(3),
            settled(6),
            settled(8),
            settled(13),
        ];
        let standing = LocalStanding::from_intents(&intents, NOW);
        assert_eq!(standing.settled, 5);
        assert_eq!(standing.delta_percent, Some(50.0));
        assert_eq!(standing.combined(), "5 (+50.0%)");
    }

    #[test]
    fn a_decline_reads_as_a_decline() {
        let intents = [settled(2), settled(8), settled(9), settled(10)];
        let standing = LocalStanding::from_intents(&intents, NOW);
        assert_eq!(standing.delta_percent, Some(-((2.0 / 3.0) * 100.0)));
        assert!(standing.combined().contains("-66.7%"));
    }

    #[test]
    fn no_prior_window_means_no_delta() {
        // A percentage change from zero is undefined, not infinite. This is
        // exactly the fabricated trend ticket 39 exists to remove.
        let intents = [settled(1), settled(2)];
        let standing = LocalStanding::from_intents(&intents, NOW);
        assert_eq!(standing.settled, 2);
        assert_eq!(standing.delta_percent, None);
        assert_eq!(standing.combined(), "2", "a missing baseline must not render as a trend");
    }

    #[test]
    fn intents_older_than_both_windows_count_only_toward_the_lifetime_total() {
        let intents = [settled(1), settled(9), settled(200)];
        let standing = LocalStanding::from_intents(&intents, NOW);
        assert_eq!(standing.settled, 3);
        // One recent against one prior: flat, and flat is a real reading.
        assert_eq!(standing.delta_percent, Some(0.0));
    }

    #[test]
    fn a_settled_intent_with_no_finish_time_is_skipped_rather_than_fatal() {
        // The ledger always sets finished_ms on a terminal transition, so this
        // is a malformed persisted entry. Skipping keeps one bad row out of the
        // count instead of panicking the whole screen.
        let broken = intent(
            IntentStatus::Settled {
                proof: ProofHash::new("0xabc"),
                proof_kind: cabal_core::ProofKind::Transaction,
                filled_at: UsdPrice::from_cents(1),
                elapsed_ms: 1,
            },
            None,
        );
        assert_eq!(LocalStanding::from_intents(&[broken], NOW).settled, 0);
    }

    #[test]
    fn large_counts_are_separated_per_the_brands_number_rules() {
        let standing = LocalStanding { settled: 1_248, delta_percent: Some(12.4) };
        assert_eq!(standing.value(), "1,248");
        assert_eq!(standing.combined(), "1,248 (+12.4%)");
    }

    #[test]
    fn a_clock_before_the_windows_does_not_underflow() {
        // now_ms below the window width happens on a device with a wrong clock,
        // and saturating_sub is what keeps it from wrapping to the far future
        // and reading every intent as prior.
        let standing = LocalStanding::from_intents(&[], 1_000);
        assert_eq!(standing.settled, 0);
    }
}
