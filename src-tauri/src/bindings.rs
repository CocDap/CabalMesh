//! Presentation contracts shared with the frontend.
//!
//! # Two rules this module exists to enforce
//!
//! **No colour crosses the boundary.** The prototype passes colours as data —
//! `dot: BLUE`, `deltaColor: GREEN`, a palette of named constants. That is
//! prototype convenience and must not survive: it hard-codes the palette into
//! Rust, so a design change becomes a backend release. Rust sends a semantic
//! *tone* whose domain matches the design system's component props exactly, and
//! the mapping to a hex value happens in the design system and nowhere else.
//!
//! **Numbers are formatted once, in Rust.** The brand's copy rules demand exact
//! separated figures — `1,248`, `+12.4%`, `99.98%`, `11.4s` — never
//! approximations. Formatting here means one implementation of the separator
//! and precision rules instead of one per screen.
//!
//! # Generated types
//!
//! Under the `ts-rs` feature these derive `TS`, so `cargo test --features ts-rs
//! export_bindings` writes `src/types/bindings.ts` at the repo root. Hand-maintaining thirty-odd
//! interfaces against a moving API is a drift generator; generating them makes
//! a mismatch a build failure instead of a runtime `undefined`.

// `Deserialize` on the log types because the intent ledger persists settlement
// logs — a queued intent has to still have its verification lines after the app
// is killed. Nothing deserializes these from the webview; the ACL does not
// admit them as command input.
use serde::{Deserialize, Serialize};

#[cfg(feature = "ts-rs")]
use ts_rs::TS;

/// Status tone. Domain matches the design system's `StatusDot` prop exactly.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[cfg_attr(feature = "ts-rs", derive(TS), ts(export, export_to = "../../src/types/bindings.ts"))]
#[serde(rename_all = "lowercase")]
pub enum StatusTone {
    Online,
    Alert,
    Info,
    Idle,
    Offline,
}

/// Terminal line tone. Domain matches `TerminalLine`.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "ts-rs", derive(TS), ts(export, export_to = "../../src/types/bindings.ts"))]
#[serde(rename_all = "lowercase")]
pub enum LogTone {
    Out,
    Dim,
    Ok,
    Err,
    Info,
    Loud,
}

/// Delta direction. Domain matches `StatBlock`'s `deltaTone`.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[cfg_attr(feature = "ts-rs", derive(TS), ts(export, export_to = "../../src/types/bindings.ts"))]
#[serde(rename_all = "lowercase")]
pub enum DeltaTone {
    Up,
    Down,
    Neutral,
}

/// Toast tone. Domain matches `Toast`.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[cfg_attr(feature = "ts-rs", derive(TS), ts(export, export_to = "../../src/types/bindings.ts"))]
#[serde(rename_all = "lowercase")]
pub enum ToastTone {
    Neutral,
    Info,
    Success,
    Alert,
}

/// One rendered terminal line.
///
/// `Box<str>` rather than `String`: these are built once, never mutated, and
/// arrive in the hundreds, so `String`'s spare-capacity word is dead weight.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[cfg_attr(feature = "ts-rs", derive(TS), ts(export, export_to = "../../src/types/bindings.ts"))]
pub struct LogLine {
    pub text: Box<str>,
    pub tone: LogTone,
}

impl LogLine {
    /// A line at the given tone.
    #[must_use]
    pub fn new(text: impl Into<Box<str>>, tone: LogTone) -> Self {
        Self { text: text.into(), tone }
    }
}

/// A home-screen stat tile, pre-formatted.
///
/// The frontend renders these verbatim. Sending a raw number and formatting it
/// there would mean reimplementing the separator and precision rules per
/// screen, and drifting from them.
#[derive(Debug, Clone, Serialize)]
#[cfg_attr(feature = "ts-rs", derive(TS), ts(export, export_to = "../../src/types/bindings.ts"))]
#[serde(rename_all = "camelCase")]
pub struct StatTile {
    pub label: &'static str,
    /// Already separated, e.g. `1,248`.
    pub value: Box<str>,
    /// Already signed and suffixed, e.g. `+12.4%`. Absent when unmeasured.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub delta: Option<Box<str>>,
    pub delta_tone: DeltaTone,
}

impl StatTile {
    /// A tile with no delta — the honest rendering for a figure with no
    /// baseline to compare against.
    #[must_use]
    pub fn plain(label: &'static str, value: impl Into<Box<str>>) -> Self {
        Self {
            label,
            value: value.into(),
            delta: None,
            delta_tone: DeltaTone::Neutral,
        }
    }

    /// A tile with a percentage delta, signed and toned from its direction.
    #[must_use]
    pub fn with_delta(label: &'static str, value: impl Into<Box<str>>, percent: f64) -> Self {
        let tone = if percent > 0.0 {
            DeltaTone::Up
        } else if percent < 0.0 {
            DeltaTone::Down
        } else {
            DeltaTone::Neutral
        };
        Self {
            label,
            value: value.into(),
            delta: Some(format!("{percent:+.1}%").into_boxed_str()),
            delta_tone: tone,
        }
    }
}

/// Formats an integer with thousands separators: `1248` becomes `1,248`.
///
/// The brand requires exact separated figures and forbids approximations like
/// "over 1,000" or "~23k", so there is no abbreviating variant of this on
/// purpose.
#[must_use]
pub fn separated(value: u64) -> String {
    let digits = value.to_string();
    let mut out = String::with_capacity(digits.len() + digits.len() / 3);
    for (index, digit) in digits.chars().enumerate() {
        if index > 0 && (digits.len() - index) % 3 == 0 {
            out.push(',');
        }
        out.push(digit);
    }
    out
}

/// Payload for the `guardian-unlock-request` event: this device's own tag
/// matched someone's unlock broadcast and a human needs to decide.
///
/// `from` is a truncated peer id for display only — never the durable
/// identity of whoever is asking, because there is no durable identity to
/// show. It is exactly as anonymous to this screen as the mesh itself keeps
/// everyone by default.
#[derive(Debug, Clone, Serialize)]
#[cfg_attr(feature = "ts-rs", derive(TS), ts(export, export_to = "../../src/types/bindings.ts"))]
#[serde(rename_all = "camelCase")]
pub struct GuardianUnlockPrompt {
    pub id: u32,
    pub from: String,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn tones_serialize_as_the_design_system_expects() {
        // These strings are the design system's prop domains. A rename here is
        // a breaking change for every component that switches on them.
        assert_eq!(serde_json::to_string(&StatusTone::Online).unwrap(), "\"online\"");
        assert_eq!(serde_json::to_string(&LogTone::Err).unwrap(), "\"err\"");
        assert_eq!(serde_json::to_string(&DeltaTone::Up).unwrap(), "\"up\"");
        assert_eq!(serde_json::to_string(&ToastTone::Alert).unwrap(), "\"alert\"");
    }

    #[test]
    fn separators_match_the_brands_number_rules() {
        assert_eq!(separated(0), "0");
        assert_eq!(separated(999), "999");
        assert_eq!(separated(1_248), "1,248");
        assert_eq!(separated(9_731), "9,731");
        assert_eq!(separated(23_118), "23,118");
        assert_eq!(separated(1_000_000), "1,000,000");
    }

    #[test]
    fn deltas_are_signed_and_toned_from_direction() {
        let up = StatTile::with_delta("NETWORK NODES", "1,248", 12.4);
        assert_eq!(up.delta.as_deref(), Some("+12.4%"));
        assert_eq!(up.delta_tone, DeltaTone::Up);

        let down = StatTile::with_delta("NETWORK NODES", "1,248", -3.0);
        assert_eq!(down.delta.as_deref(), Some("-3.0%"));
        assert_eq!(down.delta_tone, DeltaTone::Down);

        let flat = StatTile::with_delta("NETWORK NODES", "1,248", 0.0);
        assert_eq!(flat.delta_tone, DeltaTone::Neutral);
    }

    #[test]
    fn a_tile_without_a_baseline_omits_its_delta() {
        // Rendering "+0.0%" for an unmeasured figure would be a fabricated
        // trend, which the brand's exactness rule forbids. `RELAYED BYTES` has
        // no prior window to compare against, and `INTENTS SETTLED` falls back
        // to this shape until there is a previous week to compare with.
        let tile = StatTile::plain("RELAYED BYTES", "—");
        let json = serde_json::to_string(&tile).unwrap();
        assert!(!json.contains("delta\""), "absent delta must be omitted: {json}");
    }

    #[test]
    fn no_colour_crosses_the_boundary() {
        // The prototype sent `dot: BLUE`. If a hex value ever appears in a
        // serialized presentation type, the palette has leaked into Rust.
        let payloads = [
            serde_json::to_string(&StatusTone::Alert).unwrap(),
            serde_json::to_string(&LogTone::Ok).unwrap(),
            serde_json::to_string(&StatTile::with_delta("X", "1", 1.0)).unwrap(),
            serde_json::to_string(&LogLine::new("hello", LogTone::Dim)).unwrap(),
        ];
        for payload in payloads {
            assert!(!payload.contains('#'), "hex colour leaked: {payload}");
            for forbidden in ["00E5FF", "FF3B3B", "9BFF00", "BLUE", "GREEN", "RED"] {
                assert!(!payload.contains(forbidden), "palette leaked: {payload}");
            }
        }
    }
}
