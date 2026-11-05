//! `lib.rs` registers exactly one invoke-handler arm, and it carries every
//! reshaped command.
//!
//! # The failure this exists for
//!
//! `run()` used to build two `generate_handler!` lists: one for desktop with
//! the frozen legacy surface, one for mobile without it. A command added to
//! the desktop arm and forgotten on the mobile one compiled, passed every
//! test, and was granted by `capabilities/mobile.json` — the ACL allowed it,
//! the command simply was not registered, so the invoke failed at runtime on
//! the phone and nowhere else. That is exactly what happened to `ble_status`:
//! granted, present in the generated ACL schema, and missing from the mobile
//! arm. The symptom was a panel that silently rendered nothing on device
//! while every test was green, and it cost a full simulator build to find.
//!
//! The legacy arm and its `desktop-legacy` feature are gone now — desktop and
//! mobile share one frontend and one handler — but the class of bug they
//! caused is worth guarding against permanently: this file fails loudly if a
//! second arm ever reappears, or if the one arm ever drops a command that
//! `build.rs` or a capability file still expects.
//!
//! # Why this reads the source
//!
//! A macro's contents are not introspectable at runtime, and building a mock
//! app proves only that the arm the test was compiled for exists. Reading
//! `lib.rs` is crude, and it is the only thing that actually checks this.

use std::collections::BTreeSet;

/// The reshaped command surface — the commands both platforms must have.
///
/// The legacy desktop commands are deliberately absent: they exist only on
/// desktop, which is the whole point of the split.
const SHARED: &[&str] = &[
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
    "llm_get_settings",
    "llm_save_settings",
    "llm_set_api_key",
    "llm_test_connection",
    "mesh_snapshot",
    "near_account",
    "near_deposit",
    "near_withdraw",
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
    "vault_export_key",
    "vault_identities",
    "vault_import_key",
    "vault_keys",
];

fn source() -> String {
    std::fs::read_to_string(concat!(env!("CARGO_MANIFEST_DIR"), "/src/lib.rs"))
        .expect("lib.rs is readable from the crate root")
}

/// The command names inside each `generate_handler!` invocation.
fn handler_arms(source: &str) -> Vec<BTreeSet<String>> {
    let mut arms = Vec::new();
    let mut rest = source;

    while let Some(start) = rest.find("tauri::generate_handler![") {
        let body_start = start + "tauri::generate_handler![".len();
        let Some(end) = rest[body_start..].find(']') else {
            break;
        };
        let body = &rest[body_start..body_start + end];

        arms.push(
            body.split(',')
                .map(str::trim)
                .filter(|entry| !entry.is_empty())
                .filter_map(|entry| entry.rsplit("::").next())
                .map(str::to_string)
                .collect(),
        );

        rest = &rest[body_start + end..];
    }

    arms
}

#[test]
fn there_is_exactly_one_handler_arm() {
    // If a second appears, desktop and mobile are diverging again — the
    // exact split that used to hide `ble_status` from the phone.
    let arms = handler_arms(&source());
    assert_eq!(
        arms.len(),
        1,
        "expected a single handler shared by every platform, found {}",
        arms.len()
    );
}

#[test]
fn the_handler_registers_every_shared_command() {
    let arms = handler_arms(&source());
    let arm = arms.first().expect("there_is_exactly_one_handler_arm covers absence");

    let missing: Vec<&str> = SHARED
        .iter()
        .copied()
        .filter(|command| !arm.contains(*command))
        .collect();

    assert!(
        missing.is_empty(),
        "the handler is missing {missing:?} — granted by the ACL, \
         absent from the handler, and therefore broken at runtime"
    );
}

#[test]
fn the_shared_list_matches_the_acl_manifest() {
    // `build.rs` generates a permission per command in its own COMMANDS list.
    // A command in a handler but not in that list has no permission to grant,
    // and one in the list but no handler is a permission for nothing.
    let build = std::fs::read_to_string(concat!(env!("CARGO_MANIFEST_DIR"), "/build.rs"))
        .expect("build.rs is readable");

    for command in SHARED {
        assert!(
            build.contains(&format!("\"{command}\"")),
            "`{command}` is registered as a command but has no entry in build.rs COMMANDS, \
             so no permission is generated for it and the ACL denies every call"
        );
    }
}

#[test]
fn the_mobile_capability_grants_every_shared_command() {
    // The third place the same name has to appear. Granting is separate from
    // registering, and missing either one fails only on a device.
    let capability =
        std::fs::read_to_string(concat!(env!("CARGO_MANIFEST_DIR"), "/capabilities/mobile.json"))
            .expect("mobile.json is readable");

    for command in SHARED {
        let permission = format!("allow-{}", command.replace('_', "-"));
        assert!(
            capability.contains(&permission),
            "`{permission}` is not granted in capabilities/mobile.json, so `{command}` \
             is denied on the phone however correctly it is registered"
        );
    }
}

#[test]
fn the_desktop_capability_grants_every_shared_command() {
    // Desktop used to grant a 50-command legacy surface on top of this list,
    // which hid drift between the two capability files behind a superset.
    // Now the grants are meant to be identical, so check desktop directly
    // rather than relying on "wider than mobile" to happen to still cover it.
    let capability =
        std::fs::read_to_string(concat!(env!("CARGO_MANIFEST_DIR"), "/capabilities/desktop.json"))
            .expect("desktop.json is readable");

    for command in SHARED {
        let permission = format!("allow-{}", command.replace('_', "-"));
        assert!(
            capability.contains(&permission),
            "`{permission}` is not granted in capabilities/desktop.json, so `{command}` \
             is denied on desktop however correctly it is registered"
        );
    }
}
