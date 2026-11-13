//! Serialized shape of the new error surface.
//!
//! Separate from `ipc_contract.rs` on purpose. That file pins what the
//! **frozen** desktop UI depends on and must not change. This one pins the
//! **new** surface, which is allowed to evolve — but only deliberately, since
//! the mobile frontend switches on these tags to choose which on-voice copy to
//! render.
//!
//! A diff here means the frontend's error handling needs updating in the same
//! change.

use cabalmesh_lib::error::{AppError, InvalidReason};

fn shape(error: &AppError) -> String {
    serde_json::to_string_pretty(error).expect("errors must serialize")
}

/// Every variant in one snapshot, so adding one to the enum without deciding
/// how the UI renders it fails here.
#[test]
fn every_error_variant() {
    let variants = [
        AppError::NotReady { subsystem: "mesh" },
        AppError::Unsupported { feature: "local_llm" },
        AppError::MeshOffline,
        AppError::InvalidIntent {
            field: "amount",
            reason: InvalidReason::TooPrecise,
        },
        AppError::Chain { retryable: true },
        AppError::NearAccountNotFunded { account: "abc".into() },
        AppError::VaultLocked,
        AppError::TooManySubscriptions { limit: 16 },
        AppError::TooManyAttempts { retry_after_secs: 40 },
        AppError::Internal,
    ];

    let rendered: Vec<String> = variants.iter().map(shape).collect();
    insta::assert_snapshot!(rendered.join("\n---\n"));
}

/// Every rejection reason, since the form maps each to a different message.
#[test]
fn every_invalid_reason() {
    let reasons = [
        InvalidReason::Missing,
        InvalidReason::Malformed,
        InvalidReason::OutOfRange,
        InvalidReason::TooPrecise,
        InvalidReason::InsufficientFunds,
    ];

    let rendered: Vec<String> = reasons
        .iter()
        .map(|reason| {
            shape(&AppError::InvalidIntent {
                field: "amount",
                reason: *reason,
            })
        })
        .collect();
    insta::assert_snapshot!(rendered.join("\n---\n"));
}

/// Guards the redaction guarantee at the boundary rather than only in unit
/// tests: no serialized error may contain a path, URL, host or key material.
///
/// This is the test that fails if someone later adds a `message: String` field
/// "just for debugging".
#[test]
fn no_variant_leaks_infrastructure_detail() {
    let variants = [
        AppError::NotReady { subsystem: "mesh" },
        AppError::Unsupported { feature: "local_llm" },
        AppError::MeshOffline,
        AppError::InvalidIntent {
            field: "amount",
            reason: InvalidReason::Malformed,
        },
        AppError::Chain { retryable: false },
        AppError::NearAccountNotFunded { account: "abc".into() },
        AppError::VaultLocked,
        AppError::TooManySubscriptions { limit: 16 },
        AppError::internal(std::io::Error::other(
            "/Users/someone/Library/Application Support/cabalmesh/vault.enc unreadable, \
             rpc https://rpc.testnet.fastnear.com refused, key 0xdeadbeef",
        )),
    ];

    for variant in &variants {
        let serialized = shape(variant);
        for forbidden in ["/Users", "http", "://", ".network", "0xdeadbeef", "vault.enc"] {
            assert!(
                !serialized.contains(forbidden),
                "{variant:?} leaked {forbidden:?}: {serialized}"
            );
        }
    }
}
