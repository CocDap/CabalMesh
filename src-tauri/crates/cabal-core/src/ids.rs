//! Opaque identifiers.
//!
//! Every identifier in this system is a hex-ish string, which means the
//! compiler cannot tell a node from an intent from a proof if they are all
//! `String`. These newtypes make mixing them a compile error at no runtime
//! cost.

use serde::{Deserialize, Serialize};
use std::fmt;

/// Builds a newtype over `Box<str>` with the traits every identifier needs.
///
/// `Box<str>` rather than `String`: identifiers are written once and never
/// mutated, so `String`'s spare-capacity word is dead weight — and these
/// appear in lists of hundreds of rows.
macro_rules! id_newtype {
    ($(#[$meta:meta])* $name:ident, $display_example:literal) => {
        $(#[$meta])*
        #[derive(Debug, Clone, PartialEq, Eq, Hash, PartialOrd, Ord, Serialize, Deserialize)]
        // Identifiers cross the IPC boundary inside view types, so they need a
        // TypeScript face too. `transparent` keeps that face a bare string.
        #[cfg_attr(feature = "ts-rs", derive(ts_rs::TS), ts(export, export_to = "../../src/types/bindings.ts"))]
        #[serde(transparent)]
        pub struct $name(Box<str>);

        impl $name {
            /// Wraps a raw identifier.
            ///
            /// Deliberately not validating: these come from libp2p, the chain
            /// and the prover, each with its own format. Validation belongs at
            /// the boundary that knows the format, not here.
            #[must_use]
            pub fn new(value: impl Into<Box<str>>) -> Self {
                Self(value.into())
            }

            /// The underlying identifier.
            #[must_use]
            pub fn as_str(&self) -> &str {
                &self.0
            }
        }

        impl fmt::Display for $name {
            fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
                f.write_str(&self.0)
            }
        }

        impl From<String> for $name {
            fn from(value: String) -> Self {
                Self(value.into_boxed_str())
            }
        }

        impl From<&str> for $name {
            fn from(value: &str) -> Self {
                Self(value.into())
            }
        }

        impl AsRef<str> for $name {
            fn as_ref(&self) -> &str {
                &self.0
            }
        }
    };
}

id_newtype!(
    /// A mesh peer. Rendered truncated as `7F3A..8C2E`.
    NodeId,
    "7F3A..8C2E"
);

id_newtype!(
    /// An intent, from draft through settlement.
    IntentId,
    "9F7A..3B2C"
);

id_newtype!(
    /// A settlement proof hash. Displayed lowercase and unabbreviated.
    ProofHash,
    "0xa4f2c9e1b70d5533"
);

id_newtype!(
    /// A live stream subscription, so the frontend can cancel delivery.
    SubscriptionId,
    "sub-8a3f"
);

impl NodeId {
    /// The truncated form the UI shows: first four and last four characters
    /// joined by `..`, matching `7F3A..8C2E`.
    ///
    /// Identifiers of nine characters or fewer are returned whole rather than
    /// expanded, since abbreviating them would make them longer.
    #[must_use]
    pub fn truncated(&self) -> String {
        let s = &self.0;
        // Counted in characters, not bytes: a byte-length guard would
        // abbreviate a short multi-byte identifier that needs no abbreviating.
        let char_count = s.chars().count();
        if char_count <= 9 {
            return s.to_string();
        }
        let head: String = s.chars().take(4).collect();
        let tail: String = s.chars().skip(char_count - 4).collect();
        format!("{head}..{tail}")
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn ids_serialize_as_bare_strings() {
        // `serde(transparent)` matters: the frontend expects "7F3A..8C2E",
        // not {"0":"7F3A..8C2E"}.
        let id = NodeId::new("7F3A8C2E");
        assert_eq!(serde_json::to_string(&id).unwrap(), "\"7F3A8C2E\"");
    }

    #[test]
    fn ids_round_trip_through_serde() {
        let id = IntentId::new("9F7A3B2C");
        let json = serde_json::to_string(&id).unwrap();
        assert_eq!(serde_json::from_str::<IntentId>(&json).unwrap(), id);
    }

    #[test]
    fn truncates_long_node_ids_for_display() {
        assert_eq!(NodeId::new("7F3A00000000008C2E").truncated(), "7F3A..8C2E");
    }

    #[test]
    fn returns_short_node_ids_whole() {
        // Abbreviating this would produce a longer string than the original.
        assert_eq!(NodeId::new("7F3A8C2E").truncated(), "7F3A8C2E");
    }

    #[test]
    fn truncation_is_char_aware_not_byte_aware() {
        // Eleven characters, thirty-three bytes. Slicing by byte would panic
        // mid-codepoint, and a byte-length guard would misjudge the threshold.
        let id = NodeId::new("日本語テスト文字列です");
        assert_eq!(id.truncated(), "日本語テ..字列です");
    }

    #[test]
    fn short_multibyte_ids_are_not_abbreviated() {
        // Nine characters but twenty-seven bytes: a byte-based guard would
        // have abbreviated this unnecessarily.
        let id = NodeId::new("日本語テスト文字列");
        assert_eq!(id.truncated(), "日本語テスト文字列");
    }
}
