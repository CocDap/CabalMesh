/// Whether this platform lets the app launch helper binaries.
///
/// iOS and Android sandboxes forbid fork/exec, so every code path that shells
/// out to a helper (`ollama`, `nargo`) is unavailable there. The calls still
/// compile — `std::process` exists on mobile — they just always fail at
/// runtime, so guard on this instead of letting them produce a confusing
/// "No such file or directory".
pub const CAN_SPAWN_PROCESSES: bool = !cfg!(any(target_os = "ios", target_os = "android"));
