//! A [`KeyProvider`] derived from a user passphrase, via Argon2id.
//!
//! This is the layer-1 factor `docs/identity-design.md` settles on for every
//! desktop platform, and for mobile until the native key-store plugin lands:
//! there is no uniform OS key store to hold half the key, so the vault key is
//! derived instead of stored. The salt is not secret — it lives on disk next
//! to the vault, exactly like `vault.key` does for the file-backed provider —
//! but it must be stable across unlocks, or every previously written vault
//! becomes unreadable.

use crate::{DataKey, KeyProvider, VaultError};
use aes_gcm::aead::rand_core::RngCore;
use aes_gcm::aead::OsRng;
use argon2::{Algorithm, Argon2, Params, Version};
use serde::{Deserialize, Serialize};
use std::path::Path;
use zeroize::Zeroize;

const SALT_LEN: usize = 16;

/// Argon2id memory cost in KiB: 64 MiB. Decision 1 in
/// `docs/identity-design.md` — the parameter that hurts a GPU attacker with
/// a copied key file. Asserted by a test, so weakening it is deliberate.
pub const ARGON2_M_KIB: u32 = 64 * 1024;
/// Argon2id passes.
pub const ARGON2_T: u32 = 3;
/// Argon2id lanes.
pub const ARGON2_P: u32 = 1;

/// What a salt file written before parameters were recorded used: the
/// `argon2` crate's defaults at the time (19 MiB, t=2). Kept so those vaults
/// still open; [`PassphraseKeyProvider::needs_upgrade`] flags them for
/// re-encryption under the current parameters.
const LEGACY: (u32, u32, u32) = (19 * 1024, 2, 1);

/// The salt file: everything needed to re-derive, and nothing secret.
/// Parameters live here rather than in the binary so they can be raised
/// later without orphaning vaults written under the old ones.
#[derive(Debug, Clone, Serialize, Deserialize)]
struct Envelope {
    v: u8,
    salt: String,
    m: u32,
    t: u32,
    p: u32,
    /// `Some(name)` when a second, device-held secret is mixed in after
    /// Argon2id (the PIN path: see `presence` in the app). Recorded so a
    /// vault carried to a device without it is refused with a reason.
    #[serde(default)]
    pepper: Option<String>,
}

/// Derives the vault key from a passphrase and a salt persisted beside it.
pub struct PassphraseKeyProvider {
    key: DataKey,
    legacy: bool,
}

impl PassphraseKeyProvider {
    /// Re-derives the key for an already-enrolled vault. The salt must
    /// already exist — generating a fresh one here would silently derive a
    /// key that cannot decrypt anything, turning a wrong passphrase into
    /// permanent data loss instead of a rejected unlock.
    ///
    /// # Errors
    ///
    /// [`VaultError::KeyUnavailable`] if the salt file is missing or
    /// malformed, or derivation fails.
    pub fn open(salt_path: impl AsRef<Path>, passphrase: &str) -> Result<Self, VaultError> {
        Self::open_with_pepper(salt_path, passphrase, None)
    }

    /// [`Self::open`], mixing in `pepper` when the envelope says one was
    /// used. A pepper the envelope expects but the caller cannot supply is
    /// refused rather than skipped: skipping would derive a different key.
    ///
    /// # Errors
    ///
    /// As [`Self::open`], or [`VaultError::KeyUnavailable`] on a pepper
    /// mismatch.
    pub fn open_with_pepper(salt_path: impl AsRef<Path>, passphrase: &str, pepper: Option<&[u8]>) -> Result<Self, VaultError> {
        let text = std::fs::read_to_string(salt_path.as_ref()).map_err(|_| VaultError::KeyUnavailable)?;
        let (salt, (m, t, p), wants_pepper, legacy) = match serde_json::from_str::<Envelope>(&text) {
            Ok(envelope) => (decode_salt(&envelope.salt)?, (envelope.m, envelope.t, envelope.p), envelope.pepper.is_some(), false),
            Err(_) => (decode_salt(text.trim())?, LEGACY, false, true),
        };
        if wants_pepper != pepper.is_some() {
            return Err(VaultError::KeyUnavailable);
        }
        Ok(Self { key: derive(&salt, passphrase, m, t, p, pepper)?, legacy })
    }

    /// Enrolls a new vault: writes a fresh random salt and the current
    /// parameters, and derives the key.
    ///
    /// # Errors
    ///
    /// [`VaultError::KeyUnavailable`] if entropy, the write, or derivation
    /// fails.
    pub fn create(salt_path: impl AsRef<Path>, passphrase: &str) -> Result<Self, VaultError> {
        Self::create_with_pepper(salt_path, passphrase, None)
    }

    /// [`Self::create`] with a device-held `pepper` (named `pepper_name` in
    /// the envelope) mixed in after Argon2id.
    ///
    /// # Errors
    ///
    /// As [`Self::create`].
    pub fn create_with_pepper(
        salt_path: impl AsRef<Path>,
        passphrase: &str,
        pepper: Option<(&str, &[u8])>,
    ) -> Result<Self, VaultError> {
        let salt_path = salt_path.as_ref();
        let mut salt = [0_u8; SALT_LEN];
        OsRng.try_fill_bytes(&mut salt).map_err(|_| VaultError::KeyUnavailable)?;

        let key = derive(&salt, passphrase, ARGON2_M_KIB, ARGON2_T, ARGON2_P, pepper.map(|(_, bytes)| bytes))?;
        let envelope = Envelope {
            v: 2,
            salt: hex::encode(salt),
            m: ARGON2_M_KIB,
            t: ARGON2_T,
            p: ARGON2_P,
            pepper: pepper.map(|(name, _)| name.to_owned()),
        };
        if let Some(parent) = salt_path.parent() {
            std::fs::create_dir_all(parent).map_err(|_| VaultError::KeyUnavailable)?;
        }
        let json = serde_json::to_string(&envelope).map_err(|_| VaultError::KeyUnavailable)?;
        std::fs::write(salt_path, json).map_err(|_| VaultError::KeyUnavailable)?;
        Ok(Self { key, legacy: false })
    }

    /// True when this vault was enrolled under weaker, pre-envelope
    /// parameters and should be re-encrypted under the current ones.
    #[must_use]
    pub const fn needs_upgrade(&self) -> bool {
        self.legacy
    }
}

/// The pepper name an envelope recorded, if any, so the caller knows which
/// device secret to fetch before [`PassphraseKeyProvider::open_with_pepper`].
#[must_use]
pub fn envelope_pepper(salt_path: impl AsRef<Path>) -> Option<String> {
    let text = std::fs::read_to_string(salt_path.as_ref()).ok()?;
    serde_json::from_str::<Envelope>(&text).ok()?.pepper
}

fn decode_salt(text: &str) -> Result<[u8; SALT_LEN], VaultError> {
    let mut salt = [0_u8; SALT_LEN];
    hex::decode_to_slice(text, &mut salt).map_err(|_| VaultError::KeyUnavailable)?;
    Ok(salt)
}

fn derive(salt: &[u8; SALT_LEN], passphrase: &str, m: u32, t: u32, p: u32, pepper: Option<&[u8]>) -> Result<DataKey, VaultError> {
    let params = Params::new(m, t, p, Some(32)).map_err(|_| VaultError::KeyUnavailable)?;
    let mut bytes = [0_u8; 32];
    Argon2::new(Algorithm::Argon2id, Version::V0x13, params)
        .hash_password_into(passphrase.as_bytes(), salt, &mut bytes)
        .map_err(|_| VaultError::KeyUnavailable)?;
    if let Some(pepper) = pepper {
        use sha2::{Digest, Sha256};
        let mut hasher = Sha256::new();
        hasher.update(b"cabalmesh/vault-pepper/v1\0");
        hasher.update(bytes);
        hasher.update(pepper);
        bytes.zeroize();
        bytes = hasher.finalize().into();
    }
    let key = DataKey::from_bytes(bytes);
    bytes.zeroize();
    Ok(key)
}

impl KeyProvider for PassphraseKeyProvider {
    fn data_key(&self) -> Result<DataKey, VaultError> {
        Ok(self.key.clone())
    }

    fn describe(&self) -> &'static str {
        "passphrase-derived"
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::TempDir;

    #[test]
    fn open_after_create_derives_the_same_key() {
        let dir = TempDir::new().unwrap();
        let salt_path = dir.path().join("vault.salt");

        let created = PassphraseKeyProvider::create(&salt_path, "correct horse battery staple").unwrap();
        let reopened = PassphraseKeyProvider::open(&salt_path, "correct horse battery staple").unwrap();

        assert_eq!(
            created.data_key().unwrap().expose_for_storage(),
            reopened.data_key().unwrap().expose_for_storage(),
        );
    }

    #[test]
    fn a_different_passphrase_derives_a_different_key() {
        let dir = TempDir::new().unwrap();
        let salt_path = dir.path().join("vault.salt");
        PassphraseKeyProvider::create(&salt_path, "correct horse battery staple").unwrap();

        let wrong = PassphraseKeyProvider::open(&salt_path, "wrong passphrase").unwrap();
        let right = PassphraseKeyProvider::open(&salt_path, "correct horse battery staple").unwrap();

        assert_ne!(
            wrong.data_key().unwrap().expose_for_storage(),
            right.data_key().unwrap().expose_for_storage(),
        );
    }

    #[test]
    fn opening_without_an_existing_salt_fails_rather_than_inventing_one() {
        let dir = TempDir::new().unwrap();
        let salt_path = dir.path().join("vault.salt");

        assert!(matches!(
            PassphraseKeyProvider::open(&salt_path, "anything"),
            Err(VaultError::KeyUnavailable)
        ));
    }

    #[test]
    fn separate_salts_derive_separate_keys_from_the_same_passphrase() {
        let dir = TempDir::new().unwrap();
        let a = PassphraseKeyProvider::create(dir.path().join("a.salt"), "shared passphrase").unwrap();
        let b = PassphraseKeyProvider::create(dir.path().join("b.salt"), "shared passphrase").unwrap();

        assert_ne!(
            a.data_key().unwrap().expose_for_storage(),
            b.data_key().unwrap().expose_for_storage(),
        );
    }

    #[test]
    fn the_parameters_are_the_documented_ones() {
        // Changing these is a deliberate edit to `docs/identity-design.md`'s
        // decision 1, never a typo.
        assert_eq!((ARGON2_M_KIB, ARGON2_T, ARGON2_P), (65_536, 3, 1));
        let dir = TempDir::new().unwrap();
        let path = dir.path().join("vault.salt");
        PassphraseKeyProvider::create(&path, "x").unwrap();
        let envelope: Envelope = serde_json::from_str(&std::fs::read_to_string(&path).unwrap()).unwrap();
        assert_eq!((envelope.m, envelope.t, envelope.p), (ARGON2_M_KIB, ARGON2_T, ARGON2_P));
    }

    #[test]
    fn one_derivation_finishes_within_a_ceiling_on_this_platform() {
        // "Measured on the slowest target" means running this suite there.
        // Generous because CI and debug builds vary; a phone that blows it
        // is a signal to revisit `m`, not to delete the test.
        let dir = TempDir::new().unwrap();
        let start = std::time::Instant::now();
        PassphraseKeyProvider::create(dir.path().join("vault.salt"), "x").unwrap();
        assert!(start.elapsed() < std::time::Duration::from_secs(10), "took {:?}", start.elapsed());
    }

    #[test]
    fn a_pre_envelope_salt_still_opens_and_asks_to_be_upgraded() {
        let dir = TempDir::new().unwrap();
        let path = dir.path().join("vault.salt");
        std::fs::write(&path, hex::encode([7_u8; SALT_LEN])).unwrap();
        let legacy = PassphraseKeyProvider::open(&path, "pw").unwrap();
        assert!(legacy.needs_upgrade());
        // Same derivation the old code did: Argon2id at the crate defaults.
        let mut expected = [0_u8; 32];
        Argon2::default().hash_password_into(b"pw", &[7_u8; SALT_LEN], &mut expected).unwrap();
        assert_eq!(legacy.data_key().unwrap().expose_for_storage(), expected);

        let fresh = PassphraseKeyProvider::create(dir.path().join("new.salt"), "pw").unwrap();
        assert!(!fresh.needs_upgrade());
    }

    #[test]
    fn a_peppered_vault_needs_the_same_pepper() {
        let dir = TempDir::new().unwrap();
        let path = dir.path().join("vault.salt");
        let made = PassphraseKeyProvider::create_with_pepper(&path, "123456", Some(("device-presence", &[1; 32]))).unwrap();
        assert_eq!(envelope_pepper(&path).as_deref(), Some("device-presence"));

        let same = PassphraseKeyProvider::open_with_pepper(&path, "123456", Some(&[1; 32])).unwrap();
        assert_eq!(made.data_key().unwrap().expose_for_storage(), same.data_key().unwrap().expose_for_storage());
        let other = PassphraseKeyProvider::open_with_pepper(&path, "123456", Some(&[2; 32])).unwrap();
        assert_ne!(made.data_key().unwrap().expose_for_storage(), other.data_key().unwrap().expose_for_storage());
        assert!(PassphraseKeyProvider::open(&path, "123456").is_err(), "the pepper cannot be skipped");
    }
}
