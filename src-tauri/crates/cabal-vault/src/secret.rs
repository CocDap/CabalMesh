//! A wrapper that refuses to print what it holds.

use serde::{Deserialize, Serialize};
use zeroize::{Zeroize, ZeroizeOnDrop};

/// A string that never appears in `Debug` output.
///
/// # Why a type rather than discipline
///
/// A private key inside a plain `String` leaks the moment anything derives
/// `Debug` on a struct containing it — a `dbg!`, a `tracing` field, an error
/// that formats its context. None of those authors intended to log a key; they
/// just formatted a struct.
///
/// Wrapping the field makes the safe behaviour the default and the unsafe one
/// explicit: reading the value requires calling [`Secret::expose`], which is
/// greppable in review in a way that `{:?}` is not.
///
/// Serialization is *not* suppressed — the vault has to persist these. The
/// protection there is encryption at rest, not redaction.
#[derive(Clone, PartialEq, Eq, Serialize, Deserialize, Zeroize, ZeroizeOnDrop)]
#[serde(transparent)]
pub struct Secret(String);

impl Secret {
    /// Wraps a sensitive string.
    #[must_use]
    pub fn new(value: impl Into<String>) -> Self {
        Self(value.into())
    }

    /// Reveals the value.
    ///
    /// Named to be conspicuous. Every call site is a place someone decided the
    /// secret should leave its wrapper, and should read that way in review.
    #[must_use]
    pub fn expose(&self) -> &str {
        &self.0
    }

    /// Whether the secret is empty.
    #[must_use]
    pub fn is_empty(&self) -> bool {
        self.0.is_empty()
    }
}

impl std::fmt::Debug for Secret {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str("<redacted>")
    }
}

/// Also redacted: `Display` is what interpolation into a log message uses, and
/// `warn!("key {}", key)` is exactly the accident this prevents.
impl std::fmt::Display for Secret {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str("<redacted>")
    }
}

impl From<String> for Secret {
    fn from(value: String) -> Self {
        Self(value)
    }
}

impl From<&str> for Secret {
    fn from(value: &str) -> Self {
        Self(value.to_owned())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn debug_output_is_redacted() {
        let secret = Secret::new("0xdeadbeefcafe");
        assert_eq!(format!("{secret:?}"), "<redacted>");
        assert!(!format!("{secret:?}").contains("deadbeef"));
    }

    #[test]
    fn display_output_is_redacted() {
        // Covers `warn!("key {}", key)`, the most likely accident.
        let secret = Secret::new("0xdeadbeefcafe");
        assert_eq!(format!("{secret}"), "<redacted>");
    }

    #[test]
    fn redaction_survives_being_nested_in_a_derived_debug() {
        // The real failure mode: nobody logs the key directly, they log a
        // struct that happens to contain one.
        #[derive(Debug)]
        struct Identity {
            alias: String,
            private_key: Secret,
        }

        let rendered = format!(
            "{:?}",
            Identity {
                alias: "Genesis Fox".into(),
                private_key: Secret::new("0xdeadbeefcafe"),
            }
        );
        assert!(rendered.contains("Genesis Fox"));
        assert!(!rendered.contains("deadbeef"));
    }

    #[test]
    fn the_value_is_still_reachable_when_asked_for() {
        assert_eq!(Secret::new("0xabc").expose(), "0xabc");
    }

    #[test]
    fn serialization_is_transparent_because_the_vault_must_persist_it() {
        let json = serde_json::to_string(&Secret::new("0xabc")).unwrap();
        assert_eq!(json, "\"0xabc\"");
        assert_eq!(
            serde_json::from_str::<Secret>(&json).unwrap().expose(),
            "0xabc"
        );
    }
}
