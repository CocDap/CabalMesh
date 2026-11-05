//! Fixed-point money.
//!
//! The UI renders `10 wNEAR`, `1,240.00 USDC`, `$94.21`. None of that may go
//! through `f64`: 0.1 + 0.2 is famously not 0.3, and a wei value does not
//! survive a JS number at all. Amounts are integers in the asset's smallest
//! unit, parsed and validated at the boundary.

use serde::{Deserialize, Serialize};
use std::fmt;
use std::str::FromStr;

/// Why an amount could not be parsed.
#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
#[non_exhaustive]
pub enum AmountError {
    #[error("amount is empty")]
    Empty,

    #[error("amount contains a character that is not a digit or decimal point")]
    InvalidCharacter,

    #[error("amount has more than one decimal point")]
    MultipleDecimalPoints,

    #[error("amount has {found} decimal places but the asset supports {supported}")]
    TooManyDecimals { found: u8, supported: u8 },

    #[error("amount is too large to represent")]
    Overflow,
}

/// A token amount held as an integer count of the asset's smallest unit.
///
/// Two amounts with different `decimals` describe different assets, so
/// comparing or adding them is meaningless — the API only offers operations
/// where that cannot silently happen.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub struct TokenAmount {
    /// Count of the smallest unit. 1 wNEAR at 24 decimals is 10^24.
    raw: u128,
    decimals: u8,
}

impl TokenAmount {
    /// The largest `decimals` that can be represented, since 10^39 exceeds
    /// `u128`.
    pub const MAX_DECIMALS: u8 = 38;

    /// An amount directly from its smallest-unit representation — the form
    /// the chain speaks.
    #[must_use]
    pub const fn from_raw(raw: u128, decimals: u8) -> Self {
        Self { raw, decimals }
    }

    /// Zero, in the given asset.
    #[must_use]
    pub const fn zero(decimals: u8) -> Self {
        Self { raw: 0, decimals }
    }

    /// The smallest-unit value.
    #[must_use]
    pub const fn raw(self) -> u128 {
        self.raw
    }

    /// How many decimal places this asset has.
    #[must_use]
    pub const fn decimals(self) -> u8 {
        self.decimals
    }

    /// Whether this is zero.
    #[must_use]
    pub const fn is_zero(self) -> bool {
        self.raw == 0
    }

    /// Parses a human-entered decimal string, e.g. `"23.456"` or `"1,240.00"`.
    ///
    /// Thousands separators are accepted because the UI displays them and
    /// users paste what they see.
    ///
    /// # Errors
    ///
    /// Returns [`AmountError::TooManyDecimals`] if the string carries more
    /// precision than the asset supports — truncating silently would lose
    /// money — and [`AmountError::Overflow`] if the scaled value exceeds
    /// `u128`.
    pub fn parse(input: &str, decimals: u8) -> Result<Self, AmountError> {
        if decimals > Self::MAX_DECIMALS {
            return Err(AmountError::Overflow);
        }

        let cleaned: String = input.chars().filter(|c| *c != ',' && *c != '_').collect();
        let trimmed = cleaned.trim();
        if trimmed.is_empty() {
            return Err(AmountError::Empty);
        }

        let mut parts = trimmed.split('.');
        let whole = parts.next().unwrap_or("");
        let fraction = parts.next().unwrap_or("");
        if parts.next().is_some() {
            return Err(AmountError::MultipleDecimalPoints);
        }

        if whole.is_empty() && fraction.is_empty() {
            return Err(AmountError::Empty);
        }
        if !whole.bytes().all(|b| b.is_ascii_digit()) || !fraction.bytes().all(|b| b.is_ascii_digit())
        {
            return Err(AmountError::InvalidCharacter);
        }

        // Trailing zeros beyond the asset's precision are harmless — "1.500"
        // on a 2-decimal asset is 1.50, not a precision error.
        let significant = fraction.trim_end_matches('0');
        if significant.len() > usize::from(decimals) {
            return Err(AmountError::TooManyDecimals {
                found: u8::try_from(significant.len()).unwrap_or(u8::MAX),
                supported: decimals,
            });
        }

        let scale = 10_u128
            .checked_pow(u32::from(decimals))
            .ok_or(AmountError::Overflow)?;

        let whole_value: u128 = if whole.is_empty() {
            0
        } else {
            whole.parse().map_err(|_| AmountError::Overflow)?
        };

        let mut padded = fraction.to_string();
        padded.truncate(usize::from(decimals));
        while padded.len() < usize::from(decimals) {
            padded.push('0');
        }
        let fraction_value: u128 = if padded.is_empty() {
            0
        } else {
            padded.parse().map_err(|_| AmountError::Overflow)?
        };

        whole_value
            .checked_mul(scale)
            .and_then(|scaled| scaled.checked_add(fraction_value))
            .map(|raw| Self { raw, decimals })
            .ok_or(AmountError::Overflow)
    }

    /// Adds two amounts of the same asset.
    ///
    /// # Errors
    ///
    /// Returns [`AmountError::Overflow`] on wrap, or if the assets differ —
    /// adding wNEAR to USDC is a bug, not a saturating operation.
    pub fn checked_add(self, other: Self) -> Result<Self, AmountError> {
        if self.decimals != other.decimals {
            return Err(AmountError::Overflow);
        }
        self.raw
            .checked_add(other.raw)
            .map(|raw| Self { raw, decimals: self.decimals })
            .ok_or(AmountError::Overflow)
    }

    /// Formats without thousands separators, e.g. `23.456`.
    ///
    /// Trailing zeros are trimmed, so 1.500 renders as `1.5` and a whole
    /// number renders without a decimal point at all.
    #[must_use]
    pub fn to_plain_string(self) -> String {
        if self.decimals == 0 {
            return self.raw.to_string();
        }
        let scale = 10_u128.pow(u32::from(self.decimals));
        let whole = self.raw / scale;
        let fraction = self.raw % scale;
        if fraction == 0 {
            return whole.to_string();
        }
        let fraction_str = format!("{fraction:0width$}", width = usize::from(self.decimals));
        format!("{whole}.{}", fraction_str.trim_end_matches('0'))
    }
}

/// Renders with thousands separators, matching the brand's number rules:
/// exact figures, always separated, never approximated.
impl fmt::Display for TokenAmount {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        let plain = self.to_plain_string();
        let (whole, fraction) = plain.split_once('.').map_or((plain.as_str(), None), |(w, r)| (w, Some(r)));

        let mut grouped = String::with_capacity(whole.len() + whole.len() / 3);
        for (i, c) in whole.chars().enumerate() {
            if i > 0 && (whole.len() - i).is_multiple_of(3) {
                grouped.push(',');
            }
            grouped.push(c);
        }

        match fraction {
            Some(fr) => write!(f, "{grouped}.{fr}"),
            None => f.write_str(&grouped),
        }
    }
}

/// A USD price, fixed at two decimal places — the precision the UI shows
/// (`$94.21`, `UNDER $95`).
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
#[cfg_attr(feature = "ts-rs", derive(ts_rs::TS), ts(export, export_to = "../../src/types/bindings.ts"))]
#[serde(transparent)]
pub struct UsdPrice {
    /// Cents. Avoids the float rounding that makes price comparisons lie.
    cents: u64,
}

impl UsdPrice {
    /// A price from a whole-cent value.
    #[must_use]
    pub const fn from_cents(cents: u64) -> Self {
        Self { cents }
    }

    /// The value in whole cents.
    #[must_use]
    pub const fn cents(self) -> u64 {
        self.cents
    }

    /// Parses `"94.21"`, `"95"` or `"$1,240.00"`.
    ///
    /// # Errors
    ///
    /// As [`TokenAmount::parse`], with two decimal places.
    pub fn parse(input: &str) -> Result<Self, AmountError> {
        let stripped = input.trim().trim_start_matches('$');
        let amount = TokenAmount::parse(stripped, 2)?;
        u64::try_from(amount.raw())
            .map(|cents| Self { cents })
            .map_err(|_| AmountError::Overflow)
    }
}

impl FromStr for UsdPrice {
    type Err = AmountError;

    fn from_str(s: &str) -> Result<Self, Self::Err> {
        Self::parse(s)
    }
}

/// Always two decimal places and a leading `$`, e.g. `$94.21`, `$1,240.00`.
impl fmt::Display for UsdPrice {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        let whole = self.cents / 100;
        let remainder = self.cents % 100;
        let whole_str = whole.to_string();

        let mut grouped = String::with_capacity(whole_str.len() + whole_str.len() / 3);
        for (i, c) in whole_str.chars().enumerate() {
            if i > 0 && (whole_str.len() - i).is_multiple_of(3) {
                grouped.push(',');
            }
            grouped.push(c);
        }
        write!(f, "${grouped}.{remainder:02}")
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_a_whole_number() {
        let amount = TokenAmount::parse("10", 18).unwrap();
        assert_eq!(amount.raw(), 10_u128.pow(19));
    }

    #[test]
    fn parses_a_fractional_amount() {
        let amount = TokenAmount::parse("23.456", 18).unwrap();
        assert_eq!(amount.to_plain_string(), "23.456");
    }

    #[test]
    fn accepts_thousands_separators_because_the_ui_renders_them() {
        let amount = TokenAmount::parse("1,240.00", 6).unwrap();
        assert_eq!(amount.to_string(), "1,240");
    }

    #[test]
    fn rejects_precision_the_asset_cannot_hold() {
        // Truncating silently would lose money.
        let err = TokenAmount::parse("1.005", 2).unwrap_err();
        assert_eq!(err, AmountError::TooManyDecimals { found: 3, supported: 2 });
    }

    #[test]
    fn ignores_trailing_zeros_beyond_precision() {
        // "1.500" on a 2-decimal asset is 1.50, not a precision error.
        assert_eq!(TokenAmount::parse("1.500", 2).unwrap().to_plain_string(), "1.5");
    }

    #[test]
    fn rejects_two_decimal_points() {
        assert_eq!(
            TokenAmount::parse("1.2.3", 18).unwrap_err(),
            AmountError::MultipleDecimalPoints
        );
    }

    #[test]
    fn rejects_non_numeric_input() {
        assert_eq!(
            TokenAmount::parse("10 wNEAR", 18).unwrap_err(),
            AmountError::InvalidCharacter
        );
    }

    #[test]
    fn rejects_empty_input() {
        assert_eq!(TokenAmount::parse("   ", 18).unwrap_err(), AmountError::Empty);
    }

    #[test]
    fn overflows_rather_than_wrapping() {
        let huge = "999999999999999999999999999999999999999";
        assert_eq!(TokenAmount::parse(huge, 18).unwrap_err(), AmountError::Overflow);
    }

    #[test]
    fn refuses_to_add_different_assets() {
        let wnear = TokenAmount::parse("1", 24).unwrap();
        let usdc = TokenAmount::parse("1", 6).unwrap();
        assert!(wnear.checked_add(usdc).is_err());
    }

    #[test]
    fn groups_thousands_in_display() {
        let amount = TokenAmount::parse("1234567.5", 6).unwrap();
        assert_eq!(amount.to_string(), "1,234,567.5");
    }

    #[test]
    fn formats_usd_with_two_places_always() {
        assert_eq!(UsdPrice::parse("94.2").unwrap().to_string(), "$94.20");
        assert_eq!(UsdPrice::parse("95").unwrap().to_string(), "$95.00");
        assert_eq!(UsdPrice::parse("$1,240").unwrap().to_string(), "$1,240.00");
    }

    #[test]
    fn usd_prices_compare_by_value() {
        assert!(UsdPrice::parse("94.21").unwrap() < UsdPrice::parse("95").unwrap());
    }
}
