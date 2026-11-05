//! The mesh, as a state machine with no I/O.
//!
//! # Why this shape
//!
//! Time arrives as a parameter and the radio leaves as a return value. Nothing
//! in this file sleeps, spawns, opens a socket or reads a clock.
//!
//! That is not architectural taste. Bluetooth cannot be driven from CI, and it
//! cannot be driven from an iOS simulator at all — `simctl` has no input
//! command and CoreBluetooth is not virtualised. A protocol that could only be
//! exercised on two physical phones is a protocol that would be exercised
//! roughly never, and every routing bug would be found by a user in a room
//! with no signal.
//!
//! Written this way, a twenty-node mesh with a virtual clock runs in
//! milliseconds as an ordinary unit test, deterministically, on every commit.
//! The radio-shaped part that remains is small enough to inspect by reading.
//!
//! # The offline switch
//!
//! `SetOffline` stops every transmission, including announcements. The app's
//! kill switch promises nothing leaves the device; a radio still flooding
//! would make that promise false, so the gate lives here rather than in each
//! caller.

use crate::identity::{self, Ephemeral};
use crate::peers::{self, Announce, Capabilities, KnownPeer, PeerTable};
use crate::router::{Decision, DropReason, LinkId, Relay, Router, RouterCounters};
use crate::wire::{DedupKey, Packet, PacketKind, PeerId};
use crate::framing::{encode_frame, FrameReader};
use std::collections::{BTreeMap, HashMap, VecDeque};
use std::time::Duration;

/// How often expired peers are swept out.
pub const EXPIRY_SWEEP: Duration = Duration::from_secs(5);

/// Packets held while their relay waits out its jitter window.
///
/// Bounded because the window is short and the arrival rate is not: a node in
/// a crowded room with a hostile neighbour must not accumulate a copy of every
/// packet it was ever asked to forward. Overflow drops the oldest, which costs
/// one un-relayed packet — a cost the flood is designed to absorb.
pub const PENDING_RELAYS: usize = 256;

/// Something that happened to the node.
#[derive(Debug)]
pub enum Event {
    /// A link to a neighbour came up. The peer behind it is unknown until it
    /// announces.
    LinkUp(LinkId),
    /// A link went away.
    LinkDown(LinkId),
    /// Bytes arrived on a link. Any number of them, split anywhere.
    Bytes { link: LinkId, bytes: Vec<u8> },
    /// The announce timer fired.
    AnnounceDue,
    /// A scheduled relay's jitter window elapsed.
    RelayDue(DedupKey),
    /// The peer-expiry sweep is due.
    ExpiryDue,
    /// The app wants to put something on the mesh.
    Submit {
        kind: PacketKind,
        payload: Vec<u8>,
        /// `None` floods it; `Some` sends it to one peer.
        recipient: Option<PeerId>,
    },
    /// Stop or resume transmitting.
    SetOffline(bool),
}

/// Something the runtime must do.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Action {
    /// Write these bytes to this link.
    Send { link: LinkId, bytes: Vec<u8> },
    /// Call back with [`Event::RelayDue`] after the delay.
    ScheduleRelay { key: DedupKey, delay: Duration },
    /// Call back with [`Event::AnnounceDue`] after the delay.
    ScheduleAnnounce { delay: Duration },
    /// Call back with [`Event::ExpiryDue`] after the delay.
    ScheduleExpiry { delay: Duration },
    /// Hand a payload up to the app.
    Deliver {
        from: PeerId,
        kind: PacketKind,
        payload: Vec<u8>,
    },
    /// A peer became reachable.
    PeerAppeared(PeerId),
    /// A peer stopped being reachable.
    PeerGone(PeerId),
    /// Tear this link down: it sent something that means the stream is no
    /// longer synchronised.
    DropLink(LinkId),
}

/// What the nodes screen and the mesh status display need.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct MeshStatus {
    /// This session's identifier. Changes on every launch, by design.
    pub peer_id: PeerId,
    /// Radio links currently open.
    pub links: usize,
    /// Peers one hop away.
    pub direct_peers: usize,
    /// Every peer reachable, direct or through a neighbour.
    pub reachable_peers: usize,
    /// Reachable peers offering a way to the internet.
    pub gateways: usize,
    /// Packets forwarded for other people.
    pub relayed: u64,
    /// Forwards cancelled because somebody else was faster.
    pub suppressed: u64,
    pub offline: bool,
}

/// One node's mesh state.
#[derive(Debug)]
pub struct Engine {
    identity: Ephemeral,
    capabilities: Capabilities,
    router: Router,
    peers: PeerTable,
    /// Open links, and the peer each one turned out to lead to.
    ///
    /// `BTreeMap` rather than `HashMap`: link order decides fanout order, and
    /// a simulation that reorders between runs is a test that flakes.
    links: BTreeMap<LinkId, Option<PeerId>>,
    readers: BTreeMap<LinkId, FrameReader>,
    /// Signing keys learned from verified announcements, so a later packet
    /// from the same peer can be checked without re-announcing.
    keys: BTreeMap<PeerId, [u8; peers::KEY_LEN]>,
    /// Packets whose relay is scheduled but not yet sent, with the order they
    /// arrived in so the oldest can be evicted.
    pending: HashMap<DedupKey, Packet>,
    pending_order: VecDeque<DedupKey>,
    offline: bool,
}

impl Engine {
    /// A node with the given identity.
    #[must_use]
    pub fn new(identity: Ephemeral, capabilities: Capabilities) -> Self {
        let local = identity.id();
        Self {
            identity,
            capabilities,
            router: Router::new(local),
            peers: PeerTable::new(local),
            links: BTreeMap::new(),
            readers: BTreeMap::new(),
            keys: BTreeMap::new(),
            pending: HashMap::new(),
            pending_order: VecDeque::new(),
            offline: false,
        }
    }

    /// This node's identifier.
    #[must_use]
    pub fn id(&self) -> PeerId {
        self.identity.id()
    }

    /// Declares whether this node can reach the internet.
    ///
    /// Drives the gateway capability bit. The IP plane owns the answer; this
    /// crate only advertises it.
    pub fn set_gateway(&mut self, gateway: bool) {
        self.capabilities = if gateway {
            self.capabilities.with(Capabilities::GATEWAY)
        } else {
            Capabilities::from_bits(self.capabilities.bits() & !Capabilities::GATEWAY)
        };
    }

    /// Everything the UI needs about the mesh right now.
    #[must_use]
    pub fn status(&self, now_ms: u64) -> MeshStatus {
        let RouterCounters {
            relayed,
            suppressed,
            ..
        } = self.router.counters();
        MeshStatus {
            peer_id: self.identity.id(),
            links: self.links.len(),
            direct_peers: self.peers.direct(now_ms).len(),
            reachable_peers: self.peers.len(now_ms),
            gateways: self.peers.gateways(now_ms).len(),
            relayed,
            suppressed,
            offline: self.offline,
        }
    }

    /// Every reachable peer, for the nodes screen.
    #[must_use]
    pub fn peers(&self, now_ms: u64) -> Vec<KnownPeer> {
        self.peers.reachable(now_ms)
    }

    /// Starts the timers. Call once, when the radio comes up.
    #[must_use]
    pub fn start(&mut self) -> Vec<Action> {
        vec![
            Action::ScheduleAnnounce {
                delay: Duration::ZERO,
            },
            Action::ScheduleExpiry { delay: EXPIRY_SWEEP },
        ]
    }

    /// Advances the state machine.
    #[must_use]
    pub fn handle(&mut self, event: Event, now_ms: u64) -> Vec<Action> {
        match event {
            Event::LinkUp(link) => {
                self.links.insert(link, None);
                self.readers.insert(link, FrameReader::new());
                // Announce immediately rather than waiting out the interval:
                // the peer on the other end has nothing to go on until we do,
                // and a first impression that takes 15 seconds reads as a
                // broken app.
                self.announce_now(now_ms)
            }
            Event::LinkDown(link) => {
                self.links.remove(&link);
                self.readers.remove(&link);
                Vec::new()
            }
            Event::Bytes { link, bytes } => self.on_bytes(link, &bytes, now_ms),
            Event::AnnounceDue => {
                let mut actions = self.announce_now(now_ms);
                actions.push(Action::ScheduleAnnounce {
                    delay: peers::announce_interval(self.identity.id(), !self.links.is_empty()),
                });
                actions
            }
            Event::RelayDue(key) => self.on_relay_due(&key),
            Event::ExpiryDue => {
                let mut actions: Vec<Action> = self
                    .peers
                    .expire(now_ms)
                    .into_iter()
                    .map(Action::PeerGone)
                    .collect();
                actions.push(Action::ScheduleExpiry { delay: EXPIRY_SWEEP });
                actions
            }
            Event::Submit {
                kind,
                payload,
                recipient,
            } => self.submit(kind, payload, recipient, now_ms),
            Event::SetOffline(offline) => {
                self.offline = offline;
                if offline {
                    // Nothing queued survives the switch. A relay that fired
                    // one second after the user hit the kill switch would make
                    // the switch a lie.
                    Vec::new()
                } else {
                    self.announce_now(now_ms)
                }
            }
        }
    }

    /// Puts a packet of this node's own onto the mesh.
    fn submit(
        &mut self,
        kind: PacketKind,
        payload: Vec<u8>,
        recipient: Option<PeerId>,
        now_ms: u64,
    ) -> Vec<Action> {
        if self.offline {
            return Vec::new();
        }

        let mut packet = match recipient {
            Some(to) => Packet::directed(kind, self.identity.id(), to, now_ms, payload),
            None => Packet::broadcast(kind, self.identity.id(), now_ms, payload),
        };
        if self.identity.sign(&mut packet).is_err() {
            // Only reachable with an over-long payload, which the caller built.
            tracing::warn!(?kind, "refusing to send an oversized packet");
            return Vec::new();
        }

        self.router.note_origin(&packet, now_ms);
        self.broadcast_frame(&packet, None)
    }

    /// Builds and sends an announcement.
    fn announce_now(&mut self, now_ms: u64) -> Vec<Action> {
        if self.offline || self.links.is_empty() {
            return Vec::new();
        }

        let announce = Announce {
            key_agreement: self.identity.key_agreement_public(),
            signing: self.identity.signing_public(),
            capabilities: self.capabilities,
            neighbours: self.peers.advertised_neighbours(now_ms),
        };
        self.submit(PacketKind::Announce, announce.encode(), None, now_ms)
    }

    /// Reads whatever arrived on a link.
    fn on_bytes(&mut self, link: LinkId, bytes: &[u8], now_ms: u64) -> Vec<Action> {
        let Some(reader) = self.readers.get_mut(&link) else {
            // Bytes from a link we already tore down. Nothing to do, and
            // certainly not worth resurrecting the link for.
            return Vec::new();
        };
        reader.push(bytes);

        let mut actions = Vec::new();
        loop {
            match self.readers.get_mut(&link).and_then(|r| r.next_packet().transpose()) {
                None => break,
                Some(Ok(packet)) => actions.extend(self.on_packet(link, packet, now_ms)),
                Some(Err(error)) => {
                    // The stream is no longer synchronised, so the next length
                    // prefix cannot be trusted either. Dropping the link is
                    // the only safe response; resyncing would mean guessing.
                    tracing::debug!(?link, %error, "dropping a link that sent an unreadable frame");
                    actions.push(Action::DropLink(link));
                    break;
                }
            }
        }
        actions
    }

    /// Handles one decoded packet.
    fn on_packet(&mut self, link: LinkId, packet: Packet, now_ms: u64) -> Vec<Action> {
        let open: Vec<LinkId> = self.links.keys().copied().collect();
        // A link may be thinned out of a broadcast's fanout only if somebody
        // else also reaches the peer behind it. A link whose peer is not in
        // any other neighbour's advertised list is the only path there, and
        // dropping it would partition the mesh rather than save a transmission.
        let thinnable: Vec<LinkId> = self
            .links
            .iter()
            .filter_map(|(link, peer)| peer.map(|peer| (*link, peer)))
            .filter(|(_, peer)| self.peers.is_covered_by_others(*peer))
            .map(|(link, _)| link)
            .collect();
        let decision = self.router.receive(&packet, link, &open, &thinnable, now_ms);

        let (deliver, relay) = match decision {
            Decision::Accept { relay } => (true, relay),
            Decision::Forward(relay) => (false, Some(relay)),
            Decision::Drop(DropReason::Duplicate | DropReason::Loop) => return Vec::new(),
            Decision::Drop(_) => (true, None),
        };

        let mut actions = Vec::new();

        if deliver {
            actions.extend(self.deliver(link, &packet, now_ms));
        }

        if let Some(relay) = relay {
            if !self.offline {
                let key = packet.dedup_key();
                self.remember_pending(key, packet);
                actions.push(Action::ScheduleRelay {
                    key,
                    delay: relay.delay,
                });
            }
        }

        actions
    }

    /// Holds a packet until its relay fires, evicting the oldest if full.
    fn remember_pending(&mut self, key: DedupKey, packet: Packet) {
        if self.pending.insert(key, packet).is_none() {
            self.pending_order.push_back(key);
        }
        while self.pending_order.len() > PENDING_RELAYS {
            if let Some(oldest) = self.pending_order.pop_front() {
                self.pending.remove(&oldest);
            }
        }
    }

    /// Applies a packet locally.
    fn deliver(&mut self, link: LinkId, packet: &Packet, now_ms: u64) -> Vec<Action> {
        match packet.kind {
            PacketKind::Announce => self.on_announce(link, packet, now_ms),
            _ => {
                // Everything else is handed up as-is. Session decryption and
                // intent parsing belong to the layers that own those formats,
                // not to the router.
                vec![Action::Deliver {
                    from: packet.sender,
                    kind: packet.kind,
                    payload: packet.payload.clone(),
                }]
            }
        }
    }

    /// Records a peer from a verified announcement.
    fn on_announce(&mut self, link: LinkId, packet: &Packet, now_ms: u64) -> Vec<Action> {
        let Ok(announce) = Announce::decode(&packet.payload) else {
            tracing::debug!("discarding a malformed announcement");
            return Vec::new();
        };

        // Two checks, both load-bearing:
        //
        // The identifier must follow from the announced key, or a peer could
        // announce itself as somebody else and receive their directed traffic.
        //
        // The signature must verify against the announced signing key, or the
        // announcement could be replayed by anyone who overheard it — with a
        // different neighbour list, which is how a mesh is made to route into
        // a hole.
        if announce.peer_id() != packet.sender {
            tracing::debug!("discarding an announcement whose id does not follow its key");
            return Vec::new();
        }
        if !identity::verify(packet, &announce.signing) {
            tracing::debug!("discarding an unsigned or badly signed announcement");
            return Vec::new();
        }

        let known_before = self.peers.reachable(now_ms).iter().any(|p| p.id == packet.sender);

        // Only an announcement that has not been relayed came off this radio.
        // A relayed one names somebody in the mesh, not somebody in the room —
        // and the difference is what the user is being told when the screen
        // says how many nodes are nearby.
        let direct = packet.ttl >= crate::router::DEFAULT_TTL;
        let id = if direct {
            let id = self.peers.observe_direct(&announce, now_ms);
            self.links.insert(link, Some(id));
            id
        } else {
            self.peers.observe_relayed(&announce, now_ms)
        };
        self.keys.insert(id, announce.signing);

        if known_before {
            Vec::new()
        } else {
            vec![Action::PeerAppeared(id)]
        }
    }

    /// Sends a relay whose jitter window elapsed, unless it was cancelled.
    fn on_relay_due(&mut self, key: &DedupKey) -> Vec<Action> {
        if self.offline {
            return Vec::new();
        }
        let Some(Relay { links, ttl, .. }) = self.router.take_scheduled(key) else {
            // Cancelled: somebody else relayed it first. This is the common
            // case in a crowded room and is not a failure.
            return Vec::new();
        };
        let Some(packet) = self.pending.remove(key) else {
            return Vec::new();
        };

        let mut forwarded = packet;
        forwarded.ttl = ttl;
        let Ok(bytes) = encode_frame(&forwarded) else {
            return Vec::new();
        };

        links
            .into_iter()
            .filter(|link| self.links.contains_key(link))
            .map(|link| Action::Send {
                link,
                bytes: bytes.clone(),
            })
            .collect()
    }

    /// Sends a packet on every link except one.
    fn broadcast_frame(&self, packet: &Packet, except: Option<LinkId>) -> Vec<Action> {
        let Ok(bytes) = encode_frame(packet) else {
            return Vec::new();
        };
        self.links
            .keys()
            .copied()
            .filter(|link| Some(*link) != except)
            .map(|link| Action::Send {
                link,
                bytes: bytes.clone(),
            })
            .collect()
    }
}
