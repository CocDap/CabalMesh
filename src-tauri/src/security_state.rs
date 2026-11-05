//! Which key provider protects the vault: the file-backed default, or a
//! passphrase the user has opted into.
//!
//! See `docs/identity-design.md`, decision 1. Default is [`UnlockMode::File`]
//! so a fresh install keeps the zero-friction boot the rest of the app
//! promises — passphrase protection is something a user turns on later from
//! `SECURITY`, never a wall on first launch.

use cabal_store::JsonStore;
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Default)]
#[serde(rename_all = "kebab-case")]
pub enum UnlockMode {
    /// A random key held in a file next to the vault. What every install
    /// starts with.
    #[default]
    File,
    /// The vault key is derived from a passphrase via Argon2id, and the raw
    /// file-backed key has been deleted. Requires the passphrase at every
    /// app start.
    Passphrase,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Default)]
pub struct SecurityState {
    #[serde(default)]
    pub mode: UnlockMode,
}

impl SecurityState {
    #[must_use]
    pub fn load(store: &JsonStore) -> Self {
        store.load_or(Self::default())
    }

    pub fn save(self, store: &JsonStore) -> Result<(), cabal_store::StoreError> {
        store.save(&self)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::TempDir;

    #[test]
    fn defaults_to_file_backed_when_nothing_was_ever_written() {
        let dir = TempDir::new().unwrap();
        let store = JsonStore::new(dir.path().join("security.json"));
        assert_eq!(SecurityState::load(&store).mode, UnlockMode::File);
    }

    #[test]
    fn round_trips_through_disk() {
        let dir = TempDir::new().unwrap();
        let store = JsonStore::new(dir.path().join("security.json"));

        SecurityState { mode: UnlockMode::Passphrase }.save(&store).unwrap();
        assert_eq!(SecurityState::load(&store).mode, UnlockMode::Passphrase);
    }
}
