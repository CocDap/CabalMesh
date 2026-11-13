//! Serialization contract for types crossing the IPC and mesh-wire boundary.
//!
//! Without a mechanical guard, a renamed field or a changed enum tag produces
//! `undefined` in the webview (or a peer that can't parse the wire message),
//! not a compiler error.
//!
//! # Why shapes, not live output
//!
//! Producing these values for real needs something this suite must not depend
//! on: a reachable NEAR RPC, a running Ollama, or a live libp2p mesh. Snapshotting runtime output would be neither
//! reproducible nor CI-safe, and the results would drift with chain state.
//!
//! What matters is the **serialized shape** of the values crossing the
//! boundary — field names, casing, enum tagging, and how optionality is
//! represented. That is what these snapshots pin, using fixtures rather than
//! services.
//!
//! # What a failure here means
//!
//! A diff is not automatically a bug — but it is always a decision. Either the
//! change is intentional and any consumer of the old shape needs a
//! compatibility adapter to match, or it is accidental and should be
//! reverted. Never accept a snapshot without deciding which.
//!
//! Run `cargo insta review` to inspect diffs.

use cabal_core::Chain;
use cabal_near::{token_diff_payload, AccountId, NearKeypair, TokenDiff};
use cabalmesh_lib::bindings::{
    GuardianEnrollProgress, GuardianEnrollStage, GuardianPromptKind, GuardianRestorePendingNotice, GuardianRestoreProgress,
    GuardianRestoreState, GuardianUnlockPrompt, GuardianUnlockProgress,
};
use cabalmesh_lib::commands::{
    GuardianCandidate, GuardianEnrollResult, GuardianStatus, NearAccountView, NearTokenView, SecurityStatus,
};
use cabalmesh_lib::identity::IdentityView;
use cabalmesh_lib::mesh::{MeshEvent, PrivacyIntent};
use cabalmesh_lib::near_settlement::{SettlementComplete, SignedDiff};
use serde::Serialize;

/// Serializes to pretty JSON — the shape the webview receives.
fn shape<T: Serialize>(value: &T) -> String {
    serde_json::to_string_pretty(value).expect("boundary type must serialize")
}

// ---------------------------------------------------------------------------
// Identity and wallet
// ---------------------------------------------------------------------------

#[test]
fn identity_view_shape() {
    insta::assert_snapshot!(shape(&IdentityView {
        alias: "Genesis Fox".into(),
        emoji: "🦊".into(),
        address: "b6336d522196d849252c5dbef77215710abf748ac53afc63ece79378d188b1ff".into(),
    }));
}

#[test]
fn near_account_view_shape() {
    insta::assert_snapshot!(shape(&NearAccountView {
        account_id: "b6336d522196d849252c5dbef77215710abf748ac53afc63ece79378d188b1ff".into(),
        network: "NEAR Testnet".into(),
        native_balance: Some("1.5".into()),
        native_available: Some("1.48".into()),
        native_in_intents: Some("0.5".into()),
        native_wrapped: Some("0.1".into()),
        native_shares_wnear: false,
        native_transfers_supported: true,
        tokens: vec![
            NearTokenView {
                symbol: "wNEAR".into(),
                decimals: 24,
                wallet: Some("0".into()),
                in_intents: Some("40".into()),
            },
            NearTokenView { symbol: "USDC".into(), decimals: 6, wallet: None, in_intents: None },
        ],
        faucet_links: vec!["https://near-faucet.io/".into()],
        explorer_url: "https://testnet.nearblocks.io/address/b633".into(),
    }));
}

// ---------------------------------------------------------------------------
// Guardian mesh unlock
// ---------------------------------------------------------------------------

#[test]
fn guardian_candidate_shape() {
    insta::assert_snapshot!(shape(&GuardianCandidate {
        peer_id: "7f3a1209c2e100aa".into(),
        label: "7F3A…00AA".into(),
        hops: 1,
    }));
}

#[test]
fn guardian_status_shape() {
    insta::assert_snapshot!(shape(&GuardianStatus {
        enrolled: true,
        guardian_count: 5,
        threshold: 3,
        holding_for: 0,
        restore_ready: true,
        holding_unlocked: true,
        suggest_backup: false,
    }));
}

#[test]
fn guardian_enroll_result_shape() {
    insta::assert_snapshot!(shape(&GuardianEnrollResult {
        enrolled: vec!["7f3a1209c2e100aa".into()],
        no_reply: vec!["91bec2e10800aabb".into()],
    }));
}

#[test]
fn guardian_unlock_prompt_shape() {
    insta::assert_snapshot!(shape(&GuardianUnlockPrompt {
        id: 42,
        from: "4B12…9F00".into(),
        kind: GuardianPromptKind::Restore,
        enrolled_at: Some("2026-08-02T00:00:00+00:00".into()),
    }));
}

#[test]
fn guardian_restore_progress_shape() {
    insta::assert_snapshot!(shape(&GuardianRestoreProgress {
        state: GuardianRestoreState::Collecting,
        released: 1,
        threshold: 3,
        waiting: 2,
        waiting_until: Some("2026-08-03T14:02:00+00:00".into()),
        cancelled: 0,
    }));
}

#[test]
fn guardian_event_payload_shapes() {
    insta::assert_snapshot!(shape(&GuardianRestorePendingNotice { release_at: "2026-08-03T14:02:00+00:00".into() }));
    insta::assert_snapshot!(shape(&GuardianUnlockProgress { approved: 2, threshold: 3 }));
    insta::assert_snapshot!(shape(&GuardianEnrollProgress { peer_id: "7f3a1209c2e100aa".into(), stage: GuardianEnrollStage::Delivered }));
}

#[test]
fn security_status_shape() {
    insta::assert_snapshot!(shape(&SecurityStatus { locked: false, passphrase_enabled: true, pin_enabled: false, pin_available: false }));
}

// ---------------------------------------------------------------------------
// Content
// ---------------------------------------------------------------------------

#[test]
fn get_settings_never_returns_key() {
    let settings = cabalmesh_lib::llm_config::LlmSettings::default();
    let view = serde_json::to_value(cabalmesh_lib::commands::llm_settings_view(settings, true)).unwrap();
    let mut fields: Vec<_> = view.as_object().unwrap().keys().map(String::as_str).collect();
    fields.sort_unstable();
    // Exact on purpose: a new field here crosses to the webview, so it has to be
    // added by someone looking at this list.
    assert_eq!(
        fields,
        ["baseUrl", "callsPerHour", "hasApiKey", "model", "preset", "presets", "toolsSupported"]
    );
    assert_eq!(view["hasApiKey"], true);
    assert_eq!(view["preset"], "ollama-local");
}

#[test]
fn llm_presets_are_listed_in_the_screens_order() {
    let settings = cabalmesh_lib::llm_config::LlmSettings::default();
    let view = serde_json::to_value(cabalmesh_lib::commands::llm_settings_view(settings, false)).unwrap();
    let ids: Vec<_> = view["presets"].as_array().unwrap().iter().map(|preset| preset["id"].as_str().unwrap()).collect();
    assert_eq!(ids, ["near-ai", "ollama-local", "openrouter", "custom"]);
    assert_eq!(view["presets"][0]["baseUrl"], "https://cloud-api.near.ai/v1");
    assert_eq!(view["presets"][2]["baseUrl"], "https://openrouter.ai/api/v1");
    assert_eq!(view["presets"][1]["model"], "llama3.1:8b");
    assert_eq!(view["presets"][0]["model"], "");
}

// ---------------------------------------------------------------------------
// Mesh
// ---------------------------------------------------------------------------

#[test]
fn privacy_intent_shape() {
    insta::assert_snapshot!(shape(&PrivacyIntent {
        id: "intent-1".into(),
        intent_type: "trade".into(),
        payload: "{\"action\":\"buy\"}".into(),
        encrypted: true,
        relay_path: vec!["origin_node".into()],
        relay_fee: None,
    }));
}

/// Every `MeshEvent` variant in one snapshot.
///
/// These arrive at the webview through `emit("mesh-event", ..)` and the frozen
/// UI dispatches on the `type` tag, so adding, renaming or retagging a variant
/// is a breaking change even though nothing in Rust would complain.
#[test]
fn mesh_event_variants_shape() {
    let events = vec![
        MeshEvent::ListeningStarted {
            address: "/ip4/127.0.0.1/tcp/61854".into(),
        },
        MeshEvent::PeerDiscovered {
            peer_id: "12D3KooWJLUV".into(),
            address: "/ip4/192.168.1.118/tcp/60744".into(),
        },
        MeshEvent::IntentReceived {
            intent: PrivacyIntent {
                id: "intent-1".into(),
                intent_type: "trade".into(),
                payload: "{}".into(),
                encrypted: false,
                relay_path: vec!["origin_node".into()],
                relay_fee: None,
            },
        },
        MeshEvent::Negotiation {
            details: "offer 3".into(),
        },
        MeshEvent::SettlementComplete {
            details: "settled in 11.4s".into(),
        },
        MeshEvent::SignedDiff {
            details: "one side signed".into(),
        },
    ];
    insta::assert_snapshot!(shape(&events));
}

// ---------------------------------------------------------------------------
// Settlement on the wire
// ---------------------------------------------------------------------------

/// A peer running an older build has to read these, and its signature only
/// covers `payload`, so nothing else would catch a renamed field.
#[test]
fn signed_diff_shape() {
    let keypair = NearKeypair::from_secret_bytes(&[1; 32]);
    let verifier = AccountId::try_from("verifier.tidepool-sandbox.testnet").unwrap();
    let diff: TokenDiff = [("nep141:b".to_owned(), -1_000_000), ("nep141:q".to_owned(), 4_040_000)].into_iter().collect();

    insta::assert_snapshot!(shape(&SignedDiff {
        deal_id: "a:b".into(),
        chain: Chain::NearTestnet,
        side: "sell".into(),
        payload: token_diff_payload(&keypair, &verifier, &diff, None, 1_900_000_000_000, [7; 32]),
        submitting: true,
    }));
}

#[test]
fn settlement_complete_shape() {
    insta::assert_snapshot!(shape(&SettlementComplete {
        deal_id: "a:b".into(),
        tx_hash: "G1s5UDrB6f2phA7RdNvYnp8ehDninikrwFYqsESDVef5".into(),
        gateway: "339c6d2483fb8cf7ed7864b224ac4606aa426d0f0415614914880af313decc93".into(),
    }));
}

// ---------------------------------------------------------------------------
// Ad-hoc JSON payloads
// ---------------------------------------------------------------------------

/// The bootstrap progress payload, emitted on `bootstrap-status`. The struct
/// is private to `app_initializer`, so its shape is asserted structurally.
#[test]
fn bootstrap_status_shape() {
    let value = serde_json::json!({
        "phase": "PHASE_2_NETWORK",
        "message": "Booting Libp2p Swarm...",
        "progress": 70,
    });
    insta::assert_snapshot!(shape(&value));
}

// ---------------------------------------------------------------------------
// Command inventory
// ---------------------------------------------------------------------------

/// Guards the command surface itself.
///
/// Kept in step with the `COMMANDS` list in `build.rs`, which generates the
/// ACL permissions, and with the single handler registration in `lib.rs`
/// (desktop and mobile share it — see `tests/handler_arms.rs` for the
/// per-platform capability-file check). A command removed or renamed without
/// updating all three is a silent 404, and a permission granted for a command
/// that no longer exists fails the build.
#[test]
fn command_inventory() {
    let mut commands = vec![
        "ble_status",
        "broadcast_intent",
        "cancel_intent",
        "enter_mesh",
        "guardian_approve_unlock",
        "guardian_candidates",
        "guardian_deny_unlock",
        "guardian_enroll",
        "guardian_request_unlock",
        "guardian_status",
        "intent_detail",
        "intent_form_options",
        "intent_proof",
        "list_intents",
        "list_nearby_nodes",
        "mesh_snapshot",
        "near_account",
        "wallet_setup_status",
        "wallet_check_name",
        "wallet_create_testnet_account",
        "near_deposit",
        "near_withdraw",
        "near_unwrap",
        "rewards_status",
        "rewards_claim",
        "llm_get_settings",
        "llm_save_settings",
        "llm_set_api_key",
        "llm_test_connection",
        "network_set",
        "parse_intent_chat",
        "preview_intent",
        "profile_summary",
        "security_disable_passphrase",
        "security_enable_passphrase",
        "security_status",
        "security_unlock",
        "session_status",
        "set_offline_mode",
        "settle_intent",
        "subscribe_mesh_log",
        "subscribe_settlement_log",
        "unsubscribe",
        "vault_address",
        "vault_assets",
        "vault_refresh_balance",
        "vault_export_key",
        "vault_identities",
        "vault_import_key",
        "vault_keys",
    ];
    commands.sort_unstable();
    assert_eq!(commands.len(), 50, "command count changed");
    insta::assert_snapshot!(commands.join("\n"));
}
