//! This session's keys.
//!
//! # Ephemeral by construction
//!
//! Both keypairs are generated at startup and never written to disk. There is
//! no "load" function and no path parameter, because the only way to guarantee
//! an identity does not outlive a session is to give the code no way to
//! persist it.
//!
//! This mirrors what the IP plane already does — `mesh.rs` generates a fresh
//! libp2p keypair on every launch — and it is the concrete difference between
//! this design and bitchat's, whose peer identifier never rotates and whose
//! announcements name the device in cleartext.
//!
//! What it costs: no reconnect memory across launches. A peer traded with
//! yesterday is a stranger today until the handshake runs again. Counterparty
//! continuity has to come from the durable identity exchanged *inside* a
//! session, never from the radio.

use crate::peers::KEY_LEN;
use crate::wire::{Packet, PeerId, SIGNATURE_LEN};
use ed25519_dalek::{Signature, Signer, SigningKey, Verifier, VerifyingKey};

/// The keys this node uses for as long as the process lives.
pub struct Ephemeral {
    /// Stands in for the X25519 key agreement public key.
    ///
    /// The Noise handshake owns the real key exchange; what this crate needs
    /// from it is a stable 32-byte public value to derive an identifier from
    /// and to publish in announcements.
    key_agreement: [u8; KEY_LEN],
    signing: SigningKey,
    id: PeerId,
}

impl std::fmt::Debug for Ephemeral {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        // Never derived: a `Debug` that printed the signing key would put it
        // in a log the first time somebody debugged a mesh problem.
        f.debug_struct("Ephemeral").field("id", &self.id).finish_non_exhaustive()
    }
}

impl Ephemeral {
    /// Builds an identity from raw key material.
    ///
    /// Takes the bytes rather than generating them so that the caller owns the
    /// randomness — the app uses the OS generator, and tests use fixed bytes
    /// to stay deterministic.
    #[must_use]
    pub fn from_bytes(key_agreement: [u8; KEY_LEN], signing_seed: [u8; 32]) -> Self {
        let signing = SigningKey::from_bytes(&signing_seed);
        let id = PeerId::from_public_key(&key_agreement);
        Self {
            key_agreement,
            signing,
            id,
        }
    }

    /// This node's identifier for this session.
    #[must_use]
    pub fn id(&self) -> PeerId {
        self.id
    }

    /// The key agreement public key, as announced.
    #[must_use]
    pub fn key_agreement_public(&self) -> [u8; KEY_LEN] {
        self.key_agreement
    }

    /// The signing public key, as announced.
    #[must_use]
    pub fn signing_public(&self) -> [u8; KEY_LEN] {
        self.signing.verifying_key().to_bytes()
    }

    /// Signs a packet in place.
    ///
    /// # Errors
    ///
    /// Propagates [`crate::wire::WireError`] if the packet cannot be encoded.
    pub fn sign(&self, packet: &mut Packet) -> Result<(), crate::wire::WireError> {
        let bytes = packet.signing_bytes()?;
        let signature: Signature = self.signing.sign(&bytes);
        packet.signature = Some(signature.to_bytes());
        Ok(())
    }
}

/// Whether a packet's signature matches the given public key.
///
/// Returns `false` for an unsigned packet, a malformed key, or a bad
/// signature — all three mean "do not trust this", and distinguishing them for
/// the caller would only invite a caller that handles one case and forgets the
/// others.
#[must_use]
pub fn verify(packet: &Packet, signing_public: &[u8; KEY_LEN]) -> bool {
    let Some(signature) = packet.signature else {
        return false;
    };
    let Ok(key) = VerifyingKey::from_bytes(signing_public) else {
        return false;
    };
    let Ok(bytes) = packet.signing_bytes() else {
        return false;
    };
    key.verify(&bytes, &Signature::from_bytes(&signature)).is_ok()
}

/// The length of a signature, re-exported so callers do not reach into `wire`
/// for a constant that is really about identity.
pub const SIGNATURE_BYTES: usize = SIGNATURE_LEN;

#[cfg(test)]
mod tests {
    use super::*;
    use crate::wire::PacketKind;

    fn identity(seed: u8) -> Ephemeral {
        Ephemeral::from_bytes([seed; KEY_LEN], [seed.wrapping_add(9); 32])
    }

    #[test]
    fn a_signed_packet_verifies() {
        let me = identity(1);
        let mut packet = Packet::broadcast(PacketKind::Announce, me.id(), 5, b"payload".into());
        me.sign(&mut packet).unwrap();

        assert!(verify(&packet, &me.signing_public()));
    }

    #[test]
    fn a_signature_survives_a_ttl_decrement() {
        // The property the relay model rests on. If this breaks, every hop
        // silently becomes an unverifiable packet.
        let me = identity(1);
        let mut packet = Packet::broadcast(PacketKind::Announce, me.id(), 5, b"payload".into());
        me.sign(&mut packet).unwrap();

        packet.ttl -= 1;
        assert!(verify(&packet, &me.signing_public()));
    }

    #[test]
    fn a_tampered_payload_does_not_verify() {
        let me = identity(1);
        let mut packet = Packet::broadcast(PacketKind::Announce, me.id(), 5, b"payload".into());
        me.sign(&mut packet).unwrap();

        packet.payload = b"payloae".into();
        assert!(!verify(&packet, &me.signing_public()));
    }

    #[test]
    fn another_nodes_key_does_not_verify() {
        let me = identity(1);
        let them = identity(2);
        let mut packet = Packet::broadcast(PacketKind::Announce, me.id(), 5, b"payload".into());
        me.sign(&mut packet).unwrap();

        assert!(!verify(&packet, &them.signing_public()));
    }

    #[test]
    fn an_unsigned_packet_does_not_verify() {
        let me = identity(1);
        let packet = Packet::broadcast(PacketKind::Announce, me.id(), 5, b"payload".into());

        assert!(!verify(&packet, &me.signing_public()));
    }

    #[test]
    fn the_identifier_follows_the_key_agreement_key() {
        let me = identity(1);
        assert_eq!(me.id(), PeerId::from_public_key(&me.key_agreement_public()));
        assert_ne!(me.id(), identity(2).id());
    }

    #[test]
    fn debug_output_does_not_contain_key_material() {
        // A `#[derive(Debug)]` here would print the signing key into the first
        // log anybody captured while debugging a mesh problem.
        let me = identity(1);
        let rendered = format!("{me:?}");
        assert!(rendered.contains("id"));
        assert!(!rendered.contains("signing"));
    }
}
