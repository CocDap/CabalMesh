/// Commands the webview may call.
///
/// **Empty on purpose.** This plugin is reached from Rust, never from
/// JavaScript: the BLE plane's whole surface is `ble_status` and the nodes
/// screen, and granting the webview a way to drive a radio directly would be a
/// capability nothing needs. See `capabilities/README.md`.
const COMMANDS: &[&str] = &[];

fn main() {
    tauri_plugin::Builder::new(COMMANDS)
        .android_path("android")
        .build();
}
