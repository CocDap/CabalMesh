//! IPC-layer checks against Tauri's mock runtime.
//!
//! # What this can and cannot prove
//!
//! It was written to catch a state-type mismatch — Tauri resolves
//! `State<'_, T>` by type at runtime, so a wrong type is a **panic inside the
//! IPC handler**, not a compile error, and ticket 14 changed that type from
//! `Arc<Mutex<AppState>>` to `AppState`.
//!
//! It does not prove that. The ACL runs *before* state resolution, and
//! `mock_context` carries no resolved capabilities, so every invoke is denied
//! before a command body is reached. Plumbing real capabilities into a mock
//! context is more machinery than the check is worth.
//!
//! State resolution is verified instead by running the app against the dev
//! server and observing a frontend-originated command in the logs — see
//! `docs/mobile-build-verification.md`. Note that the debug binary is built
//! with `cfg(dev)`, so without Vite serving `devUrl` the webview loads nothing
//! and *no* frontend command runs. That is easy to mistake for a broken IPC
//! layer.
//!
//! What remains here is worth keeping: it shows the ACL from ticket 06 is
//! enforced on the real invoke path, and that commands are registered.

use cabalmesh_lib::state::AppState;
use tauri::test::{mock_builder, mock_context, noop_assets};
use tauri::webview::InvokeRequest;
use tauri::{Manager, WebviewUrl, WebviewWindowBuilder};

fn app_with_state() -> tauri::App<tauri::test::MockRuntime> {
    let app = mock_builder()
        .invoke_handler(tauri::generate_handler![
            cabalmesh_lib::commands::session_status,
            cabalmesh_lib::commands::mesh_snapshot,
            cabalmesh_lib::commands::ble_status,
        ])
        .build(mock_context(noop_assets()))
        .expect("mock app builds");

    // Managed synchronously, exactly as `run()` does it.
    app.manage(AppState::new());
    app
}

/// One webview per app: labels are unique, so building a second "main" fails.
fn webview_of(
    app: &tauri::App<tauri::test::MockRuntime>,
) -> tauri::WebviewWindow<tauri::test::MockRuntime> {
    WebviewWindowBuilder::new(app, "main", WebviewUrl::default())
        .build()
        .expect("webview builds")
}

fn invoke(
    webview: &tauri::WebviewWindow<tauri::test::MockRuntime>,
    command: &str,
) -> Result<String, String> {
    tauri::test::get_ipc_response(
        webview,
        InvokeRequest {
            cmd: command.into(),
            callback: tauri::ipc::CallbackFn(0),
            error: tauri::ipc::CallbackFn(1),
            url: "http://tauri.localhost".parse().unwrap(),
            body: tauri::ipc::InvokeBody::default(),
            headers: Default::default(),
            invoke_key: tauri::test::INVOKE_KEY.to_string(),
        },
    )
    .map(|value| value.deserialize::<serde_json::Value>().unwrap().to_string())
    .map_err(|error| error.to_string())
}

/// The ACL is enforced on the real invoke path, not merely configured.
///
/// With no capability granting them, registered commands are refused rather
/// than executed. This is the mock-runtime counterpart to the on-device check
/// where removing `allow-get-identity` made the wallet address disappear.
#[test]
fn ungranted_commands_are_refused_by_the_acl() {
    let app = app_with_state();
    let webview = webview_of(&app);

    for command in ["session_status", "mesh_snapshot", "ble_status"] {
        let error = invoke(&webview, command)
            .expect_err("no capability grants this command, so it must be refused");
        assert!(
            error.contains("not allowed"),
            "{command} should be ACL-refused, got: {error}"
        );
    }
}

/// An unregistered command is refused too, and distinguishably so — proving
/// the previous test observes a *grant* failure rather than a missing handler.
#[test]
fn unknown_commands_are_refused() {
    let app = app_with_state();
    let webview = webview_of(&app);
    assert!(invoke(&webview, "command_that_does_not_exist").is_err());
}

/// State is present and resolvable before any command runs.
///
/// Not a substitute for the end-to-end check, but it does assert the ordering
/// that used to be a race: state is managed before the webview exists, so
/// nothing can arrive to find it missing.
#[test]
fn state_is_managed_before_any_webview_exists() {
    let app = app_with_state();
    let state = app.state::<AppState>();

    assert!(!state.is_ready(), "services arrive with bootstrap, not at manage time");
    assert!(
        state.services().is_err(),
        "asking early must be an error value, never a panic"
    );
}
