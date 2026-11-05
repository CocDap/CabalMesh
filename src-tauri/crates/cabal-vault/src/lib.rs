//! Encryption at rest for key material.
//!
//! # What was wrong
//!
//! Private keys were stored as **plaintext hex in a JSON file**. Anything that
//! could read the app's data directory — another process, a backup, a synced
//! folder, a support bundle — had the wallet.
//!
//! The vault screen's own copy promises *"HELD LOCALLY. NEVER SYNCED"* and
//! *"RECOVERY PHRASE / NOT BACKED UP"*. That is a claim the storage layer has
//! to actually keep.
//!
//! # Shape
//!
//! A [`Vault`] holds a [`KeyProvider`] and encrypts a serializable document
//! with AES-256-GCM under a key the provider supplies. The provider is where
//! platforms differ: a device keystore on mobile, a derived key on desktop.
//! The encryption is identical either way, so it is tested once.
//!
//! # Rules
//!
//! - Key material never appears in `Debug`, in an error, or in a log. The
//!   redaction is at the type level ([`Secret`]), not by remembering.
//! - Every nonce is random and stored with the ciphertext. Reusing a nonce
//!   under one key is catastrophic for GCM, so nonces are never derived from
//!   anything reusable like a counter or a filename.
//! - Migration verifies a full decrypt round trip **before** the plaintext is
//!   removed.

#![forbid(unsafe_code)]

// rand_core comes from aes-gcm rather than a direct `rand` dependency, so
// there is exactly one RNG version in the tree.
use aes_gcm::aead::rand_core::RngCore;
use aes_gcm::aead::{Aead, KeyInit, OsRng};
use aes_gcm::{Aes256Gcm, Key, Nonce};
use serde::de::DeserializeOwned;
use serde::{Deserialize, Serialize};
use zeroize::{Zeroize, ZeroizeOnDrop};

mod secret;
pub use secret::Secret;

mod passphrase;
pub use passphrase::PassphraseKeyProvider;

/// Why a vault operation failed.
///
/// No variant carries key material, ciphertext or a passphrase. `Display` on
/// these reaches logs, and a decrypt failure that echoed the key would be worse
/// than the failure.
#[derive(Debug, thiserror::Error)]
#[non_exhaustive]
pub enum VaultError {
    #[error("the platform key store is unavailable")]
    KeyUnavailable,

    #[error("decryption failed: wrong key, or the vault was tampered with")]
    Decrypt,

    #[error("encryption failed")]
    Encrypt,

    #[error("vault contents are not valid for this type")]
    Malformed,

    #[error("vault storage failed")]
    Storage(#[from] cabal_store::StoreError),
}

/// A 256-bit data-encryption key.
///
/// Zeroized on drop, so a key does not linger in freed memory for a later heap
/// dump to find.
#[derive(Clone, Zeroize, ZeroizeOnDrop)]
pub struct DataKey([u8; 32]);

impl DataKey {
    /// Wraps raw key bytes.
    #[must_use]
    pub const fn from_bytes(bytes: [u8; 32]) -> Self {
        Self(bytes)
    }

    /// The raw key bytes, for a [`KeyProvider`] that must persist them.
    ///
    /// Named to be conspicuous. Only a provider should call this — it is the
    /// component whose job is storing the key. Anything else reaching for raw
    /// bytes is a bug.
    #[must_use]
    pub const fn expose_for_storage(&self) -> [u8; 32] {
        self.0
    }

    /// A fresh random key.
    ///
    /// # Errors
    ///
    /// [`VaultError::KeyUnavailable`] if the system entropy source fails.
    /// Generating a key from a degraded source would be worse than refusing.
    pub fn generate() -> Result<Self, VaultError> {
        let mut bytes = [0_u8; 32];
        OsRng
            .try_fill_bytes(&mut bytes)
            .map_err(|_| VaultError::KeyUnavailable)?;
        Ok(Self(bytes))
    }
}

/// Deliberately opaque: a key that printed itself would defeat the point.
impl std::fmt::Debug for DataKey {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str("DataKey(<redacted>)")
    }
}

/// Supplies the key the vault encrypts under.
///
/// The one thing that genuinely differs per platform. On iOS and Android this
/// is backed by the device key store, so the key is protected by hardware and
/// by the user's unlock. On desktop, where there is no uniform equivalent, it
/// is derived — better than plaintext, and honest about being weaker.
pub trait KeyProvider: Send + Sync {
    /// Returns the data key, creating and persisting one on first use.
    ///
    /// # Errors
    ///
    /// [`VaultError::KeyUnavailable`] if the key cannot be obtained — a locked
    /// device, a denied prompt, or a missing store.
    fn data_key(&self) -> Result<DataKey, VaultError>;

    /// A short description for logs. Must not identify the key itself.
    fn describe(&self) -> &'static str;
}

/// Lets a [`Vault`] hold whichever provider was chosen at runtime.
///
/// `IdentityStore` picks file-backed or passphrase-derived depending on
/// what `SECURITY → ADVANCED` has configured, and swaps providers again when
/// the user turns passphrase protection on or off. A generic `Vault<P>`
/// cannot express "P changes after construction"; a trait object can.
impl KeyProvider for Box<dyn KeyProvider> {
    fn data_key(&self) -> Result<DataKey, VaultError> {
        (**self).data_key()
    }

    fn describe(&self) -> &'static str {
        (**self).describe()
    }
}

/// Hands back a key this crate did not derive and does not own.
///
/// For a candidate key that arrived some other way — guardian shares
/// reconstructed via Shamir, most notably, where the reconstruction
/// happens entirely outside this crate and the only thing left to do is
/// see whether it actually opens the vault. `Vault::load` succeeding is
/// what proves the candidate right; this type carries no opinion either
/// way, it only makes the candidate speakable as a [`KeyProvider`].
pub struct StaticKeyProvider(DataKey);

impl StaticKeyProvider {
    #[must_use]
    pub const fn new(key: DataKey) -> Self {
        Self(key)
    }
}

impl KeyProvider for StaticKeyProvider {
    fn data_key(&self) -> Result<DataKey, VaultError> {
        Ok(self.0.clone())
    }

    fn describe(&self) -> &'static str {
        "externally-supplied"
    }
}

/// An encrypted document as written to disk.
#[derive(Debug, Serialize, Deserialize)]
struct Envelope {
    /// Format version, so a future change can migrate rather than fail.
    version: u8,
    /// Random per-write nonce, stored alongside because GCM needs it to
    /// decrypt and it is not secret.
    nonce: Vec<u8>,
    ciphertext: Vec<u8>,
}

const ENVELOPE_VERSION: u8 = 1;
const NONCE_LEN: usize = 12;

/// Encrypted storage for one document.
pub struct Vault<P: KeyProvider> {
    store: cabal_store::JsonStore,
    provider: P,
}

impl<P: KeyProvider> Vault<P> {
    /// A vault at `path`, encrypting under `provider`.
    pub fn new(path: impl Into<std::path::PathBuf>, provider: P) -> Self {
        Self {
            // Compact: nobody reads ciphertext by hand.
            store: cabal_store::JsonStore::compact(path),
            provider,
        }
    }

    /// The key this vault currently encrypts under.
    ///
    /// For the guardian scheme, which has to split this same key with
    /// Shamir's scheme before it can hand pieces of it to anyone — there is
    /// no other legitimate reason to reach for the raw key, and the naming
    /// stays conspicuous for that reason.
    ///
    /// # Errors
    ///
    /// [`VaultError::KeyUnavailable`], exactly as [`Vault::save`] and
    /// [`Vault::load`] can fail to obtain the same key.
    pub fn key(&self) -> Result<DataKey, VaultError> {
        self.provider.data_key()
    }

    /// Whether an encrypted document exists.
    #[must_use]
    pub fn exists(&self) -> bool {
        self.store.exists()
    }

    /// Encrypts and writes `value`, atomically.
    ///
    /// # Errors
    ///
    /// [`VaultError::KeyUnavailable`], [`VaultError::Encrypt`], or a storage
    /// failure.
    pub fn save<T: Serialize>(&self, value: &T) -> Result<(), VaultError> {
        let plaintext = serde_json::to_vec(value).map_err(|_| VaultError::Encrypt)?;
        let key = self.provider.data_key()?;
        let cipher = Aes256Gcm::new(Key::<Aes256Gcm>::from_slice(&key.0));

        // Fresh nonce every write. Reuse under one key breaks GCM completely,
        // so this is never derived from a counter or a filename.
        let mut nonce_bytes = [0_u8; NONCE_LEN];
        OsRng
            .try_fill_bytes(&mut nonce_bytes)
            .map_err(|_| VaultError::Encrypt)?;

        let ciphertext = cipher
            .encrypt(Nonce::from_slice(&nonce_bytes), plaintext.as_ref())
            .map_err(|_| VaultError::Encrypt)?;

        self.store.save(&Envelope {
            version: ENVELOPE_VERSION,
            nonce: nonce_bytes.to_vec(),
            ciphertext,
        })?;
        Ok(())
    }

    /// Reads and decrypts the document.
    ///
    /// # Errors
    ///
    /// [`VaultError::Decrypt`] if the key is wrong or the file was tampered
    /// with — GCM authenticates, so those are indistinguishable and both mean
    /// "do not trust this".
    pub fn load<T: DeserializeOwned>(&self) -> Result<T, VaultError> {
        let envelope: Envelope = self.store.load()?;
        if envelope.version != ENVELOPE_VERSION || envelope.nonce.len() != NONCE_LEN {
            return Err(VaultError::Malformed);
        }

        let key = self.provider.data_key()?;
        let cipher = Aes256Gcm::new(Key::<Aes256Gcm>::from_slice(&key.0));
        let mut plaintext = cipher
            .decrypt(
                Nonce::from_slice(&envelope.nonce),
                envelope.ciphertext.as_ref(),
            )
            .map_err(|_| VaultError::Decrypt)?;

        let value = serde_json::from_slice(&plaintext).map_err(|_| VaultError::Malformed);
        plaintext.zeroize();
        value
    }

    /// Adopts a plaintext document, then removes it — but only once the
    /// encrypted copy is proven readable.
    ///
    /// The ordering is the entire point. Write, **decrypt back and compare**,
    /// and only then unlink. A migration that deleted first and failed second
    /// would destroy a wallet, which is exactly the outcome this whole crate
    /// exists to avoid.
    ///
    /// Returns `Ok(false)` when there was nothing to migrate.
    ///
    /// # Errors
    ///
    /// Any failure leaves the plaintext untouched.
    pub fn migrate_plaintext<T>(&self, plaintext: &cabal_store::JsonStore) -> Result<bool, VaultError>
    where
        T: Serialize + DeserializeOwned + PartialEq,
    {
        if !plaintext.exists() || self.exists() {
            return Ok(false);
        }

        let value: T = plaintext.load()?;
        self.save(&value)?;

        let round_tripped: T = self.load()?;
        if round_tripped != value {
            tracing::error!(
                target: "cabal_vault",
                "migrated vault did not read back identically; keeping the plaintext"
            );
            return Err(VaultError::Decrypt);
        }

        plaintext.delete()?;
        tracing::info!(target: "cabal_vault", provider = self.provider.describe(), "migrated to encrypted vault");
        Ok(true)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::TempDir;

    struct FixedKey([u8; 32]);
    impl KeyProvider for FixedKey {
        fn data_key(&self) -> Result<DataKey, VaultError> {
            Ok(DataKey::from_bytes(self.0))
        }
        fn describe(&self) -> &'static str {
            "test-fixed"
        }
    }

    struct NoKey;
    impl KeyProvider for NoKey {
        fn data_key(&self) -> Result<DataKey, VaultError> {
            Err(VaultError::KeyUnavailable)
        }
        fn describe(&self) -> &'static str {
            "test-none"
        }
    }

    #[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
    struct Identity {
        alias: String,
        private_key_hex: String,
    }

    fn identity() -> Vec<Identity> {
        vec![Identity {
            alias: "Genesis Fox".into(),
            private_key_hex: "0xdeadbeefcafe".into(),
        }]
    }

    #[test]
    fn round_trips_a_document() {
        let dir = TempDir::new().unwrap();
        let vault = Vault::new(dir.path().join("vault.enc"), FixedKey([7; 32]));

        vault.save(&identity()).unwrap();
        assert_eq!(vault.load::<Vec<Identity>>().unwrap(), identity());
    }

    #[test]
    fn the_key_never_appears_on_disk() {
        // The whole point of the crate: reading the file must not reveal the
        // secret it protects.
        let dir = TempDir::new().unwrap();
        let path = dir.path().join("vault.enc");
        Vault::new(&path, FixedKey([7; 32])).save(&identity()).unwrap();

        let raw = std::fs::read_to_string(&path).unwrap();
        assert!(!raw.contains("deadbeefcafe"), "private key leaked to disk");
        assert!(!raw.contains("Genesis Fox"), "metadata leaked to disk");
    }

    #[test]
    fn every_write_uses_a_fresh_nonce() {
        // Nonce reuse under one key breaks GCM outright, so identical
        // plaintext must still produce different ciphertext.
        let dir = TempDir::new().unwrap();
        let path = dir.path().join("vault.enc");
        let vault = Vault::new(&path, FixedKey([7; 32]));

        vault.save(&identity()).unwrap();
        let first = std::fs::read_to_string(&path).unwrap();
        vault.save(&identity()).unwrap();
        let second = std::fs::read_to_string(&path).unwrap();

        assert_ne!(first, second, "identical plaintext produced identical ciphertext");
    }

    #[test]
    fn a_wrong_key_cannot_decrypt() {
        let dir = TempDir::new().unwrap();
        let path = dir.path().join("vault.enc");
        Vault::new(&path, FixedKey([7; 32])).save(&identity()).unwrap();

        assert!(matches!(
            Vault::new(&path, FixedKey([8; 32])).load::<Vec<Identity>>(),
            Err(VaultError::Decrypt)
        ));
    }

    #[test]
    fn tampering_is_detected() {
        // GCM authenticates, so a flipped byte fails rather than decrypting to
        // garbage the caller might act on.
        let dir = TempDir::new().unwrap();
        let path = dir.path().join("vault.enc");
        let vault = Vault::new(&path, FixedKey([7; 32]));
        vault.save(&identity()).unwrap();

        let mut envelope: Envelope =
            serde_json::from_str(&std::fs::read_to_string(&path).unwrap()).unwrap();
        envelope.ciphertext[0] ^= 0xff;
        std::fs::write(&path, serde_json::to_vec(&envelope).unwrap()).unwrap();

        assert!(matches!(
            vault.load::<Vec<Identity>>(),
            Err(VaultError::Decrypt)
        ));
    }

    #[test]
    fn migration_removes_the_plaintext_only_after_verifying() {
        let dir = TempDir::new().unwrap();
        let plaintext = cabal_store::JsonStore::new(dir.path().join("identities.json"));
        plaintext.save(&identity()).unwrap();

        let vault = Vault::new(dir.path().join("vault.enc"), FixedKey([7; 32]));
        assert!(vault.migrate_plaintext::<Vec<Identity>>(&plaintext).unwrap());

        assert!(!plaintext.exists(), "plaintext should be gone");
        assert_eq!(vault.load::<Vec<Identity>>().unwrap(), identity());
    }

    #[test]
    fn a_failed_migration_leaves_the_plaintext_intact() {
        // The dangerous path. If the key is unavailable the wallet must survive
        // untouched rather than be deleted on optimism.
        let dir = TempDir::new().unwrap();
        let plaintext = cabal_store::JsonStore::new(dir.path().join("identities.json"));
        plaintext.save(&identity()).unwrap();

        let vault = Vault::new(dir.path().join("vault.enc"), NoKey);
        assert!(vault.migrate_plaintext::<Vec<Identity>>(&plaintext).is_err());

        assert!(plaintext.exists(), "plaintext was destroyed by a failed migration");
        assert_eq!(plaintext.load::<Vec<Identity>>().unwrap(), identity());
    }

    #[test]
    fn migration_never_overwrites_an_existing_vault() {
        let dir = TempDir::new().unwrap();
        let plaintext = cabal_store::JsonStore::new(dir.path().join("identities.json"));
        plaintext.save(&identity()).unwrap();

        let vault = Vault::new(dir.path().join("vault.enc"), FixedKey([7; 32]));
        let existing = vec![Identity {
            alias: "Already Here".into(),
            private_key_hex: "0xabc".into(),
        }];
        vault.save(&existing).unwrap();

        assert!(!vault.migrate_plaintext::<Vec<Identity>>(&plaintext).unwrap());
        assert_eq!(vault.load::<Vec<Identity>>().unwrap(), existing);
    }

    #[test]
    fn migration_is_a_no_op_without_plaintext() {
        let dir = TempDir::new().unwrap();
        let plaintext = cabal_store::JsonStore::new(dir.path().join("absent.json"));
        let vault = Vault::new(dir.path().join("vault.enc"), FixedKey([7; 32]));

        assert!(!vault.migrate_plaintext::<Vec<Identity>>(&plaintext).unwrap());
    }

    #[test]
    fn keys_and_errors_stay_opaque() {
        let key = DataKey::from_bytes([7; 32]);
        assert_eq!(format!("{key:?}"), "DataKey(<redacted>)");

        for error in [VaultError::Decrypt, VaultError::KeyUnavailable, VaultError::Encrypt] {
            let rendered = error.to_string();
            assert!(!rendered.contains("deadbeef"));
            assert!(!rendered.contains('7'));
        }
    }
}
