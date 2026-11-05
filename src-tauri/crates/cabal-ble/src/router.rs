//! Flood routing, and the four things that stop a flood from being a storm.
//!
//! # Why flooding at all
//!
//! gossipsub maintains a mesh: it heartbeats every second, grafts and prunes
//! toward a target degree, and gossips message identifiers to peers that might
//! be missing them. On a link that carries 100–300 kbit/s shared across every
//! neighbour in the room, that maintenance *is* the traffic.
//!
//! Flooding has no maintenance. It needs four suppressions instead, and all
//! four are here:
//!
//! 1. **Deduplication** — a packet already seen is never relayed again.
//! 2. **TTL** — a bound on how far a packet travels even if suppression fails.
//! 3. **Jitter with cancellation** — relays wait a short random delay, and a
//!    scheduled relay is cancelled when the same packet arrives from somebody
//!    else first. This is what actually keeps a dense room quiet; the TTL is
//!    only the backstop.
//! 4. **Fanout subsetting** — a broadcast is re-sent to a subset of links, not
//!    all of them.
//!
//! # Why the "random" delay is not random
//!
//! Jitter exists to decorrelate transmissions, not to be unpredictable. It is
//! derived by hashing the packet's identity together with this node's, so two
//! nodes pick different delays for the same packet while each node's choice is
//! reproducible. That costs no `rand` dependency and, far more usefully, makes
//! a twenty-node flood a deterministic test rather than a flake.
//!
//! # Parameters
//!
//! The numbers below are bitchat's, adopted rather than re-derived. They are
//! what a large deployment converged on; inventing different ones without a
//! testbed of comparable size would be worse, not braver.

use crate::wire::{DedupKey, Packet, PacketKind, PeerId};
use std::collections::{HashMap, VecDeque};
use std::time::Duration;

/// Hops a packet starts with.
pub const DEFAULT_TTL: u8 = 7;

/// Neighbour count at or above which a room counts as dense.
pub const DENSE_DEGREE: usize = 6;

/// TTL a broadcast is clamped to in a dense room.
///
/// In a crowd, a packet reaches everyone in far fewer hops than in a chain, so
/// the extra hops only cost transmissions.
pub const DENSE_TTL_CLAMP: u8 = 5;

/// Neighbour count at or below which a node counts as a sparse chain link.
///
/// A node with one or two neighbours is probably a bridge between two clumps.
/// Thinning its relays is how a mesh partitions.
pub const SPARSE_DEGREE: usize = 2;

/// Shortest relay delay.
pub const JITTER_MIN: Duration = Duration::from_millis(10);

/// Longest relay delay, in a dense room.
pub const JITTER_MAX: Duration = Duration::from_millis(220);

/// Relay delay for directed traffic, which nobody else will relay for us.
pub const DIRECTED_JITTER_MAX: Duration = Duration::from_millis(40);

/// How many recently seen packets are remembered.
pub const SEEN_CAPACITY: usize = 1000;

/// How long a packet stays remembered.
pub const SEEN_TTL: Duration = Duration::from_secs(300);

/// A link to one neighbour.
///
/// Opaque and assigned by the runtime: the protocol never needs to know
/// whether a link is an L2CAP channel, a TCP socket in a test, or an in-memory
/// queue in a simulation.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub struct LinkId(pub u64);

/// What to do with a packet that just arrived.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Decision {
    /// New, and this node is an intended recipient. Hand it up, and relay if
    /// `relay` says so.
    Accept {
        /// Present when the packet should also be forwarded.
        relay: Option<Relay>,
    },
    /// New, but addressed to somebody else. Forward without handing it up.
    Forward(Relay),
    /// Nothing to do, with the reason — which is worth keeping, because "the
    /// mesh is quiet" and "every packet is being dropped as a duplicate" look
    /// identical from outside.
    Drop(DropReason),
}

/// A forwarding instruction.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Relay {
    /// Links to send on. Never includes the link the packet arrived on.
    pub links: Vec<LinkId>,
    /// How long to wait first. A duplicate arriving inside this window
    /// cancels the send.
    pub delay: Duration,
    /// TTL the forwarded copy should carry.
    pub ttl: u8,
}

/// Why a packet went no further.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum DropReason {
    /// Seen before. The common case, and not a problem.
    Duplicate,
    /// Out of hops.
    TtlExhausted,
    /// This node sent it; it has come back around.
    Loop,
    /// Nowhere left to send it.
    NoLinks,
}

/// Remembers recently seen packets, bounded in both count and age.
///
/// A `HashMap` for the lookup and a `VecDeque` for the order. Not an `lru`
/// crate: this is thirty lines, and a dependency that reaches the phone should
/// buy more than that.
#[derive(Debug, Default)]
struct SeenSet {
    order: VecDeque<DedupKey>,
    at: HashMap<DedupKey, u64>,
}

impl SeenSet {
    /// Records a key, returning whether it was already present.
    fn insert(&mut self, key: DedupKey, now_ms: u64) -> bool {
        self.expire(now_ms);

        if let Some(seen_at) = self.at.get_mut(&key) {
            // Refreshed rather than left to expire: a packet still circulating
            // is a packet still worth suppressing.
            *seen_at = now_ms;
            return true;
        }

        if self.order.len() >= SEEN_CAPACITY {
            if let Some(oldest) = self.order.pop_front() {
                self.at.remove(&oldest);
            }
        }
        self.order.push_back(key);
        self.at.insert(key, now_ms);
        false
    }

    fn contains(&self, key: &DedupKey) -> bool {
        self.at.contains_key(key)
    }

    fn expire(&mut self, now_ms: u64) {
        let horizon = now_ms.saturating_sub(
            u64::try_from(SEEN_TTL.as_millis()).unwrap_or(u64::MAX),
        );
        while let Some(oldest) = self.order.front() {
            match self.at.get(oldest) {
                Some(&seen_at) if seen_at <= horizon => {
                    let oldest = *oldest;
                    self.order.pop_front();
                    self.at.remove(&oldest);
                }
                // Refreshed entries can sit out of order; keep them and let
                // capacity eviction handle the tail.
                _ => break,
            }
        }
    }

    fn len(&self) -> usize {
        self.order.len()
    }
}

/// Routing state for one node.
#[derive(Debug)]
pub struct Router {
    local: PeerId,
    seen: SeenSet,
    /// Packets this node has scheduled to relay but not yet sent. A duplicate
    /// arriving first removes the entry, which is how the send is cancelled.
    scheduled: HashMap<DedupKey, Relay>,
    relayed: u64,
    suppressed: u64,
}

impl Router {
    /// A router for the given local identity.
    #[must_use]
    pub fn new(local: PeerId) -> Self {
        Self {
            local,
            seen: SeenSet::default(),
            scheduled: HashMap::new(),
            relayed: 0,
            suppressed: 0,
        }
    }

    /// Decides what happens to a packet that arrived on `ingress`.
    ///
    /// `links` is every link currently up, including the ingress one, which
    /// this function excludes itself — a caller that had to remember to do so
    /// would eventually forget, and the symptom is a packet echoing back to
    /// the neighbour that just sent it.
    /// `thinnable` names the links whose peer is also reachable through
    /// another neighbour. Only those may be dropped from a broadcast's fanout;
    /// a link nobody else covers is not a saving, it is a partition.
    pub fn receive(
        &mut self,
        packet: &Packet,
        ingress: LinkId,
        links: &[LinkId],
        thinnable: &[LinkId],
        now_ms: u64,
    ) -> Decision {
        let key = packet.dedup_key();

        if packet.sender == self.local {
            return Decision::Drop(DropReason::Loop);
        }

        if self.seen.insert(key, now_ms) {
            // A duplicate means the neighbour it came from already has this
            // packet, so the pending send *to that neighbour* is pure cost.
            //
            // It proves nothing about the others. Cancelling the whole relay —
            // which is the obvious implementation, and the one this code had
            // first — silently strands every peer that only this node reaches:
            // in a line of twenty, a node hearing the same packet from both
            // sides dropped its forward to the chain's far end and five nodes
            // never saw it. The symptom read as a delivery bug, not a routing
            // policy.
            if let Some(relay) = self.scheduled.get_mut(&key) {
                let before = relay.links.len();
                relay.links.retain(|&link| link != ingress);
                if relay.links.len() != before {
                    self.suppressed = self.suppressed.saturating_add(1);
                }
                if relay.links.is_empty() {
                    self.scheduled.remove(&key);
                }
            }
            return Decision::Drop(DropReason::Duplicate);
        }

        let for_us = match packet.recipient {
            None => true,
            Some(recipient) => recipient == self.local,
        };

        // A packet addressed to this node has arrived. Relaying it further
        // would only tell the room who it was for.
        if for_us && packet.recipient.is_some() {
            return Decision::Accept { relay: None };
        }

        let relay = self.plan_relay(packet, &key, ingress, links, thinnable);

        if for_us {
            Decision::Accept { relay }
        } else {
            match relay {
                Some(relay) => Decision::Forward(relay),
                None => Decision::Drop(DropReason::TtlExhausted),
            }
        }
    }

    /// Plans the forwarding of a packet, or declines to.
    fn plan_relay(
        &mut self,
        packet: &Packet,
        key: &DedupKey,
        ingress: LinkId,
        links: &[LinkId],
        thinnable: &[LinkId],
    ) -> Option<Relay> {
        if packet.ttl <= 1 {
            return None;
        }

        let outgoing: Vec<LinkId> = links.iter().copied().filter(|&l| l != ingress).collect();
        if outgoing.is_empty() {
            return None;
        }

        let degree = links.len();
        let ttl = next_ttl(packet.ttl, packet.kind, degree);
        let chosen = if packet.kind.needs_full_fanout() || !packet.kind.is_broadcast() {
            outgoing
        } else {
            // Only links somebody else also covers may be thinned. In a star
            // the hub covers every leaf alone, so nothing is thinnable and the
            // broadcast reaches all of them; in a crowded room almost
            // everything is covered several times over and most of it can go.
            //
            // Without this split, fanout subsetting silently drops a third of
            // a star's leaves and looks like a delivery bug rather than a
            // routing policy.
            let (candidates, mandatory): (Vec<LinkId>, Vec<LinkId>) = outgoing
                .into_iter()
                .partition(|link| thinnable.contains(link));
            let mut chosen = fanout_subset(&candidates, key, self.local);
            chosen.extend(mandatory);
            chosen.sort_unstable();
            chosen.dedup();
            chosen
        };
        if chosen.is_empty() {
            return None;
        }

        let relay = Relay {
            links: chosen,
            delay: jitter(key, self.local, degree, packet.kind),
            ttl,
        };
        self.scheduled.insert(*key, relay.clone());
        Some(relay)
    }

    /// Confirms a scheduled relay is still wanted, at the moment of sending.
    ///
    /// Returns `None` when a duplicate arrived during the jitter window. The
    /// runtime calls this rather than trusting its own timer, so cancellation
    /// has exactly one source of truth.
    pub fn take_scheduled(&mut self, key: &DedupKey) -> Option<Relay> {
        let relay = self.scheduled.remove(key)?;
        self.relayed = self.relayed.saturating_add(1);
        Some(relay)
    }

    /// Records a packet this node originated, so its own flood does not come
    /// back to it as new.
    pub fn note_origin(&mut self, packet: &Packet, now_ms: u64) {
        self.seen.insert(packet.dedup_key(), now_ms);
    }

    /// Whether a packet has been seen recently.
    #[must_use]
    pub fn has_seen(&self, key: &DedupKey) -> bool {
        self.seen.contains(key)
    }

    /// Counters for the mesh status display: relays performed, and relays
    /// cancelled because somebody else was faster.
    #[must_use]
    pub fn counters(&self) -> RouterCounters {
        RouterCounters {
            relayed: self.relayed,
            suppressed: self.suppressed,
            remembered: self.seen.len(),
        }
    }
}

/// What the router has been doing, for the status display.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct RouterCounters {
    pub relayed: u64,
    pub suppressed: u64,
    pub remembered: usize,
}

/// The TTL a forwarded copy should carry.
///
/// Directed traffic always steps down by one. Broadcasts are clamped in a
/// dense room, and left alone in a sparse one — a node with two neighbours is
/// probably the only path between two groups.
#[must_use]
pub fn next_ttl(incoming: u8, kind: PacketKind, degree: usize) -> u8 {
    let stepped = incoming.saturating_sub(1);
    if !kind.is_broadcast() {
        return stepped;
    }
    if degree >= DENSE_DEGREE {
        stepped.min(DENSE_TTL_CLAMP)
    } else {
        stepped
    }
}

/// Chooses which links a broadcast is re-sent on.
///
/// About `log2(degree)` of them, at least one, picked by hashing the packet's
/// identity with this node's. Different packets take different subsets, so
/// coverage comes from many packets rather than from any one, and two nodes
/// with the same neighbours do not choose the same subset.
#[must_use]
pub fn fanout_subset(links: &[LinkId], key: &DedupKey, local: PeerId) -> Vec<LinkId> {
    if links.len() <= 2 {
        return links.to_vec();
    }

    let want = (usize::BITS - links.len().leading_zeros()) as usize;
    let want = want.clamp(1, links.len());

    let mut ordered: Vec<(u64, LinkId)> = links
        .iter()
        .map(|&link| (mix(key, local, link.0), link))
        .collect();
    ordered.sort_unstable();
    ordered.into_iter().take(want).map(|(_, link)| link).collect()
}

/// How long to wait before relaying.
///
/// Wider in a dense room, because that is where duplicates are most likely to
/// arrive and cancel the send before it costs anything.
#[must_use]
pub fn jitter(key: &DedupKey, local: PeerId, degree: usize, kind: PacketKind) -> Duration {
    let max = if kind.is_broadcast() {
        let span = JITTER_MAX.as_millis() - JITTER_MIN.as_millis();
        let scaled = span * (degree.min(DENSE_DEGREE) as u128) / (DENSE_DEGREE as u128);
        JITTER_MIN.as_millis() + scaled
    } else {
        DIRECTED_JITTER_MAX.as_millis()
    };

    let min = JITTER_MIN.as_millis();
    let span = max.saturating_sub(min).max(1);
    let offset = u128::from(mix(key, local, 0)) % span;
    Duration::from_millis(u64::try_from(min + offset).unwrap_or(u64::MAX))
}

/// A cheap deterministic mixer over the packet identity, this node, and a salt.
fn mix(key: &DedupKey, local: PeerId, salt: u64) -> u64 {
    use sha2::{Digest, Sha256};
    let mut hasher = Sha256::new();
    hasher.update(key.sender.as_bytes());
    hasher.update(key.timestamp_ms.to_be_bytes());
    hasher.update([key.kind as u8]);
    hasher.update(key.payload_digest);
    hasher.update(local.as_bytes());
    hasher.update(salt.to_be_bytes());
    let digest = hasher.finalize();
    let mut out = [0u8; 8];
    out.copy_from_slice(&digest[..8]);
    u64::from_be_bytes(out)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn peer(seed: u8) -> PeerId {
        PeerId([seed; 8])
    }

    fn links(count: u64) -> Vec<LinkId> {
        (0..count).map(LinkId).collect()
    }

    fn intent(sender: PeerId, timestamp_ms: u64) -> Packet {
        Packet::broadcast(PacketKind::Intent, sender, timestamp_ms, b"payload".into())
    }

    #[test]
    fn a_new_broadcast_is_accepted_and_relayed() {
        let mut router = Router::new(peer(0));
        let decision = router.receive(&intent(peer(1), 1), LinkId(0), &links(3), &[], 1_000);

        let Decision::Accept { relay: Some(relay) } = decision else {
            panic!("expected accept with relay, got {decision:?}");
        };
        assert!(!relay.links.contains(&LinkId(0)), "relayed back to the sender");
        assert_eq!(relay.ttl, DEFAULT_TTL - 1);
    }

    #[test]
    fn the_same_packet_twice_is_relayed_once() {
        // The property that makes flooding survivable. Without it, a room of
        // n phones turns one intent into n! transmissions.
        let mut router = Router::new(peer(0));
        let packet = intent(peer(1), 1);

        assert!(matches!(
            router.receive(&packet, LinkId(0), &links(3), &[], 1_000),
            Decision::Accept { relay: Some(_) }
        ));
        assert_eq!(
            router.receive(&packet, LinkId(1), &links(3), &[], 1_010),
            Decision::Drop(DropReason::Duplicate)
        );
    }

    #[test]
    fn a_duplicate_drops_the_link_it_came_from_and_leaves_the_rest() {
        // A duplicate proves one neighbour has the packet. It proves nothing
        // about the others, and cancelling their sends too is how a node at
        // the end of a chain never hears anything.
        let mut router = Router::new(peer(0));
        let packet = intent(peer(1), 1);
        let key = packet.dedup_key();

        router.receive(&packet, LinkId(0), &links(4), &[], 1_000);
        router.receive(&packet, LinkId(1), &links(4), &[], 1_005);

        let relay = router
            .take_scheduled(&key)
            .expect("the remaining links were cancelled along with the duplicate's");
        assert!(!relay.links.contains(&LinkId(0)), "relayed back to the ingress");
        assert!(!relay.links.contains(&LinkId(1)), "relayed to a peer that already has it");
        assert_eq!(relay.links, vec![LinkId(2), LinkId(3)]);
        assert_eq!(router.counters().suppressed, 1);
    }

    #[test]
    fn duplicates_from_every_neighbour_cancel_the_relay_entirely() {
        // The case the coarse version got right: when everybody already has
        // it, there is nobody left to send to.
        let mut router = Router::new(peer(0));
        let packet = intent(peer(1), 1);
        let key = packet.dedup_key();

        router.receive(&packet, LinkId(0), &links(3), &[], 1_000);
        router.receive(&packet, LinkId(1), &links(3), &[], 1_005);
        router.receive(&packet, LinkId(2), &links(3), &[], 1_010);

        assert!(
            router.take_scheduled(&key).is_none(),
            "a fully covered relay was still sent"
        );
        assert_eq!(router.counters().suppressed, 2);
        assert_eq!(router.counters().relayed, 0);
    }

    #[test]
    fn an_uncancelled_relay_is_sent_once_and_only_once() {
        let mut router = Router::new(peer(0));
        let packet = intent(peer(1), 1);
        let key = packet.dedup_key();

        router.receive(&packet, LinkId(0), &links(3), &[], 1_000);

        assert!(router.take_scheduled(&key).is_some());
        assert!(
            router.take_scheduled(&key).is_none(),
            "the same relay was taken twice"
        );
        assert_eq!(router.counters().relayed, 1);
    }

    #[test]
    fn a_packet_at_its_last_hop_is_delivered_but_not_forwarded() {
        let mut router = Router::new(peer(0));
        let mut packet = intent(peer(1), 1);
        packet.ttl = 1;

        assert_eq!(
            router.receive(&packet, LinkId(0), &links(3), &[], 1_000),
            Decision::Accept { relay: None }
        );
    }

    #[test]
    fn a_node_never_relays_its_own_packet_back() {
        let mut router = Router::new(peer(0));
        let own = intent(peer(0), 1);

        assert_eq!(
            router.receive(&own, LinkId(0), &links(3), &[], 1_000),
            Decision::Drop(DropReason::Loop)
        );
    }

    #[test]
    fn a_packet_for_someone_else_is_forwarded_without_being_delivered() {
        let mut router = Router::new(peer(0));
        let packet = Packet::directed(PacketKind::Sealed, peer(1), peer(2), 1, b"x".into());

        assert!(matches!(
            router.receive(&packet, LinkId(0), &links(3), &[], 1_000),
            Decision::Forward(_)
        ));
    }

    #[test]
    fn a_directed_packet_that_arrived_is_not_forwarded_further() {
        // Forwarding it on would tell the rest of the room who it was for.
        let mut router = Router::new(peer(2));
        let packet = Packet::directed(PacketKind::Sealed, peer(1), peer(2), 1, b"x".into());

        assert_eq!(
            router.receive(&packet, LinkId(0), &links(3), &[], 1_000),
            Decision::Accept { relay: None }
        );
    }

    #[test]
    fn a_node_with_only_the_ingress_link_forwards_nowhere() {
        let mut router = Router::new(peer(0));
        let decision = router.receive(&intent(peer(1), 1), LinkId(0), &[LinkId(0)], &[], 1_000);

        assert_eq!(decision, Decision::Accept { relay: None });
    }

    #[test]
    fn a_dense_room_clamps_the_ttl_and_a_sparse_chain_does_not() {
        // A crowd reaches everyone in few hops; a chain needs all of them.
        assert_eq!(next_ttl(7, PacketKind::Intent, DENSE_DEGREE), DENSE_TTL_CLAMP);
        assert_eq!(next_ttl(7, PacketKind::Intent, SPARSE_DEGREE), 6);
        // Directed traffic steps down regardless of how crowded it is.
        assert_eq!(next_ttl(7, PacketKind::Sealed, DENSE_DEGREE), 6);
    }

    #[test]
    fn ttl_never_underflows() {
        assert_eq!(next_ttl(0, PacketKind::Intent, 1), 0);
        assert_eq!(next_ttl(1, PacketKind::Sealed, 9), 0);
    }

    #[test]
    fn fanout_thins_a_broadcast_but_never_to_nothing() {
        let all = links(16);
        let key = intent(peer(1), 1).dedup_key();
        let chosen = fanout_subset(&all, &key, peer(0));

        assert!(!chosen.is_empty());
        assert!(chosen.len() < all.len(), "fanout did not thin anything");
        assert!(chosen.iter().all(|link| all.contains(link)));
    }

    #[test]
    fn fanout_keeps_every_link_when_there_are_only_a_couple() {
        // Thinning a two-link node is how a chain breaks.
        assert_eq!(fanout_subset(&links(2), &intent(peer(1), 1).dedup_key(), peer(0)).len(), 2);
        assert_eq!(fanout_subset(&links(1), &intent(peer(1), 1).dedup_key(), peer(0)).len(), 1);
    }

    #[test]
    fn different_nodes_choose_different_subsets() {
        // Coverage comes from nodes disagreeing. If every node picked the same
        // links, a subset would be a permanent partition rather than a sample.
        let all = links(16);
        let key = intent(peer(1), 1).dedup_key();

        let mut distinct = std::collections::HashSet::new();
        for seed in 0..12u8 {
            distinct.insert(fanout_subset(&all, &key, peer(seed)));
        }
        assert!(distinct.len() > 1, "every node chose the same links");
    }

    #[test]
    fn different_packets_choose_different_subsets() {
        let all = links(16);
        let mut distinct = std::collections::HashSet::new();
        for timestamp in 0..12u64 {
            distinct.insert(fanout_subset(&all, &intent(peer(1), timestamp).dedup_key(), peer(0)));
        }
        assert!(distinct.len() > 1, "every packet chose the same links");
    }

    #[test]
    fn fanout_is_reproducible() {
        // Determinism is what makes the multi-node simulation a test rather
        // than a flake.
        let all = links(16);
        let key = intent(peer(1), 1).dedup_key();
        assert_eq!(fanout_subset(&all, &key, peer(0)), fanout_subset(&all, &key, peer(0)));
    }

    #[test]
    fn jitter_stays_inside_its_documented_bounds() {
        for seed in 0..64u8 {
            for degree in 0..12usize {
                let delay = jitter(&intent(peer(seed), 1).dedup_key(), peer(0), degree, PacketKind::Intent);
                assert!(delay >= JITTER_MIN, "{delay:?} below the floor");
                assert!(delay <= JITTER_MAX, "{delay:?} above the ceiling");
            }
        }
    }

    #[test]
    fn directed_jitter_is_tighter_than_broadcast_jitter() {
        // Nobody else will relay a directed packet, so waiting to be
        // pre-empted only adds latency.
        for seed in 0..32u8 {
            let key = intent(peer(seed), 1).dedup_key();
            let directed = jitter(&key, peer(0), 8, PacketKind::Sealed);
            assert!(directed <= DIRECTED_JITTER_MAX, "{directed:?} too long for directed traffic");
        }
    }

    #[test]
    fn two_nodes_pick_different_delays_for_the_same_packet() {
        // The entire point of jitter. Identical delays mean simultaneous
        // transmissions, which is the collision it exists to avoid.
        let key = intent(peer(1), 1).dedup_key();
        let mut distinct = std::collections::HashSet::new();
        for seed in 0..16u8 {
            distinct.insert(jitter(&key, peer(seed), 6, PacketKind::Intent));
        }
        assert!(distinct.len() > 8, "delays clustered: {} distinct of 16", distinct.len());
    }

    #[test]
    fn the_seen_set_is_bounded() {
        // A full set must degrade to more duplicate relays, never to
        // unbounded memory on a 2 GB phone.
        let mut router = Router::new(peer(0));
        for timestamp in 0..(SEEN_CAPACITY as u64 * 3) {
            router.receive(&intent(peer(1), timestamp), LinkId(0), &links(2), &[], 1_000);
        }
        assert!(router.counters().remembered <= SEEN_CAPACITY);
    }

    #[test]
    fn a_packet_is_forgotten_after_its_window() {
        let mut router = Router::new(peer(0));
        let packet = intent(peer(1), 1);

        router.receive(&packet, LinkId(0), &links(2), &[], 1_000);
        assert!(router.has_seen(&packet.dedup_key()));

        // One millisecond past the window, arriving again is new again.
        let later = 1_000 + SEEN_TTL.as_millis() as u64 + 1;
        assert!(matches!(
            router.receive(&packet, LinkId(0), &links(2), &[], later),
            Decision::Accept { .. }
        ));
    }

    #[test]
    fn a_node_does_not_treat_its_own_flood_as_new_when_it_returns() {
        let mut router = Router::new(peer(0));
        let own = intent(peer(0), 1);
        router.note_origin(&own, 1_000);

        assert!(router.has_seen(&own.dedup_key()));
    }
}
