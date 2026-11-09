//! CabalMesh domain model.
//!
//! Everything the UI displays, expressed as types the compiler can check.
//!
//! # The constraint that makes this crate worth having
//!
//! This crate depends on `serde` and `thiserror` and **nothing else**. No
//! `tauri`, no `tokio`, no `reqwest`, no `libp2p`.
//!
//! That is not tidiness. Those dependencies are what make the app crate slow
//! to build and impossible to test without a device, a chain endpoint or a
//! running mesh. Keeping the domain free of them means the rules that actually
//! matter — which intent transitions are legal, whether an amount parses
//! without losing money — are tested in milliseconds on the host, thousands of
//! cases at a time, instead of behind a multi-minute cross-compile and link.
//!
//! If something here needs I/O, it belongs in a different crate.
//!
//! # What lives here
//!
//! - [`ids`] — opaque identifiers, so a node cannot be passed where an intent
//!   is expected.
//! - [`money`] — fixed-point amounts and prices. Never `f64`.
//! - [`intent`] — actions, modes, conditions, and the intent lifecycle.

#![forbid(unsafe_code)]

pub mod ids;
pub mod intent;
pub mod money;

pub use ids::{IntentId, NodeId, ProofHash, SubscriptionId};
pub use intent::{
    PublicIntent,
    Action, Chain, Condition, ExecutionMode, FailureReason, IntentDraft, IntentStatus, PrivacyLevel, ProofKind,
};
pub use money::{AmountError, TokenAmount, UsdPrice};
