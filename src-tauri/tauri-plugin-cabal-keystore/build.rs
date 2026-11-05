/// Commands the webview may call.
///
/// **Empty on purpose**, and more pointedly here than for the radio: this
/// plugin's only command hands back the device half of the vault key. A
/// capability that let JavaScript ask for it would put the secret one XSS away
/// from leaving the device, which is the entire thing the binding exists to
/// prevent. It is reached from Rust and nowhere else.
const COMMANDS: &[&str] = &[];

fn main() {
    tauri_plugin::Builder::new(COMMANDS)
        .android_path("android")
        .build();
}
