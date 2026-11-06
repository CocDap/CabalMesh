//! Which model this device talks to.
//!
//! Each device chooses its own endpoint. Base URL and model live in a plain file;
//! API keys live in their own encrypted file under the vault's data key, and never
//! cross the mesh or reach the webview.
//!
//! # Why a key is bound to the URL it was saved for
//!
//! A key is scoped per preset, but `custom` and `ollama-local` have editable URLs.
//! Scoping by preset alone would send the key to whatever host was typed next. So a
//! key is only ever returned for the exact base URL it was saved with.

use crate::error::{AppError, InvalidReason};
use cabal_store::DocumentStore;
use cabal_vault::{DataKey, Secret, StaticKeyProvider, Vault};
use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;
use std::path::Path;

pub const DEFAULT_CALLS_PER_HOUR: u32 = 30;
pub const MIN_CALLS_PER_HOUR: u32 = 1;
pub const MAX_CALLS_PER_HOUR: u32 = 600;

const OLLAMA_MODEL: &str = "llama3.1:8b";

/// Android Emulator's stable alias for the development host. A physical device
/// must be pointed at a reachable LAN address instead.
#[cfg(target_os = "android")]
const OLLAMA_URL: &str = "http://10.0.2.2:11434/v1";
#[cfg(not(target_os = "android"))]
const OLLAMA_URL: &str = "http://localhost:11434/v1";

/// Where a set of model settings came from.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
#[cfg_attr(feature = "ts-rs", derive(ts_rs::TS), ts(export, export_to = "../../src/types/bindings.ts"))]
#[serde(rename_all = "kebab-case")]
pub enum Preset {
    NearAi,
    OllamaLocal,
    #[serde(rename = "openrouter")]
    OpenRouter,
    Custom,
}

impl Preset {
    /// In the order the Profile screen lists them.
    pub const ALL: [Self; 4] = [Self::NearAi, Self::OllamaLocal, Self::OpenRouter, Self::Custom];

    #[must_use]
    pub const fn label(self) -> &'static str {
        match self {
            Self::NearAi => "NEAR AI Cloud",
            Self::OllamaLocal => "Ollama local",
            Self::OpenRouter => "OpenRouter",
            Self::Custom => "Custom",
        }
    }

    #[must_use]
    pub const fn base_url(self) -> &'static str {
        match self {
            Self::NearAi => "https://cloud-api.near.ai/v1",
            Self::OllamaLocal => OLLAMA_URL,
            Self::OpenRouter => "https://openrouter.ai/api/v1",
            Self::Custom => "",
        }
    }

    #[must_use]
    pub const fn model(self) -> &'static str {
        match self {
            Self::OllamaLocal => OLLAMA_MODEL,
            Self::NearAi | Self::OpenRouter | Self::Custom => "",
        }
    }
}

/// One device's model settings. Holds no key, so it is safe to log and to store
/// in the clear.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct LlmSettings {
    pub preset: Preset,
    pub base_url: String,
    pub model: String,
    pub calls_per_hour: u32,
    /// What the last connection test found. `None` until one has run against this
    /// base URL and model.
    #[serde(default)]
    pub tools_supported: Option<bool>,
}

impl Default for LlmSettings {
    /// Existing installs keep today's behaviour.
    fn default() -> Self {
        Self {
            preset: Preset::OllamaLocal,
            base_url: Preset::OllamaLocal.base_url().to_string(),
            model: Preset::OllamaLocal.model().to_string(),
            calls_per_hour: DEFAULT_CALLS_PER_HOUR,
            tools_supported: None,
        }
    }
}

fn store() -> DocumentStore {
    DocumentStore::new(crate::app_paths::in_data_dir("llm.json"))
}

/// This device's settings, or the defaults.
#[must_use]
pub fn load() -> LlmSettings {
    load_from(&store())
}

/// Persists this device's settings.
///
/// # Errors
///
/// The file could not be written.
pub fn save(settings: &LlmSettings) -> Result<(), AppError> {
    store().save(settings).map_err(AppError::internal)
}

/// Checks what the Profile screen submitted, and returns what should be saved.
///
/// # Errors
///
/// [`AppError::InvalidIntent`] on a bad base URL or a limit outside the allowed range.
pub fn applied(
    before: &LlmSettings,
    preset: Preset,
    base_url: &str,
    model: &str,
    calls_per_hour: u32,
) -> Result<LlmSettings, AppError> {
    if !(MIN_CALLS_PER_HOUR..=MAX_CALLS_PER_HOUR).contains(&calls_per_hour) {
        return Err(AppError::InvalidIntent { field: "CALLS PER HOUR", reason: InvalidReason::OutOfRange });
    }
    let base_url = normalize_base_url(base_url)?;
    let model = model.trim().to_string();
    // A connection test only speaks for the endpoint and model it ran against.
    let tools_supported =
        if before.base_url == base_url && before.model == model { before.tools_supported } else { None };
    Ok(LlmSettings { preset, base_url, model, calls_per_hour, tools_supported })
}

pub(crate) fn load_from(store: &DocumentStore) -> LlmSettings {
    if store.exists() {
        return store.load_or(LlmSettings::default());
    }
    // An install from before this file. A phone can only reach a model through the
    // URL it saved, so losing it would silently break intent parsing there.
    let mut settings = LlmSettings::default();
    let older = std::fs::read_to_string(store.path().with_file_name("ollama_url.txt"));
    if let Ok(url) = older.and_then(|raw| Ok(format!("{}/v1", raw.trim().trim_end_matches('/')))) {
        if let Ok(url) = normalize_base_url(&url) {
            settings.base_url = url;
        }
    }
    settings
}

/// Reduces what someone pasted to the root that `/chat/completions` hangs off.
///
/// # Errors
///
/// [`AppError::InvalidIntent`] on anything but a plain http(s) URL. Credentials and
/// query strings are refused because this value is stored and logged in the clear.
pub fn normalize_base_url(raw: &str) -> Result<String, AppError> {
    let invalid = || AppError::InvalidIntent { field: "BASE URL", reason: InvalidReason::Malformed };
    let url = reqwest::Url::parse(raw.trim()).map_err(|_| invalid())?;
    if !matches!(url.scheme(), "http" | "https")
        || !url.has_host()
        || !url.username().is_empty()
        || url.password().is_some()
        || url.query().is_some()
        || url.fragment().is_some()
    {
        return Err(invalid());
    }
    let root = url.as_str().trim_end_matches('/');
    let root = root.strip_suffix("/chat/completions").unwrap_or(root);
    Ok(root.trim_end_matches('/').to_string())
}

/// Whether a key may be sent to this URL. Anything but https is readable on the
/// wire, which is only acceptable on this machine or the local network.
#[must_use]
pub fn key_may_travel_to(base_url: &str) -> bool {
    let Ok(url) = reqwest::Url::parse(base_url) else {
        return false;
    };
    if url.scheme() == "https" {
        return true;
    }
    let host = url.host_str().unwrap_or_default();
    match host.trim_matches(['[', ']']).parse::<std::net::IpAddr>() {
        Ok(std::net::IpAddr::V4(ip)) => ip.is_loopback() || ip.is_private() || ip.is_link_local(),
        Ok(std::net::IpAddr::V6(ip)) => ip.is_loopback(),
        Err(_) => host == "localhost" || host.ends_with(".local"),
    }
}

#[derive(Clone, Serialize, Deserialize)]
struct SavedKey {
    key: Secret,
    base_url: String,
}

/// The API keys saved on this device, encrypted under the vault's data key.
pub struct KeyFile {
    vault: Vault<StaticKeyProvider>,
}

impl KeyFile {
    #[must_use]
    pub fn new(path: &Path, key: DataKey) -> Self {
        Self { vault: Vault::new(path, StaticKeyProvider::new(key)) }
    }

    /// The key saved for this preset, if it was saved for this exact base URL.
    #[must_use]
    pub fn get(&self, preset: Preset, base_url: &str) -> Option<Secret> {
        self.all().remove(&preset).filter(|saved| saved.base_url == base_url).map(|saved| saved.key)
    }

    /// Saves or, with `None`, forgets the key for this preset.
    ///
    /// # Errors
    ///
    /// The file could not be written.
    pub fn set(&self, preset: Preset, base_url: &str, key: Option<&str>) -> Result<(), cabal_vault::VaultError> {
        let mut all = self.all();
        match key.map(str::trim).filter(|key| !key.is_empty()) {
            Some(key) => {
                all.insert(preset, SavedKey { key: key.into(), base_url: base_url.to_string() });
            }
            None => {
                all.remove(&preset);
            }
        }
        self.vault.save(&all)
    }

    /// Re-encrypts every saved key under the key `to` uses, when the vault's key changes.
    ///
    /// # Errors
    ///
    /// The file could not be written.
    pub fn move_to(&self, to: &Self) -> Result<(), cabal_vault::VaultError> {
        if !self.vault.exists() {
            return Ok(());
        }
        to.vault.save(&self.all())
    }

    /// Unreadable counts as empty: the user can type a key again, and a bad file
    /// must not stop the app from reaching a model that needs no key.
    fn all(&self) -> BTreeMap<Preset, SavedKey> {
        if !self.vault.exists() {
            return BTreeMap::new();
        }
        self.vault.load().unwrap_or_else(|error| {
            tracing::warn!(target: "cabalmesh::llm", %error, "saved API keys could not be read");
            BTreeMap::new()
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn invalid(raw: &str) -> bool {
        matches!(normalize_base_url(raw), Err(AppError::InvalidIntent { .. }))
    }

    fn data_key(byte: u8) -> DataKey {
        DataKey::from_bytes([byte; 32])
    }

    #[test]
    fn normalize_strips_trailing_slash_and_endpoint() {
        for raw in [
            "https://x/v1",
            "https://x/v1/",
            "  https://x/v1/  ",
            "https://x/v1/chat/completions",
            "https://x/v1/chat/completions/",
        ] {
            assert_eq!(normalize_base_url(raw).unwrap(), "https://x/v1", "from {raw:?}");
        }
    }

    #[test]
    fn normalize_rejects_non_http() {
        for raw in ["", "   ", "x/v1", "ftp://x/v1", "file:///etc/passwd", "https://"] {
            assert!(invalid(raw), "{raw:?} should be refused");
        }
    }

    #[test]
    fn normalize_refuses_secrets_in_the_url() {
        for raw in ["https://user:pw@x/v1", "https://user@x/v1", "https://x/v1?key=abc", "https://x/v1#abc"] {
            assert!(invalid(raw), "{raw:?} would store a secret in the clear");
        }
    }

    #[test]
    fn first_run_defaults_to_ollama_local() {
        let dir = tempfile::tempdir().unwrap();
        let settings = load_from(&DocumentStore::new(dir.path().join("llm.json")));
        assert_eq!(settings.preset, Preset::OllamaLocal);
        assert_eq!(settings.model, "llama3.1:8b");
        assert_eq!(settings.calls_per_hour, 30);
        assert!(settings.base_url.ends_with(":11434/v1"));
    }

    #[test]
    fn settings_round_trip_through_their_file() {
        let dir = tempfile::tempdir().unwrap();
        let store = DocumentStore::new(dir.path().join("llm.json"));
        let settings = LlmSettings {
            preset: Preset::OpenRouter,
            base_url: Preset::OpenRouter.base_url().into(),
            model: "m".into(),
            calls_per_hour: 5,
            tools_supported: Some(false),
        };
        store.save(&settings).unwrap();
        assert_eq!(load_from(&store), settings);
    }

    #[test]
    fn a_remote_ollama_url_from_an_older_install_is_kept() {
        let dir = tempfile::tempdir().unwrap();
        std::fs::write(dir.path().join("ollama_url.txt"), "http://192.168.1.9:11434/\n").unwrap();
        let settings = load_from(&DocumentStore::new(dir.path().join("llm.json")));
        assert_eq!(settings.preset, Preset::OllamaLocal);
        assert_eq!(settings.base_url, "http://192.168.1.9:11434/v1");
    }

    #[test]
    fn saved_settings_win_over_an_older_url() {
        let dir = tempfile::tempdir().unwrap();
        std::fs::write(dir.path().join("ollama_url.txt"), "http://192.168.1.9:11434").unwrap();
        let store = DocumentStore::new(dir.path().join("llm.json"));
        store.save(&LlmSettings::default()).unwrap();
        assert_eq!(load_from(&store), LlmSettings::default());
    }

    #[test]
    fn a_submitted_form_is_cleaned_up() {
        let saved = applied(&LlmSettings::default(), Preset::Custom, " https://x/v1/ ", " m ", 5).unwrap();
        assert_eq!(saved.preset, Preset::Custom);
        assert_eq!(saved.base_url, "https://x/v1");
        assert_eq!(saved.model, "m");
        assert_eq!(saved.calls_per_hour, 5);
    }

    #[test]
    fn the_call_limit_must_be_in_range() {
        let before = LlmSettings::default();
        let url = Preset::OllamaLocal.base_url();
        for bad in [0, 601] {
            assert!(
                matches!(
                    applied(&before, Preset::OllamaLocal, url, "m", bad),
                    Err(AppError::InvalidIntent { field: "CALLS PER HOUR", reason: InvalidReason::OutOfRange })
                ),
                "{bad}"
            );
        }
        for good in [1, 600] {
            assert!(applied(&before, Preset::OllamaLocal, url, "m", good).is_ok(), "{good}");
        }
    }

    #[test]
    fn a_connection_test_only_speaks_for_what_it_ran_against() {
        let before = LlmSettings { tools_supported: Some(true), ..LlmSettings::default() };
        let (url, model) = (before.base_url.clone(), before.model.clone());
        assert_eq!(applied(&before, before.preset, &url, &model, 9).unwrap().tools_supported, Some(true));
        assert_eq!(applied(&before, before.preset, &url, "other", 9).unwrap().tools_supported, None);
        assert_eq!(applied(&before, before.preset, "https://other/v1", &model, 9).unwrap().tools_supported, None);
    }

    #[test]
    fn a_key_only_travels_in_the_clear_on_this_machine_or_network() {
        for url in [
            "https://api.example.com/v1",
            "http://localhost:11434/v1",
            "http://127.0.0.1:11434/v1",
            "http://[::1]:11434/v1",
            "http://192.168.1.9:11434/v1",
            "http://10.0.2.2:11434/v1",
            "http://mac-mini.local:11434/v1",
        ] {
            assert!(key_may_travel_to(url), "{url}");
        }
        for url in ["http://api.example.com/v1", "http://8.8.8.8/v1", "not a url"] {
            assert!(!key_may_travel_to(url), "{url}");
        }
    }

    #[test]
    fn api_key_is_scoped_per_preset() {
        let dir = tempfile::tempdir().unwrap();
        let keys = KeyFile::new(&dir.path().join("llm_keys.enc"), data_key(1));
        let url = Preset::OpenRouter.base_url();
        keys.set(Preset::OpenRouter, url, Some("sk-or")).unwrap();
        assert_eq!(keys.get(Preset::OpenRouter, url).unwrap().expose(), "sk-or");
        assert!(keys.get(Preset::NearAi, url).is_none());
    }

    #[test]
    fn a_key_is_not_returned_for_a_url_it_was_not_saved_for() {
        let dir = tempfile::tempdir().unwrap();
        let keys = KeyFile::new(&dir.path().join("llm_keys.enc"), data_key(1));
        keys.set(Preset::Custom, "https://mine/v1", Some("sk")).unwrap();
        assert!(keys.get(Preset::Custom, "https://someone-else/v1").is_none());
    }

    #[test]
    fn switching_presets_and_back_keeps_each_key() {
        let dir = tempfile::tempdir().unwrap();
        let keys = KeyFile::new(&dir.path().join("llm_keys.enc"), data_key(1));
        keys.set(Preset::OpenRouter, "https://or/v1", Some("a")).unwrap();
        keys.set(Preset::NearAi, "https://near/v1", Some("b")).unwrap();
        assert_eq!(keys.get(Preset::OpenRouter, "https://or/v1").unwrap().expose(), "a");
        assert_eq!(keys.get(Preset::NearAi, "https://near/v1").unwrap().expose(), "b");
    }

    #[test]
    fn a_key_can_be_cleared() {
        let dir = tempfile::tempdir().unwrap();
        let keys = KeyFile::new(&dir.path().join("llm_keys.enc"), data_key(1));
        keys.set(Preset::Custom, "https://x/v1", Some("sk")).unwrap();
        keys.set(Preset::Custom, "https://x/v1", None).unwrap();
        assert!(keys.get(Preset::Custom, "https://x/v1").is_none());
    }

    #[test]
    fn the_key_file_holds_no_plaintext_and_needs_the_right_data_key() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("llm_keys.enc");
        KeyFile::new(&path, data_key(1)).set(Preset::Custom, "https://x/v1", Some("sk-plain")).unwrap();
        let raw = std::fs::read_to_string(&path).unwrap();
        assert!(!raw.contains("sk-plain"));
        assert!(KeyFile::new(&path, data_key(2)).get(Preset::Custom, "https://x/v1").is_none());
    }

    #[test]
    fn keys_move_to_a_new_data_key() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("llm_keys.enc");
        let old = KeyFile::new(&path, data_key(1));
        old.set(Preset::Custom, "https://x/v1", Some("sk")).unwrap();
        let new = KeyFile::new(&path, data_key(2));
        old.move_to(&new).unwrap();
        assert_eq!(new.get(Preset::Custom, "https://x/v1").unwrap().expose(), "sk");
        assert!(old.get(Preset::Custom, "https://x/v1").is_none());
    }

    #[test]
    fn settings_debug_has_no_key() {
        let dir = tempfile::tempdir().unwrap();
        let keys = KeyFile::new(&dir.path().join("llm_keys.enc"), data_key(1));
        keys.set(Preset::Custom, "https://x/v1", Some("sk-secret")).unwrap();
        let printed = format!("{:?} {:?}", LlmSettings::default(), keys.get(Preset::Custom, "https://x/v1"));
        assert!(!printed.contains("sk-secret"), "{printed}");
    }
}
