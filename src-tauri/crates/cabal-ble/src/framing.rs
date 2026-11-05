//! Packet boundaries on a byte stream.
//!
//! # Why there is no fragmentation layer here
//!
//! bitchat cuts every packet into ~469-byte pieces with an 8-byte fragment
//! identifier, and each node maintains up to 128 concurrent reassembly buffers
//! with a 30-second timeout and a 1 MiB cap. That exists because GATT
//! notifications are small, unordered messages rather than a stream.
//!
//! L2CAP connection-oriented channels are a stream: reliable, ordered,
//! flow-controlled, with SDUs up to 64 KiB. So the whole subsystem collapses
//! into a length prefix. A dropped link drops a partial frame and there is no
//! reassembly state to expire, no buffer budget to enforce, and no way for a
//! peer to hold memory open by sending fragment 1 of 200 and going quiet.
//!
//! That is the concrete return on the `minSdkVersion` 24 → 29 floor.

use crate::wire::{Packet, WireError, MAX_PACKET};

/// Bytes of length prefix before each packet.
pub const LENGTH_PREFIX: usize = 4;

/// Why a frame could not be read.
#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
#[non_exhaustive]
pub enum FrameError {
    /// A length prefix larger than any legal packet.
    ///
    /// Refused before allocating: the attack is a 4-byte write claiming 4 GiB,
    /// repeated, from anyone in radio range.
    #[error("frame claims {claimed} bytes, more than the {MAX_PACKET}-byte maximum")]
    TooLarge { claimed: u64 },

    /// The frame was the right size but was not a packet.
    #[error("frame did not decode: {0}")]
    Packet(#[from] WireError),
}

/// Wraps a packet for the wire.
///
/// # Errors
///
/// Propagates [`WireError`] if the packet cannot be encoded.
pub fn encode_frame(packet: &Packet) -> Result<Vec<u8>, WireError> {
    let body = packet.encode()?;
    let mut frame = Vec::with_capacity(LENGTH_PREFIX + body.len());
    // The cast is bounded by MAX_PACKET, which is far below u32::MAX.
    frame.extend_from_slice(&(body.len() as u32).to_be_bytes());
    frame.extend_from_slice(&body);
    Ok(frame)
}

/// Accumulates bytes from one link and yields whole packets.
///
/// One per link. A link that dies takes its reader with it, which is what
/// makes a partial frame cost nothing to clean up.
#[derive(Debug, Default)]
pub struct FrameReader {
    buffer: Vec<u8>,
}

impl FrameReader {
    /// An empty reader.
    #[must_use]
    pub fn new() -> Self {
        Self::default()
    }

    /// Adds bytes that just arrived.
    pub fn push(&mut self, bytes: &[u8]) {
        self.buffer.extend_from_slice(bytes);
    }

    /// Takes the next complete packet, if one has arrived.
    ///
    /// Returns `Ok(None)` when more bytes are needed — the ordinary case on a
    /// stream, not a failure.
    ///
    /// # Errors
    ///
    /// [`FrameError::TooLarge`] for an impossible length, or
    /// [`FrameError::Packet`] for a frame that is not a packet. Either means
    /// the peer is broken or hostile and the link should be dropped: the
    /// stream is no longer synchronised, so the next length prefix cannot be
    /// trusted either.
    pub fn next_packet(&mut self) -> Result<Option<Packet>, FrameError> {
        if self.buffer.len() < LENGTH_PREFIX {
            return Ok(None);
        }

        let mut prefix = [0u8; LENGTH_PREFIX];
        prefix.copy_from_slice(&self.buffer[..LENGTH_PREFIX]);
        let claimed = u64::from(u32::from_be_bytes(prefix));

        if claimed > MAX_PACKET as u64 {
            return Err(FrameError::TooLarge { claimed });
        }
        // The cast is bounded by the check above.
        let claimed = claimed as usize;

        if self.buffer.len() < LENGTH_PREFIX + claimed {
            return Ok(None);
        }

        let packet = Packet::decode(&self.buffer[LENGTH_PREFIX..LENGTH_PREFIX + claimed])?;
        self.buffer.drain(..LENGTH_PREFIX + claimed);
        Ok(Some(packet))
    }

    /// Bytes held pending a complete frame.
    ///
    /// Bounded by [`MAX_PACKET`] plus one prefix in normal operation, because
    /// a longer claim is refused rather than buffered.
    #[must_use]
    pub fn buffered(&self) -> usize {
        self.buffer.len()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::wire::{PacketKind, PeerId};

    fn packet(seed: u8, payload: &[u8]) -> Packet {
        Packet::broadcast(PacketKind::Intent, PeerId([seed; 8]), 1, payload.to_vec())
    }

    #[test]
    fn a_whole_frame_yields_its_packet() {
        let original = packet(1, b"hello");
        let mut reader = FrameReader::new();
        reader.push(&encode_frame(&original).unwrap());

        assert_eq!(reader.next_packet().unwrap(), Some(original));
        assert_eq!(reader.next_packet().unwrap(), None);
        assert_eq!(reader.buffered(), 0);
    }

    #[test]
    fn two_packets_in_one_read_both_come_out() {
        let first = packet(1, b"one");
        let second = packet(2, b"two");

        let mut reader = FrameReader::new();
        let mut bytes = encode_frame(&first).unwrap();
        bytes.extend_from_slice(&encode_frame(&second).unwrap());
        reader.push(&bytes);

        assert_eq!(reader.next_packet().unwrap(), Some(first));
        assert_eq!(reader.next_packet().unwrap(), Some(second));
        assert_eq!(reader.next_packet().unwrap(), None);
    }

    #[test]
    fn a_packet_split_across_every_possible_boundary_still_arrives() {
        // A stream gives no guarantee about where a read ends. Spot-checking
        // one split would pass while the reader was broken at another.
        let original = packet(1, b"a payload long enough to be split in many places");
        let frame = encode_frame(&original).unwrap();

        for split in 0..frame.len() {
            let mut reader = FrameReader::new();
            reader.push(&frame[..split]);
            assert_eq!(reader.next_packet().unwrap(), None, "yielded early at {split}");
            reader.push(&frame[split..]);
            assert_eq!(reader.next_packet().unwrap(), Some(original.clone()), "lost at {split}");
        }
    }

    #[test]
    fn a_byte_at_a_time_still_arrives() {
        let original = packet(1, b"drip");
        let frame = encode_frame(&original).unwrap();

        let mut reader = FrameReader::new();
        for byte in &frame[..frame.len() - 1] {
            reader.push(std::slice::from_ref(byte));
            assert_eq!(reader.next_packet().unwrap(), None);
        }
        reader.push(&frame[frame.len() - 1..]);
        assert_eq!(reader.next_packet().unwrap(), Some(original));
    }

    #[test]
    fn an_impossible_length_is_refused_before_allocating() {
        // The attack: four bytes claiming four gigabytes, repeated.
        let mut reader = FrameReader::new();
        reader.push(&u32::MAX.to_be_bytes());

        assert!(matches!(
            reader.next_packet(),
            Err(FrameError::TooLarge { .. })
        ));
    }

    #[test]
    fn a_frame_that_is_not_a_packet_is_an_error() {
        let mut reader = FrameReader::new();
        reader.push(&4u32.to_be_bytes());
        reader.push(&[0xFF, 0xFF, 0xFF, 0xFF]);

        assert!(matches!(reader.next_packet(), Err(FrameError::Packet(_))));
    }

    #[test]
    fn a_reader_holds_no_more_than_one_oversized_claim() {
        // What bounds memory per link: anything longer than a legal packet is
        // refused rather than buffered while the sender goes quiet.
        let mut reader = FrameReader::new();
        reader.push(&((MAX_PACKET as u32) + 1).to_be_bytes());
        assert!(reader.next_packet().is_err());
    }
}
