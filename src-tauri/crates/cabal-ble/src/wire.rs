//! The packet on the wire.
//!
//! # Why this is its own module with its own tests
//!
//! Every byte here arrives over the air from anyone within radio range, before
//! any handshake has happened and before anything is known about who sent it.
//! A decoder that panics on a malformed length is a remote crash triggered by
//! standing near the victim.
//!
//! So: no `unwrap`, no slice indexing that can go out of range, no allocation
//! sized by an attacker-supplied number that has not been checked first.
//!
//! # Layout
//!
//! ```text
//! ┌─────────┬──────┬─────┬───────────┬───────┬─────────────┐
//! │ version │ kind │ TTL │ timestamp │ flags │ payload_len │
//! │  u8     │  u8  │ u8  │  u64 BE   │  u8   │   u16 BE    │
//! └─────────┴──────┴─────┴───────────┴───────┴─────────────┘
//! ┌──────────┬─────────────┬─────────┬───────────┐
//! │ sender   │ recipient   │ payload │ signature │
//! │ 8 bytes  │ 8 bytes  *  │ n bytes │ 64 bytes *│
//! └──────────┴─────────────┴─────────┴───────────┘
//! * present per flags
//! ```
//!
//! # The TTL is excluded from the signature
//!
//! Relays decrement TTL in place. If it were signed, every hop would have to
//! re-sign — which means every hop would need to be trusted to re-sign, which
//! defeats the point of signing at all. Excluding that one byte is what lets a
//! packet be authenticated end to end while still being routed hop by hop.

use std::fmt;

/// Protocol version. Bumped when the layout changes incompatibly.
pub const VERSION: u8 = 1;

/// Fixed header length: version + kind + ttl + timestamp + flags + payload_len.
pub const HEADER_LEN: usize = 1 + 1 + 1 + 8 + 1 + 2;

/// Length of a peer identifier on the wire.
pub const PEER_ID_LEN: usize = 8;

/// Length of an Ed25519 signature.
pub const SIGNATURE_LEN: usize = 64;

/// The largest payload a packet can carry, bounded by the `u16` length field.
pub const MAX_PAYLOAD: usize = u16::MAX as usize;

/// The largest a whole packet can be: header, sender, recipient, payload,
/// signature. A frame claiming more than this is refused *before* allocating.
pub const MAX_PACKET: usize =
    HEADER_LEN + PEER_ID_LEN + PEER_ID_LEN + MAX_PAYLOAD + SIGNATURE_LEN;

const FLAG_HAS_RECIPIENT: u8 = 0b0000_0001;
const FLAG_HAS_SIGNATURE: u8 = 0b0000_0010;
const FLAG_COMPRESSED: u8 = 0b0000_0100;
const FLAG_RESERVED: u8 = 0b1111_1000;

/// A node's short identifier: the first 8 bytes of the SHA-256 of its
/// ephemeral X25519 public key.
///
/// Ephemeral by construction. See `identity::Ephemeral` for why the durable
/// identity never appears on the wire in cleartext.
#[derive(Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord, Default)]
pub struct PeerId(pub [u8; PEER_ID_LEN]);

impl PeerId {
    /// Derives an identifier from a public key.
    #[must_use]
    pub fn from_public_key(key: &[u8]) -> Self {
        use sha2::{Digest, Sha256};
        let digest = Sha256::digest(key);
        let mut id = [0u8; PEER_ID_LEN];
        id.copy_from_slice(&digest[..PEER_ID_LEN]);
        Self(id)
    }

    /// The raw bytes.
    #[must_use]
    pub fn as_bytes(&self) -> &[u8; PEER_ID_LEN] {
        &self.0
    }
}

impl fmt::Display for PeerId {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        for byte in self.0 {
            write!(f, "{byte:02x}")?;
        }
        Ok(())
    }
}

impl fmt::Debug for PeerId {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "PeerId({self})")
    }
}

/// The inverse of [`PeerId`]'s `Display` — parses the hex a screen sent back
/// after showing it, e.g. a guardian candidate the user picked from a list.
impl std::str::FromStr for PeerId {
    type Err = WireError;

    fn from_str(s: &str) -> Result<Self, Self::Err> {
        let mut id = [0_u8; PEER_ID_LEN];
        hex_decode(s, &mut id).ok_or(WireError::InvalidPeerId).map(|()| Self(id))
    }
}

/// Decodes exactly `out.len()` bytes of lowercase or uppercase hex, or
/// refuses. No `hex` crate dependency for one call site in a crate that
/// otherwise has none.
fn hex_decode(s: &str, out: &mut [u8]) -> Option<()> {
    if s.len() != out.len() * 2 {
        return None;
    }
    for (i, byte) in out.iter_mut().enumerate() {
        *byte = u8::from_str_radix(&s[i * 2..i * 2 + 2], 16).ok()?;
    }
    Some(())
}

/// What a packet is for.
///
/// Encoded as a `u8`. An unknown value is an error rather than a default: a
/// node that silently treated an unrecognised type as an intent would act on
/// something a future version meant differently.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
#[repr(u8)]
pub enum PacketKind {
    /// Signed presence: ephemeral keys, neighbour list, capabilities.
    Announce = 0x01,
    /// One Noise XX handshake message.
    Handshake = 0x02,
    /// Ciphertext inside an established session.
    Sealed = 0x03,
    /// A `PrivacyIntent`, flooded to the whole mesh.
    Intent = 0x10,
    /// Directed delivery acknowledgement.
    IntentAck = 0x11,
    /// "Someone with internet, please carry this."
    GatewayRequest = 0x20,
    /// The outcome of a gateway request, flooded back.
    GatewayResult = 0x21,
    /// Guardian-recovery traffic: an enrollment share, an unlock request, or
    /// an unlock reply. All three share one kind because what distinguishes
    /// them is a tag inside the payload, not anything the router needs to
    /// know — see `cabal_guardian::protocol`. Directed like `Sealed`
    /// (full-fanout relay) even for the broadcast unlock request, which
    /// wants a request to reach every nearby guardian fast rather than
    /// thinned like an `Intent`.
    Guardian = 0x30,
}

impl PacketKind {
    fn from_u8(value: u8) -> Result<Self, WireError> {
        Ok(match value {
            0x01 => Self::Announce,
            0x02 => Self::Handshake,
            0x03 => Self::Sealed,
            0x10 => Self::Intent,
            0x11 => Self::IntentAck,
            0x20 => Self::GatewayRequest,
            0x21 => Self::GatewayResult,
            0x30 => Self::Guardian,
            other => return Err(WireError::UnknownKind(other)),
        })
    }

    /// Whether this kind is flooded to everyone rather than sent to one peer.
    #[must_use]
    pub fn is_broadcast(self) -> bool {
        matches!(
            self,
            Self::Announce | Self::Intent | Self::GatewayRequest | Self::GatewayResult
        )
    }

    /// Whether this kind uses full fanout rather than a subset.
    ///
    /// Announcements and gateway traffic are how the mesh learns its own shape
    /// and how an offline node reaches the chain. Losing one to a fanout
    /// subset costs more than the bandwidth it saves.
    #[must_use]
    pub fn needs_full_fanout(self) -> bool {
        matches!(
            self,
            Self::Announce | Self::GatewayRequest | Self::GatewayResult
        )
    }
}

/// Why a packet could not be decoded or encoded.
#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
#[non_exhaustive]
pub enum WireError {
    /// Fewer bytes than the layout requires.
    #[error("packet truncated: need {need} bytes, have {have}")]
    Truncated { need: usize, have: usize },

    /// A version this build does not speak.
    #[error("unsupported protocol version {0}")]
    Version(u8),

    /// A packet type this build does not know.
    #[error("unknown packet kind 0x{0:02x}")]
    UnknownKind(u8),

    /// A reserved flag bit was set.
    ///
    /// Refused rather than masked off: a future version that means something
    /// by that bit must not be silently mis-parsed by this one.
    #[error("reserved flag bits set: 0b{0:08b}")]
    ReservedFlags(u8),

    /// The compressed flag was set. Defined in the format, not implemented.
    #[error("compressed payloads are not supported by this build")]
    CompressionUnsupported,

    /// Payload longer than the length field can describe.
    #[error("payload of {0} bytes exceeds the {MAX_PAYLOAD}-byte maximum")]
    PayloadTooLarge(usize),

    /// Bytes remained after the packet was fully decoded.
    #[error("{0} trailing bytes after packet")]
    TrailingBytes(usize),

    /// Not the hex a [`PeerId`]'s own `Display` produces.
    #[error("not a valid peer id")]
    InvalidPeerId,
}

/// A decoded packet.
///
/// Owns its payload. Packets are small — an intent and its negotiation are
/// kilobytes — and borrowing from the link buffer would tie a routing decision
/// to the lifetime of the read that produced it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Packet {
    pub kind: PacketKind,
    /// Hops remaining. Decremented in place by relays; excluded from the
    /// signature so that decrement stays valid.
    pub ttl: u8,
    /// Milliseconds since the Unix epoch, as claimed by the sender.
    ///
    /// Used for deduplication and freshness, never for ordering across peers:
    /// nothing here trusts another device's clock.
    pub timestamp_ms: u64,
    pub sender: PeerId,
    /// Present for directed packets, absent for broadcasts.
    pub recipient: Option<PeerId>,
    pub payload: Vec<u8>,
    pub signature: Option<[u8; SIGNATURE_LEN]>,
}

impl Packet {
    /// A broadcast packet with no signature yet.
    #[must_use]
    pub fn broadcast(kind: PacketKind, sender: PeerId, timestamp_ms: u64, payload: Vec<u8>) -> Self {
        Self {
            kind,
            ttl: crate::router::DEFAULT_TTL,
            timestamp_ms,
            sender,
            recipient: None,
            payload,
            signature: None,
        }
    }

    /// A packet addressed to one peer.
    #[must_use]
    pub fn directed(
        kind: PacketKind,
        sender: PeerId,
        recipient: PeerId,
        timestamp_ms: u64,
        payload: Vec<u8>,
    ) -> Self {
        Self {
            kind,
            ttl: crate::router::DEFAULT_TTL,
            timestamp_ms,
            sender,
            recipient: Some(recipient),
            payload,
            signature: None,
        }
    }

    /// The bytes a signature covers: everything except the TTL and the
    /// signature itself.
    ///
    /// Built by encoding with the TTL forced to zero, so signer and verifier
    /// cannot disagree about the layout — they call the same function.
    ///
    /// # Errors
    ///
    /// [`WireError::PayloadTooLarge`] if the payload exceeds [`MAX_PAYLOAD`].
    pub fn signing_bytes(&self) -> Result<Vec<u8>, WireError> {
        let mut without_ttl = self.clone();
        without_ttl.ttl = 0;
        without_ttl.signature = None;
        without_ttl.encode()
    }

    /// Serializes to the wire.
    ///
    /// # Errors
    ///
    /// [`WireError::PayloadTooLarge`] if the payload exceeds [`MAX_PAYLOAD`].
    pub fn encode(&self) -> Result<Vec<u8>, WireError> {
        if self.payload.len() > MAX_PAYLOAD {
            return Err(WireError::PayloadTooLarge(self.payload.len()));
        }

        let mut flags = 0u8;
        if self.recipient.is_some() {
            flags |= FLAG_HAS_RECIPIENT;
        }
        if self.signature.is_some() {
            flags |= FLAG_HAS_SIGNATURE;
        }

        let mut out = Vec::with_capacity(
            HEADER_LEN
                + PEER_ID_LEN
                + usize::from(self.recipient.is_some()) * PEER_ID_LEN
                + self.payload.len()
                + usize::from(self.signature.is_some()) * SIGNATURE_LEN,
        );

        out.push(VERSION);
        out.push(self.kind as u8);
        out.push(self.ttl);
        out.extend_from_slice(&self.timestamp_ms.to_be_bytes());
        out.push(flags);
        // Cast is bounded by the check above.
        out.extend_from_slice(&(self.payload.len() as u16).to_be_bytes());
        out.extend_from_slice(&self.sender.0);
        if let Some(recipient) = self.recipient {
            out.extend_from_slice(&recipient.0);
        }
        out.extend_from_slice(&self.payload);
        if let Some(signature) = self.signature {
            out.extend_from_slice(&signature);
        }

        Ok(out)
    }

    /// Parses a packet, requiring the input to be exactly one packet.
    ///
    /// # Errors
    ///
    /// Any [`WireError`]. Never panics, whatever the input.
    pub fn decode(bytes: &[u8]) -> Result<Self, WireError> {
        let (packet, consumed) = Self::decode_prefix(bytes)?;
        if consumed != bytes.len() {
            return Err(WireError::TrailingBytes(bytes.len() - consumed));
        }
        Ok(packet)
    }

    /// Parses one packet from the front of a buffer, returning how many bytes
    /// it used.
    ///
    /// # Errors
    ///
    /// Any [`WireError`].
    pub fn decode_prefix(bytes: &[u8]) -> Result<(Self, usize), WireError> {
        let need = HEADER_LEN + PEER_ID_LEN;
        if bytes.len() < need {
            return Err(WireError::Truncated {
                need,
                have: bytes.len(),
            });
        }

        let version = bytes[0];
        if version != VERSION {
            return Err(WireError::Version(version));
        }
        let kind = PacketKind::from_u8(bytes[1])?;
        let ttl = bytes[2];

        let mut timestamp = [0u8; 8];
        timestamp.copy_from_slice(&bytes[3..11]);
        let timestamp_ms = u64::from_be_bytes(timestamp);

        let flags = bytes[11];
        if flags & FLAG_RESERVED != 0 {
            return Err(WireError::ReservedFlags(flags & FLAG_RESERVED));
        }
        if flags & FLAG_COMPRESSED != 0 {
            return Err(WireError::CompressionUnsupported);
        }

        let payload_len = usize::from(u16::from_be_bytes([bytes[12], bytes[13]]));
        let has_recipient = flags & FLAG_HAS_RECIPIENT != 0;
        let has_signature = flags & FLAG_HAS_SIGNATURE != 0;

        // Every variable-length section is accounted for before a single byte
        // of it is read, so a lying length field is refused rather than
        // allocated.
        let total = HEADER_LEN
            + PEER_ID_LEN
            + usize::from(has_recipient) * PEER_ID_LEN
            + payload_len
            + usize::from(has_signature) * SIGNATURE_LEN;
        if bytes.len() < total {
            return Err(WireError::Truncated {
                need: total,
                have: bytes.len(),
            });
        }

        let mut cursor = HEADER_LEN;
        let mut sender = [0u8; PEER_ID_LEN];
        sender.copy_from_slice(&bytes[cursor..cursor + PEER_ID_LEN]);
        cursor += PEER_ID_LEN;

        let recipient = if has_recipient {
            let mut id = [0u8; PEER_ID_LEN];
            id.copy_from_slice(&bytes[cursor..cursor + PEER_ID_LEN]);
            cursor += PEER_ID_LEN;
            Some(PeerId(id))
        } else {
            None
        };

        let payload = bytes[cursor..cursor + payload_len].to_vec();
        cursor += payload_len;

        let signature = if has_signature {
            let mut signature = [0u8; SIGNATURE_LEN];
            signature.copy_from_slice(&bytes[cursor..cursor + SIGNATURE_LEN]);
            cursor += SIGNATURE_LEN;
            Some(signature)
        } else {
            None
        };

        Ok((
            Self {
                kind,
                ttl,
                timestamp_ms,
                sender: PeerId(sender),
                recipient,
                payload,
                signature,
            },
            cursor,
        ))
    }

    /// The deduplication key: who sent it, when, what kind, and a digest of
    /// the payload.
    ///
    /// The TTL is excluded, so the same packet arriving by a long path and a
    /// short one is recognised as one packet rather than two.
    #[must_use]
    pub fn dedup_key(&self) -> DedupKey {
        use sha2::{Digest, Sha256};
        let digest = Sha256::digest(&self.payload);
        let mut payload_digest = [0u8; 16];
        payload_digest.copy_from_slice(&digest[..16]);
        DedupKey {
            sender: self.sender,
            timestamp_ms: self.timestamp_ms,
            kind: self.kind,
            payload_digest,
        }
    }
}

/// What makes two received packets the same packet.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct DedupKey {
    pub sender: PeerId,
    pub timestamp_ms: u64,
    pub kind: PacketKind,
    pub payload_digest: [u8; 16],
}

#[cfg(test)]
mod tests {
    use super::*;

    fn peer(seed: u8) -> PeerId {
        PeerId([seed; PEER_ID_LEN])
    }

    #[test]
    fn a_broadcast_round_trips() {
        let packet = Packet::broadcast(PacketKind::Intent, peer(1), 1_700_000_000_000, b"hi".into());
        let encoded = packet.encode().unwrap();
        assert_eq!(Packet::decode(&encoded).unwrap(), packet);
    }

    #[test]
    fn a_directed_signed_packet_round_trips() {
        let mut packet = Packet::directed(
            PacketKind::Sealed,
            peer(1),
            peer(2),
            42,
            vec![0xAB; 300],
        );
        packet.signature = Some([7u8; SIGNATURE_LEN]);

        let encoded = packet.encode().unwrap();
        assert_eq!(Packet::decode(&encoded).unwrap(), packet);
    }

    #[test]
    fn the_header_is_the_documented_size() {
        // The layout is duplicated in docs/ble-mesh-design.md and in the Swift
        // and Kotlin sides of the plugin. A change here without a change there
        // is a mesh that cannot parse itself.
        let packet = Packet::broadcast(PacketKind::Announce, peer(1), 0, Vec::new());
        assert_eq!(packet.encode().unwrap().len(), HEADER_LEN + PEER_ID_LEN);
        assert_eq!(HEADER_LEN, 14);
    }

    #[test]
    fn the_ttl_is_not_covered_by_the_signature() {
        // The property the whole relay model rests on: a hop may decrement TTL
        // without invalidating an end-to-end signature.
        let mut packet = Packet::broadcast(PacketKind::Intent, peer(1), 9, b"payload".into());
        let before = packet.signing_bytes().unwrap();
        packet.ttl -= 1;
        assert_eq!(packet.signing_bytes().unwrap(), before);
    }

    #[test]
    fn everything_else_is_covered_by_the_signature() {
        let base = Packet::broadcast(PacketKind::Intent, peer(1), 9, b"payload".into());
        let signed = base.signing_bytes().unwrap();

        let mut other_payload = base.clone();
        other_payload.payload = b"payloae".into();
        assert_ne!(other_payload.signing_bytes().unwrap(), signed);

        let mut other_sender = base.clone();
        other_sender.sender = peer(2);
        assert_ne!(other_sender.signing_bytes().unwrap(), signed);

        let mut other_kind = base.clone();
        other_kind.kind = PacketKind::GatewayRequest;
        assert_ne!(other_kind.signing_bytes().unwrap(), signed);

        let mut other_time = base.clone();
        other_time.timestamp_ms = 10;
        assert_ne!(other_time.signing_bytes().unwrap(), signed);

        let mut other_recipient = base;
        other_recipient.recipient = Some(peer(3));
        assert_ne!(other_recipient.signing_bytes().unwrap(), signed);
    }

    #[test]
    fn a_lying_length_field_is_refused_before_allocating() {
        // The attack this closes: a 22-byte frame claiming a 65535-byte
        // payload, repeated, until the phone is killed by the OS.
        let mut encoded = Packet::broadcast(PacketKind::Intent, peer(1), 0, Vec::new())
            .encode()
            .unwrap();
        encoded[12] = 0xFF;
        encoded[13] = 0xFF;

        assert!(matches!(
            Packet::decode(&encoded),
            Err(WireError::Truncated { .. })
        ));
    }

    #[test]
    fn reserved_flags_are_refused_rather_than_ignored() {
        let mut encoded = Packet::broadcast(PacketKind::Intent, peer(1), 0, Vec::new())
            .encode()
            .unwrap();
        encoded[11] |= 0b1000_0000;

        assert!(matches!(
            Packet::decode(&encoded),
            Err(WireError::ReservedFlags(_))
        ));
    }

    #[test]
    fn the_compression_flag_is_refused_by_this_build() {
        // Defined in the format so a later version can use it without a
        // version bump; refused here so this build never pretends to have
        // decompressed something.
        let mut encoded = Packet::broadcast(PacketKind::Intent, peer(1), 0, Vec::new())
            .encode()
            .unwrap();
        encoded[11] |= FLAG_COMPRESSED;

        assert_eq!(
            Packet::decode(&encoded),
            Err(WireError::CompressionUnsupported)
        );
    }

    #[test]
    fn an_unknown_kind_is_refused() {
        let mut encoded = Packet::broadcast(PacketKind::Intent, peer(1), 0, Vec::new())
            .encode()
            .unwrap();
        encoded[1] = 0x7F;

        assert_eq!(Packet::decode(&encoded), Err(WireError::UnknownKind(0x7F)));
    }

    #[test]
    fn a_future_version_is_refused() {
        let mut encoded = Packet::broadcast(PacketKind::Intent, peer(1), 0, Vec::new())
            .encode()
            .unwrap();
        encoded[0] = VERSION + 1;

        assert_eq!(Packet::decode(&encoded), Err(WireError::Version(VERSION + 1)));
    }

    #[test]
    fn trailing_bytes_are_refused() {
        let mut encoded = Packet::broadcast(PacketKind::Intent, peer(1), 0, b"x".into())
            .encode()
            .unwrap();
        encoded.push(0);

        assert_eq!(Packet::decode(&encoded), Err(WireError::TrailingBytes(1)));
    }

    #[test]
    fn decode_prefix_reports_what_it_consumed() {
        // How the link layer reads two packets that arrived in one read.
        let first = Packet::broadcast(PacketKind::Intent, peer(1), 1, b"one".into());
        let second = Packet::broadcast(PacketKind::Intent, peer(2), 2, b"two".into());

        let mut buffer = first.encode().unwrap();
        buffer.extend_from_slice(&second.encode().unwrap());

        let (decoded, consumed) = Packet::decode_prefix(&buffer).unwrap();
        assert_eq!(decoded, first);
        assert_eq!(Packet::decode(&buffer[consumed..]).unwrap(), second);
    }

    #[test]
    fn every_truncation_is_an_error_and_never_a_panic() {
        // Ranged over rather than spot-checked: the decoder reads at fixed
        // offsets, and one unchecked slice is a remote crash.
        let mut packet = Packet::directed(PacketKind::Sealed, peer(1), peer(2), 5, vec![9; 40]);
        packet.signature = Some([1u8; SIGNATURE_LEN]);
        let encoded = packet.encode().unwrap();

        for cut in 0..encoded.len() {
            assert!(
                Packet::decode(&encoded[..cut]).is_err(),
                "a {cut}-byte prefix decoded as a whole packet"
            );
        }
    }

    #[test]
    fn dedup_ignores_the_ttl() {
        // The same packet arriving by a two-hop path and a five-hop path is
        // one packet. Without this, flooding never terminates.
        let mut near = Packet::broadcast(PacketKind::Intent, peer(1), 77, b"same".into());
        let mut far = near.clone();
        near.ttl = 6;
        far.ttl = 2;

        assert_eq!(near.dedup_key(), far.dedup_key());
    }

    #[test]
    fn dedup_separates_different_payloads_from_the_same_sender() {
        let first = Packet::broadcast(PacketKind::Intent, peer(1), 77, b"a".into());
        let second = Packet::broadcast(PacketKind::Intent, peer(1), 77, b"b".into());

        assert_ne!(first.dedup_key(), second.dedup_key());
    }

    #[test]
    fn peer_ids_derive_from_the_key_and_display_as_hex() {
        let id = PeerId::from_public_key(&[0xAA; 32]);
        assert_eq!(id, PeerId::from_public_key(&[0xAA; 32]));
        assert_ne!(id, PeerId::from_public_key(&[0xAB; 32]));
        assert_eq!(id.to_string().len(), PEER_ID_LEN * 2);
    }

    #[test]
    fn a_peer_id_round_trips_through_its_own_display() {
        use std::str::FromStr;
        let id = PeerId::from_public_key(&[0x42; 32]);
        assert_eq!(PeerId::from_str(&id.to_string()).unwrap(), id);
    }

    #[test]
    fn parsing_a_peer_id_refuses_anything_that_is_not_valid_hex_of_the_right_length() {
        use std::str::FromStr;
        assert_eq!(PeerId::from_str("too short"), Err(WireError::InvalidPeerId));
        assert_eq!(PeerId::from_str("zz00000000000000"), Err(WireError::InvalidPeerId));
        assert_eq!(PeerId::from_str(""), Err(WireError::InvalidPeerId));
    }

    #[test]
    fn broadcast_and_fanout_classes_match_the_design() {
        assert!(PacketKind::Intent.is_broadcast());
        assert!(PacketKind::Announce.is_broadcast());
        assert!(!PacketKind::Sealed.is_broadcast());
        assert!(!PacketKind::IntentAck.is_broadcast());
        // Full-fanout even when used for the (broadcast, recipient: None)
        // unlock request — see the variant's doc comment for why.
        assert!(!PacketKind::Guardian.is_broadcast());

        // Announcements and gateway traffic must not be thinned: one is how
        // the mesh learns its shape, the other is how an offline node reaches
        // the chain.
        assert!(PacketKind::Announce.needs_full_fanout());
        assert!(PacketKind::GatewayRequest.needs_full_fanout());
        assert!(PacketKind::GatewayResult.needs_full_fanout());
        assert!(!PacketKind::Intent.needs_full_fanout());
    }
}
