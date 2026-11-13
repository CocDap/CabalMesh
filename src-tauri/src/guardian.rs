//! Guardian enrollment and mesh unlock: persistence and orchestration around
//! `cabal_guardian`'s crypto core.
//!
//! See `docs/identity-design.md` for the design this implements, and the
//! module docs on `cabal_guardian::protocol` for why an unlock request is a
//! broadcast of unlinkable tags rather than something addressed by durable
//! identity — BLE peer ids reset every launch, and this mesh's own stated
//! rule is that nothing durable ever travels in the clear.
//!
//! # What is deliberately not here
//!
//! The 24–48h recovery delay with a veto notification (decision 3 in the
//! design doc) is not implemented. It needs a background task and a local
//! notification that survive the app being closed, which is platform
//! integration work — real on iOS/Android, not something this file can
//! provide or verify without a physical device. What is here is everyday
//! unlock: guardians who are currently reachable, answering in real time,
//! which is also the doc's own "Unlock — owner side" mock-up.
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
    recognition_tag, EnrollAccept, EnrollPayload, EnrollRequest, GuardianMessage, UnlockReplyPayload, UnlockRequest,
};
use cabal_guardian::sealed::{self, GuardianPublicKey, GuardianSecretKey, Sealed};
use cabal_guardian::{self as shamir, Share};
use cabal_vault::{DataKey, StaticKeyProvider, Vault};
use std::path::PathBuf;
use aes_gcm::aead::{rand_core::RngCore, OsRng};
use chrono::{DateTime, Utc};
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
}

#[derive(Debug, Clone, Serialize, Deserialize)]
struct HeldShare {
    /// Which owner this share belongs to — a key, not a BLE identifier, for
    /// the same reason the owner's own recovery key is one.
    owner_recovery_public_key: [u8; 32],
    /// A `cabal_guardian::Share`, serialized.
    share: Vec<u8>,
    enrolled_at: DateTime<Utc>,
}

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
    #[error("need at least two guardians, and no more than 255")]
    InvalidGuardianCount,
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
        Self { vault, doc, held_path: app_dir.join("guardian_held.enc"), held_vault: None }
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
        let next = Vault::new(self.held_path.clone(), StaticKeyProvider::new(vault_key.derive(HELD_SUBKEY_LABEL)));

        if self.held_vault.is_some() {
            // Already readable: what is in memory is authoritative.
            next.save(&self.doc.held)?;
            self.held_vault = Some(next);
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
        if migrating {
            // Only now that the shares are safely in the new file.
            self.save_owner()?;
        }
        Ok(())
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

    /// Splits `vault_key` across `guardians`, returning each guardian's
    /// sealed enrollment message paired with its public key, ready to send
    /// directed. Persists the guardian list and threshold — **never the
    /// shares**, matching the design doc: "owner does NOT store the
    /// shares (storing them defeats the mechanism)."
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
    ) -> Result<Vec<(GuardianPublicKey, GuardianMessage)>, GuardianServiceError> {
        if guardians.len() < 2 || guardians.len() > usize::from(u8::MAX) {
            return Err(GuardianServiceError::InvalidGuardianCount);
        }
        let n = u8::try_from(guardians.len()).expect("bounds checked above");

        let recovery = self.owner_recovery_key()?;
        let shares = shamir::split(vault_key, k, n)?;

        let messages: Vec<(GuardianPublicKey, GuardianMessage)> = guardians
            .iter()
            .zip(shares)
            .map(|(guardian, share)| {
                let payload = EnrollPayload {
                    owner_recovery_public_key: recovery.public_key().to_bytes(),
                    threshold: k,
                    total: n,
                    share: share.to_bytes(),
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
        self.save_owner()?;

        Ok(messages)
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

        self.doc.held.held.push(HeldShare {
            owner_recovery_public_key: payload.owner_recovery_public_key,
            share: payload.share,
            enrolled_at: Utc::now(),
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
        let messages = owner.service.owner_prepare_enrollment(vault_key, &public_keys, k).unwrap();

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
        assert_eq!(reconstructed.expose_for_storage(), vault_key.expose_for_storage());
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

        for (_, message) in owner_a.service.owner_prepare_enrollment(&key_a, &[shared_key, only_a_key], 2).unwrap() {
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
        for (_, message) in owner_b.service.owner_prepare_enrollment(&key_b, &[shared_key, only_b_key], 2).unwrap() {
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
            owner.service.owner_prepare_enrollment(&vault_key, &[key], 2),
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
        owner.owner_prepare_enrollment(&vault_key, &public_keys, 2).unwrap();
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
                held: vec![HeldShare { owner_recovery_public_key: [5; 32], share: vec![1, 2, 3], enrolled_at: Utc::now() }],
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
}
