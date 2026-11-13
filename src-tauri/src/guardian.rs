//! Guardian enrollment and mesh unlock: persistence and orchestration around
//! `cabal_guardian`'s crypto core.
//!
//! See `docs/identity-design.md` for the design this implements, and the
//! module docs on `cabal_guardian::protocol` for why an unlock request is a
//! broadcast of unlinkable tags rather than something addressed by durable
//! identity — BLE peer ids reset every launch, and this mesh's own stated
//! rule is that nothing durable ever travels in the clear.
//!
//! # Three flows
//!
//! - **Everyday unlock** — the enrolled device asks reachable guardians in
//!   real time. Guardians hold shares of a stable recovery key RK; the
//!   vault key is wrapped under RK on the owner's device, so K colluding
//!   guardians still need the device (doc: "two independent compromises").
//! - **Restore onto a new device** — needs only the restore code
//!   ([`RestoreSession`]). Guardians hold the vault's contents wrapped under
//!   a key derived from RK *and* the code.
//! - **Delay and veto** — an approved restore is held for
//!   [`RESTORE_DELAY_HOURS`] (waived for a vault last reported empty), and
//!   announced so the original device can cancel it.
//!
//! The delay is enforced by guardians, not the new device, which may be the
//! attacker. The veto reaches the original device **over the mesh only**:
//! there is no server, so a device out of range of every guardian holding
//! the restore never hears about it. The doc's "if that device is still
//! alive somewhere with a network connection" is narrower here — alive and
//! within mesh reach of a guardian during the delay.
//!
//! # Storage
//!
//! One encrypted document, holding both roles a device can play — the
//! guardians *it* has enrolled, and the shares it holds *for others* — since
//! a real device plausibly does both — split across two files, because the
//! two roles need different protection:
//!
//! - `guardian.enc` — the *owner* side (this device's recovery key and the
//!   guardians it enrolled). File-key protected regardless of `SECURITY`'s
//!   passphrase switch, deliberately: mesh unlock is a way to *open* a
//!   locked vault, so what it needs has to be readable before the vault is.
//!   What it exposes to someone holding the file is guardian public keys and
//!   a key that opens replies only guardians, by a human tap, would send.
//! - `guardian_held.enc` — shares this device holds *for other people*.
//!   Encrypted under a subkey of the identity vault's own key
//!   ([`DataKey::derive`]), so it follows the passphrase switch exactly: while
//!   the vault is locked these shares are unreadable and this device answers
//!   no one; re-keyed whenever the vault key changes
//!   ([`GuardianService::attach_vault_key`]).

use cabal_guardian::protocol::{
    recognition_tag, BackupEntry, BackupPayload, BackupUpdate, EnrollAccept, EnrollPayload, EnrollRequest,
    GuardianMessage, RestoreCancel, RestorePending, RestoreReplyPayload, RestoreRequest, RestoreStatus,
    UnlockReplyPayload, UnlockRequest, BACKUP_MAC_DOMAIN, CANCEL_MAC_DOMAIN, TAG_LEN,
};
use cabal_guardian::restore::{self, RestoreSecret};
use cabal_guardian::sealed::{self, GuardianPublicKey, GuardianSecretKey, Sealed};
use cabal_guardian::{self as shamir, Share};
use cabal_vault::{DataKey, StaticKeyProvider, Vault};
use std::path::PathBuf;
use aes_gcm::aead::{rand_core::RngCore, OsRng};
use chrono::{DateTime, Duration, Utc};
use serde::{Deserialize, Serialize};

/// Everything this device remembers about the guardian scheme, in both
/// roles.
#[derive(Debug, Clone, Serialize, Deserialize, Default)]
struct GuardianDocument {
    owner: OwnerState,
    held: HeldState,
}

/// What this device remembers about *its own* recovery setup.
#[derive(Debug, Clone, Serialize, Deserialize, Default)]
struct OwnerState {
    /// Generated on first enrollment, then stable. Guardians seal their
    /// unlock replies to the public half of this.
    recovery_secret_key: Option<[u8; 32]>,
    guardians: Vec<GuardianRecord>,
    /// Shares needed to reconstruct. 0 until enrollment has actually run.
    threshold: u8,
    /// Shown to the person as the restore code. Rotated on every
    /// enrollment, so guardians from an older enrollment stop recognising
    /// restore requests.
    #[serde(default)]
    restore_secret: Option<[u8; 32]>,
    /// Authenticates backup refreshes and restore cancels to guardians.
    #[serde(default)]
    owner_auth_key: Option<[u8; 32]>,
    /// The recovery key RK, wrapped under the current vault key. Lets the
    /// unlocked owner re-wrap when the vault key changes and refresh backups.
    /// Guardians hold shares of RK, never of the vault key, so a passphrase
    /// change does not silently invalidate every share.
    #[serde(default)]
    rk_under_vk: Option<Vec<u8>>,
    /// The vault key, wrapped under RK: what K reconstructed shares open
    /// during everyday unlock. Absent in pre-RK enrollments, whose shares
    /// are of the vault key directly.
    #[serde(default)]
    vk_under_rk: Option<Vec<u8>>,
    #[serde(default)]
    backup_seq: u64,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
struct GuardianRecord {
    public_key: [u8; 32],
    enrolled_at: DateTime<Utc>,
}

/// What this device remembers about shares it holds *for other people*.
#[derive(Debug, Clone, Serialize, Deserialize, Default)]
struct HeldState {
    /// This device's own guardian key pair, generated the first time it is
    /// asked to hold a share, then reused for every owner who enrolls it —
    /// reuse across owners does not weaken sealed-box security, and a fresh
    /// key per relationship would only make "who is this device a guardian
    /// for" a question with more surface area, not less.
    secret_key: Option<[u8; 32]>,
    held: Vec<HeldShare>,
    #[serde(default)]
    pending: Vec<PendingRestore>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
struct HeldShare {
    /// Which owner this share belongs to — a key, not a BLE identifier, for
    /// the same reason the owner's own recovery key is one.
    owner_recovery_public_key: [u8; 32],
    /// A `cabal_guardian::Share`, serialized.
    share: Vec<u8>,
    enrolled_at: DateTime<Utc>,
    #[serde(default)]
    threshold: u8,
    #[serde(default)]
    restore_lookup: Option<[u8; 32]>,
    #[serde(default)]
    owner_auth_key: Option<[u8; 32]>,
    #[serde(default)]
    backup: Option<Vec<u8>>,
    #[serde(default)]
    backup_seq: u64,
    /// `None` means unknown, treated as "has value" — the delay applies.
    #[serde(default)]
    has_value: Option<bool>,
}

/// A restore this device (as guardian) approved and is holding back.
#[derive(Debug, Clone, Serialize, Deserialize)]
struct PendingRestore {
    owner_recovery_public_key: [u8; 32],
    nonce: [u8; TAG_LEN],
    reply_public_key: [u8; 32],
    release_at: DateTime<Utc>,
    cancelled: bool,
}

/// How long an approved restore is held before the share is released.
/// Decision 3 in `docs/identity-design.md`.
pub const RESTORE_DELAY_HOURS: i64 = 24;
/// How often a guardian re-announces restores it is holding.
pub const ANNOUNCE_INTERVAL: std::time::Duration = std::time::Duration::from_secs(5 * 60);
/// How often the owner refreshes guardians' backup copies.
pub const BACKUP_REFRESH_INTERVAL: std::time::Duration = std::time::Duration::from_secs(30 * 60);
/// At or below this much native NEAR (0.05 NEAR, in yocto) and no tokens,
/// a vault counts as empty and a restore skips the delay: there is nothing
/// for the delay to protect.
pub const DUST_YOCTO: u128 = 50_000_000_000_000_000_000_000;
/// How long a finished or cancelled pending restore is remembered, so a
/// re-broadcast request still gets its final answer.
const PENDING_RETENTION_DAYS: i64 = 7;
const BACKUP_AAD: &[u8] = b"cabalmesh/guardian-backup/v1";
const RK_UNDER_VK_LABEL: &str = "guardian-rk";
const VK_UNDER_RK_LABEL: &str = "guardian-vk";

#[derive(Debug, thiserror::Error)]
#[non_exhaustive]
pub enum GuardianServiceError {
    #[error("guardian store failed")]
    Store(#[from] cabal_vault::VaultError),
    #[error(transparent)]
    Crypto(#[from] shamir::GuardianError),
    #[error(transparent)]
    Sealed(#[from] sealed::SealedError),
    #[error(transparent)]
    Protocol(#[from] cabal_guardian::protocol::ProtocolError),
    #[error("malformed guardian payload")]
    Malformed,
    #[error("no recovery key exists yet — enroll guardians first")]
    NotEnrolled,
    #[error("the vault is locked — shares held for others are unreadable until it opens")]
    Locked,
    #[error("the restore code is not valid")]
    BadRestoreCode,
    #[error("the returned shares and backups did not open — not enough guardians from the latest enrollment")]
    RestoreIncomplete,
    #[error("need at least two guardians, and no more than 255")]
    InvalidGuardianCount,
}

impl From<restore::RestoreError> for GuardianServiceError {
    fn from(error: restore::RestoreError) -> Self {
        match error {
            restore::RestoreError::BadCode => Self::BadRestoreCode,
            _ => Self::Malformed,
        }
    }
}

impl From<serde_json::Error> for GuardianServiceError {
    fn from(_: serde_json::Error) -> Self {
        Self::Malformed
    }
}

/// Persistence and pure protocol logic for both guardian roles. No BLE
/// dependency — sending what this produces, and feeding it what arrives, is
/// the caller's job (see the `mod actor` below).
pub struct GuardianService {
    /// `guardian.enc`: the owner side. See the module docs for why this one
    /// stays file-key protected.
    vault: Vault<crate::vault_key::FileKeyProvider>,
    /// In memory, both roles. `doc.held` is persisted to `held_vault`, never
    /// to `vault` — except in a store written before the split, which
    /// [`Self::attach_vault_key`] migrates.
    doc: GuardianDocument,
    held_path: PathBuf,
    /// `None` until the identity vault's key is attached — i.e. while the
    /// vault is locked.
    held_vault: Option<Vault<StaticKeyProvider>>,
    /// The identity vault key last attached — needed to re-wrap RK when the
    /// vault key changes. `None` while the vault is locked.
    attached_key: Option<DataKey>,
}

/// Label for the held-shares subkey. Changing it orphans every held share.
const HELD_SUBKEY_LABEL: &str = "guardian-held";

impl GuardianService {
    /// Opens (or creates) the guardian store under `app_dir`. Shares held for
    /// others stay unreadable until [`Self::attach_vault_key`].
    #[must_use]
    pub fn open(app_dir: &std::path::Path) -> Self {
        let vault = Vault::new(
            app_dir.join("guardian.enc"),
            crate::vault_key::platform_provider(app_dir.join("guardian.key")),
        );
        let doc = if vault.exists() { vault.load().unwrap_or_default() } else { GuardianDocument::default() };
        Self { vault, doc, held_path: app_dir.join("guardian_held.enc"), held_vault: None, attached_key: None }
    }

    /// Supplies the identity vault's current key: opens the held-shares
    /// store under it, or — if it was already open under a previous key —
    /// re-encrypts it under this one. Call after every unlock and after
    /// anything that changes the vault key (passphrase on/off, restore).
    ///
    /// # Errors
    ///
    /// The held store exists but does not open under `vault_key` while
    /// nothing is open in memory to re-key from, or a write failed. On error
    /// the previous state is left as it was.
    pub fn attach_vault_key(&mut self, vault_key: &DataKey) -> Result<(), GuardianServiceError> {
        self.rewrap_recovery_key(vault_key)?;
        let next = Vault::new(self.held_path.clone(), StaticKeyProvider::new(vault_key.derive(HELD_SUBKEY_LABEL)));

        if self.held_vault.is_some() {
            // Already readable: what is in memory is authoritative.
            next.save(&self.doc.held)?;
            self.held_vault = Some(next);
            self.attached_key = Some(vault_key.clone());
            return Ok(());
        }

        let legacy = std::mem::take(&mut self.doc.held);
        let mut held: HeldState = if next.exists() {
            match next.load() {
                Ok(held) => held,
                Err(error) => {
                    self.doc.held = legacy;
                    return Err(error.into());
                }
            }
        } else {
            HeldState::default()
        };
        let migrating = legacy.secret_key.is_some() || !legacy.held.is_empty();
        if migrating {
            if held.secret_key.is_none() {
                held.secret_key = legacy.secret_key;
            }
            held.held.extend(legacy.held);
        }
        next.save(&held)?;
        self.doc.held = held;
        self.held_vault = Some(next);
        self.attached_key = Some(vault_key.clone());
        if migrating {
            // Only now that the shares are safely in the new file.
            self.save_owner()?;
        }
        Ok(())
    }

    /// Keeps RK reachable across a vault-key change: unwraps it under the
    /// previously attached key and re-wraps both directions under `new`.
    /// A no-op on first attach, for pre-RK enrollments, or when the key did
    /// not actually change.
    fn rewrap_recovery_key(&mut self, new: &DataKey) -> Result<(), GuardianServiceError> {
        let Some(old) = self.attached_key.clone() else { return Ok(()) };
        if old.expose_for_storage() == new.expose_for_storage() {
            return Ok(());
        }
        let Some(rk) = self.recovery_key_with(&old) else { return Ok(()) };
        self.wrap_recovery_key(&rk, new);
        self.save_owner()
    }

    fn recovery_key_with(&self, vault_key: &DataKey) -> Option<DataKey> {
        let wrapped = self.doc.owner.rk_under_vk.as_ref()?;
        let bytes = restore::aead_open(&vault_key.derive(RK_UNDER_VK_LABEL).expose_for_storage(), b"", wrapped).ok()?;
        Some(DataKey::from_bytes(bytes.try_into().ok()?))
    }

    fn wrap_recovery_key(&mut self, rk: &DataKey, vault_key: &DataKey) {
        self.doc.owner.rk_under_vk = Some(restore::aead_seal(
            &vault_key.derive(RK_UNDER_VK_LABEL).expose_for_storage(),
            b"",
            &rk.expose_for_storage(),
        ));
        self.doc.owner.vk_under_rk = Some(restore::aead_seal(
            &rk.derive(VK_UNDER_RK_LABEL).expose_for_storage(),
            b"",
            &vault_key.expose_for_storage(),
        ));
    }

    /// Whether shares held for others are readable this session.
    #[must_use]
    pub fn is_held_unlocked(&self) -> bool {
        self.held_vault.is_some()
    }

    fn save_owner(&self) -> Result<(), GuardianServiceError> {
        let owner_only = GuardianDocument { owner: self.doc.owner.clone(), held: HeldState::default() };
        self.vault.save(&owner_only)?;
        Ok(())
    }

    fn save_held(&self) -> Result<(), GuardianServiceError> {
        self.held_vault.as_ref().ok_or(GuardianServiceError::Locked)?.save(&self.doc.held)?;
        Ok(())
    }

    // ---------------------------------------------------------------
    // Owner side: this device's own recovery setup.
    // ---------------------------------------------------------------

    /// This device's recovery key pair, generating one on first use.
    fn owner_recovery_key(&mut self) -> Result<GuardianSecretKey, GuardianServiceError> {
        if self.doc.owner.recovery_secret_key.is_none() {
            let key = GuardianSecretKey::generate();
            self.doc.owner.recovery_secret_key = Some(key.to_bytes());
            self.save_owner()?;
        }
        Ok(GuardianSecretKey::from_bytes(self.doc.owner.recovery_secret_key.expect("just set")))
    }

    /// Enrolls `guardians`: generates a fresh recovery key RK, splits it,
    /// and returns each guardian's sealed enrollment message paired with its
    /// public key, ready to send directed. Also rotates the restore code
    /// and the owner auth key, and wraps `backup_plaintext` (the vault's
    /// contents) for guardians to hold.
    ///
    /// Persists the guardian list, threshold and the RK/vault-key wrappings —
    /// **never the shares**, matching the design doc: "owner does NOT store
    /// the shares (storing them defeats the mechanism)."
    ///
    /// # Errors
    ///
    /// [`GuardianServiceError::InvalidGuardianCount`] if `guardians` has
    /// fewer than two entries or more than 255. Whatever
    /// [`cabal_guardian::split`] rejects about `k` otherwise.
    pub fn owner_prepare_enrollment(
        &mut self,
        vault_key: &DataKey,
        guardians: &[GuardianPublicKey],
        k: u8,
        backup_plaintext: &[u8],
        has_value: bool,
    ) -> Result<Vec<(GuardianPublicKey, GuardianMessage)>, GuardianServiceError> {
        if guardians.len() < 2 || guardians.len() > usize::from(u8::MAX) {
            return Err(GuardianServiceError::InvalidGuardianCount);
        }
        let n = u8::try_from(guardians.len()).expect("bounds checked above");

        let recovery = self.owner_recovery_key()?;
        let rk = DataKey::generate()?;
        let shares = shamir::split(&rk, k, n)?;
        let secret = RestoreSecret::generate();
        let auth_key = restore::random_key();
        let backup = restore::aead_seal(&secret.backup_key(&rk), BACKUP_AAD, backup_plaintext);
        let backup_seq = self.doc.owner.backup_seq + 1;

        let messages: Vec<(GuardianPublicKey, GuardianMessage)> = guardians
            .iter()
            .zip(shares)
            .map(|(guardian, share)| {
                let payload = EnrollPayload {
                    owner_recovery_public_key: recovery.public_key().to_bytes(),
                    threshold: k,
                    total: n,
                    share: share.to_bytes(),
                    restore_lookup: Some(secret.lookup()),
                    owner_auth_key: Some(auth_key),
                    backup: Some(backup.clone()),
                    backup_seq,
                    has_value: Some(has_value),
                };
                let bytes = serde_json::to_vec(&payload).expect("EnrollPayload always serializes");
                let sealed = sealed::seal(&bytes, guardian);
                (*guardian, GuardianMessage::Enroll(sealed))
            })
            .collect();

        self.doc.owner.guardians = guardians
            .iter()
            .map(|g| GuardianRecord { public_key: g.to_bytes(), enrolled_at: Utc::now() })
            .collect();
        self.doc.owner.threshold = k;
        self.doc.owner.restore_secret = Some(secret.to_bytes());
        self.doc.owner.owner_auth_key = Some(auth_key);
        self.doc.owner.backup_seq = backup_seq;
        self.wrap_recovery_key(&rk, vault_key);
        self.attached_key = Some(vault_key.clone());
        self.save_owner()?;

        Ok(messages)
    }

    /// The restore code to show the person, once enrolled.
    #[must_use]
    pub fn owner_restore_code(&self) -> Option<String> {
        self.doc.owner.restore_secret.map(|s| RestoreSecret::from_bytes(s).to_code())
    }

    /// Turns a key reconstructed from unlock replies into the vault key:
    /// opens the RK wrapping, or — for a pre-RK enrollment, whose shares
    /// were of the vault key itself — passes it through. Still a candidate:
    /// only opening the vault proves it.
    #[must_use]
    pub fn owner_vault_key_from_recovery(&self, reconstructed: DataKey) -> Option<DataKey> {
        match &self.doc.owner.vk_under_rk {
            None => Some(reconstructed),
            Some(wrapped) => {
                let bytes =
                    restore::aead_open(&reconstructed.derive(VK_UNDER_RK_LABEL).expose_for_storage(), b"", wrapped).ok()?;
                Some(DataKey::from_bytes(bytes.try_into().ok()?))
            }
        }
    }

    /// A broadcast refreshing every guardian's backup copy and value flag.
    /// `None` when not enrolled with restore support or while locked (RK is
    /// unreachable without the vault key).
    pub fn owner_build_backup_update(
        &mut self,
        backup_plaintext: &[u8],
        has_value: bool,
    ) -> Result<Option<GuardianMessage>, GuardianServiceError> {
        let (Some(vault_key), Some(secret), Some(auth_key)) =
            (self.attached_key.clone(), self.doc.owner.restore_secret, self.doc.owner.owner_auth_key)
        else {
            return Ok(None);
        };
        let Some(rk) = self.recovery_key_with(&vault_key) else { return Ok(None) };
        let Some(recovery_bytes) = self.doc.owner.recovery_secret_key else { return Ok(None) };
        let owner_public = GuardianSecretKey::from_bytes(recovery_bytes).public_key().to_bytes();

        let secret = RestoreSecret::from_bytes(secret);
        let backup = restore::aead_seal(&secret.backup_key(&rk), BACKUP_AAD, backup_plaintext);
        let seq = self.doc.owner.backup_seq + 1;
        let mac = restore::mac(&auth_key, BACKUP_MAC_DOMAIN, &[&backup, &seq.to_be_bytes(), &[u8::from(has_value)]]);
        let payload = serde_json::to_vec(&BackupPayload { owner_recovery_public_key: owner_public, backup, seq, has_value, mac })?;

        let mut nonce = [0_u8; TAG_LEN];
        OsRng.fill_bytes(&mut nonce);
        let entries = self
            .doc
            .owner
            .guardians
            .iter()
            .map(|g| {
                let key = GuardianPublicKey::from_bytes(g.public_key);
                BackupEntry { tag: recognition_tag(&key, &nonce), sealed: sealed::seal(&payload, &key).to_bytes() }
            })
            .collect();

        self.doc.owner.backup_seq = seq;
        self.save_owner()?;
        Ok(Some(GuardianMessage::BackupUpdate(BackupUpdate { nonce, entries })))
    }

    /// Whether a guardian's [`RestorePending`] broadcast is about *this*
    /// device's vault. Returns when the restore would complete.
    #[must_use]
    pub fn owner_match_pending(&self, pending: &RestorePending) -> Option<DateTime<Utc>> {
        let auth_key = self.doc.owner.owner_auth_key?;
        (restore::pending_tag(&auth_key, &pending.nonce) == pending.tag)
            .then(|| DateTime::from_timestamp(pending.release_at, 0))
            .flatten()
    }

    /// The veto: a broadcast cancelling every pending restore of this vault.
    ///
    /// # Errors
    ///
    /// [`GuardianServiceError::NotEnrolled`] without an enrollment that
    /// supports restore.
    pub fn owner_build_cancel(&self) -> Result<GuardianMessage, GuardianServiceError> {
        let auth_key = self.doc.owner.owner_auth_key.ok_or(GuardianServiceError::NotEnrolled)?;
        let mut nonce = [0_u8; TAG_LEN];
        OsRng.fill_bytes(&mut nonce);
        let mac = restore::mac(&auth_key, CANCEL_MAC_DOMAIN, &[&nonce]);
        Ok(GuardianMessage::RestoreCancel(RestoreCancel { nonce, mac }))
    }

    /// Whether this device has completed enrollment at least once.
    #[must_use]
    pub fn is_enrolled(&self) -> bool {
        self.doc.owner.recovery_secret_key.is_some() && !self.doc.owner.guardians.is_empty()
    }

    /// How many guardians are enrolled, and how many are needed to unlock.
    #[must_use]
    pub fn owner_guardian_status(&self) -> (usize, u8) {
        (self.doc.owner.guardians.len(), self.doc.owner.threshold)
    }

    /// Builds a fresh unlock request: one unlinkable tag per enrolled
    /// guardian, under a nonce that will never be reused.
    ///
    /// # Errors
    ///
    /// [`GuardianServiceError::NotEnrolled`] if no guardians are enrolled.
    pub fn owner_build_unlock_request(&self) -> Result<UnlockRequest, GuardianServiceError> {
        if self.doc.owner.guardians.is_empty() {
            return Err(GuardianServiceError::NotEnrolled);
        }
        let mut nonce = [0_u8; 16];
        OsRng.fill_bytes(&mut nonce);
        let tags = self
            .doc
            .owner
            .guardians
            .iter()
            .map(|g| recognition_tag(&GuardianPublicKey::from_bytes(g.public_key), &nonce))
            .collect();
        Ok(UnlockRequest { nonce, tags })
    }

    /// Opens a guardian's unlock reply and extracts the share inside.
    ///
    /// Returns `None` (not an error) when `sealed` was not sealed to this
    /// device's recovery key — expected whenever a reply on the mesh was
    /// meant for a different owner this device also happens to guard for.
    /// See the `protocol` module docs for why that is a normal, harmless
    /// occurrence rather than a sign anything went wrong.
    #[must_use]
    pub fn owner_open_unlock_reply(&self, sealed_bytes: &Sealed) -> Option<Share> {
        let recovery_bytes = self.doc.owner.recovery_secret_key?;
        let recovery = GuardianSecretKey::from_bytes(recovery_bytes);
        let opened = sealed::open(sealed_bytes, &recovery).ok()?;
        let payload: UnlockReplyPayload = serde_json::from_slice(&opened).ok()?;
        Share::from_bytes(&payload.share).ok()
    }

    /// The threshold this device's own vault needs to reconstruct.
    #[must_use]
    pub fn owner_threshold(&self) -> u8 {
        self.doc.owner.threshold
    }

    // ---------------------------------------------------------------
    // Guardian side: shares this device holds for other people.
    // ---------------------------------------------------------------

    /// This device's own guardian key pair, generating one the first time it
    /// is asked to hold a share.
    fn guardian_key(&mut self) -> Result<GuardianSecretKey, GuardianServiceError> {
        if self.held_vault.is_none() {
            return Err(GuardianServiceError::Locked);
        }
        if self.doc.held.secret_key.is_none() {
            let key = GuardianSecretKey::generate();
            self.doc.held.secret_key = Some(key.to_bytes());
            self.save_held()?;
        }
        Ok(GuardianSecretKey::from_bytes(self.doc.held.secret_key.expect("just set")))
    }

    /// The public key to hand back in an [`EnrollAccept`].
    ///
    /// # Errors
    ///
    /// A store failure the first time a key pair has to be generated.
    pub fn guardian_public_key(&mut self) -> Result<GuardianPublicKey, GuardianServiceError> {
        Ok(self.guardian_key()?.public_key())
    }

    /// Opens and stores a share sealed to this device.
    ///
    /// # Errors
    ///
    /// [`GuardianServiceError::Sealed`] if `sealed` was not sealed to this
    /// device's guardian key. [`GuardianServiceError::Malformed`] if the
    /// opened payload is not a well-formed [`EnrollPayload`].
    pub fn guardian_receive_enroll(&mut self, sealed_bytes: &Sealed) -> Result<(), GuardianServiceError> {
        let key = self.guardian_key()?;
        let opened = sealed::open(sealed_bytes, &key)?;
        let payload: EnrollPayload = serde_json::from_slice(&opened)?;

        // A re-enrollment by the same owner replaces the old share: it
        // belongs to a recovery key that no longer exists.
        self.doc.held.held.retain(|h| h.owner_recovery_public_key != payload.owner_recovery_public_key);
        self.doc.held.held.push(HeldShare {
            owner_recovery_public_key: payload.owner_recovery_public_key,
            share: payload.share,
            enrolled_at: Utc::now(),
            threshold: payload.threshold,
            restore_lookup: payload.restore_lookup,
            owner_auth_key: payload.owner_auth_key,
            backup: payload.backup,
            backup_seq: payload.backup_seq,
            has_value: payload.has_value,
        });
        self.save_held()?;
        Ok(())
    }

    /// Every owner this device holds a share for, and when.
    #[must_use]
    pub fn held_for(&self) -> Vec<([u8; 32], DateTime<Utc>)> {
        self.doc.held.held.iter().map(|h| (h.owner_recovery_public_key, h.enrolled_at)).collect()
    }

    /// Checks an incoming unlock request against this device's own tag and,
    /// if it matches, returns one sealed reply per owner this device holds a
    /// share for.
    ///
    /// Deliberately not narrower than "every held share": the tag proves
    /// only that *some* owner who has this device as a guardian is asking
    /// right now, never *which* one — see the `protocol` module docs. Each
    /// reply is sealed to its own owner's recovery key, so a reply meant for
    /// someone else is harmless ciphertext to everyone but them.
    #[must_use]
    pub fn guardian_match_unlock_request(&self, request: &UnlockRequest) -> Vec<GuardianMessage> {
        if self.held_vault.is_none() {
            return Vec::new();
        }
        let Some(secret_bytes) = self.doc.held.secret_key else {
            return Vec::new();
        };
        let public = GuardianSecretKey::from_bytes(secret_bytes).public_key();
        let my_tag = recognition_tag(&public, &request.nonce);
        if !request.tags.contains(&my_tag) {
            return Vec::new();
        }

        self.doc
            .held
            .held
            .iter()
            .map(|held| {
                let payload = UnlockReplyPayload { share: held.share.clone() };
                let bytes = serde_json::to_vec(&payload).expect("UnlockReplyPayload always serializes");
                let owner_key = GuardianPublicKey::from_bytes(held.owner_recovery_public_key);
                GuardianMessage::UnlockReply(sealed::seal(&bytes, &owner_key))
            })
            .collect()
    }
}

// -------------------------------------------------------------------
// Guardian side: backups and restores for other people.
// -------------------------------------------------------------------

/// What a guardian should do about an incoming [`RestoreRequest`].
#[derive(Debug)]
pub enum RestoreMatch {
    /// Not for any owner this device guards.
    NotMine,
    /// New: a human must approve before anything is sent.
    NeedsApproval { enrolled_at: DateTime<Utc> },
    /// Already approved earlier; send this reply without asking again.
    Reply(GuardianMessage),
}

impl GuardianService {
    fn my_guardian_public_key(&self) -> Option<GuardianPublicKey> {
        self.held_vault.as_ref()?;
        Some(GuardianSecretKey::from_bytes(self.doc.held.secret_key?).public_key())
    }

    /// Applies a [`BackupUpdate`] if it carries an entry for this device
    /// from an owner it guards, authenticated by that owner and newer than
    /// what is held. Returns whether anything changed.
    pub fn guardian_receive_backup_update(&mut self, update: &BackupUpdate) -> Result<bool, GuardianServiceError> {
        let Some(public) = self.my_guardian_public_key() else { return Ok(false) };
        let my_tag = recognition_tag(&public, &update.nonce);
        let Some(entry) = update.entries.iter().find(|e| e.tag == my_tag) else { return Ok(false) };
        let secret = GuardianSecretKey::from_bytes(self.doc.held.secret_key.expect("checked above"));
        let Ok(opened) = sealed::open(&Sealed::from_bytes(&entry.sealed)?, &secret) else { return Ok(false) };
        let payload: BackupPayload = serde_json::from_slice(&opened)?;

        let Some(held) = self
            .doc
            .held
            .held
            .iter_mut()
            .find(|h| h.owner_recovery_public_key == payload.owner_recovery_public_key)
        else {
            return Ok(false);
        };
        let Some(auth_key) = held.owner_auth_key else { return Ok(false) };
        let authentic = restore::verify_mac(
            &auth_key,
            BACKUP_MAC_DOMAIN,
            &[&payload.backup, &payload.seq.to_be_bytes(), &[u8::from(payload.has_value)]],
            &payload.mac,
        );
        if !authentic || payload.seq <= held.backup_seq {
            return Ok(false);
        }
        held.backup = Some(payload.backup);
        held.backup_seq = payload.seq;
        held.has_value = Some(payload.has_value);
        self.save_held()?;
        Ok(true)
    }

    fn held_for_restore(&self, request: &RestoreRequest) -> Option<&HeldShare> {
        self.held_vault.as_ref()?;
        self.doc.held.held.iter().find(|h| {
            h.restore_lookup.is_some_and(|lookup| restore::restore_tag(&lookup, &request.nonce) == request.tag)
        })
    }

    /// Classifies an incoming [`RestoreRequest`]. See [`RestoreMatch`].
    #[must_use]
    pub fn guardian_match_restore_request(&self, request: &RestoreRequest, now: DateTime<Utc>) -> RestoreMatch {
        let Some(held) = self.held_for_restore(request) else { return RestoreMatch::NotMine };
        match self.find_pending(held, request) {
            Some(pending) => RestoreMatch::Reply(self.restore_reply(held, pending, request, now)),
            None => RestoreMatch::NeedsApproval { enrolled_at: held.enrolled_at },
        }
    }

    fn find_pending(&self, held: &HeldShare, request: &RestoreRequest) -> Option<&PendingRestore> {
        self.doc.held.pending.iter().find(|p| {
            p.owner_recovery_public_key == held.owner_recovery_public_key
                && p.nonce == request.nonce
                && p.reply_public_key == request.reply_public_key
        })
    }

    fn restore_reply(&self, held: &HeldShare, pending: &PendingRestore, request: &RestoreRequest, now: DateTime<Utc>) -> GuardianMessage {
        let status = if pending.cancelled {
            RestoreStatus::Cancelled
        } else if now >= pending.release_at {
            RestoreStatus::Released {
                share: held.share.clone(),
                threshold: held.threshold,
                backup: held.backup.clone().unwrap_or_default(),
                backup_seq: held.backup_seq,
            }
        } else {
            RestoreStatus::Waiting { release_at: pending.release_at.timestamp() }
        };
        let bytes = serde_json::to_vec(&RestoreReplyPayload { status }).expect("RestoreReplyPayload always serializes");
        GuardianMessage::RestoreReply(sealed::seal(&bytes, &GuardianPublicKey::from_bytes(request.reply_public_key)))
    }

    /// A human approved `request`: start its delay — or skip it when the
    /// owner last reported an empty vault — and return the reply to send.
    ///
    /// # Errors
    ///
    /// [`GuardianServiceError::Malformed`] if `request` is no longer one
    /// this device recognises; a store failure persisting the approval.
    pub fn guardian_approve_restore(
        &mut self,
        request: &RestoreRequest,
        now: DateTime<Utc>,
    ) -> Result<GuardianMessage, GuardianServiceError> {
        let held = self.held_for_restore(request).ok_or(GuardianServiceError::Malformed)?.clone();
        if self.find_pending(&held, request).is_none() {
            let delay = if held.has_value == Some(false) { Duration::zero() } else { Duration::hours(RESTORE_DELAY_HOURS) };
            self.doc.held.pending.push(PendingRestore {
                owner_recovery_public_key: held.owner_recovery_public_key,
                nonce: request.nonce,
                reply_public_key: request.reply_public_key,
                release_at: now + delay,
                cancelled: false,
            });
            self.prune_pending(now);
            self.save_held()?;
        }
        let pending = self.find_pending(&held, request).expect("just inserted");
        Ok(self.restore_reply(&held, pending, request, now))
    }

    /// Broadcasts announcing every restore still inside its delay, for the
    /// original device to notice. Fresh nonces each time, so repeated
    /// announcements are unlinkable to anyone but that device.
    #[must_use]
    pub fn guardian_pending_announcements(&self, now: DateTime<Utc>) -> Vec<GuardianMessage> {
        if self.held_vault.is_none() {
            return Vec::new();
        }
        self.doc
            .held
            .pending
            .iter()
            .filter(|p| !p.cancelled && p.release_at > now)
            .filter_map(|p| {
                let held = self.doc.held.held.iter().find(|h| h.owner_recovery_public_key == p.owner_recovery_public_key)?;
                let auth_key = held.owner_auth_key?;
                let mut nonce = [0_u8; TAG_LEN];
                OsRng.fill_bytes(&mut nonce);
                Some(GuardianMessage::RestorePending(RestorePending {
                    nonce,
                    tag: restore::pending_tag(&auth_key, &nonce),
                    release_at: p.release_at.timestamp(),
                }))
            })
            .collect()
    }

    /// Applies a [`RestoreCancel`]: if it authenticates as an owner this
    /// device guards, every pending restore of that owner's vault is
    /// cancelled. Returns how many were.
    pub fn guardian_receive_cancel(&mut self, cancel: &RestoreCancel) -> Result<usize, GuardianServiceError> {
        if self.held_vault.is_none() {
            return Ok(0);
        }
        let owners: Vec<[u8; 32]> = self
            .doc
            .held
            .held
            .iter()
            .filter(|h| {
                h.owner_auth_key
                    .is_some_and(|key| restore::verify_mac(&key, CANCEL_MAC_DOMAIN, &[&cancel.nonce], &cancel.mac))
            })
            .map(|h| h.owner_recovery_public_key)
            .collect();
        let mut count = 0;
        for pending in &mut self.doc.held.pending {
            if !pending.cancelled && owners.contains(&pending.owner_recovery_public_key) {
                pending.cancelled = true;
                count += 1;
            }
        }
        if count > 0 {
            self.save_held()?;
        }
        Ok(count)
    }

    fn prune_pending(&mut self, now: DateTime<Utc>) {
        let horizon = now - Duration::days(PENDING_RETENTION_DAYS);
        self.doc.held.pending.retain(|p| p.release_at > horizon);
    }
}

// -------------------------------------------------------------------
// New device: restoring from guardians.
// -------------------------------------------------------------------

/// One restore attempt on a new device. Persisted between app launches
/// (see [`RestoreSession::to_bytes`]) because guardians bind their approval
/// to this exact nonce and reply key — a fresh session would need fresh
/// approvals and restart the delay.
#[derive(Clone, Serialize, Deserialize)]
pub struct RestoreSession {
    secret: [u8; 32],
    reply_secret_key: [u8; 32],
    nonce: [u8; TAG_LEN],
}

/// A released share with its guardian's backup copy.
#[derive(Clone)]
pub struct ReleasedShare {
    pub share: Share,
    pub threshold: u8,
    pub backup: Vec<u8>,
    pub backup_seq: u64,
}

impl RestoreSession {
    /// # Errors
    ///
    /// [`GuardianServiceError::BadRestoreCode`] if `code` does not parse.
    pub fn new(code: &str) -> Result<Self, GuardianServiceError> {
        let secret = RestoreSecret::from_code(code)?;
        let mut nonce = [0_u8; TAG_LEN];
        OsRng.fill_bytes(&mut nonce);
        Ok(Self { secret: secret.to_bytes(), reply_secret_key: GuardianSecretKey::generate().to_bytes(), nonce })
    }

    #[must_use]
    pub fn request(&self) -> GuardianMessage {
        let secret = RestoreSecret::from_bytes(self.secret);
        GuardianMessage::RestoreRequest(RestoreRequest {
            nonce: self.nonce,
            tag: restore::restore_tag(&secret.lookup(), &self.nonce),
            reply_public_key: GuardianSecretKey::from_bytes(self.reply_secret_key).public_key().to_bytes(),
        })
    }

    /// Opens a [`GuardianMessage::RestoreReply`] meant for this session.
    #[must_use]
    pub fn open_reply(&self, sealed_bytes: &Sealed) -> Option<RestoreStatus> {
        let opened = sealed::open(sealed_bytes, &GuardianSecretKey::from_bytes(self.reply_secret_key)).ok()?;
        Some(serde_json::from_slice::<RestoreReplyPayload>(&opened).ok()?.status)
    }

    /// Reconstructs RK from `released` and opens the newest backup that
    /// decrypts, returning the vault's contents.
    ///
    /// # Errors
    ///
    /// [`GuardianServiceError::RestoreIncomplete`] when there are too few
    /// shares or nothing opens.
    pub fn finish(&self, released: &[ReleasedShare]) -> Result<Vec<u8>, GuardianServiceError> {
        let threshold = released.iter().map(|r| r.threshold).max().unwrap_or(0);
        if threshold == 0 || released.len() < usize::from(threshold) {
            return Err(GuardianServiceError::RestoreIncomplete);
        }
        let shares: Vec<Share> = released.iter().map(|r| r.share.clone()).collect();
        let rk = shamir::reconstruct(threshold, &shares).map_err(|_| GuardianServiceError::RestoreIncomplete)?;
        let key = RestoreSecret::from_bytes(self.secret).backup_key(&rk);

        let mut backups: Vec<&ReleasedShare> = released.iter().collect();
        backups.sort_by(|a, b| b.backup_seq.cmp(&a.backup_seq));
        backups
            .into_iter()
            .find_map(|r| restore::aead_open(&key, BACKUP_AAD, &r.backup).ok())
            .ok_or(GuardianServiceError::RestoreIncomplete)
    }

    #[must_use]
    pub fn to_bytes(&self) -> Vec<u8> {
        serde_json::to_vec(self).expect("RestoreSession always serializes")
    }

    /// # Errors
    ///
    /// [`GuardianServiceError::Malformed`] on anything unparseable.
    pub fn from_bytes(bytes: &[u8]) -> Result<Self, GuardianServiceError> {
        Ok(serde_json::from_slice(bytes)?)
    }
}

/// Replies to an [`EnrollRequest`] with this device's guardian public key,
/// generating one first if needed.
///
/// # Errors
///
/// A store failure generating the key.
pub fn accept_enrollment(service: &mut GuardianService, _request: &EnrollRequest) -> Result<EnrollAccept, GuardianServiceError> {
    Ok(EnrollAccept { guardian_public_key: service.guardian_public_key()?.to_bytes() })
}

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::TempDir;

    const BACKUP: &[u8] = b"[\"the vault's contents\"]";

    /// A device: its own temp directory (kept alive for the test's
    /// duration) plus the service opened on it.
    struct Device {
        _dir: TempDir,
        service: GuardianService,
    }

    fn device() -> Device {
        let dir = TempDir::new().unwrap();
        let mut service = GuardianService::open(dir.path());
        service.attach_vault_key(&DataKey::from_bytes([0xAB; 32])).unwrap();
        Device { _dir: dir, service }
    }

    fn enroll(owner: &mut Device, guardians: &mut [Device], vault_key: &DataKey, k: u8) {
        let public_keys: Vec<GuardianPublicKey> =
            guardians.iter_mut().map(|g| g.service.guardian_public_key().unwrap()).collect();
        let messages = owner.service.owner_prepare_enrollment(vault_key, &public_keys, k, BACKUP, true).unwrap();

        for ((_, message), guardian) in messages.into_iter().zip(guardians.iter_mut()) {
            let GuardianMessage::Enroll(sealed) = message else { panic!("expected Enroll") };
            guardian.service.guardian_receive_enroll(&sealed).unwrap();
        }
    }

    #[test]
    fn enrolling_persists_guardians_and_threshold_but_never_shares() {
        let mut owner = device();
        let mut guardians: Vec<Device> = (0..5).map(|_| device()).collect();
        let vault_key = DataKey::from_bytes([7; 32]);

        enroll(&mut owner, &mut guardians, &vault_key, 3);

        assert!(owner.service.is_enrolled());
        assert_eq!(owner.service.owner_guardian_status(), (5, 3));
        for guardian in &guardians {
            assert_eq!(guardian.service.held_for().len(), 1, "each guardian should hold exactly one share");
        }
    }

    #[test]
    fn a_full_unlock_round_trip_reconstructs_the_original_key() {
        let mut owner = device();
        let mut guardians: Vec<Device> = (0..5).map(|_| device()).collect();
        let vault_key = DataKey::from_bytes([42; 32]);

        enroll(&mut owner, &mut guardians, &vault_key, 3);

        // A later "session": only 3 of the 5 guardians are reachable.
        let request = owner.service.owner_build_unlock_request().unwrap();
        let mut collected = Vec::new();
        for guardian in guardians.iter().take(3) {
            for reply in guardian.service.guardian_match_unlock_request(&request) {
                let GuardianMessage::UnlockReply(sealed) = reply else { panic!("expected UnlockReply") };
                if let Some(share) = owner.service.owner_open_unlock_reply(&sealed) {
                    collected.push(share);
                }
            }
        }

        assert_eq!(collected.len(), 3);
        let reconstructed = shamir::reconstruct(owner.service.owner_threshold(), &collected).unwrap();
        let opened = owner.service.owner_vault_key_from_recovery(reconstructed).unwrap();
        assert_eq!(opened.expose_for_storage(), vault_key.expose_for_storage());
    }

    #[test]
    fn unlock_fails_gracefully_below_threshold() {
        let mut owner = device();
        let mut guardians: Vec<Device> = (0..5).map(|_| device()).collect();
        let vault_key = DataKey::from_bytes([5; 32]);

        enroll(&mut owner, &mut guardians, &vault_key, 3);

        let request = owner.service.owner_build_unlock_request().unwrap();
        let mut collected = Vec::new();
        for guardian in guardians.iter().take(2) {
            for reply in guardian.service.guardian_match_unlock_request(&request) {
                let GuardianMessage::UnlockReply(sealed) = reply else { panic!("expected UnlockReply") };
                if let Some(share) = owner.service.owner_open_unlock_reply(&sealed) {
                    collected.push(share);
                }
            }
        }

        assert_eq!(collected.len(), 2, "two of five guardians should have answered");
        assert!(shamir::reconstruct(owner.service.owner_threshold(), &collected).is_err());
    }

    #[test]
    fn a_guardian_not_addressed_by_the_request_does_not_reply() {
        let mut owner = device();
        let mut guardians: Vec<Device> = (0..2).map(|_| device()).collect();
        let bystander = device(); // never enrolled by anyone
        let vault_key = DataKey::from_bytes([1; 32]);

        enroll(&mut owner, &mut guardians, &vault_key, 2);

        let request = owner.service.owner_build_unlock_request().unwrap();
        assert!(bystander.service.guardian_match_unlock_request(&request).is_empty());
    }

    #[test]
    fn a_guardian_serving_two_owners_answers_each_correctly_and_only_each() {
        let mut owner_a = device();
        let mut owner_b = device();
        let mut shared = device();
        let mut only_a = device();
        let mut only_b = device();

        let key_a = DataKey::from_bytes([11; 32]);
        let key_b = DataKey::from_bytes([22; 32]);

        // `shared` ends up in both groups — built manually since `enroll`
        // takes one guardian slice per owner and this needs the same device
        // in two different slices.
        let shared_key = shared.service.guardian_public_key().unwrap();
        let only_a_key = only_a.service.guardian_public_key().unwrap();
        let only_b_key = only_b.service.guardian_public_key().unwrap();

        for (_, message) in owner_a.service.owner_prepare_enrollment(&key_a, &[shared_key, only_a_key], 2, BACKUP, true).unwrap() {
            let GuardianMessage::Enroll(sealed) = message else { panic!("expected Enroll") };
            // Route by trying to open with each candidate recipient in turn
            // would be the network's job; here we know statically who each
            // message was sealed to.
            if sealed::open(&sealed, &GuardianSecretKey::from_bytes(guardian_key_bytes(&shared))).is_ok() {
                shared.service.guardian_receive_enroll(&sealed).unwrap();
            } else {
                only_a.service.guardian_receive_enroll(&sealed).unwrap();
            }
        }
        for (_, message) in owner_b.service.owner_prepare_enrollment(&key_b, &[shared_key, only_b_key], 2, BACKUP, true).unwrap() {
            let GuardianMessage::Enroll(sealed) = message else { panic!("expected Enroll") };
            if sealed::open(&sealed, &GuardianSecretKey::from_bytes(guardian_key_bytes(&shared))).is_ok() {
                shared.service.guardian_receive_enroll(&sealed).unwrap();
            } else {
                only_b.service.guardian_receive_enroll(&sealed).unwrap();
            }
        }

        assert_eq!(shared.service.held_for().len(), 2, "shared guardian should hold one share per owner");

        // The shared guardian cannot tell which owner's request this is (see
        // `guardian_match_unlock_request`'s docs), so it answers with one
        // reply per owner it holds a share for — both sealed, each opening
        // only for its own owner. In the real system only owner A's device
        // ever receives either reply at all, because both are sent
        // *directed* back to whoever asked (proved separately by
        // `a_directed_send_reaches_only_its_recipient_even_through_a_relay`
        // in `tests/ble_loopback.rs`); this layer has no network, so what it
        // can prove is the sealing half of that guarantee: owner A opens
        // exactly its own share and nothing decrypts as owner B's.
        let request_a = owner_a.service.owner_build_unlock_request().unwrap();
        let replies = shared.service.guardian_match_unlock_request(&request_a);
        assert_eq!(replies.len(), 2, "the shared guardian should answer for both owners it holds a share for");

        let mut a_shares = Vec::new();
        for reply in replies {
            let GuardianMessage::UnlockReply(sealed) = reply else { panic!("expected UnlockReply") };
            if let Some(share) = owner_a.service.owner_open_unlock_reply(&sealed) {
                a_shares.push(share);
            }
        }
        assert_eq!(a_shares.len(), 1);
    }

    /// Test-only accessor: the raw bytes behind a device's guardian key, so
    /// the cross-owner test above can decide which sealed message belongs to
    /// which device without a real transport to route it.
    fn guardian_key_bytes(device: &Device) -> [u8; 32] {
        device.service.doc.held.secret_key.expect("guardian_public_key should have generated one")
    }

    #[test]
    fn requesting_unlock_before_enrollment_is_refused() {
        let owner = device();
        assert!(matches!(owner.service.owner_build_unlock_request(), Err(GuardianServiceError::NotEnrolled)));
    }

    #[test]
    fn enrolling_with_fewer_than_two_guardians_is_refused() {
        let mut owner = device();
        let mut only_one = device();
        let vault_key = DataKey::from_bytes([1; 32]);
        let key = only_one.service.guardian_public_key().unwrap();

        assert!(matches!(
            owner.service.owner_prepare_enrollment(&vault_key, &[key], 2, BACKUP, true),
            Err(GuardianServiceError::InvalidGuardianCount)
        ));
    }

    #[test]
    fn accepting_an_enrollment_request_generates_a_stable_key() {
        let mut candidate = device();
        let first = accept_enrollment(&mut candidate.service, &EnrollRequest).unwrap();
        let second = accept_enrollment(&mut candidate.service, &EnrollRequest).unwrap();
        assert_eq!(first.guardian_public_key, second.guardian_public_key);
    }

    #[test]
    fn the_guardian_store_survives_being_reopened() {
        let dir = TempDir::new().unwrap();
        let mut owner = GuardianService::open(dir.path());
        let mut guardians: Vec<Device> = (0..3).map(|_| device()).collect();
        let vault_key = DataKey::from_bytes([3; 32]);

        let public_keys: Vec<GuardianPublicKey> =
            guardians.iter_mut().map(|g| g.service.guardian_public_key().unwrap()).collect();
        owner.owner_prepare_enrollment(&vault_key, &public_keys, 2, BACKUP, true).unwrap();
        drop(owner);

        let reopened = GuardianService::open(dir.path());
        assert!(reopened.is_enrolled());
        assert_eq!(reopened.owner_guardian_status(), (3, 2));
    }

    #[test]
    fn held_shares_are_unreadable_until_the_vault_key_is_attached() {
        let dir = TempDir::new().unwrap();
        let identity_key = DataKey::from_bytes([9; 32]);
        let mut owner = device();
        {
            let mut guardian = Device { _dir: TempDir::new().unwrap(), service: GuardianService::open(dir.path()) };
            assert!(matches!(guardian.service.guardian_public_key(), Err(GuardianServiceError::Locked)));
            guardian.service.attach_vault_key(&identity_key).unwrap();
            let mut guardians = vec![guardian, device()];
            enroll(&mut owner, &mut guardians, &DataKey::from_bytes([1; 32]), 2);
        }

        // Restart: locked until attached, and answers nobody meanwhile.
        let mut reopened = GuardianService::open(dir.path());
        let request = owner.service.owner_build_unlock_request().unwrap();
        assert!(reopened.guardian_match_unlock_request(&request).is_empty());
        assert!(!reopened.is_held_unlocked());

        // The wrong key (a wrong passphrase's vault key) opens nothing.
        assert!(reopened.attach_vault_key(&DataKey::from_bytes([8; 32])).is_err());
        reopened.attach_vault_key(&identity_key).unwrap();
        assert_eq!(reopened.guardian_match_unlock_request(&request).len(), 1);
    }

    #[test]
    fn re_keying_follows_the_vault_key_across_a_restart() {
        let dir = TempDir::new().unwrap();
        let mut owner = device();
        let mut guardian = Device { _dir: TempDir::new().unwrap(), service: GuardianService::open(dir.path()) };
        guardian.service.attach_vault_key(&DataKey::from_bytes([1; 32])).unwrap();
        let mut guardians = vec![guardian, device()];
        enroll(&mut owner, &mut guardians, &DataKey::from_bytes([2; 32]), 2);

        // Passphrase turned on: the vault key changes.
        let new_key = DataKey::from_bytes([3; 32]);
        guardians[0].service.attach_vault_key(&new_key).unwrap();
        drop(guardians);

        let mut reopened = GuardianService::open(dir.path());
        assert!(reopened.attach_vault_key(&DataKey::from_bytes([1; 32])).is_err(), "old key must no longer open it");
        reopened.attach_vault_key(&new_key).unwrap();
        assert_eq!(reopened.held_for().len(), 1);
    }

    #[test]
    fn a_pre_split_store_migrates_its_held_shares_out_of_the_owner_file() {
        let dir = TempDir::new().unwrap();
        let legacy = GuardianDocument {
            owner: OwnerState::default(),
            held: HeldState {
                secret_key: Some([4; 32]),
                held: vec![HeldShare {
                    owner_recovery_public_key: [5; 32],
                    share: vec![1, 2, 3],
                    enrolled_at: Utc::now(),
                    threshold: 2,
                    restore_lookup: None,
                    owner_auth_key: None,
                    backup: None,
                    backup_seq: 0,
                    has_value: None,
                }],
                pending: Vec::new(),
            },
        };
        Vault::new(dir.path().join("guardian.enc"), crate::vault_key::platform_provider(dir.path().join("guardian.key")))
            .save(&legacy)
            .unwrap();

        let key = DataKey::from_bytes([6; 32]);
        let mut service = GuardianService::open(dir.path());
        service.attach_vault_key(&key).unwrap();
        assert_eq!(service.held_for().len(), 1);
        drop(service);

        let owner_file: GuardianDocument =
            Vault::new(dir.path().join("guardian.enc"), crate::vault_key::platform_provider(dir.path().join("guardian.key")))
                .load()
                .unwrap();
        assert!(owner_file.held.held.is_empty() && owner_file.held.secret_key.is_none(), "shares must leave the file-key store");

        let mut reopened = GuardianService::open(dir.path());
        reopened.attach_vault_key(&key).unwrap();
        assert_eq!(reopened.held_for().len(), 1);
    }

    // ---------------------------------------------------------------
    // RK, restore, delay, veto.
    // ---------------------------------------------------------------

    fn unlock_with(owner: &Device, guardians: &[Device]) -> Option<DataKey> {
        let request = owner.service.owner_build_unlock_request().unwrap();
        let shares: Vec<Share> = guardians
            .iter()
            .flat_map(|g| g.service.guardian_match_unlock_request(&request))
            .filter_map(|reply| match reply {
                GuardianMessage::UnlockReply(sealed) => owner.service.owner_open_unlock_reply(&sealed),
                _ => None,
            })
            .collect();
        let rk = shamir::reconstruct(owner.service.owner_threshold(), &shares).ok()?;
        owner.service.owner_vault_key_from_recovery(rk)
    }

    #[test]
    fn shares_survive_a_vault_key_change() {
        // Turning a passphrase on changes the vault key. Shares are of RK,
        // so they keep working — the bug this replaced split the vault key
        // itself and silently orphaned every share on that toggle.
        let mut owner = device();
        let mut guardians: Vec<Device> = (0..3).map(|_| device()).collect();
        let original = DataKey::from_bytes([0xAB; 32]); // what `device()` attached
        enroll(&mut owner, &mut guardians, &original, 2);

        let changed = DataKey::from_bytes([0xCD; 32]);
        owner.service.attach_vault_key(&changed).unwrap();

        let opened = unlock_with(&owner, &guardians[..2]).unwrap();
        assert_eq!(opened.expose_for_storage(), changed.expose_for_storage());
    }

    struct Restore {
        owner: Device,
        guardians: Vec<Device>,
        code: String,
    }

    fn enrolled_for_restore(has_value: bool) -> Restore {
        let mut owner = device();
        let mut guardians: Vec<Device> = (0..3).map(|_| device()).collect();
        let keys: Vec<GuardianPublicKey> = guardians.iter_mut().map(|g| g.service.guardian_public_key().unwrap()).collect();
        let messages =
            owner.service.owner_prepare_enrollment(&DataKey::from_bytes([0xAB; 32]), &keys, 2, BACKUP, has_value).unwrap();
        for ((_, message), guardian) in messages.into_iter().zip(guardians.iter_mut()) {
            let GuardianMessage::Enroll(sealed) = message else { panic!() };
            guardian.service.guardian_receive_enroll(&sealed).unwrap();
        }
        let code = owner.service.owner_restore_code().unwrap();
        Restore { owner, guardians, code }
    }

    fn as_request(message: &GuardianMessage) -> RestoreRequest {
        let GuardianMessage::RestoreRequest(request) = message else { panic!("expected RestoreRequest") };
        request.clone()
    }

    fn status(session: &RestoreSession, reply: &GuardianMessage) -> RestoreStatus {
        let GuardianMessage::RestoreReply(sealed) = reply else { panic!("expected RestoreReply") };
        session.open_reply(sealed).expect("sealed to this session")
    }

    fn released(status: RestoreStatus) -> ReleasedShare {
        let RestoreStatus::Released { share, threshold, backup, backup_seq } = status else { panic!("expected Released, got {status:?}") };
        ReleasedShare { share: Share::from_bytes(&share).unwrap(), threshold, backup, backup_seq }
    }

    #[test]
    fn a_new_device_restores_after_the_delay() {
        let mut r = enrolled_for_restore(true);
        let session = RestoreSession::new(&r.code).unwrap();
        let request = as_request(&session.request());
        let now = Utc::now();

        // First contact: every guardian needs a human.
        for g in &r.guardians {
            assert!(matches!(g.service.guardian_match_restore_request(&request, now), RestoreMatch::NeedsApproval { .. }));
        }
        // Approved: held for 24h, not released.
        for g in r.guardians.iter_mut().take(2) {
            let reply = g.service.guardian_approve_restore(&request, now).unwrap();
            let RestoreStatus::Waiting { release_at } = status(&session, &reply) else { panic!("should wait") };
            assert_eq!(release_at, (now + Duration::hours(RESTORE_DELAY_HOURS)).timestamp());
        }
        // An hour later, a re-broadcast is answered without asking again — still waiting.
        let RestoreMatch::Reply(reply) = r.guardians[0].service.guardian_match_restore_request(&request, now + Duration::hours(1)) else { panic!() };
        assert!(matches!(status(&session, &reply), RestoreStatus::Waiting { .. }));

        // After the delay: released, and the backup opens.
        let later = now + Duration::hours(RESTORE_DELAY_HOURS) + Duration::seconds(1);
        let shares: Vec<ReleasedShare> = r.guardians[..2]
            .iter()
            .map(|g| {
                let RestoreMatch::Reply(reply) = g.service.guardian_match_restore_request(&request, later) else { panic!() };
                released(status(&session, &reply))
            })
            .collect();
        assert_eq!(session.finish(&shares).unwrap(), BACKUP);
        assert!(session.finish(&shares[..1]).is_err(), "one share is below threshold");
    }

    #[test]
    fn an_empty_vault_skips_the_delay() {
        let mut r = enrolled_for_restore(false);
        let session = RestoreSession::new(&r.code).unwrap();
        let request = as_request(&session.request());
        let reply = r.guardians[0].service.guardian_approve_restore(&request, Utc::now()).unwrap();
        released(status(&session, &reply));
    }

    #[test]
    fn the_original_device_hears_about_a_pending_restore_and_can_veto_it() {
        let mut r = enrolled_for_restore(true);
        let session = RestoreSession::new(&r.code).unwrap();
        let request = as_request(&session.request());
        let now = Utc::now();
        r.guardians[0].service.guardian_approve_restore(&request, now).unwrap();

        // The guardian announces; only the owner recognises it.
        let announcements = r.guardians[0].service.guardian_pending_announcements(now);
        assert_eq!(announcements.len(), 1);
        let GuardianMessage::RestorePending(pending) = &announcements[0] else { panic!() };
        assert!(r.owner.service.owner_match_pending(pending).is_some());
        assert!(device().service.owner_match_pending(pending).is_none(), "a stranger must not recognise it");

        // A forged cancel does nothing; the owner's does.
        let forged = RestoreCancel { nonce: [1; TAG_LEN], mac: [0; 32] };
        assert_eq!(r.guardians[0].service.guardian_receive_cancel(&forged).unwrap(), 0);
        let GuardianMessage::RestoreCancel(cancel) = r.owner.service.owner_build_cancel().unwrap() else { panic!() };
        assert_eq!(r.guardians[0].service.guardian_receive_cancel(&cancel).unwrap(), 1);

        // Even after the delay, the share is never released.
        let later = now + Duration::hours(RESTORE_DELAY_HOURS + 1);
        let RestoreMatch::Reply(reply) = r.guardians[0].service.guardian_match_restore_request(&request, later) else { panic!() };
        assert!(matches!(status(&session, &reply), RestoreStatus::Cancelled));
        assert!(r.guardians[0].service.guardian_pending_announcements(now).is_empty());
    }

    #[test]
    fn a_wrong_code_is_not_recognised_and_a_stale_one_stops_working_after_re_enrollment() {
        let mut r = enrolled_for_restore(true);
        let stranger = RestoreSession::new(&cabal_guardian::restore::RestoreSecret::generate().to_code()).unwrap();
        assert!(matches!(
            r.guardians[0].service.guardian_match_restore_request(&as_request(&stranger.request()), Utc::now()),
            RestoreMatch::NotMine
        ));

        let old_code = r.code.clone();
        let keys: Vec<GuardianPublicKey> = r.guardians.iter_mut().map(|g| g.service.guardian_public_key().unwrap()).collect();
        for ((_, message), guardian) in r
            .owner
            .service
            .owner_prepare_enrollment(&DataKey::from_bytes([0xAB; 32]), &keys, 2, BACKUP, true)
            .unwrap()
            .into_iter()
            .zip(r.guardians.iter_mut())
        {
            let GuardianMessage::Enroll(sealed) = message else { panic!() };
            guardian.service.guardian_receive_enroll(&sealed).unwrap();
        }
        assert_ne!(r.owner.service.owner_restore_code().unwrap(), old_code, "re-enrollment rotates the code");
        assert_eq!(r.guardians[0].service.held_for().len(), 1, "the old share is replaced, not kept beside");
        let stale = RestoreSession::new(&old_code).unwrap();
        assert!(matches!(
            r.guardians[0].service.guardian_match_restore_request(&as_request(&stale.request()), Utc::now()),
            RestoreMatch::NotMine
        ));
    }

    #[test]
    fn a_backup_refresh_reaches_guardians_and_the_newest_copy_wins() {
        let mut r = enrolled_for_restore(true);
        let newer: &[u8] = b"[\"newer contents\"]";
        let update = r.owner.service.owner_build_backup_update(newer, false).unwrap().expect("enrolled and unlocked");
        let GuardianMessage::BackupUpdate(update) = update else { panic!() };
        for g in &mut r.guardians {
            assert!(g.service.guardian_receive_backup_update(&update).unwrap());
            assert!(!g.service.guardian_receive_backup_update(&update).unwrap(), "a replay is not newer");
        }
        assert!(!device().service.guardian_receive_backup_update(&update).unwrap(), "a stranger has no entry");

        // has_value=false arrived with it, so the delay is waived now too.
        let session = RestoreSession::new(&r.code).unwrap();
        let request = as_request(&session.request());
        let shares: Vec<ReleasedShare> = r.guardians[..2]
            .iter_mut()
            .map(|g| released(status(&session, &g.service.guardian_approve_restore(&request, Utc::now()).unwrap())))
            .collect();
        assert_eq!(session.finish(&shares).unwrap(), newer);
    }

    #[test]
    fn a_restore_session_survives_serialization() {
        let r = enrolled_for_restore(true);
        let session = RestoreSession::new(&r.code).unwrap();
        let again = RestoreSession::from_bytes(&session.to_bytes()).unwrap();
        assert_eq!(as_request(&session.request()).tag, as_request(&again.request()).tag);
        assert!(RestoreSession::new("not a code").is_err());
    }
}
