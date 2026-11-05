//! The amounts both sides of a deal must agree on, byte for byte.
//!
//! Each side computes its own diff from the agreed price with no further round
//! trip, and the verifier only applies a pair that nets to zero. So this has to
//! be pure integer arithmetic with one rounding rule.

use crate::TokenDiff;

/// What the verifier takes from each positive leg, in millionths.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct FeeRule {
    pub pips: u32,
}

impl FeeRule {
    /// Tidepool charges nothing (`simulate_intents(...).state.fee`).
    pub const NONE: Self = Self { pips: 0 };

    fn after_fee(self, amount: u128) -> u128 {
        const MILLION: u128 = 1_000_000;
        let keep = MILLION.saturating_sub(u128::from(self.pips));
        // Divides first where it can, so a large amount cannot overflow here.
        amount / MILLION * keep + (amount % MILLION) * keep / MILLION
    }
}

/// `floor(base_raw * price_cents * 10^quote_decimals / (100 * 10^base_decimals))`,
/// or `None` if it does not fit.
///
/// The powers of ten cancel before multiplying, because a 24-decimal base would
/// otherwise overflow on an amount nowhere near the limit.
#[must_use]
pub fn quote_amount(base_raw: u128, base_decimals: u8, price_cents: u64, quote_decimals: u8) -> Option<u128> {
    let numerator = base_raw.checked_mul(u128::from(price_cents))?;
    if base_decimals >= quote_decimals {
        let scale = 10u128.checked_pow(u32::from(base_decimals - quote_decimals))?;
        Some(numerator / 100 / scale)
    } else {
        let scale = 10u128.checked_pow(u32::from(quote_decimals - base_decimals))?;
        Some(numerator.checked_mul(scale)? / 100)
    }
}

fn signed(amount: u128) -> i128 {
    i128::try_from(amount).unwrap_or(i128::MAX)
}

/// The seller gives base and receives quote. `base` and `quote` are verifier
/// token ids (`nep141:<contract>`).
#[must_use]
pub fn seller_diff(base: &str, quote: &str, base_raw: u128, quote_raw: u128, fee: &FeeRule) -> TokenDiff {
    [
        (base.to_owned(), -signed(base_raw)),
        (quote.to_owned(), signed(fee.after_fee(quote_raw))),
    ]
    .into_iter()
    .collect()
}

/// The buyer gives quote and receives base. Same parameters as [`seller_diff`].
#[must_use]
pub fn buyer_diff(base: &str, quote: &str, base_raw: u128, quote_raw: u128, fee: &FeeRule) -> TokenDiff {
    [
        (base.to_owned(), signed(fee.after_fee(base_raw))),
        (quote.to_owned(), -signed(quote_raw)),
    ]
    .into_iter()
    .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn both_sides_compute_identical_diffs() {
        for (amount, price) in [(1_000_000_000_000_000_000_000_000u128, 404u64), (1, 1), (123_456_789, 99_999)] {
            let q = quote_amount(amount, 24, price, 6).unwrap();
            let s = seller_diff("nep141:w", "nep141:u", amount, q, &FeeRule::NONE);
            let b = buyer_diff("nep141:w", "nep141:u", amount, q, &FeeRule::NONE);
            for (k, v) in s.iter() {
                assert_eq!(b.get(k), Some(&-v));
            }
        }
    }

    #[test]
    fn one_wnear_at_four_dollars_four_is_the_spike_trade() {
        // The amounts that settled on testnet in docs/near-intents-spike.md.
        assert_eq!(quote_amount(1_000_000_000_000_000_000_000_000, 24, 404, 6), Some(4_040_000));
    }

    #[test]
    fn a_quote_with_more_decimals_than_the_base_scales_up() {
        assert_eq!(quote_amount(1_000_000, 6, 150, 24), Some(1_500_000_000_000_000_000_000_000));
    }

    #[test]
    fn quote_amount_rounds_down() {
        assert_eq!(quote_amount(1, 24, 99, 6), Some(0));
        assert_eq!(quote_amount(1_500_000, 6, 33, 6), Some(495_000));
    }

    #[test]
    fn quote_amount_overflow_is_none() {
        assert_eq!(quote_amount(u128::MAX, 24, 2, 6), None);
        assert_eq!(quote_amount(u128::MAX / 2, 6, 1, 24), None);
    }

    #[test]
    fn a_fee_comes_off_the_leg_each_side_receives() {
        let fee = FeeRule { pips: 1_000 };
        let seller = seller_diff("b", "q", 1_000_000, 2_000_000, &fee);
        assert_eq!(seller.get("q"), Some(&1_998_000));
        assert_eq!(seller.get("b"), Some(&-1_000_000));
    }
}
