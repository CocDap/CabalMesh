//! Who is out there, how far, and for how long.
//!
//! # Announcements, and what is deliberately not in them
//!
//! bitchat's whitepaper names its own weakest point plainly: the sender
//! identifier in every packet derives from a key that never rotates, and
//! announcements carry the static public key and a nickname in cleartext. A
//! passive listener enumerates who is present and follows a device across
//! places and days.
//!
//! For an app whose first sentence is "you are a Nobody", shipping that would
//! contradict the product. So an announcement here carries **only this
//! session's ephemeral keys**. No nickname, no wallet address, no durable
//! identifier of any kind. The durable identity — the NEAR account the IP
//! plane already publishes — travels as ciphertext inside an established
//! session, never in the clear.
//!
//! What that costs is real and is not hidden: a peer traded with yesterday is
//! a stranger today until the handshake runs again.
//!
//! # Neighbour lists
//!
//! Each announcement carries up to ten of the sender's own direct neighbours.
//! That is how a node learns the shape of the mesh two hops out without a
//! routing protocol — enough to know a directed packet has somewhere to go,
//! and not enough to be a routing table worth attacking.

use crate::wire::{PeerId, PEER_ID_LEN};
use std::collections::HashMap;
use std::time::Duration;

/// How long a peer stays reachable after its last verified announcement.
///
/// Long enough to ride out a missed announce and a duty-cycled scan; short
/// enough that someone who walked out of the room stops being listed as
/// present while they are visibly gone.
pub const REACHABLE_FOR: Duration = Duration::from_secs(60);

/// Announce interval while no peer is connected.
///
/// Fast, because this is the case where the user is staring at an empty screen
/// wondering whether the app works.
pub const ANNOUNCE_ISOLATED: Duration = Duration::from_secs(4);

/// Shortest announce interval once connected.
pub const ANNOUNCE_CONNECTED_MIN: Duration = Duration::from_secs(15);

/// Longest announce interval once connected.
pub const ANNOUNCE_CONNECTED_MAX: Duration = Duration::from_secs(30);

/// Neighbours carried in one announcement.
pub const MAX_ADVERTISED_NEIGHBOURS: usize = 10;

/// Length of each ephemeral public key.
pub const KEY_LEN: usize = 32;

/// What a node can do for others.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct Capabilities(u8);

impl Capabilities {
    /// This node has internet and will carry signed transactions to the chain.
    pub const GATEWAY: u8 = 0b0000_0001;

    /// No capabilities.
    #[must_use]
    pub const fn none() -> Self {
        Self(0)
    }

    /// From raw bits.
    #[must_use]
    pub const fn from_bits(bits: u8) -> Self {
        Self(bits)
    }

    /// The raw bits.
    #[must_use]
    pub const fn bits(self) -> u8 {
        self.0
    }

    /// Adds a capability.
    #[must_use]
    pub const fn with(self, bit: u8) -> Self {
        Self(self.0 | bit)
    }

    /// Whether this node offers a way to the internet.
    #[must_use]
    pub const fn is_gateway(self) -> bool {
        self.0 & Self::GATEWAY != 0
    }
}

/// A presence announcement.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Announce {
    /// This session's X25519 public key. The sender id derives from it.
    pub key_agreement: [u8; KEY_LEN],
    /// This session's Ed25519 public key, which signed the packet.
    pub signing: [u8; KEY_LEN],
    pub capabilities: Capabilities,
    /// Up to [`MAX_ADVERTISED_NEIGHBOURS`] of the sender's direct neighbours.
    pub neighbours: Vec<PeerId>,
}

/// Why an announcement could not be parsed.
#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
#[non_exhaustive]
pub enum AnnounceError {
    #[error("announcement truncated: need {need} bytes, have {have}")]
    Truncated { need: usize, have: usize },
    #[error("announcement claims {0} neighbours, more than the {MAX_ADVERTISED_NEIGHBOURS} allowed")]
    TooManyNeighbours(usize),
    #[error("{0} trailing bytes after announcement")]
    TrailingBytes(usize),
}

impl Announce {
    /// The identifier this announcement claims, derived from its own key.
    ///
    /// Derived rather than carried, so a peer cannot announce itself under
    /// somebody else's identifier.
    #[must_use]
    pub fn peer_id(&self) -> PeerId {
        PeerId::from_public_key(&self.key_agreement)
    }

    /// Serializes to an announcement payload.
    #[must_use]
    pub fn encode(&self) -> Vec<u8> {
        let neighbours = &self.neighbours[..self.neighbours.len().min(MAX_ADVERTISED_NEIGHBOURS)];
        let mut out = Vec::with_capacity(KEY_LEN * 2 + 2 + neighbours.len() * PEER_ID_LEN);
        out.extend_from_slice(&self.key_agreement);
        out.extend_from_slice(&self.signing);
        out.push(self.capabilities.bits());
        // Bounded by the slice above; the cast cannot truncate.
        out.push(neighbours.len() as u8);
        for neighbour in neighbours {
            out.extend_from_slice(neighbour.as_bytes());
        }
        out
    }

    /// Parses an announcement payload.
    ///
    /// # Errors
    ///
    /// [`AnnounceError`] on anything malformed. Never panics — this arrives
    /// over the air from an unauthenticated stranger.
    pub fn decode(bytes: &[u8]) -> Result<Self, AnnounceError> {
        let fixed = KEY_LEN * 2 + 2;
        if bytes.len() < fixed {
            return Err(AnnounceError::Truncated {
                need: fixed,
                have: bytes.len(),
            });
        }

        let mut key_agreement = [0u8; KEY_LEN];
        key_agreement.copy_from_slice(&bytes[..KEY_LEN]);
        let mut signing = [0u8; KEY_LEN];
        signing.copy_from_slice(&bytes[KEY_LEN..KEY_LEN * 2]);

        let capabilities = Capabilities::from_bits(bytes[KEY_LEN * 2]);
        let count = usize::from(bytes[KEY_LEN * 2 + 1]);
        if count > MAX_ADVERTISED_NEIGHBOURS {
            return Err(AnnounceError::TooManyNeighbours(count));
        }

        let need = fixed + count * PEER_ID_LEN;
        if bytes.len() < need {
            return Err(AnnounceError::Truncated {
                need,
                have: bytes.len(),
            });
        }
        if bytes.len() > need {
            return Err(AnnounceError::TrailingBytes(bytes.len() - need));
        }

        let neighbours = bytes[fixed..need]
            .chunks_exact(PEER_ID_LEN)
            .map(|chunk| {
                let mut id = [0u8; PEER_ID_LEN];
                id.copy_from_slice(chunk);
                PeerId(id)
            })
            .collect();

        Ok(Self {
            key_agreement,
            signing,
            capabilities,
            neighbours,
        })
    }
}

/// What is known about one peer.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct KnownPeer {
    pub id: PeerId,
    /// Hops away: 1 for a direct radio neighbour, 2 for a peer named in a
    /// neighbour's announcement, and no more than that — this is not a routing
    /// table.
    pub hops: u8,
    pub capabilities: Capabilities,
    /// When this peer was last heard from, in milliseconds since the epoch.
    pub last_seen_ms: u64,
}

impl KnownPeer {
    /// Whether this peer is still within its reachability window.
    #[must_use]
    pub fn is_reachable(&self, now_ms: u64) -> bool {
        // `saturating_sub` rather than a comparison, so a peer whose clock is
        // ahead of ours is treated as present rather than as an overflow.
        now_ms.saturating_sub(self.last_seen_ms) < REACHABLE_FOR.as_millis() as u64
    }
}

/// Everyone this node knows about.
#[derive(Debug, Default)]
pub struct PeerTable {
    /// This node, so it can be excluded from its own neighbour count.
    ///
    /// Every neighbour advertises *us* in its own neighbour list, so without
    /// this a two-node mesh reports three peers: the other node at one hop,
    /// and ourselves at two. It is the sort of error that looks like the mesh
    /// working better than it does.
    local: PeerId,
    peers: HashMap<PeerId, KnownPeer>,
    /// Each direct neighbour's own advertised neighbours.
    ///
    /// Kept because it answers the one question fanout thinning depends on:
    /// *is there another path to this peer, or am I it?* Thinning a link that
    /// nobody else covers is not a saving, it is a partition.
    advertised: HashMap<PeerId, Vec<PeerId>>,
}

impl PeerTable {
    /// An empty table for the given local node.
    #[must_use]
    pub fn new(local: PeerId) -> Self {
        Self {
            local,
            peers: HashMap::new(),
            advertised: HashMap::new(),
        }
    }

    /// Records a peer heard directly over the radio.
    pub fn observe_direct(&mut self, announce: &Announce, now_ms: u64) -> PeerId {
        let id = announce.peer_id();
        self.upsert(id, 1, announce.capabilities, now_ms);
        self.advertised.insert(id, announce.neighbours.clone());

        // A neighbour's neighbours are two hops away — unless we already hear
        // them ourselves, in which case one hop wins.
        for &neighbour in &announce.neighbours {
            if neighbour != id && neighbour != self.local {
                self.upsert(neighbour, 2, Capabilities::none(), now_ms);
            }
        }
        id
    }

    /// Records a peer heard about second-hand, from a relayed announcement.
    ///
    /// Separate from [`PeerTable::observe_direct`] because the difference
    /// matters to the user: "four nodes are in the room with you" and "four
    /// nodes exist somewhere in the mesh" are different claims, and only the
    /// first one is about a radio.
    pub fn observe_relayed(&mut self, announce: &Announce, now_ms: u64) -> PeerId {
        let id = announce.peer_id();
        self.upsert(id, 2, announce.capabilities, now_ms);
        id
    }

    /// Whether another of our direct neighbours also reaches `peer`.
    ///
    /// The question fanout thinning turns on. A peer only we can hear must
    /// always be sent to; a peer three of our neighbours also hear can be left
    /// to them.
    #[must_use]
    pub fn is_covered_by_others(&self, peer: PeerId) -> bool {
        self.advertised
            .iter()
            .any(|(neighbour, theirs)| *neighbour != peer && theirs.contains(&peer))
    }

    fn upsert(&mut self, id: PeerId, hops: u8, capabilities: Capabilities, now_ms: u64) {
        self.peers
            .entry(id)
            .and_modify(|peer| {
                peer.last_seen_ms = peer.last_seen_ms.max(now_ms);
                peer.hops = peer.hops.min(hops);
                if capabilities.bits() != 0 {
                    peer.capabilities = capabilities;
                }
            })
            .or_insert(KnownPeer {
                id,
                hops,
                capabilities,
                last_seen_ms: now_ms,
            });
    }

    /// Drops peers past their reachability window, returning who was dropped.
    ///
    /// Returned rather than silently removed so the UI can say a node left
    /// rather than having a row disappear between renders.
    pub fn expire(&mut self, now_ms: u64) -> Vec<PeerId> {
        let gone: Vec<PeerId> = self
            .peers
            .values()
            .filter(|peer| !peer.is_reachable(now_ms))
            .map(|peer| peer.id)
            .collect();
        for id in &gone {
            self.peers.remove(id);
            self.advertised.remove(id);
        }
        gone
    }

    /// Every reachable peer, nearest first, then by identifier so the order is
    /// stable between renders.
    #[must_use]
    pub fn reachable(&self, now_ms: u64) -> Vec<KnownPeer> {
        let mut peers: Vec<KnownPeer> = self
            .peers
            .values()
            .filter(|peer| peer.is_reachable(now_ms))
            .cloned()
            .collect();
        peers.sort_by(|a, b| a.hops.cmp(&b.hops).then(a.id.cmp(&b.id)));
        peers
    }

    /// Peers one hop away.
    #[must_use]
    pub fn direct(&self, now_ms: u64) -> Vec<PeerId> {
        self.reachable(now_ms)
            .into_iter()
            .filter(|peer| peer.hops == 1)
            .map(|peer| peer.id)
            .collect()
    }

    /// Reachable peers offering a way to the internet.
    #[must_use]
    pub fn gateways(&self, now_ms: u64) -> Vec<PeerId> {
        self.reachable(now_ms)
            .into_iter()
            .filter(|peer| peer.capabilities.is_gateway())
            .map(|peer| peer.id)
            .collect()
    }

    /// What this node should advertise as its own neighbours.
    #[must_use]
    pub fn advertised_neighbours(&self, now_ms: u64) -> Vec<PeerId> {
        let mut direct = self.direct(now_ms);
        direct.truncate(MAX_ADVERTISED_NEIGHBOURS);
        direct
    }

    /// How many peers are reachable.
    #[must_use]
    pub fn len(&self, now_ms: u64) -> usize {
        self.reachable(now_ms).len()
    }

    /// Whether anyone is reachable.
    #[must_use]
    pub fn is_empty(&self, now_ms: u64) -> bool {
        self.len(now_ms) == 0
    }
}

/// How long to wait before the next announcement.
///
/// The spread once connected is deterministic per node rather than random, for
/// the same reason the relay jitter is: two nodes should not announce in
/// lockstep, and a test should not have to tolerate a range.
#[must_use]
pub fn announce_interval(local: PeerId, connected: bool) -> Duration {
    if !connected {
        return ANNOUNCE_ISOLATED;
    }
    let span = ANNOUNCE_CONNECTED_MAX.as_secs() - ANNOUNCE_CONNECTED_MIN.as_secs();
    let offset = u64::from(local.as_bytes()[0]) % (span + 1);
    Duration::from_secs(ANNOUNCE_CONNECTED_MIN.as_secs() + offset)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn announce(seed: u8, neighbours: Vec<PeerId>) -> Announce {
        Announce {
            key_agreement: [seed; KEY_LEN],
            signing: [seed.wrapping_add(100); KEY_LEN],
            capabilities: Capabilities::none(),
            neighbours,
        }
    }

    #[test]
    fn an_announcement_round_trips() {
        let original = Announce {
            capabilities: Capabilities::none().with(Capabilities::GATEWAY),
            neighbours: vec![PeerId([1; 8]), PeerId([2; 8])],
            ..announce(9, Vec::new())
        };
        assert_eq!(Announce::decode(&original.encode()).unwrap(), original);
    }

    #[test]
    fn an_announcement_carries_no_durable_identity() {
        // The whole privacy argument in one assertion: the bytes on the air
        // contain the session keys and nothing that survives a restart.
        //
        // If a nickname or a wallet address is ever added to this struct, this
        // test is where the argument gets re-examined.
        let encoded = announce(1, vec![PeerId([7; 8])]).encode();
        assert_eq!(encoded.len(), KEY_LEN * 2 + 2 + PEER_ID_LEN);
    }

    #[test]
    fn the_identifier_is_derived_and_not_claimed() {
        // A peer must not be able to announce itself under somebody else's id.
        let announcement = announce(3, Vec::new());
        assert_eq!(
            announcement.peer_id(),
            PeerId::from_public_key(&announcement.key_agreement)
        );
    }

    #[test]
    fn a_truncated_announcement_is_refused() {
        let encoded = announce(1, vec![PeerId([7; 8])]).encode();
        for cut in 0..encoded.len() {
            assert!(Announce::decode(&encoded[..cut]).is_err(), "{cut}-byte prefix accepted");
        }
    }

    #[test]
    fn an_over_long_neighbour_list_is_refused_before_reading_it() {
        // The bound this enforces: an announcement claiming 255 neighbours is
        // 2 KiB of allocation per packet, from anyone in radio range.
        let mut encoded = announce(1, Vec::new()).encode();
        encoded[KEY_LEN * 2 + 1] = 255;

        assert_eq!(
            Announce::decode(&encoded),
            Err(AnnounceError::TooManyNeighbours(255))
        );
    }

    #[test]
    fn encoding_caps_the_neighbour_list() {
        let many: Vec<PeerId> = (0..50u8).map(|seed| PeerId([seed; 8])).collect();
        let decoded = Announce::decode(&announce(1, many).encode()).unwrap();
        assert_eq!(decoded.neighbours.len(), MAX_ADVERTISED_NEIGHBOURS);
    }

    #[test]
    fn a_direct_peer_is_one_hop_and_its_neighbours_are_two() {
        let mut table = PeerTable::new(PeerId([0xEE; 8]));
        let neighbour = PeerId([42; 8]);
        let id = table.observe_direct(&announce(1, vec![neighbour]), 1_000);

        let reachable = table.reachable(1_000);
        assert_eq!(reachable.len(), 2);
        assert_eq!(reachable[0].id, id);
        assert_eq!(reachable[0].hops, 1);
        assert_eq!(reachable[1].hops, 2);
    }

    #[test]
    fn a_node_is_never_its_own_peer() {
        // Every neighbour advertises *us* in its neighbour list, so a table
        // that does not exclude itself reports a two-node mesh as three: the
        // other node at one hop, and ourselves at two.
        //
        // Found by running two real nodes and reading `reachable=2` where one
        // was correct. It is the kind of error that makes the mesh look like
        // it is working better than it is, which is the worst kind to ship.
        let me = PeerId([0xEE; 8]);
        let mut table = PeerTable::new(me);

        table.observe_direct(&announce(1, vec![me]), 1_000);

        assert_eq!(table.len(1_000), 1, "the node counted itself");
        assert!(!table.reachable(1_000).iter().any(|peer| peer.id == me));
    }

    #[test]
    fn hearing_a_peer_directly_beats_hearing_about_it() {
        let mut table = PeerTable::new(PeerId([0xEE; 8]));
        let far = announce(2, Vec::new());

        table.observe_direct(&announce(1, vec![far.peer_id()]), 1_000);
        table.observe_direct(&far, 1_100);

        let direct = table.direct(1_100);
        assert!(direct.contains(&far.peer_id()), "a peer we can hear was still listed as two hops");
    }

    #[test]
    fn a_peer_expires_after_its_window_and_says_so() {
        let mut table = PeerTable::new(PeerId([0xEE; 8]));
        let id = table.observe_direct(&announce(1, Vec::new()), 1_000);

        let still_here = 1_000 + REACHABLE_FOR.as_millis() as u64 - 1;
        assert_eq!(table.expire(still_here), Vec::new());
        assert_eq!(table.len(still_here), 1);

        let gone = 1_000 + REACHABLE_FOR.as_millis() as u64;
        assert_eq!(table.expire(gone), vec![id]);
        assert!(table.is_empty(gone));
    }

    #[test]
    fn a_peer_with_a_clock_ahead_of_ours_is_present_rather_than_absent() {
        // Nothing here trusts another device's clock. A peer whose timestamp
        // is in our future must not underflow into "expired".
        let peer = KnownPeer {
            id: PeerId([1; 8]),
            hops: 1,
            capabilities: Capabilities::none(),
            last_seen_ms: 10_000,
        };
        assert!(peer.is_reachable(1_000));
    }

    #[test]
    fn gateways_are_listed_separately() {
        let mut table = PeerTable::new(PeerId([0xEE; 8]));
        table.observe_direct(&announce(1, Vec::new()), 1_000);
        table.observe_direct(
            &Announce {
                capabilities: Capabilities::none().with(Capabilities::GATEWAY),
                ..announce(2, Vec::new())
            },
            1_000,
        );

        assert_eq!(table.len(1_000), 2);
        assert_eq!(table.gateways(1_000).len(), 1);
    }

    #[test]
    fn the_advertised_neighbour_list_is_capped() {
        let mut table = PeerTable::new(PeerId([0xEE; 8]));
        for seed in 0..30u8 {
            table.observe_direct(&announce(seed, Vec::new()), 1_000);
        }
        assert_eq!(table.advertised_neighbours(1_000).len(), MAX_ADVERTISED_NEIGHBOURS);
    }

    #[test]
    fn peer_order_is_stable_between_renders() {
        // A list that reorders itself on every poll is a list nobody can tap.
        let mut table = PeerTable::new(PeerId([0xEE; 8]));
        for seed in 0..8u8 {
            table.observe_direct(&announce(seed, Vec::new()), 1_000);
        }
        assert_eq!(table.reachable(1_000), table.reachable(1_000));
    }

    #[test]
    fn an_isolated_node_announces_faster_than_a_connected_one() {
        let id = PeerId([5; 8]);
        assert_eq!(announce_interval(id, false), ANNOUNCE_ISOLATED);

        let connected = announce_interval(id, true);
        assert!(connected >= ANNOUNCE_CONNECTED_MIN);
        assert!(connected <= ANNOUNCE_CONNECTED_MAX);
    }

    #[test]
    fn connected_nodes_do_not_announce_in_lockstep() {
        let mut distinct = std::collections::HashSet::new();
        for seed in 0..32u8 {
            distinct.insert(announce_interval(PeerId([seed; 8]), true));
        }
        assert!(distinct.len() > 1, "every node announces on the same tick");
    }
}
