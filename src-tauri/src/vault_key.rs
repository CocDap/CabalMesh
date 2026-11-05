//! Where the vault's data key comes from, per platform.
//!
//! # The honest state of this
//!
//! **Desktop** stores a random data key in a file with owner-only permissions.
//! That is a real improvement — the key is no longer the wallet, and a leaked
//! `vault.enc` alone is useless — but it is not equivalent to a hardware key
//! store. Anything running as this user can read the key file. There is no
//! uniform desktop key store now that `keyring` has been removed, and the
//! honest options are a passphrase prompt (which the frozen UI has no way to
//! show) or this.
//!
//! **Mobile** should use the device key store: the iOS keychain with
//! `kSecAttrAccessibleAfterFirstUnlockThisDeviceOnly`, and the Android key
//! store with StrongBox where available. That requires the native plugin from
//! ticket 21's family of work, so it is not wired yet — and rather than pretend
//! otherwise, [`platform_provider`] returns the file-backed provider on mobile
//! too, with a warning at startup.
//!
//! The app-sandbox file is still meaningfully protected on a phone (no other
//! app can read it), so this is a weaker key store rather than none. The gap is
//! recorded here so it cannot be quietly forgotten.

use cabal_vault::{DataKey, KeyProvider, VaultError};
use std::path::PathBuf;

/// A data key persisted beside the vault, readable only by this user.
pub struct FileKeyProvider {
    path: PathBuf,
}

impl FileKeyProvider {
    /// A provider storing its key at `path`.
    #[must_use]
    pub fn new(path: impl Into<PathBuf>) -> Self {
        Self { path: path.into() }
    }

    #[cfg(unix)]
    fn restrict_permissions(path: &std::path::Path) -> std::io::Result<()> {
        use std::os::unix::fs::PermissionsExt;
        // 0600 before anything is written into it, so the key is never briefly
        // world-readable.
        std::fs::set_permissions(path, std::fs::Permissions::from_mode(0o600))
    }

    #[cfg(not(unix))]
    fn restrict_permissions(_path: &std::path::Path) -> std::io::Result<()> {
        // Windows inherits the user's profile ACL, which is already
        // owner-scoped for the app data directory.
        Ok(())
    }
}

impl KeyProvider for FileKeyProvider {
    fn data_key(&self) -> Result<DataKey, VaultError> {
        if let Ok(encoded) = std::fs::read_to_string(&self.path) {
            let trimmed = encoded.trim();
            let mut bytes = [0_u8; 32];
            if hex::decode_to_slice(trimmed, &mut bytes).is_ok() {
                return Ok(DataKey::from_bytes(bytes));
            }
            // A corrupt key file is not recoverable by regenerating: doing so
            // would silently orphan an existing vault. Fail instead.
            tracing::error!(
                target: "cabalmesh::vault",
                "key file is unreadable; refusing to generate a new key over an existing vault"
            );
            return Err(VaultError::KeyUnavailable);
        }

        let key = DataKey::generate()?;
        let mut encoded = hex::encode(key_bytes(&key));

        if let Some(parent) = self.path.parent() {
            std::fs::create_dir_all(parent).map_err(|_| VaultError::KeyUnavailable)?;
        }
        std::fs::File::create(&self.path).map_err(|_| VaultError::KeyUnavailable)?;
        Self::restrict_permissions(&self.path).map_err(|_| VaultError::KeyUnavailable)?;
        std::fs::write(&self.path, &encoded).map_err(|_| VaultError::KeyUnavailable)?;

        // The hex copy is a second plaintext of the key; do not leave it in
        // freed memory.
        use zeroize::Zeroize;
        encoded.zeroize();

        tracing::info!(target: "cabalmesh::vault", "generated a new vault key");
        Ok(key)
    }

    fn describe(&self) -> &'static str {
        "file-backed"
    }
}

/// Exposes the key bytes for encoding.
///
/// Deliberately private and narrow: `DataKey` has no public accessor, so this
/// is the only path to its bytes and it lives next to the code that needs it.
fn key_bytes(key: &DataKey) -> [u8; 32] {
    // `DataKey` is opaque by design, so round-trip through its constructor
    // contract: the provider is the component that owns key encoding.
    key.expose_for_storage()
}

/// Stands in for the real provider while the vault is passphrase-protected
/// and locked.
///
/// Refuses every call rather than falling back to generating or reading a
/// file-backed key — that would silently defeat passphrase protection the
/// moment `unlock_with_passphrase` had not yet run. The bridge installs this
/// at startup when `SecurityState::mode` is [`crate::security_state::UnlockMode::Passphrase`],
/// and replaces it once the correct passphrase is supplied.
pub struct LockedProvider;

impl KeyProvider for LockedProvider {
    fn data_key(&self) -> Result<DataKey, VaultError> {
        Err(VaultError::KeyUnavailable)
    }

    fn describe(&self) -> &'static str {
        "locked"
    }
}

/// The provider for this platform.
pub fn platform_provider(key_path: PathBuf) -> FileKeyProvider {
    if !cfg!(desktop) {
        tracing::warn!(
            target: "cabalmesh::vault",
            "using a file-backed vault key; the device key store is not wired yet"
        );
    }
    FileKeyProvider::new(key_path)
}

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::TempDir;

    #[test]
    fn the_key_is_stable_across_calls() {
        // A provider that returned a different key each time would make every
        // previously written vault unreadable.
        let dir = TempDir::new().unwrap();
        let provider = FileKeyProvider::new(dir.path().join("vault.key"));

        let first = provider.data_key().unwrap();
        let second = provider.data_key().unwrap();
        assert_eq!(key_bytes(&first), key_bytes(&second));
    }

    #[test]
    fn separate_locations_get_separate_keys() {
        let dir = TempDir::new().unwrap();
        let a = FileKeyProvider::new(dir.path().join("a.key")).data_key().unwrap();
        let b = FileKeyProvider::new(dir.path().join("b.key")).data_key().unwrap();
        assert_ne!(key_bytes(&a), key_bytes(&b));
    }

    #[cfg(unix)]
    #[test]
    fn the_key_file_is_owner_only() {
        use std::os::unix::fs::PermissionsExt;
        let dir = TempDir::new().unwrap();
        let path = dir.path().join("vault.key");
        FileKeyProvider::new(&path).data_key().unwrap();

        let mode = std::fs::metadata(&path).unwrap().permissions().mode();
        assert_eq!(mode & 0o777, 0o600, "key file is readable by others");
    }

    #[test]
    fn a_corrupt_key_file_fails_rather_than_regenerating() {
        // Regenerating would orphan an existing vault, turning a recoverable
        // problem into permanent data loss.
        let dir = TempDir::new().unwrap();
        let path = dir.path().join("vault.key");
        std::fs::write(&path, "not hex").unwrap();

        assert!(matches!(
            FileKeyProvider::new(&path).data_key(),
            Err(VaultError::KeyUnavailable)
        ));
    }
}
