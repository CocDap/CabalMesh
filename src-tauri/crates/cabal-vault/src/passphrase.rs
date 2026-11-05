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
use argon2::Argon2;
use std::path::Path;
use zeroize::Zeroize;

const SALT_LEN: usize = 16;

/// Derives the vault key from a passphrase and a salt persisted beside it.
pub struct PassphraseKeyProvider {
    key: DataKey,
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
    /// unreadable. A wrong passphrase is not detected here — [`Vault::load`]
    /// on the resulting key is what proves it wrong, by failing to decrypt.
    ///
    /// [`Vault::load`]: crate::Vault::load
    pub fn open(salt_path: impl AsRef<Path>, passphrase: &str) -> Result<Self, VaultError> {
        let encoded = std::fs::read_to_string(salt_path.as_ref()).map_err(|_| VaultError::KeyUnavailable)?;
        let mut salt = [0_u8; SALT_LEN];
        hex::decode_to_slice(encoded.trim(), &mut salt).map_err(|_| VaultError::KeyUnavailable)?;
        Self::derive(&salt, passphrase)
    }

    /// Starts passphrase protection: generates a fresh salt, persists it, and
    /// derives the key from it.
    ///
    /// Only for the enable flow. Calling this on a vault already protected by
    /// a passphrase overwrites its salt and orphans it — the caller must
    /// re-encrypt under the new key in the same operation, the way
    /// `IdentityStore::enable_passphrase` does.
    ///
    /// # Errors
    ///
    /// [`VaultError::KeyUnavailable`] if the salt cannot be generated or
    /// written.
    pub fn create(salt_path: impl AsRef<Path>, passphrase: &str) -> Result<Self, VaultError> {
        let salt_path = salt_path.as_ref();
        let mut salt = [0_u8; SALT_LEN];
        OsRng.try_fill_bytes(&mut salt).map_err(|_| VaultError::KeyUnavailable)?;

        if let Some(parent) = salt_path.parent() {
            std::fs::create_dir_all(parent).map_err(|_| VaultError::KeyUnavailable)?;
        }
        std::fs::write(salt_path, hex::encode(salt)).map_err(|_| VaultError::KeyUnavailable)?;

        Self::derive(&salt, passphrase)
    }

    fn derive(salt: &[u8; SALT_LEN], passphrase: &str) -> Result<Self, VaultError> {
        let mut bytes = [0_u8; 32];
        let result = Argon2::default()
            .hash_password_into(passphrase.as_bytes(), salt, &mut bytes)
            .map(|()| DataKey::from_bytes(bytes))
            .map_err(|_| VaultError::KeyUnavailable);

        // A second plaintext of the key; do not leave it in freed memory.
        bytes.zeroize();
        result.map(|key| Self { key })
    }
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
}
