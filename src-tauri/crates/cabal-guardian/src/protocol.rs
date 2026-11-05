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

const TAG_LEN: usize = 16;
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
    /// A `cabal_guardian::Share`, serialized via `Share::to_bytes`.
    pub share: Vec<u8>,
}

/// Sealed to the owner's recovery public key, sent directed at the
/// requesting peer id.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct UnlockReplyPayload {
    pub share: Vec<u8>,
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
            other => Err(ProtocolError::UnknownMessageType(other)),
        }
    }
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
}
