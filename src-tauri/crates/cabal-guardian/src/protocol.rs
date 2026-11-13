//! The message shapes exchanged over `PacketKind::Guardian` on the mesh.
//!
//! # Why an unlock request cannot simply be "sent to my guardians"
//!
//! BLE peer identifiers are ephemeral by design — a new one every launch,
//! deliberately, so a passive listener cannot follow a device across days
//! (see `cabal_ble::peers`'s module docs). That is also true of the *owner*
//! requesting an unlock and of the *guardian* who might answer: neither
//! knows the other's current identifier going in, only their durable public
//! key from enrollment.
//!
//! So [`UnlockRequest`] cannot be directed. It is broadcast, and it does not
//! name which guardians it is for — it carries one *unlinkable recognition
//! tag* per enrolled guardian ([`recognition_tag`]), each a keyed hash of
//! that guardian's public key and a fresh nonce. Anyone can see a request
//! went out; nobody but a holder of one of the matching public keys can tell
//! it was meant for them, and a guardian's response to two different
//! requests produces two unrelated tags even for the same owner. This is the
//! same shape as the durable-identity rule the mesh already enforces for the
//! NEAR account — nothing durable travels except as ciphertext or an
//! unlinkable derivative of it.
//!
//! A guardian who recognises a tag replies directly to the request's sender
//! — trivial, since that peer id is right there in the packet that just
//! arrived — with [`UnlockReply`], sealed to the *owner's* recovery public
//! key (also from enrollment). The owner never needs to know which of their
//! guardians a given reply came from; only that enough of them arrived.

use crate::sealed::{GuardianPublicKey, Sealed};
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};

pub const TAG_LEN: usize = 16;
const TAG_DOMAIN: &[u8] = b"cabalmesh-guardian-tag-v1";

/// A one-time, unlinkable stand-in for "this request is meant for you,"
/// computed the same way by the owner (for every guardian they have) and by
/// each guardian (for every owner they hold a share for) so both sides can
/// recognise a match without either side naming who they mean.
#[must_use]
pub fn recognition_tag(guardian_public_key: &GuardianPublicKey, nonce: &[u8; TAG_LEN]) -> [u8; TAG_LEN] {
    let mut hasher = Sha256::new();
    hasher.update(TAG_DOMAIN);
    hasher.update(guardian_public_key.to_bytes());
    hasher.update(nonce);
    let digest = hasher.finalize();
    let mut tag = [0_u8; TAG_LEN];
    tag.copy_from_slice(&digest[..TAG_LEN]);
    tag
}

/// Directed, unsealed: "will you hold a share for me?" A guardian candidate
/// has no key to seal anything to yet — this is what triggers generating
/// one, via [`EnrollAccept`]. Nothing in this message is secret; the owner
/// already chose this exact peer, visibly, from a list of nearby nodes.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct EnrollRequest;

/// Directed reply to [`EnrollRequest`], carrying the guardian's public key
/// so the owner can seal a share to it. Also unsealed — a public key is not
/// secret, and the sender is already the specific peer the owner asked.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct EnrollAccept {
    pub guardian_public_key: [u8; 32],
}

/// Broadcast, unsealed — see the module docs for why this is safe to send in
/// the clear: a tag reveals nothing without already knowing the guardian
/// public key it was computed from.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct UnlockRequest {
    pub nonce: [u8; TAG_LEN],
    pub tags: Vec<[u8; TAG_LEN]>,
}

/// Sealed to the guardian's public key, sent directed at enrollment time
/// (the owner already has a live link to this exact guardian, since
/// enrollment happens with them physically present).
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct EnrollPayload {
    /// Where a reply should be sealed to. Not a BLE identifier — those do
    /// not survive a restart.
    pub owner_recovery_public_key: [u8; 32],
    pub threshold: u8,
    pub total: u8,
    /// A `cabal_guardian::Share`, serialized via `Share::to_bytes`. A share
    /// of the owner's stable *recovery key*, not of the vault key itself, so
    /// it stays valid when the vault key changes (passphrase on/off).
    pub share: Vec<u8>,
    /// What a restore request from a new device must be tagged with — see
    /// `restore::RestoreSecret::lookup`. Absent from pre-restore enrollments.
    #[serde(default)]
    pub restore_lookup: Option<[u8; 32]>,
    /// Authenticates later messages that only the original device may send
    /// (backup refresh, restore cancel). Never in the restore code.
    #[serde(default)]
    pub owner_auth_key: Option<[u8; 32]>,
    /// The vault backup, wrapped under `RestoreSecret::backup_key`.
    #[serde(default)]
    pub backup: Option<Vec<u8>>,
    #[serde(default)]
    pub backup_seq: u64,
    /// Whether the vault held anything worth a restore delay when this was
    /// sent. `None` (old enrollments) is treated as "yes".
    #[serde(default)]
    pub has_value: Option<bool>,
}

/// Sealed to the owner's recovery public key, sent directed at the
/// requesting peer id.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct UnlockReplyPayload {
    pub share: Vec<u8>,
}

/// Broadcast by a **new** device restoring from guardians. Carries no
/// identifier: `tag` is `restore::restore_tag(lookup, nonce)`, which only a
/// guardian holding that owner's `lookup` can recognise. Replies are sealed
/// to `reply_public_key`, an ephemeral key the new device generated.
///
/// Re-broadcast unchanged (same nonce and reply key) while waiting out the
/// restore delay, so a guardian can recognise it as the request it already
/// approved and release the share once the delay has passed.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RestoreRequest {
    pub nonce: [u8; TAG_LEN],
    pub tag: [u8; TAG_LEN],
    pub reply_public_key: [u8; 32],
}

/// Sealed to a [`RestoreRequest`]'s `reply_public_key`.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RestoreReplyPayload {
    pub status: RestoreStatus,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case", tag = "kind")]
pub enum RestoreStatus {
    /// Approved, but held until `release_at` (unix seconds) so the original
    /// device has a chance to veto.
    Waiting { release_at: i64 },
    /// The share, and this guardian's copy of the vault backup.
    Released { share: Vec<u8>, threshold: u8, backup: Vec<u8>, backup_seq: u64 },
    /// The original device vetoed this restore.
    Cancelled,
}

/// Broadcast by the owner to refresh what guardians hold. One entry per
/// guardian, each tagged with [`recognition_tag`] and sealed to that
/// guardian, so nobody else learns who guards whom or reads the backup.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct BackupUpdate {
    pub nonce: [u8; TAG_LEN],
    pub entries: Vec<BackupEntry>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct BackupEntry {
    pub tag: [u8; TAG_LEN],
    /// A [`Sealed`] [`BackupPayload`], as `Sealed::to_bytes`.
    pub sealed: Vec<u8>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct BackupPayload {
    pub owner_recovery_public_key: [u8; 32],
    pub backup: Vec<u8>,
    pub seq: u64,
    pub has_value: bool,
    /// `restore::mac(owner_auth_key, BACKUP_MAC_DOMAIN, [backup, seq, has_value])`.
    pub mac: [u8; 32],
}

pub const BACKUP_MAC_DOMAIN: &[u8] = b"cabalmesh/backup-update/v1";
pub const CANCEL_MAC_DOMAIN: &[u8] = b"cabalmesh/restore-cancel/v1";

/// Broadcast by a guardian while a restore for some owner is waiting out its
/// delay. `tag` is `restore::pending_tag(owner_auth_key, nonce)`: only the
/// original device recognises it.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RestorePending {
    pub nonce: [u8; TAG_LEN],
    pub tag: [u8; TAG_LEN],
    /// Unix seconds, so the original device can say how long it has.
    pub release_at: i64,
}

/// Broadcast by the original device to veto every pending restore of its
/// vault. `mac` is `restore::mac(owner_auth_key, CANCEL_MAC_DOMAIN, [nonce])`.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RestoreCancel {
    pub nonce: [u8; TAG_LEN],
    pub mac: [u8; 32],
}

/// One message as it travels inside a `PacketKind::Guardian` payload.
///
/// [`GuardianMessage::Enroll`] and [`GuardianMessage::UnlockReply`] carry an
/// already-opaque [`Sealed`] payload as raw bytes; [`GuardianMessage::UnlockRequest`]
/// has nothing secret in it and is JSON, matching every other app-level
/// payload on this mesh (`PrivacyIntent` in `mesh.rs`). Wrapping the sealed
/// bytes in JSON too would just be base64 padding around ciphertext that is
/// already self-describing.
#[derive(Debug, Clone)]
pub enum GuardianMessage {
    EnrollRequest(EnrollRequest),
    EnrollAccept(EnrollAccept),
    Enroll(Sealed),
    UnlockRequest(UnlockRequest),
    UnlockReply(Sealed),
    RestoreRequest(RestoreRequest),
    RestoreReply(Sealed),
    BackupUpdate(BackupUpdate),
    RestorePending(RestorePending),
    RestoreCancel(RestoreCancel),
}

#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
#[non_exhaustive]
pub enum ProtocolError {
    #[error("empty guardian payload")]
    Empty,
    #[error("malformed guardian message")]
    Malformed,
    #[error("unknown guardian message type 0x{0:02x}")]
    UnknownMessageType(u8),
}

impl GuardianMessage {
    const ENROLL_REQUEST: u8 = 0x00;
    const ENROLL_ACCEPT: u8 = 0x01;
    const ENROLL: u8 = 0x02;
    const UNLOCK_REQUEST: u8 = 0x03;
    const UNLOCK_REPLY: u8 = 0x04;
    const RESTORE_REQUEST: u8 = 0x05;
    const RESTORE_REPLY: u8 = 0x06;
    const BACKUP_UPDATE: u8 = 0x07;
    const RESTORE_PENDING: u8 = 0x08;
    const RESTORE_CANCEL: u8 = 0x09;

    /// Serializes to `[type byte][message bytes]`, ready to send as a
    /// `PacketKind::Guardian` payload.
    #[must_use]
    pub fn to_bytes(&self) -> Vec<u8> {
        match self {
            Self::EnrollRequest(request) => prefix(
                Self::ENROLL_REQUEST,
                serde_json::to_vec(request).expect("EnrollRequest has no reason to fail to serialize"),
            ),
            Self::EnrollAccept(accept) => prefix(
                Self::ENROLL_ACCEPT,
                serde_json::to_vec(accept).expect("EnrollAccept has no reason to fail to serialize"),
            ),
            Self::Enroll(sealed) => prefix(Self::ENROLL, sealed.to_bytes()),
            Self::UnlockRequest(request) => prefix(
                Self::UNLOCK_REQUEST,
                serde_json::to_vec(request).expect("UnlockRequest has no reason to fail to serialize"),
            ),
            Self::UnlockReply(sealed) => prefix(Self::UNLOCK_REPLY, sealed.to_bytes()),
            Self::RestoreRequest(m) => prefix(Self::RESTORE_REQUEST, json(m)),
            Self::RestoreReply(sealed) => prefix(Self::RESTORE_REPLY, sealed.to_bytes()),
            Self::BackupUpdate(m) => prefix(Self::BACKUP_UPDATE, json(m)),
            Self::RestorePending(m) => prefix(Self::RESTORE_PENDING, json(m)),
            Self::RestoreCancel(m) => prefix(Self::RESTORE_CANCEL, json(m)),
        }
    }

    /// Parses bytes previously produced by [`GuardianMessage::to_bytes`].
    ///
    /// # Errors
    ///
    /// [`ProtocolError`] on anything malformed — this arrives over the mesh
    /// from a peer nothing here has authenticated yet.
    pub fn from_bytes(bytes: &[u8]) -> Result<Self, ProtocolError> {
        let (&tag, rest) = bytes.split_first().ok_or(ProtocolError::Empty)?;
        match tag {
            Self::ENROLL_REQUEST => {
                Ok(Self::EnrollRequest(serde_json::from_slice(rest).map_err(|_| ProtocolError::Malformed)?))
            }
            Self::ENROLL_ACCEPT => {
                Ok(Self::EnrollAccept(serde_json::from_slice(rest).map_err(|_| ProtocolError::Malformed)?))
            }
            Self::ENROLL => Ok(Self::Enroll(Sealed::from_bytes(rest).map_err(|_| ProtocolError::Malformed)?)),
            Self::UNLOCK_REQUEST => {
                Ok(Self::UnlockRequest(serde_json::from_slice(rest).map_err(|_| ProtocolError::Malformed)?))
            }
            Self::UNLOCK_REPLY => {
                Ok(Self::UnlockReply(Sealed::from_bytes(rest).map_err(|_| ProtocolError::Malformed)?))
            }
            Self::RESTORE_REQUEST => Ok(Self::RestoreRequest(parse(rest)?)),
            Self::RESTORE_REPLY => {
                Ok(Self::RestoreReply(Sealed::from_bytes(rest).map_err(|_| ProtocolError::Malformed)?))
            }
            Self::BACKUP_UPDATE => Ok(Self::BackupUpdate(parse(rest)?)),
            Self::RESTORE_PENDING => Ok(Self::RestorePending(parse(rest)?)),
            Self::RESTORE_CANCEL => Ok(Self::RestoreCancel(parse(rest)?)),
            other => Err(ProtocolError::UnknownMessageType(other)),
        }
    }
}

fn json<T: Serialize>(message: &T) -> Vec<u8> {
    serde_json::to_vec(message).expect("guardian messages have no reason to fail to serialize")
}

fn parse<T: serde::de::DeserializeOwned>(bytes: &[u8]) -> Result<T, ProtocolError> {
    serde_json::from_slice(bytes).map_err(|_| ProtocolError::Malformed)
}

fn prefix(tag: u8, mut body: Vec<u8>) -> Vec<u8> {
    let mut out = Vec::with_capacity(1 + body.len());
    out.push(tag);
    out.append(&mut body);
    out
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::sealed::GuardianSecretKey;

    #[test]
    fn recognition_tags_match_for_the_same_key_and_nonce() {
        let guardian = GuardianSecretKey::generate();
        let nonce = [7_u8; TAG_LEN];

        assert_eq!(
            recognition_tag(&guardian.public_key(), &nonce),
            recognition_tag(&guardian.public_key(), &nonce)
        );
    }

    #[test]
    fn recognition_tags_differ_across_nonces() {
        // Otherwise a guardian's tag would be a stable, trackable identifier
        // across every unlock request they ever answer — exactly what this
        // scheme exists to avoid.
        let guardian = GuardianSecretKey::generate();
        assert_ne!(
            recognition_tag(&guardian.public_key(), &[1; TAG_LEN]),
            recognition_tag(&guardian.public_key(), &[2; TAG_LEN])
        );
    }

    #[test]
    fn recognition_tags_differ_across_guardians() {
        let nonce = [7_u8; TAG_LEN];
        let a = GuardianSecretKey::generate();
        let b = GuardianSecretKey::generate();
        assert_ne!(recognition_tag(&a.public_key(), &nonce), recognition_tag(&b.public_key(), &nonce));
    }

    #[test]
    fn an_enroll_request_round_trips_through_bytes() {
        let message = GuardianMessage::EnrollRequest(EnrollRequest);
        assert!(matches!(
            GuardianMessage::from_bytes(&message.to_bytes()),
            Ok(GuardianMessage::EnrollRequest(_))
        ));
    }

    #[test]
    fn an_enroll_accept_round_trips_through_bytes() {
        let message = GuardianMessage::EnrollAccept(EnrollAccept { guardian_public_key: [42; 32] });

        let GuardianMessage::EnrollAccept(decoded) = GuardianMessage::from_bytes(&message.to_bytes()).unwrap()
        else {
            panic!("wrong variant decoded");
        };
        assert_eq!(decoded.guardian_public_key, [42; 32]);
    }

    #[test]
    fn an_unlock_request_round_trips_through_bytes() {
        let request = UnlockRequest { nonce: [3; TAG_LEN], tags: vec![[1; TAG_LEN], [2; TAG_LEN]] };
        let message = GuardianMessage::UnlockRequest(request.clone());

        let GuardianMessage::UnlockRequest(decoded) = GuardianMessage::from_bytes(&message.to_bytes()).unwrap()
        else {
            panic!("wrong variant decoded");
        };
        assert_eq!(decoded.nonce, request.nonce);
        assert_eq!(decoded.tags, request.tags);
    }

    #[test]
    fn an_enroll_message_round_trips_through_bytes() {
        let guardian = GuardianSecretKey::generate();
        let payload = EnrollPayload {
            owner_recovery_public_key: [9; 32],
            threshold: 3,
            total: 5,
            share: vec![1, 2, 3, 4],
            restore_lookup: None,
            owner_auth_key: None,
            backup: None,
            backup_seq: 0,
            has_value: None,
        };
        let sealed = crate::sealed::seal(&serde_json::to_vec(&payload).unwrap(), &guardian.public_key());
        let message = GuardianMessage::Enroll(sealed);

        let GuardianMessage::Enroll(decoded_sealed) = GuardianMessage::from_bytes(&message.to_bytes()).unwrap()
        else {
            panic!("wrong variant decoded");
        };
        let opened = crate::sealed::open(&decoded_sealed, &guardian).unwrap();
        let decoded_payload: EnrollPayload = serde_json::from_slice(&opened).unwrap();
        assert_eq!(decoded_payload.share, payload.share);
        assert_eq!(decoded_payload.threshold, payload.threshold);
    }

    #[test]
    fn an_empty_payload_is_rejected_not_a_panic() {
        assert!(matches!(GuardianMessage::from_bytes(&[]), Err(ProtocolError::Empty)));
    }

    #[test]
    fn an_unknown_message_type_is_rejected() {
        assert!(matches!(
            GuardianMessage::from_bytes(&[0xff, 1, 2, 3]),
            Err(ProtocolError::UnknownMessageType(0xff))
        ));
    }

    #[test]
    fn restore_messages_round_trip_through_bytes() {
        let request = GuardianMessage::RestoreRequest(RestoreRequest { nonce: [1; TAG_LEN], tag: [2; TAG_LEN], reply_public_key: [3; 32] });
        assert!(matches!(GuardianMessage::from_bytes(&request.to_bytes()), Ok(GuardianMessage::RestoreRequest(r)) if r.reply_public_key == [3; 32]));

        let pending = GuardianMessage::RestorePending(RestorePending { nonce: [1; TAG_LEN], tag: [2; TAG_LEN], release_at: 99 });
        assert!(matches!(GuardianMessage::from_bytes(&pending.to_bytes()), Ok(GuardianMessage::RestorePending(p)) if p.release_at == 99));

        let cancel = GuardianMessage::RestoreCancel(RestoreCancel { nonce: [1; TAG_LEN], mac: [4; 32] });
        assert!(matches!(GuardianMessage::from_bytes(&cancel.to_bytes()), Ok(GuardianMessage::RestoreCancel(c)) if c.mac == [4; 32]));

        let update = GuardianMessage::BackupUpdate(BackupUpdate { nonce: [1; TAG_LEN], entries: vec![BackupEntry { tag: [5; TAG_LEN], sealed: vec![6, 7] }] });
        assert!(matches!(GuardianMessage::from_bytes(&update.to_bytes()), Ok(GuardianMessage::BackupUpdate(u)) if u.entries.len() == 1));
    }

    #[test]
    fn an_enroll_payload_from_before_restore_existed_still_parses() {
        let old = br#"{"owner_recovery_public_key":[0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0],"threshold":2,"total":3,"share":[1]}"#;
        let parsed: EnrollPayload = serde_json::from_slice(old).unwrap();
        assert!(parsed.restore_lookup.is_none() && parsed.has_value.is_none());
    }
}
