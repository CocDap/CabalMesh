/// Declares every command this app exposes over IPC so Tauri generates an
/// `allow-*` / `deny-*` permission for each one.
///
/// Without a manifest, the app's own commands sit outside the ACL entirely.
/// Tauri 2.11.1 closed the hole where IPC from a remote origin bypassed access
/// control when no manifest was configured — but relying on the old behaviour
/// was never safe, and a declared manifest is what makes least privilege
/// expressible at all.
///
/// A generated permission does nothing until a capability references it, so
/// this list and `capabilities/desktop.json` (which now grants the same set
/// as `capabilities/mobile.json` — desktop and mobile share one frontend and
/// one handler in `lib.rs`) must stay in step. Declaring a command here
/// without granting it there makes the command unreachable.
const COMMANDS: &[&str] = &[
    // reshaped surface (src/commands.rs) — registered on every platform
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
    "llm_get_settings",
    "llm_save_settings",
    "llm_set_api_key",
    "llm_test_connection",
    "near_account",
    "wallet_setup_status",
    "wallet_check_name",
    "wallet_create_testnet_account",
    "near_deposit",
    "near_withdraw",
    "near_unwrap",
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

/// Read at build time, unlike the rest of the file, which the desktop app loads
/// when it starts. A mobile bundle has no such file, and a node that skipped the
/// fee would strand the fee its counterparty signed.
const REWARDS_ACCOUNTS: [&str; 2] = ["CABALMESH_REWARDS_TESTNET", "CABALMESH_REWARDS_MAINNET"];
const LOCAL_CONFIG: &str = ".env";

/// Hands the rewards accounts to `option_env!` in `src/network_config.rs`. A
/// variable already set in the shell wins, and an empty one counts as unset.
fn embed_rewards_accounts() {
    for name in REWARDS_ACCOUNTS {
        println!("cargo:rerun-if-env-changed={name}");
    }
    // Only if it exists: Cargo treats a missing path as changed on every build,
    // which would recompile the whole app each time.
    if std::path::Path::new(LOCAL_CONFIG).exists() {
        println!("cargo:rerun-if-changed={LOCAL_CONFIG}");
    }
    let Ok(entries) = dotenv::from_path_iter(LOCAL_CONFIG) else {
        return;
    };
    for (name, value) in entries.flatten() {
        let shell_wins = std::env::var_os(&name).is_some();
        if REWARDS_ACCOUNTS.contains(&name.as_str()) && !value.is_empty() && !shell_wins {
            println!("cargo:rustc-env={name}={value}");
        }
    }
}

fn main() {
    embed_rewards_accounts();
    tauri_build::try_build(
        tauri_build::Attributes::new()
            .app_manifest(tauri_build::AppManifest::new().commands(COMMANDS)),
    )
    .expect("failed to run tauri-build");
}
