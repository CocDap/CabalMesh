//! The error type that crosses the IPC boundary.
//!
//! # What was wrong before
//!
//! Every command returned `Result<T, String>` built from `e.to_string()`. That
//! has two costs. The frontend cannot branch — it receives prose and can only
//! display it, so on-voice copy is impossible and any localisation is
//! impossible. And it leaks: `e.to_string()` on an I/O error contains the
//! filesystem path, on a transport error the RPC URL, and both travel to the
//! webview.
//!
//! # The shape
//!
//! [`AppError`] serializes as a discriminated union tagged on `kind`, so the
//! frontend switches on a variant and renders its own copy. The variant is the
//! contract; the sentence is not.
//!
//! # Redaction is the point, not a side effect
//!
//! Variants carry only what is safe to show. Anything diagnostic —
//! source chains, paths, URLs, key material — goes to the log, never to the
//! return value. [`AppError::internal`] exists to make that the easy path:
//! it consumes the real error and yields a variant that carries none of it.
//!
//! # Not yet wired
//!
//! Services still return their own error types; this lands the taxonomy and
//! its guarantees first so later tickets have somewhere to convert *to*.

use serde::Serialize;

/// Every failure the frontend is allowed to see.
///
/// `#[non_exhaustive]` so adding a variant is not a breaking change for
/// downstream matches, and `tag = "kind"` so TypeScript gets a union it can
/// switch on.
#[derive(Debug, thiserror::Error, Serialize)]
#[cfg_attr(feature = "ts-rs", derive(ts_rs::TS), ts(export, export_to = "../../src/types/bindings.ts"))]
#[serde(tag = "kind", content = "detail", rename_all = "snake_case")]
#[non_exhaustive]
pub enum AppError {
    /// A subsystem has not finished starting.
    ///
    /// Not a failure — the connecting screen renders this as progress. It
    /// exists because state is managed before bootstrap completes, so a
    /// command can legitimately arrive early.
    #[error("subsystem not ready")]
    NotReady { subsystem: &'static str },

    /// The platform cannot do this at all.
    ///
    /// How mobile answers the desktop-only ZK and LLM commands. The frontend
    /// hides those affordances via the capability probe, so reaching this is a
    /// UI bug rather than a user error.
    #[error("not supported on this platform")]
    Unsupported { feature: &'static str },

    /// The mesh is unreachable. Renders as the offline banner.
    #[error("mesh unreachable")]
    MeshOffline,

    /// An intent was rejected before anything left the device.
    ///
    /// `field` is a stable identifier the form can attach to an input, not
    /// prose.
    #[error("invalid intent")]
    InvalidIntent {
        field: &'static str,
        reason: InvalidReason,
    },

    /// A chain interaction failed.
    ///
    /// Deliberately carries no message: RPC errors routinely embed the
    /// endpoint URL, and that is infrastructure detail the webview has no
    /// business holding.
    #[error("chain call failed")]
    Chain { retryable: bool },

    /// This account has no native NEAR, so it cannot pay for a transaction.
    ///
    /// Its own variant because the fix is something the user does: the screen
    /// names the account and says to fund it.
    #[error("account has no NEAR for gas")]
    NearAccountNotFunded { account: String },
    #[error("NEAR balance must also cover storage and gas")]
    InsufficientNative,
    #[error("native transfer needs confirmation")]
    NativeTransferUnconfirmed { transaction: Option<String> },

    /// Key material is not available — the vault is locked or the platform
    /// keystore refused.
    #[error("vault locked")]
    VaultLocked,

    /// Too many live streams. Guards the subscription registry against a UI
    /// that subscribes without tearing down.
    #[error("too many active subscriptions")]
    TooManySubscriptions { limit: u16 },

    /// The model call failed in a way the user can fix in Profile.
    ///
    /// Carries no text: a provider's error can name the endpoint.
    #[error("model call failed")]
    Model { problem: ModelProblem },

    /// Something went wrong that the frontend cannot act on.
    ///
    /// Carries nothing. The real error is logged; see [`AppError::internal`].
    #[error("internal error")]
    Internal,
}

/// Why an intent was rejected. A variant rather than a sentence, so the form
/// can render on-voice copy per field.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[cfg_attr(feature = "ts-rs", derive(ts_rs::TS), ts(export, export_to = "../../src/types/bindings.ts"))]
#[serde(rename_all = "snake_case")]
#[non_exhaustive]
pub enum InvalidReason {
    /// Required and absent.
    Missing,
    /// Present but unparseable.
    Malformed,
    /// Parsed, but outside the permitted range.
    OutOfRange,
    /// More decimal places than the asset supports.
    TooPrecise,
    /// Balance does not cover it.
    InsufficientFunds,
    /// Would send a secret where it can be read on the wire.
    Insecure,
}

/// What went wrong with a model call, so the screen can say what to fix.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[cfg_attr(feature = "ts-rs", derive(ts_rs::TS), ts(export, export_to = "../../src/types/bindings.ts"))]
#[serde(rename_all = "snake_case")]
#[non_exhaustive]
pub enum ModelProblem {
    Unreachable,
    /// The endpoint answered with an error: usually a bad key or model name.
    Refused,
    /// The model answered with something unusable.
    BadAnswer,
    BudgetUsedUp,
    NoModel,
}

impl From<crate::llm_client::LlmError> for AppError {
    fn from(error: crate::llm_client::LlmError) -> Self {
        use crate::llm_client::LlmError;
        tracing::warn!(target: "cabalmesh::llm", %error, "model call failed");
        let problem = match error {
            LlmError::Unreachable => ModelProblem::Unreachable,
            LlmError::Http { .. } => ModelProblem::Refused,
            LlmError::BadResponse(_) => ModelProblem::BadAnswer,
            LlmError::BudgetExhausted => ModelProblem::BudgetUsedUp,
            LlmError::NoModel => ModelProblem::NoModel,
        };
        Self::Model { problem }
    }
}

impl AppError {
    /// Wraps an error that the frontend cannot act on.
    ///
    /// The source is **dropped from the return value on purpose** and logged
    /// instead. Call this rather than constructing [`AppError::Internal`]
    /// directly, so exactly one place is responsible for recording the detail
    /// being withheld — and so it is recorded exactly once, rather than at
    /// every level that propagates it.
    #[must_use]
    pub fn internal<E: std::error::Error>(source: E) -> Self {
        tracing::error!(
            target: "cabalmesh::error",
            error = %source,
            chain = %SourceChain(&source),
            "internal error"
        );
        Self::Internal
    }

    /// As [`AppError::internal`], for sources that only implement `Display`.
    ///
    /// Prefer the typed version: `Box<dyn Error>` and bare strings lose the
    /// source chain, which is the most useful part of a diagnostic.
    #[must_use]
    pub fn internal_msg<E: std::fmt::Display>(source: E) -> Self {
        tracing::error!(target: "cabalmesh::error", error = %source, "internal error");
        Self::Internal
    }

    /// Whether retrying unchanged could plausibly succeed. Drives whether the
    /// UI offers a retry affordance.
    #[must_use]
    pub const fn is_retryable(&self) -> bool {
        match self {
            Self::MeshOffline | Self::NotReady { .. } => true,
            Self::Chain { retryable } => *retryable,
            Self::Model { problem } => matches!(problem, ModelProblem::Unreachable),
            Self::Unsupported { .. }
            | Self::InvalidIntent { .. }
            | Self::NearAccountNotFunded { .. }
            | Self::InsufficientNative
            | Self::NativeTransferUnconfirmed { .. }
            | Self::VaultLocked
            | Self::TooManySubscriptions { .. }
            | Self::Internal => false,
        }
    }
}

/// Renders an error's full `source()` chain as `outer: middle: root`.
///
/// The root cause is usually the useful part and is exactly what `Display` on
/// the outermost error throws away.
struct SourceChain<'a>(&'a dyn std::error::Error);

impl std::fmt::Display for SourceChain<'_> {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}", self.0)?;
        let mut current = self.0.source();
        while let Some(cause) = current {
            write!(f, ": {cause}")?;
            current = cause.source();
        }
        Ok(())
    }
}

impl From<crate::near_bridge::NearError> for AppError {
    fn from(error: crate::near_bridge::NearError) -> Self {
        use crate::near_bridge::NearError as N;
        tracing::warn!(target: "cabalmesh::error", %error, "chain call failed");
        match error {
            N::AccountNotFunded { account } => Self::NearAccountNotFunded { account },
            N::InsufficientNative => Self::InsufficientNative,
            N::NativeTransferUnconfirmed { transaction } => Self::NativeTransferUnconfirmed { transaction },
            N::Rejected(_) => Self::Chain { retryable: false },
            N::Rpc(_) | N::Network(_) => Self::Chain { retryable: true },
        }
    }
}

impl From<cabal_core::AmountError> for AppError {
    /// Amount parsing failures are user input problems, so they map to a
    /// field-level rejection rather than an internal error.
    fn from(error: cabal_core::AmountError) -> Self {
        use cabal_core::AmountError as A;
        let reason = match error {
            A::Empty => InvalidReason::Missing,
            A::InvalidCharacter | A::MultipleDecimalPoints => InvalidReason::Malformed,
            A::TooManyDecimals { .. } => InvalidReason::TooPrecise,
            A::Overflow => InvalidReason::OutOfRange,
            // AmountError is #[non_exhaustive]; anything new is malformed
            // until it is classified deliberately.
            _ => InvalidReason::Malformed,
        };
        Self::InvalidIntent { field: "amount", reason }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn json(error: &AppError) -> String {
        serde_json::to_string(error).unwrap()
    }

    #[test]
    fn serializes_as_a_tagged_union() {
        assert_eq!(json(&AppError::MeshOffline), r#"{"kind":"mesh_offline"}"#);
        assert_eq!(
            json(&AppError::NotReady { subsystem: "mesh" }),
            r#"{"kind":"not_ready","detail":{"subsystem":"mesh"}}"#
        );
    }

    #[test]
    fn internal_errors_carry_nothing_to_the_webview() {
        // The whole reason this variant is unit-shaped: an io::Error's Display
        // contains the path that failed, and that must not reach the frontend.
        let source = std::io::Error::new(
            std::io::ErrorKind::NotFound,
            "/Users/someone/Library/Application Support/cabalmesh/vault.enc",
        );
        let serialized = json(&AppError::internal(source));

        assert_eq!(serialized, r#"{"kind":"internal"}"#);
        assert!(!serialized.contains("Users"));
        assert!(!serialized.contains("vault"));
    }

    #[test]
    fn chain_errors_do_not_leak_the_endpoint() {
        // Transport errors routinely embed the RPC URL. The variant has no
        // field that could carry it.
        let serialized = json(&AppError::Chain { retryable: true });
        assert!(!serialized.contains("http"));
        assert!(!serialized.contains("rpc."));
    }

    #[test]
    fn amount_failures_become_field_level_rejections() {
        let error: AppError = cabal_core::AmountError::TooManyDecimals { found: 3, supported: 2 }.into();
        assert!(matches!(
            error,
            AppError::InvalidIntent { field: "amount", reason: InvalidReason::TooPrecise }
        ));
    }

    #[test]
    fn retryability_matches_the_variant() {
        assert!(AppError::MeshOffline.is_retryable());
        assert!(AppError::Chain { retryable: true }.is_retryable());
        assert!(!AppError::Chain { retryable: false }.is_retryable());
        assert!(!AppError::VaultLocked.is_retryable());
        assert!(!AppError::Internal.is_retryable());
    }

    #[test]
    fn display_messages_are_lowercase_and_unpunctuated() {
        // Rust convention, and these are logged rather than shown — the UI
        // renders its own copy from the variant.
        for error in [
            AppError::MeshOffline,
            AppError::VaultLocked,
            AppError::Internal,
            AppError::Unsupported { feature: "local_llm" },
        ] {
            let message = error.to_string();
            assert!(!message.ends_with('.'), "{message}");
            assert_eq!(message, message.to_lowercase(), "{message}");
        }
    }
}
