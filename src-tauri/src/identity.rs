//! The encrypted identity vault: who this device is on NEAR.
//!
//! An identity is a random ed25519 key, optionally bound to a verified named
//! testnet account. Nothing here talks to a chain; `near_bridge` does that, built on
//! demand from [`IdentityStore::primary_keypair`] because the vault may still
//! be locked at startup.

use cabal_near::{AccountId, NearKeypair};
use cabal_store::JsonStore;
use cabal_vault::{Secret, Vault};
use serde::{Deserialize, Serialize};
use std::error::Error;
use std::fs;
use std::path::{Path, PathBuf};

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct IdentityRecord {
    pub alias: String,
    pub emoji: String,
    /// Hex of the 32-byte ed25519 secret.
    ///
    /// `Secret`, not `String`: this struct derives `Debug`, and any `{:?}` of
    /// it — a `dbg!`, a tracing field, an error formatting its context — would
    /// otherwise print the wallet. Encryption at rest protects the file;
    /// this protects the logs.
    ///
    /// Optional only so a vault record without one still opens;
    /// [`IdentityStore::ensure_near_keys`] fills it in.
    #[serde(default)]
    pub near_secret: Option<Secret>,
    /// Missing on pre-onboarding wallets: those identities are preserved as-is.
    #[serde(default)]
    pub wallet_setup_required: bool,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub testnet_account: Option<AccountId>,
    /// Saved before asking the helper, so a timeout/restart retries the same name and key.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub pending_testnet_account: Option<AccountId>,
}

impl IdentityRecord {
    fn keypair(&self) -> Option<NearKeypair> {
        self.keypair_for(crate::network_config::NetworkConfig::current().network)
    }

    fn keypair_for(&self, network: crate::network_config::Network) -> Option<NearKeypair> {
        let bytes = parse_near_secret(self.near_secret.as_ref()?.expose())?;
        let key = NearKeypair::from_secret_bytes(&bytes);
        Some(match (&self.testnet_account, network) {
            (Some(account), crate::network_config::Network::Testnet) => key.with_account_id(account.clone()),
            _ => key,
        })
    }
}

/// 64 hex characters.
fn parse_near_secret(hex_secret: &str) -> Option<[u8; 32]> {
    hex::decode(hex_secret).ok()?.try_into().ok()
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct IdentityView {
    pub alias: String,
    pub emoji: String,
    /// The selected named or implicit NEAR account id.
    pub address: String,
}

pub struct IdentityStore {
    pub identities: Vec<IdentityRecord>,
    /// Encrypted store for identities. Replaces the plaintext
    /// `identities.json`, which held private keys in the clear.
    ///
    /// Boxed rather than a concrete `FileKeyProvider`: `SECURITY` lets the
    /// user switch to a passphrase-derived key at runtime (see
    /// `enable_passphrase`/`disable_passphrase`), and a generic `Vault<P>`
    /// cannot express a provider that changes after construction.
    pub identity_vault: Vault<Box<dyn cabal_vault::KeyProvider>>,
    /// Where `identity_vault` is written. Kept alongside it because switching
    /// key providers means building a whole new `Vault` at the same path.
    vault_path: PathBuf,
    /// The file-backed key `identity_vault` uses in `UnlockMode::File`.
    vault_key_path: PathBuf,
    /// The Argon2id salt `identity_vault` uses in `UnlockMode::Passphrase`.
    vault_salt_path: PathBuf,
    security_state_path: PathBuf,
    /// True while the vault is passphrase-protected and no correct passphrase
    /// has been supplied yet this session.
    locked: bool,
}

impl IdentityStore {
    pub fn new() -> Self {
        Self::open(&crate::app_paths::data_dir())
    }

    pub(crate) fn open(app_dir: &Path) -> Self {
        let vault_path = app_dir.join("vault.enc");
        let vault_key_path = app_dir.join("vault.key");
        let vault_salt_path = app_dir.join("vault.salt");
        let security_state_path = app_dir.join("security.json");

        let security = crate::security_state::SecurityState::load(&JsonStore::new(security_state_path.clone()));
        let (provider, locked): (Box<dyn cabal_vault::KeyProvider>, bool) = match security.mode {
            crate::security_state::UnlockMode::File => {
                (Box::new(crate::vault_key::platform_provider(vault_key_path.clone())), false)
            }
            // Nothing decrypts the vault until `unlock_with_passphrase`
            // supplies the passphrase; `load_identities` below is skipped
            // while locked, so `identities` stays empty rather than the
            // command surface racing an open that never happened.
            crate::security_state::UnlockMode::Passphrase => (Box::new(crate::vault_key::LockedProvider), true),
        };

        let mut store = Self {
            identities: Vec::new(),
            identity_vault: Vault::new(vault_path.clone(), provider),
            vault_path,
            vault_key_path,
            vault_salt_path,
            security_state_path,
            locked,
        };
        if !store.locked {
            let _ = store.load_identities();
        }
        store
    }

    fn save_identities(&self) -> Result<(), Box<dyn Error>> {
        self.identity_vault.save(&self.identities)?;
        Ok(())
    }

    /// Gives every identity without a NEAR key one, and saves before anyone can
    /// fund it: an account that only ever existed in memory is money lost.
    fn ensure_near_keys(&mut self) -> Result<(), Box<dyn Error>> {
        let mut changed = false;
        for identity in &mut self.identities {
            if identity.near_secret.is_none() {
                identity.near_secret = Some(hex::encode(NearKeypair::generate().secret_bytes()).into());
                changed = true;
            }
        }
        if changed {
            self.save_identities()?;
        }
        Ok(())
    }

    /// Whether the vault needs a passphrase before any identity can be read
    /// or signed with. Always `false` in `UnlockMode::File`.
    pub fn is_locked(&self) -> bool {
        self.locked
    }

    /// Which key provider currently protects the vault. Re-read from disk
    /// rather than cached, so it can never drift from what `enable_passphrase`
    /// / `disable_passphrase` last committed.
    pub fn security_mode(&self) -> crate::security_state::UnlockMode {
        crate::security_state::SecurityState::load(&JsonStore::new(self.security_state_path.clone())).mode
    }

    /// Supplies the passphrase for a vault in `UnlockMode::Passphrase`.
    ///
    /// Verified by actually decrypting: Argon2id never fails on a wrong
    /// passphrase, only the AES-GCM decrypt that follows does, so a derived
    /// key that merely didn't error proves nothing. Only a successful decrypt
    /// replaces `identity_vault` and populates `identities` — a wrong
    /// passphrase leaves the store locked and untouched, so a mistyped
    /// attempt cannot brick the real vault.
    ///
    /// # Errors
    ///
    /// The passphrase was wrong, or no salt exists (passphrase mode was
    /// never actually enabled).
    pub fn unlock_with_passphrase(&mut self, passphrase: &str) -> Result<Vec<IdentityView>, Box<dyn Error>> {
        let provider = cabal_vault::PassphraseKeyProvider::open(&self.vault_salt_path, passphrase)?;
        let candidate = Vault::new(self.vault_path.clone(), Box::new(provider) as Box<dyn cabal_vault::KeyProvider>);
        let records: Vec<IdentityRecord> = candidate.load()?;

        self.identity_vault = candidate;
        self.identities = records;
        self.locked = false;
        self.ensure_near_keys()?;
        self.get_identity_views()
    }

    /// The key currently protecting the vault, for splitting into guardian
    /// shares. Whatever the current unlock mode derives it from — a
    /// file-backed key or a passphrase — the guardian scheme does not care;
    /// it only ever needs this one 32-byte value.
    ///
    /// # Errors
    ///
    /// The vault is locked, or the key is unavailable.
    pub fn current_vault_key(&self) -> Result<cabal_vault::DataKey, Box<dyn Error>> {
        Ok(self.identity_vault.key()?)
    }

    fn llm_key_file(&self) -> Result<crate::llm_config::KeyFile, Box<dyn Error>> {
        Ok(crate::llm_config::KeyFile::new(&self.llm_keys_path(), self.identity_vault.key()?))
    }

    fn llm_keys_path(&self) -> PathBuf {
        self.vault_path.with_file_name("llm_keys.enc")
    }

    /// The API key for these settings. `None` while the vault is locked.
    #[must_use]
    pub fn llm_api_key(&self, settings: &crate::llm_config::LlmSettings) -> Option<cabal_vault::Secret> {
        self.llm_key_file().ok()?.get(settings.preset, &settings.base_url)
    }

    /// Saves or, with `None`, forgets the API key for these settings.
    ///
    /// # Errors
    ///
    /// The vault is locked, or the file could not be written.
    pub fn set_llm_api_key(
        &self,
        settings: &crate::llm_config::LlmSettings,
        key: Option<&str>,
    ) -> Result<(), Box<dyn Error>> {
        self.llm_key_file()?.set(settings.preset, &settings.base_url, key)?;
        Ok(())
    }

    /// Keeps saved API keys readable when `new_vault` replaces the current vault.
    fn move_llm_keys(&self, new_vault: &Vault<Box<dyn cabal_vault::KeyProvider>>) -> Result<(), Box<dyn Error>> {
        let to = crate::llm_config::KeyFile::new(&self.llm_keys_path(), new_vault.key()?);
        self.llm_key_file()?.move_to(&to)?;
        Ok(())
    }

    /// Supplies a vault key reconstructed from guardian shares.
    ///
    /// Exactly [`Self::unlock_with_passphrase`]'s shape, deliberately: a
    /// Shamir reconstruction has no integrity check of its own (see
    /// `cabal_guardian::reconstruct`'s docs) — the only thing that actually
    /// proves `candidate` is right is the same thing that proves any vault
    /// key right, `Vault::load` succeeding, because AES-GCM authenticates
    /// and Shamir does not. A wrong candidate — too few genuine shares,
    /// stale ones from a removed guardian, anything — fails to decrypt here
    /// and leaves the store locked and untouched.
    ///
    /// # Errors
    ///
    /// The candidate key was wrong.
    pub fn unlock_with_guardian_key(&mut self, candidate: cabal_vault::DataKey) -> Result<Vec<IdentityView>, Box<dyn Error>> {
        let provider = cabal_vault::StaticKeyProvider::new(candidate);
        let candidate_vault = Vault::new(self.vault_path.clone(), Box::new(provider) as Box<dyn cabal_vault::KeyProvider>);
        let records: Vec<IdentityRecord> = candidate_vault.load()?;

        self.identity_vault = candidate_vault;
        self.identities = records;
        self.locked = false;
        self.ensure_near_keys()?;
        self.get_identity_views()
    }

    /// Turns passphrase protection on: re-encrypts the vault under a key
    /// derived from `passphrase`, deletes the file-backed key it replaces,
    /// and persists the switch so the next launch asks for the passphrase.
    ///
    /// # Errors
    ///
    /// The vault is locked, or re-encryption failed to round-trip.
    pub fn enable_passphrase(&mut self, passphrase: &str) -> Result<(), Box<dyn Error>> {
        if self.locked {
            return Err("cannot enable passphrase protection on a locked vault".into());
        }

        let provider = cabal_vault::PassphraseKeyProvider::create(&self.vault_salt_path, passphrase)?;
        let new_vault = Vault::new(self.vault_path.clone(), Box::new(provider) as Box<dyn cabal_vault::KeyProvider>);
        new_vault.save(&self.identities)?;

        // Round-trip before committing: an unreadable re-encryption must
        // leave the working vault exactly as it was, not swap in something
        // this session can decrypt but a future one cannot.
        let round_tripped: Vec<IdentityRecord> = new_vault.load()?;
        if round_tripped != self.identities {
            return Err("re-encrypted vault did not read back identically".into());
        }

        // Not fatal: the wallet is already re-encrypted, and an API key can be typed again.
        if let Err(error) = self.move_llm_keys(&new_vault) {
            tracing::warn!(target: "cabalmesh::llm", %error, "saved API keys were not re-encrypted");
        }
        self.identity_vault = new_vault;
        // The old key protects nothing now; leaving it on disk would make
        // the KEYS screen's claim of passphrase protection false.
        let _ = fs::remove_file(&self.vault_key_path);

        crate::security_state::SecurityState { mode: crate::security_state::UnlockMode::Passphrase }
            .save(&JsonStore::new(self.security_state_path.clone()))?;
        Ok(())
    }

    /// Turns passphrase protection off: re-encrypts under a fresh file-backed
    /// key and removes the salt.
    ///
    /// # Errors
    ///
    /// The vault is locked, or re-encryption failed to round-trip.
    pub fn disable_passphrase(&mut self) -> Result<(), Box<dyn Error>> {
        if self.locked {
            return Err("cannot disable passphrase protection on a locked vault".into());
        }

        let provider = crate::vault_key::platform_provider(self.vault_key_path.clone());
        let new_vault = Vault::new(self.vault_path.clone(), Box::new(provider) as Box<dyn cabal_vault::KeyProvider>);
        new_vault.save(&self.identities)?;

        let round_tripped: Vec<IdentityRecord> = new_vault.load()?;
        if round_tripped != self.identities {
            return Err("re-encrypted vault did not read back identically".into());
        }

        // Not fatal: the wallet is already re-encrypted, and an API key can be typed again.
        if let Err(error) = self.move_llm_keys(&new_vault) {
            tracing::warn!(target: "cabalmesh::llm", %error, "saved API keys were not re-encrypted");
        }
        self.identity_vault = new_vault;
        let _ = fs::remove_file(&self.vault_salt_path);

        crate::security_state::SecurityState { mode: crate::security_state::UnlockMode::File }
            .save(&JsonStore::new(self.security_state_path.clone()))?;
        Ok(())
    }

    pub fn load_identities(&mut self) -> Result<Vec<IdentityView>, Box<dyn Error>> {
        // Adopt any pre-encryption wallet first. This verifies a full decrypt
        // round trip before removing the plaintext, so a failure here leaves
        // the old file exactly where it was.
        let plaintext = JsonStore::new(self.plaintext_identity_path());
        match self
            .identity_vault
            .migrate_plaintext::<Vec<IdentityRecord>>(&plaintext)
        {
            Ok(true) => tracing::info!("🔐 Identities migrated into the encrypted vault"),
            Ok(false) => {}
            Err(error) => {
                // Do not fall through to generating a fresh wallet: that is
                // how a recoverable problem becomes permanent loss.
                tracing::error!(%error, "identity migration failed; plaintext left intact");
                return Err(Box::new(error));
            }
        }

        if self.identity_vault.exists() {
            tracing::info!("🔑 Loading identities from the encrypted vault");
            match self.identity_vault.load::<Vec<IdentityRecord>>() {
                Ok(records) => {
                    self.identities = records;
                    if self.identities.is_empty() {
                        return self.generate_new_identity("Primary Fox".to_string(), "🦊".to_string());
                    }
                    self.ensure_near_keys()?;
                    self.get_identity_views()
                }
                Err(error) => {
                    // A vault that exists but will not open means the key is
                    // wrong or the file was tampered with. Generating a new
                    // wallet over it would discard the user's funds, so this
                    // fails loudly instead.
                    tracing::error!(%error, "vault exists but cannot be decrypted");
                    Err(Box::new(error))
                }
            }
        } else {
            self.generate_new_identity("Genesis Fox".to_string(), "🦊".to_string())
        }
    }

    /// Where the pre-encryption wallet lived. Only used to migrate away from.
    fn plaintext_identity_path(&self) -> PathBuf {
        self.vault_path.with_file_name("identities.json")
    }

    pub fn generate_new_identity(&mut self, alias: String, emoji: String) -> Result<Vec<IdentityView>, Box<dyn Error>> {
        tracing::info!("🆕 Generating NEW Identity '{}' [{}]...", alias, emoji);
        let near_secret = Some(hex::encode(NearKeypair::generate().secret_bytes()).into());
        self.identities.push(IdentityRecord {
            alias, emoji, near_secret, wallet_setup_required: true,
            testnet_account: None, pending_testnet_account: None,
        });
        self.save_identities()?;
        self.get_identity_views()
    }

    pub fn get_identity_views(&self) -> Result<Vec<IdentityView>, Box<dyn Error>> {
        let mut views = Vec::new();
        for identity in &self.identities {
            let keypair = identity.keypair().ok_or("identity has no usable NEAR key")?;
            views.push(IdentityView {
                alias: identity.alias.clone(),
                emoji: identity.emoji.clone(),
                address: keypair.account_id().to_string(),
            });
        }
        Ok(views)
    }

    /// Discards the current wallet entirely and replaces it with a fresh
    /// randomly-generated one. There's no multi-wallet slot concept anywhere
    /// (every signing path uses `identities[0]`), so "logout" means wiping the
    /// vec and generating a brand-new identity to take its place, not just
    /// clearing state and leaving no signer at all.
    pub fn logout_identity(&mut self) -> Result<Vec<IdentityView>, Box<dyn Error>> {
        self.ensure_no_pending_registration()?;
        self.identities.clear();
        self.generate_new_identity("Genesis Fox".to_string(), "🦊".to_string())
    }

    /// Replaces the current wallet identity with one derived from a
    /// user-supplied key. The key is validated before anything is persisted.
    pub fn import_identity(
        &mut self,
        near_secret_hex: String,
        alias: String,
        emoji: String,
    ) -> Result<Vec<IdentityView>, Box<dyn Error>> {
        self.import_identity_with_testnet_account(near_secret_hex, alias, emoji, None)
    }

    /// Caller verifies a named account's full-access key before importing its binding.
    pub(crate) fn import_identity_with_testnet_account(
        &mut self, near_secret_hex: String, alias: String, emoji: String, account: Option<AccountId>,
    ) -> Result<Vec<IdentityView>, Box<dyn Error>> {
        self.ensure_no_pending_registration()?;
        let normalized = near_secret_hex.trim().to_ascii_lowercase();
        parse_near_secret(&normalized).ok_or("a NEAR key is 64 hex characters with no 0x prefix")?;
        let previous = self.identities.clone();
        self.identities = vec![IdentityRecord {
            alias,
            emoji,
            near_secret: Some(normalized.into()),
            wallet_setup_required: false,
            testnet_account: account,
            pending_testnet_account: None,
        }];
        if let Err(error) = self.save_identities() {
            self.identities = previous;
            return Err(error);
        }
        self.get_identity_views()
    }

    /// The current wallet's keys as labelled lines, so the user can save them
    /// before logging out or importing a different one. Nothing else persists
    /// them anywhere recoverable.
    #[must_use]
    pub fn exported_keys(&self) -> Option<String> {
        let identity = self.identities.first()?;
        let near = identity.near_secret.as_ref()?;
        let account = self.primary_account()?;
        let testnet = identity.testnet_account.as_ref().map(|id| format!("\nNEAR TESTNET ACCOUNT: {id}")).unwrap_or_default();
        Some(format!("NEAR ACCOUNT: {account}{testnet}\nNEAR KEY: {}", near.expose()))
    }

    /// The signing key, or `None` while the vault is locked.
    #[must_use]
    pub fn primary_keypair(&self) -> Option<NearKeypair> {
        self.identities.first()?.keypair()
    }

    /// The NEAR account, or `None` while the vault is locked.
    #[must_use]
    pub fn primary_account(&self) -> Option<AccountId> {
        Some(self.primary_keypair()?.account_id())
    }

    fn ensure_no_pending_registration(&self) -> Result<(), Box<dyn Error>> {
        if self.identities.first().is_some_and(|record| record.pending_testnet_account.is_some()) {
            return Err("finish pending wallet setup before replacing this key".into());
        }
        Ok(())
    }

    /// Persist registration state without replacing the signing key or other identities.
    /// On disk failure the in-memory record is restored too.
    pub fn set_testnet_registration(&mut self, account: AccountId, complete: bool) -> Result<(), Box<dyn Error>> {
        let previous = self.identities.first().ok_or("identity unavailable")?.clone();
        if previous.pending_testnet_account.as_ref().is_some_and(|pending| pending != &account)
            || previous.testnet_account.as_ref().is_some_and(|current| current != &account) {
            return Err("another named account is already selected".into());
        }
        let record = self.identities.first_mut().ok_or("identity unavailable")?;
        if complete {
            record.testnet_account = Some(account);
            record.pending_testnet_account = None;
            record.wallet_setup_required = false;
        } else {
            record.pending_testnet_account = Some(account);
        }
        if let Err(error) = self.save_identities() {
            self.identities[0] = previous;
            return Err(error);
        }
        Ok(())
    }

    pub fn clear_pending_testnet_registration(&mut self) -> Result<(), Box<dyn Error>> {
        let previous = self.identities.first().ok_or("identity unavailable")?.clone();
        self.identities[0].pending_testnet_account = None;
        if let Err(error) = self.save_identities() {
            self.identities[0] = previous;
            return Err(error);
        }
        Ok(())
    }
}

impl Default for IdentityStore {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
mod security_tests {
    use super::*;

    #[test]
    fn named_setup_preserves_the_key_and_survives_restart() {
        let dir = tempfile::tempdir().unwrap();
        let mut identity = IdentityStore::open(dir.path());
        assert!(identity.identities[0].wallet_setup_required);
        let original = identity.primary_keypair().unwrap().public_key_bytes();
        let name = AccountId::try_from("alice.testnet").unwrap();
        identity.set_testnet_registration(name.clone(), false).unwrap();
        assert!(identity.set_testnet_registration(AccountId::try_from("other.testnet").unwrap(), false).is_err());
        let mut restarted = IdentityStore::open(dir.path());
        assert_eq!(restarted.identities[0].pending_testnet_account, Some(name.clone()));
        assert!(restarted.import_identity("01".repeat(32), "Other".into(), "".into()).is_err());
        restarted.set_testnet_registration(name.clone(), true).unwrap();
        let final_store = IdentityStore::open(dir.path());
        let record = &final_store.identities[0];
        assert!(!record.wallet_setup_required);
        assert!(record.pending_testnet_account.is_none());
        assert_eq!(record.keypair_for(crate::network_config::Network::Testnet).unwrap().account_id(), name);
        assert_eq!(record.keypair_for(crate::network_config::Network::Mainnet).unwrap().public_key_bytes(), original);
        assert_ne!(record.keypair_for(crate::network_config::Network::Mainnet).unwrap().account_id(), name);
        assert_eq!(record.keypair_for(crate::network_config::Network::Testnet).unwrap().public_key_bytes(), original);
    }

    #[test]
    fn pre_onboarding_records_do_not_force_a_wallet_replacement() {
        let record: IdentityRecord = serde_json::from_value(serde_json::json!({
            "alias": "Original", "emoji": "", "near_secret": "01".repeat(32),
        })).unwrap();
        assert!(!record.wallet_setup_required);
        assert!(record.testnet_account.is_none());
        assert!(record.pending_testnet_account.is_none());
    }

    #[test]
    fn imported_named_account_is_saved_with_its_key() {
        let dir = tempfile::tempdir().unwrap();
        let mut identity = IdentityStore::open(dir.path());
        let account = AccountId::try_from("restored.testnet").unwrap();
        identity.import_identity_with_testnet_account("07".repeat(32), "Restored".into(), "".into(), Some(account.clone())).unwrap();
        let restored = IdentityStore::open(dir.path());
        assert_eq!(restored.identities[0].testnet_account, Some(account));
        assert!(!restored.identities[0].wallet_setup_required);
        assert_eq!(restored.primary_keypair().unwrap().public_key_bytes(), NearKeypair::from_secret_bytes(&[7; 32]).public_key_bytes());
    }

    #[test]
    fn failed_registration_write_does_not_change_the_active_account() {
        let dir = tempfile::tempdir().unwrap();
        let mut identity = IdentityStore::open(dir.path());
        let before = identity.primary_account();
        let impossible = dir.path().join("directory");
        fs::create_dir(&impossible).unwrap();
        identity.identity_vault = Vault::new(impossible, Box::new(crate::vault_key::platform_provider(dir.path().join("vault.key"))));
        assert!(identity.set_testnet_registration(AccountId::try_from("alice.testnet").unwrap(), true).is_err());
        assert_eq!(identity.primary_account(), before);
        assert!(identity.identities[0].wallet_setup_required);
        assert!(identity.identities[0].testnet_account.is_none());
    }

    /// Builds a bridge exactly as `IdentityStore::new` does, but scoped to
    /// `tmp_dir` instead of the real app directory — so calling this twice
    /// against the same directory simulates a process restart: whatever
    /// `enable_passphrase` / `disable_passphrase` wrote to disk in the first
    /// call is what the second call reads.
    fn bridge_from_disk(tmp_dir: &PathBuf) -> IdentityStore {
        let vault_path = tmp_dir.join("vault.enc");
        let vault_key_path = tmp_dir.join("vault.key");
        let vault_salt_path = tmp_dir.join("vault.salt");
        let security_state_path = tmp_dir.join("security.json");

        let security = crate::security_state::SecurityState::load(&JsonStore::new(security_state_path.clone()));
        let (provider, locked): (Box<dyn cabal_vault::KeyProvider>, bool) = match security.mode {
            crate::security_state::UnlockMode::File => {
                (Box::new(crate::vault_key::platform_provider(vault_key_path.clone())), false)
            }
            crate::security_state::UnlockMode::Passphrase => (Box::new(crate::vault_key::LockedProvider), true),
        };

        let mut store = IdentityStore {
            identities: Vec::new(),
            identity_vault: Vault::new(vault_path.clone(), provider),
            vault_path,
            vault_key_path,
            vault_salt_path,
            security_state_path,
            locked,
        };
        if !store.locked {
            let _ = store.load_identities();
        }
        store
    }

    #[test]
    fn llm_keys_follow_the_vault_through_a_passphrase_and_back() {
        let dir = tempfile::tempdir().unwrap();
        let tmp_dir = dir.path().to_path_buf();
        let settings = crate::llm_config::LlmSettings::default();

        let mut bridge = bridge_from_disk(&tmp_dir);
        bridge.generate_new_identity("Test".to_string(), "🔐".to_string()).unwrap();
        bridge.set_llm_api_key(&settings, Some("sk-test")).unwrap();
        bridge.enable_passphrase("correct horse battery staple").unwrap();
        assert_eq!(bridge.llm_api_key(&settings).unwrap().expose(), "sk-test");

        let mut restarted = bridge_from_disk(&tmp_dir);
        assert!(restarted.llm_api_key(&settings).is_none(), "locked means no key");
        assert!(restarted.set_llm_api_key(&settings, Some("x")).is_err());

        restarted.unlock_with_passphrase("correct horse battery staple").unwrap();
        assert_eq!(restarted.llm_api_key(&settings).unwrap().expose(), "sk-test");

        restarted.disable_passphrase().unwrap();
        assert_eq!(bridge_from_disk(&tmp_dir).llm_api_key(&settings).unwrap().expose(), "sk-test");
    }

    #[test]
    fn enabling_passphrase_survives_a_restart() {
        let tmp_dir = std::env::temp_dir().join(format!("cabalmesh_security_enable_test_{}", std::process::id()));
        std::fs::create_dir_all(&tmp_dir).unwrap();

        let mut bridge = bridge_from_disk(&tmp_dir);
        bridge.generate_new_identity("Test".to_string(), "🔐".to_string()).unwrap();
        let original = bridge.identities.clone();

        bridge.enable_passphrase("correct horse battery staple").unwrap();
        assert_eq!(bridge.security_mode(), crate::security_state::UnlockMode::Passphrase);
        assert!(
            !bridge.vault_key_path.exists(),
            "the file-backed key must be gone once a passphrase protects the vault"
        );

        // A fresh process reading the same directory must come up locked,
        // with no identities in memory until the passphrase is supplied.
        let mut restarted = bridge_from_disk(&tmp_dir);
        assert!(restarted.is_locked());
        assert!(restarted.identities.is_empty());

        assert!(
            restarted.unlock_with_passphrase("wrong passphrase").is_err(),
            "a wrong passphrase must be rejected"
        );
        assert!(restarted.is_locked(), "a rejected passphrase must not unlock the vault");

        let views = restarted.unlock_with_passphrase("correct horse battery staple").unwrap();
        assert!(!restarted.is_locked());
        assert_eq!(restarted.identities, original);
        // `bridge_from_disk` auto-generates a "Genesis Fox" identity the
        // first time it runs (matching the app's zero-friction first-run
        // behaviour), so `original` already holds that plus the "Test"
        // identity pushed below — this just confirms nothing was lost or
        // duplicated across the enable/lock/unlock round trip.
        assert_eq!(views.len(), original.len());

        std::fs::remove_dir_all(&tmp_dir).ok();
    }

    #[test]
    fn disabling_passphrase_restores_file_backed_unlock() {
        let tmp_dir = std::env::temp_dir().join(format!("cabalmesh_security_disable_test_{}", std::process::id()));
        std::fs::create_dir_all(&tmp_dir).unwrap();

        let mut bridge = bridge_from_disk(&tmp_dir);
        bridge.generate_new_identity("Test".to_string(), "🔓".to_string()).unwrap();
        let original = bridge.identities.clone();

        bridge.enable_passphrase("hunter2222").unwrap();
        bridge.disable_passphrase().unwrap();
        assert_eq!(bridge.security_mode(), crate::security_state::UnlockMode::File);
        assert!(!bridge.vault_salt_path.exists(), "the salt must be gone once passphrase protection is off");

        // A fresh process must come up unlocked and already holding the
        // identities — file mode needs no user input to open.
        let restarted = bridge_from_disk(&tmp_dir);
        assert!(!restarted.is_locked());
        assert_eq!(restarted.identities, original);

        std::fs::remove_dir_all(&tmp_dir).ok();
    }

    #[test]
    fn enabling_passphrase_on_a_locked_vault_is_refused() {
        let tmp_dir = std::env::temp_dir().join(format!("cabalmesh_security_locked_test_{}", std::process::id()));
        std::fs::create_dir_all(&tmp_dir).unwrap();

        let mut bridge = bridge_from_disk(&tmp_dir);
        bridge.generate_new_identity("Test".to_string(), "🔐".to_string()).unwrap();
        bridge.enable_passphrase("correct horse battery staple").unwrap();

        // Nothing has unlocked this instance, so there is no in-memory
        // plaintext to re-encrypt — enabling again must refuse rather than
        // silently re-encrypting an empty identity list over the real vault.
        let mut locked_view = bridge_from_disk(&tmp_dir);
        assert!(locked_view.is_locked());
        assert!(locked_view.enable_passphrase("another passphrase").is_err());

        std::fs::remove_dir_all(&tmp_dir).ok();
    }

    #[test]
    fn a_key_reconstructed_from_guardian_shares_unlocks_the_real_vault() {
        // The other half of the guardian scheme's contract: `cabal_guardian`
        // proves reconstruction round-trips a `DataKey` in its own tests,
        // but the property that actually matters is whether that key opens
        // *this* vault — which is what `Vault::load` authenticating proves,
        // not anything Shamir's math can promise on its own.
        let tmp_dir = std::env::temp_dir().join(format!("cabalmesh_guardian_unlock_test_{}", std::process::id()));
        std::fs::create_dir_all(&tmp_dir).unwrap();

        let mut bridge = bridge_from_disk(&tmp_dir);
        bridge.generate_new_identity("Test".to_string(), "🛡️".to_string()).unwrap();
        let original = bridge.identities.clone();

        let vault_key = bridge.current_vault_key().unwrap();
        let shares = cabal_guardian::split(&vault_key, 3, 5).unwrap();
        let reconstructed = cabal_guardian::reconstruct(3, &shares[1..4]).unwrap();

        let views = bridge.unlock_with_guardian_key(reconstructed).unwrap();
        assert!(!bridge.is_locked());
        assert_eq!(bridge.identities, original);
        assert_eq!(views.len(), original.len());

        std::fs::remove_dir_all(&tmp_dir).ok();
    }

    #[test]
    fn a_wrong_reconstructed_key_is_refused() {
        let tmp_dir = std::env::temp_dir().join(format!("cabalmesh_guardian_unlock_wrong_test_{}", std::process::id()));
        std::fs::create_dir_all(&tmp_dir).unwrap();

        let mut bridge = bridge_from_disk(&tmp_dir);
        bridge.generate_new_identity("Test".to_string(), "🛡️".to_string()).unwrap();
        let original = bridge.identities.clone();

        // Shares from an unrelated split — the exact scenario the guardian
        // crate's own docs warn reconstruction cannot detect on its own.
        let wrong_key = cabal_vault::DataKey::from_bytes([0xAB; 32]);
        let wrong_shares = cabal_guardian::split(&wrong_key, 3, 5).unwrap();
        let reconstructed = cabal_guardian::reconstruct(3, &wrong_shares[0..3]).unwrap();

        assert!(bridge.unlock_with_guardian_key(reconstructed).is_err());
        assert!(!bridge.is_locked(), "a rejected candidate must not disturb an already-unlocked vault");
        assert_eq!(bridge.identities, original);

        std::fs::remove_dir_all(&tmp_dir).ok();
    }

    /// A record from before NEAR holds another kind of key. Nothing here reads
    /// it, and the first save must not carry it forward.
    #[test]
    fn a_foreign_key_in_the_vault_is_dropped_on_open() {
        let tmp_dir = std::env::temp_dir().join(format!("cabalmesh_foreign_key_test_{}", std::process::id()));
        std::fs::create_dir_all(&tmp_dir).unwrap();
        let open_vault = || Vault::new(tmp_dir.join("vault.enc"), crate::vault_key::platform_provider(tmp_dir.join("vault.key")));
        open_vault()
            .save(&serde_json::json!([{ "alias": "Fox", "emoji": "🦊", "private_key_hex": "0x1111" }]))
            .unwrap();

        let store = bridge_from_disk(&tmp_dir);
        assert!(store.primary_account().is_some());
        assert!(!store.exported_keys().unwrap().contains("0x1111"));
        let on_disk: serde_json::Value = open_vault().load().unwrap();
        assert!(!on_disk.to_string().contains("0x1111"), "{on_disk}");

        std::fs::remove_dir_all(&tmp_dir).ok();
    }

    #[test]
    fn an_identity_without_a_near_key_gains_one_once() {
        let tmp_dir = std::env::temp_dir().join(format!("cabalmesh_keyless_identity_test_{}", std::process::id()));
        std::fs::create_dir_all(&tmp_dir).unwrap();
        Vault::new(tmp_dir.join("vault.enc"), crate::vault_key::platform_provider(tmp_dir.join("vault.key")))
            .save(&serde_json::json!([{ "alias": "Genesis Fox", "emoji": "🦊" }]))
            .unwrap();

        let first = bridge_from_disk(&tmp_dir);
        let account = first.primary_account().unwrap();
        assert_eq!(account.as_str().len(), 64, "an implicit account id: {account}");

        // Another launch must not mint another account: that would strand
        // whatever the first one holds.
        let second = bridge_from_disk(&tmp_dir);
        assert_eq!(second.primary_account().unwrap(), account);

        let exported = second.exported_keys().unwrap();
        assert!(exported.contains("NEAR KEY"));

        std::fs::remove_dir_all(&tmp_dir).ok();
    }

    #[test]
    fn a_keyless_vault_behind_a_passphrase_gains_its_near_key_on_unlock() {
        let tmp_dir = std::env::temp_dir().join(format!("cabalmesh_keyless_passphrase_test_{}", std::process::id()));
        std::fs::create_dir_all(&tmp_dir).unwrap();

        let provider = cabal_vault::PassphraseKeyProvider::create(&tmp_dir.join("vault.salt"), "hunter2222").unwrap();
        Vault::new(tmp_dir.join("vault.enc"), provider)
            .save(&serde_json::json!([{ "alias": "Fox", "emoji": "🦊" }]))
            .unwrap();
        crate::security_state::SecurityState { mode: crate::security_state::UnlockMode::Passphrase }
            .save(&JsonStore::new(tmp_dir.join("security.json")))
            .unwrap();

        let mut locked = bridge_from_disk(&tmp_dir);
        assert!(locked.primary_account().is_none());
        locked.unlock_with_passphrase("hunter2222").unwrap();
        let account = locked.primary_account().unwrap();

        let mut restarted = bridge_from_disk(&tmp_dir);
        restarted.unlock_with_passphrase("hunter2222").unwrap();
        assert_eq!(restarted.primary_account().unwrap(), account, "the key was saved, not re-rolled");

        std::fs::remove_dir_all(&tmp_dir).ok();
    }

    #[test]
    fn a_guardian_recovery_keeps_the_near_account() {
        let tmp_dir = std::env::temp_dir().join(format!("cabalmesh_guardian_near_test_{}", std::process::id()));
        std::fs::create_dir_all(&tmp_dir).unwrap();

        let mut store = bridge_from_disk(&tmp_dir);
        let account = store.primary_account().unwrap();

        let shares = cabal_guardian::split(&store.current_vault_key().unwrap(), 3, 5).unwrap();
        let reconstructed = cabal_guardian::reconstruct(3, &shares[..3]).unwrap();
        store.identities.clear();
        store.unlock_with_guardian_key(reconstructed).unwrap();
        assert_eq!(store.primary_account().unwrap(), account);

        std::fs::remove_dir_all(&tmp_dir).ok();
    }

    #[test]
    fn identity_debug_never_prints_near_secret() {
        let tmp_dir = std::env::temp_dir().join(format!("cabalmesh_identity_debug_test_{}", std::process::id()));
        std::fs::create_dir_all(&tmp_dir).unwrap();

        let store = bridge_from_disk(&tmp_dir);
        let record = store.identities[0].clone();
        let secret = record.near_secret.as_ref().unwrap().expose().to_owned();
        assert!(!format!("{record:?}").contains(&secret));

        std::fs::remove_dir_all(&tmp_dir).ok();
    }

    #[test]
    fn importing_a_near_key_replaces_the_account_and_an_evm_key_is_refused() {
        let tmp_dir = std::env::temp_dir().join(format!("cabalmesh_import_test_{}", std::process::id()));
        std::fs::create_dir_all(&tmp_dir).unwrap();

        let mut store = bridge_from_disk(&tmp_dir);
        let before = store.primary_account().unwrap();

        let prefixed = format!("0x{}", "11".repeat(32));
        assert!(store.import_identity(prefixed, "A".into(), "🦊".into()).is_err());
        assert!(store.import_identity("zz".into(), "A".into(), "🦊".into()).is_err());
        assert_eq!(store.primary_account().unwrap(), before, "a refused key changes nothing");

        store.import_identity("01".repeat(32), "A".into(), "🦊".into()).unwrap();
        assert_eq!(
            store.primary_account().unwrap(),
            NearKeypair::from_secret_bytes(&[1; 32]).account_id()
        );

        std::fs::remove_dir_all(&tmp_dir).ok();
    }

    #[test]
    fn logout_replaces_the_account() {
        let tmp_dir = std::env::temp_dir().join(format!("cabalmesh_logout_test_{}", std::process::id()));
        std::fs::create_dir_all(&tmp_dir).unwrap();

        let mut store = bridge_from_disk(&tmp_dir);
        let before = store.primary_account().unwrap();
        store.logout_identity().unwrap();
        assert_ne!(store.primary_account().unwrap(), before);

        std::fs::remove_dir_all(&tmp_dir).ok();
    }
}
