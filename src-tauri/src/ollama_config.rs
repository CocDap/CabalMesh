use std::fs;
use std::path::PathBuf;
use std::sync::RwLock;

use crate::app_paths;

/// Model used to translate free text into the `NEW INTENT` form.
pub const INTENT_MODEL: &str = "llama3.1:8b";

/// Android Emulator's stable alias for the development host.
///
/// A physical Android device must persist a reachable LAN URL with
/// [`set_url`]; `localhost` and `10.0.2.2` both refer to something other than
/// the developer machine there.
#[cfg(target_os = "android")]
pub const DEFAULT_URL: &str = "http://10.0.2.2:11434";

#[cfg(not(target_os = "android"))]
pub const DEFAULT_URL: &str = "http://localhost:11434";
pub const ENV_VAR: &str = "CABALMESH_OLLAMA_URL";

/// Cache of the persisted URL, so every request doesn't hit the disk.
/// `None` means "not loaded yet", `Some(None)` means "loaded, nothing stored".
static CACHED: RwLock<Option<Option<String>>> = RwLock::new(None);

fn config_path() -> PathBuf {
    app_paths::data_dir().join("ollama_url.txt")
}

/// Base URL of the Ollama server, without a trailing slash.
///
/// Resolution order: `CABALMESH_OLLAMA_URL`, then whatever `set_url` last
/// stored, then the platform default. Android Emulator uses its `10.0.2.2`
/// host alias; other platforms use localhost. Physical mobile devices cannot
/// spawn Ollama and must be pointed at a reachable server with [`set_url`].
pub fn url() -> String {
    if let Ok(from_env) = std::env::var(ENV_VAR) {
        if let Some(cleaned) = normalize(&from_env) {
            return cleaned;
        }
    }

    if let Some(stored) = CACHED.read().ok().and_then(|c| c.clone()) {
        return stored.unwrap_or_else(|| DEFAULT_URL.to_string());
    }

    let stored = fs::read_to_string(config_path())
        .ok()
        .and_then(|s| normalize(&s));
    if let Ok(mut cache) = CACHED.write() {
        *cache = Some(stored.clone());
    }
    stored.unwrap_or_else(|| DEFAULT_URL.to_string())
}

/// Point the app at a different Ollama server, persisting across restarts.
/// An empty value clears the override and falls back to localhost.
pub fn set_url(new_url: &str) -> Result<String, String> {
    let cleaned = normalize(new_url);

    match &cleaned {
        Some(u) => {
            if !u.starts_with("http://") && !u.starts_with("https://") {
                return Err(format!(
                    "Ollama URL must start with http:// or https://, got {u}"
                ));
            }
            fs::write(config_path(), u).map_err(|e| format!("Failed to save Ollama URL: {e}"))?;
        }
        None => {
            // Missing file is the "no override" state, so removing is the reset.
            if let Err(e) = fs::remove_file(config_path()) {
                if e.kind() != std::io::ErrorKind::NotFound {
                    return Err(format!("Failed to clear Ollama URL: {e}"));
                }
            }
        }
    }

    if let Ok(mut cache) = CACHED.write() {
        *cache = Some(cleaned);
    }
    Ok(url())
}

fn normalize(raw: &str) -> Option<String> {
    let trimmed = raw.trim().trim_end_matches('/');
    if trimmed.is_empty() {
        None
    } else {
        Some(trimmed.to_string())
    }
}
