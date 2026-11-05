//! A whole mesh, on one thread, with a clock that does what it is told.
//!
//! # What this replaces
//!
//! The acceptance question for a flood router is "does an intent reach every
//! node, and does the flood stop". Answering it on hardware needs as many
//! phones as the topology has nodes, in the right physical arrangement, and it
//! answers once. This machine cannot even tap an iOS simulator — `simctl` has
//! no input command and CoreBluetooth is not virtualised — so the honest
//! alternative to this file is not "test it on devices", it is "do not test
//! it".
//!
//! Here the same question is a unit test over twenty nodes in a chain, a star
//! and a ring, and it runs in milliseconds on every commit.
//!
//! # Why it is deterministic
//!
//! Nothing in the protocol calls a random number generator. Relay jitter and
//! fanout selection are derived by hashing the packet's identity with the
//! node's, which decorrelates transmissions — the actual purpose — without
//! making a run irreproducible. A failure here can be re-run and stepped
//! through rather than starred as flaky.

use cabal_ble::engine::{Action, Engine, Event};
use cabal_ble::peers::Capabilities;
use cabal_ble::wire::{PacketKind, PeerId};
use cabal_ble::{Ephemeral, LinkId};
use std::collections::{BTreeMap, BinaryHeap, HashMap, HashSet};
use std::time::Duration;

/// One-way radio latency in the simulation.
const HOP_MS: u64 = 3;

/// A scheduled event, ordered so the earliest comes out first.
struct Scheduled {
    at_ms: u64,
    sequence: u64,
    node: usize,
    event: Event,
}

impl PartialEq for Scheduled {
    fn eq(&self, other: &Self) -> bool {
        (self.at_ms, self.sequence) == (other.at_ms, other.sequence)
    }
}
impl Eq for Scheduled {}
impl Ord for Scheduled {
    fn cmp(&self, other: &Self) -> std::cmp::Ordering {
        // Reversed: `BinaryHeap` is a max-heap and we want the earliest event.
        // Ties break on insertion order, so a run is reproducible.
        (other.at_ms, other.sequence).cmp(&(self.at_ms, self.sequence))
    }
}
impl PartialOrd for Scheduled {
    fn partial_cmp(&self, other: &Self) -> Option<std::cmp::Ordering> {
        Some(self.cmp(other))
    }
}

/// A mesh of engines wired together by links that exist only in memory.
struct World {
    nodes: Vec<Engine>,
    /// `(node, link)` on one side to `(node, link)` on the other.
    wiring: HashMap<(usize, LinkId), (usize, LinkId)>,
    queue: BinaryHeap<Scheduled>,
    now_ms: u64,
    sequence: u64,
    /// What each node handed up to its app.
    delivered: Vec<Vec<(PeerId, PacketKind, Vec<u8>)>>,
    /// Frames actually put on a link, for counting transmissions.
    transmissions: u64,
    next_link: u64,
}

impl World {
    fn new(count: usize) -> Self {
        let nodes = (0..count)
            .map(|index| {
                // Fixed key material: the simulation must produce the same
                // identifiers, and therefore the same jitter and fanout, on
                // every run.
                let seed = u8::try_from(index + 1).expect("simulation stays under 255 nodes");
                Engine::new(
                    Ephemeral::from_bytes([seed; 32], [seed.wrapping_mul(7).wrapping_add(3); 32]),
                    Capabilities::none(),
                )
            })
            .collect();

        Self {
            nodes,
            wiring: HashMap::new(),
            queue: BinaryHeap::new(),
            now_ms: 1_000,
            sequence: 0,
            delivered: vec![Vec::new(); count],
            transmissions: 0,
            next_link: 0,
        }
    }

    fn id(&self, node: usize) -> PeerId {
        self.nodes[node].id()
    }

    /// Connects two nodes, as a radio link coming up on both sides.
    fn connect(&mut self, a: usize, b: usize) {
        let link_a = LinkId(self.next_link);
        let link_b = LinkId(self.next_link + 1);
        self.next_link += 2;

        self.wiring.insert((a, link_a), (b, link_b));
        self.wiring.insert((b, link_b), (a, link_a));

        self.dispatch(a, Event::LinkUp(link_a));
        self.dispatch(b, Event::LinkUp(link_b));
    }

    /// Drops the link between two nodes, in both directions.
    fn disconnect(&mut self, a: usize, b: usize) {
        let found: Vec<((usize, LinkId), (usize, LinkId))> = self
            .wiring
            .iter()
            .filter(|((from, _), (to, _))| (*from == a && *to == b) || (*from == b && *to == a))
            .map(|(k, v)| (*k, *v))
            .collect();

        for (near, far) in found {
            self.wiring.remove(&near);
            self.dispatch(near.0, Event::LinkDown(near.1));
            let _ = far;
        }
    }

    fn start_all(&mut self) {
        for node in 0..self.nodes.len() {
            let actions = self.nodes[node].start();
            self.apply(node, actions);
        }
    }

    fn dispatch(&mut self, node: usize, event: Event) {
        let actions = self.nodes[node].handle(event, self.now_ms);
        self.apply(node, actions);
    }

    fn apply(&mut self, node: usize, actions: Vec<Action>) {
        for action in actions {
            match action {
                Action::Send { link, bytes } => {
                    self.transmissions += 1;
                    if let Some(&(peer, peer_link)) = self.wiring.get(&(node, link)) {
                        self.at(HOP_MS, peer, Event::Bytes {
                            link: peer_link,
                            bytes,
                        });
                    }
                }
                Action::ScheduleRelay { key, delay } => {
                    self.at(millis(delay), node, Event::RelayDue(key));
                }
                Action::ScheduleAnnounce { delay } => {
                    self.at(millis(delay).max(1), node, Event::AnnounceDue);
                }
                Action::ScheduleExpiry { delay } => {
                    self.at(millis(delay), node, Event::ExpiryDue);
                }
                Action::Deliver { from, kind, payload } => {
                    self.delivered[node].push((from, kind, payload));
                }
                Action::PeerAppeared(_) | Action::PeerGone(_) | Action::DropLink(_) => {}
            }
        }
    }

    fn at(&mut self, delay_ms: u64, node: usize, event: Event) {
        self.sequence += 1;
        self.queue.push(Scheduled {
            at_ms: self.now_ms + delay_ms,
            sequence: self.sequence,
            node,
            event,
        });
    }

    /// Runs until the given instant, or until nothing is left to do.
    fn run_for(&mut self, span: Duration) {
        let deadline = self.now_ms + millis(span);
        while let Some(next) = self.queue.peek() {
            if next.at_ms > deadline {
                break;
            }
            let Scheduled {
                at_ms, node, event, ..
            } = self.queue.pop().expect("peeked");
            self.now_ms = at_ms;
            self.dispatch(node, event);
        }
        self.now_ms = deadline;
    }

    /// Puts an intent on the mesh from one node.
    fn submit(&mut self, node: usize, payload: &[u8]) {
        self.dispatch(
            node,
            Event::Submit {
                kind: PacketKind::Intent,
                payload: payload.to_vec(),
                recipient: None,
            },
        );
    }

    fn intents(&self, node: usize) -> Vec<Vec<u8>> {
        self.delivered[node]
            .iter()
            .filter(|(_, kind, _)| *kind == PacketKind::Intent)
            .map(|(_, _, payload)| payload.clone())
            .collect()
    }

    /// Every node except the origin received the payload exactly once.
    fn assert_everyone_got(&self, payload: &[u8], origin: usize) {
        for node in 0..self.nodes.len() {
            if node == origin {
                continue;
            }
            let received = self.intents(node);
            let matching = received.iter().filter(|got| got.as_slice() == payload).count();
            assert_eq!(
                matching, 1,
                "node {node} received the intent {matching} times, wanted exactly 1"
            );
        }
    }
}

fn millis(duration: Duration) -> u64 {
    u64::try_from(duration.as_millis()).unwrap_or(u64::MAX)
}

/// A chain: 0—1—2—…—n. The hardest topology for a flood, and the one where a
/// TTL that is too short silently truncates delivery.
fn chain(count: usize) -> World {
    let mut world = World::new(count);
    for index in 1..count {
        world.connect(index - 1, index);
    }
    world.start_all();
    world.run_for(Duration::from_secs(2));
    world
}

/// A ring, which is a chain that can flood back into itself.
fn ring(count: usize) -> World {
    let mut world = World::new(count);
    for index in 1..count {
        world.connect(index - 1, index);
    }
    world.connect(count - 1, 0);
    world.start_all();
    world.run_for(Duration::from_secs(2));
    world
}

/// One node with every other hanging off it.
fn star(count: usize) -> World {
    let mut world = World::new(count);
    for index in 1..count {
        world.connect(0, index);
    }
    world.start_all();
    world.run_for(Duration::from_secs(2));
    world
}

#[test]
fn every_node_in_a_chain_receives_an_intent() {
    // Five hops, inside the TTL of seven. This is the delivery guarantee the
    // whole crate exists to provide.
    let mut world = chain(6);
    world.submit(0, b"intent from the end of the chain");
    world.run_for(Duration::from_secs(5));

    world.assert_everyone_got(b"intent from the end of the chain", 0);
}

#[test]
fn every_node_in_a_star_receives_an_intent() {
    let mut world = star(8);
    world.submit(3, b"intent from a leaf");
    world.run_for(Duration::from_secs(5));

    world.assert_everyone_got(b"intent from a leaf", 3);
}

#[test]
fn every_node_in_a_ring_receives_an_intent_exactly_once() {
    // A ring is where a flood without deduplication circulates forever. The
    // "exactly once" in the assertion is the whole point.
    let mut world = ring(8);
    world.submit(0, b"once around");
    world.run_for(Duration::from_secs(5));

    world.assert_everyone_got(b"once around", 0);
}

#[test]
fn a_flood_terminates() {
    // Not "delivers" — terminates. A mesh that delivers and then keeps
    // relaying is a mesh that flattens every battery in the room.
    let mut world = ring(8);
    world.submit(0, b"quiet down");
    world.run_for(Duration::from_secs(5));

    let after_delivery = world.transmissions;
    world.run_for(Duration::from_secs(20));
    let announces = world.transmissions - after_delivery;

    world.submit(0, b"and again");
    world.run_for(Duration::from_secs(5));
    let second_flood = world.transmissions - after_delivery - announces;

    assert!(
        second_flood < 8 * 8,
        "one intent cost {second_flood} transmissions across 8 nodes"
    );
}

#[test]
fn a_chain_longer_than_the_ttl_does_not_deliver_to_the_far_end() {
    // Documented rather than fixed. The TTL is a bound on how far a packet
    // travels, and a room deeper than seven hops of Bluetooth is not the case
    // this protocol is for. The test exists so the limit is discovered here
    // and not by a user.
    let mut world = chain(12);
    world.submit(0, b"too far");
    world.run_for(Duration::from_secs(5));

    assert!(!world.intents(1).is_empty(), "the first hop should have it");
    assert!(
        world.intents(11).is_empty(),
        "a packet travelled further than its TTL allows"
    );
}

#[test]
fn nodes_discover_each_other_without_being_told() {
    // Announcements alone, no configuration, no bootstrap list.
    let world = chain(4);

    for node in 0..4 {
        assert!(
            !world.nodes[node].peers(world.now_ms).is_empty(),
            "node {node} found nobody"
        );
    }
}

#[test]
fn a_node_learns_its_neighbours_neighbours() {
    // Two hops out, from the neighbour lists inside announcements. This is
    // what lets a directed packet be sent to somebody not in radio range.
    let world = chain(3);

    let far = world.id(2);
    let known: HashSet<PeerId> = world.nodes[0]
        .peers(world.now_ms)
        .into_iter()
        .map(|peer| peer.id)
        .collect();

    assert!(known.contains(&far), "node 0 never heard about node 2");
}

#[test]
fn a_partition_heals_when_one_link_returns() {
    // Two clumps, joined by a single node. The case that a fanout subset gets
    // wrong if it ever thins a two-link node.
    let mut world = World::new(6);
    world.connect(0, 1);
    world.connect(1, 2);
    world.connect(3, 4);
    world.connect(4, 5);
    world.start_all();
    world.run_for(Duration::from_secs(2));

    world.submit(0, b"before the bridge");
    world.run_for(Duration::from_secs(3));
    assert!(world.intents(5).is_empty(), "the partition was not a partition");

    world.connect(2, 3);
    world.run_for(Duration::from_secs(5));

    world.submit(0, b"after the bridge");
    world.run_for(Duration::from_secs(5));

    assert!(
        world.intents(5).iter().any(|got| got == b"after the bridge"),
        "the mesh did not heal"
    );
}

#[test]
fn a_peer_that_leaves_stops_being_listed() {
    let mut world = chain(2);
    assert_eq!(world.nodes[0].status(world.now_ms).direct_peers, 1);

    world.disconnect(0, 1);
    // Past the reachability window, with the expiry sweep running.
    world.run_for(Duration::from_secs(90));

    let status = world.nodes[0].status(world.now_ms);
    assert_eq!(status.links, 0, "the link was not torn down");
    assert_eq!(status.direct_peers, 0, "a peer that walked away is still listed");
}

#[test]
fn the_offline_switch_stops_every_transmission() {
    // The kill switch promises nothing leaves the device. A radio still
    // announcing would make that promise false.
    let mut world = chain(3);
    world.dispatch(1, Event::SetOffline(true));

    let before = world.transmissions;
    world.run_for(Duration::from_secs(60));
    let while_offline = world.transmissions - before;

    // Nodes 0 and 2 keep announcing to node 1; what must not happen is node 1
    // sending anything at all. Counting per-node is clearer than counting the
    // world, so submit from the offline node and require silence.
    let before_submit = world.transmissions;
    world.submit(1, b"this must not leave");
    world.run_for(Duration::from_secs(2));

    assert_eq!(
        world.transmissions,
        before_submit,
        "an offline node transmitted"
    );
    assert!(while_offline > 0, "the other nodes stopped too, so this proves nothing");
    assert!(world.intents(0).is_empty(), "an offline node's intent reached a peer");
    assert!(world.intents(2).is_empty(), "an offline node's intent reached a peer");
}

#[test]
fn coming_back_online_rejoins_the_mesh() {
    let mut world = chain(3);
    world.dispatch(1, Event::SetOffline(true));
    world.run_for(Duration::from_secs(10));

    world.dispatch(1, Event::SetOffline(false));
    world.run_for(Duration::from_secs(5));

    world.submit(1, b"back");
    world.run_for(Duration::from_secs(3));

    assert!(world.intents(0).iter().any(|got| got == b"back"));
    assert!(world.intents(2).iter().any(|got| got == b"back"));
}

#[test]
fn a_directed_packet_reaches_only_its_recipient() {
    // In a chain of four, a packet from 0 to 3 passes through 1 and 2 without
    // either of them handing it up.
    let mut world = chain(4);
    let target = world.id(3);

    world.dispatch(
        0,
        Event::Submit {
            kind: PacketKind::Sealed,
            payload: b"for you only".to_vec(),
            recipient: Some(target),
        },
    );
    world.run_for(Duration::from_secs(5));

    let sealed = |world: &World, node: usize| {
        world.delivered[node]
            .iter()
            .filter(|(_, kind, _)| *kind == PacketKind::Sealed)
            .count()
    };

    assert_eq!(sealed(&world, 3), 1, "the recipient did not receive it");
    assert_eq!(sealed(&world, 1), 0, "a relay handed up a packet not addressed to it");
    assert_eq!(sealed(&world, 2), 0, "a relay handed up a packet not addressed to it");
}

#[test]
fn a_dense_room_costs_less_per_intent_than_naive_flooding() {
    // Naive flooding in a full mesh of n nodes is n*(n-1) transmissions per
    // packet. Suppression and fanout should beat that comfortably; if a change
    // makes this fail, the room got louder rather than quieter.
    const NODES: usize = 8;
    let mut world = World::new(NODES);
    for a in 0..NODES {
        for b in (a + 1)..NODES {
            world.connect(a, b);
        }
    }
    world.start_all();
    world.run_for(Duration::from_secs(2));

    let before = world.transmissions;
    world.submit(0, b"crowded");
    world.run_for(Duration::from_secs(3));
    let cost = world.transmissions - before;

    world.assert_everyone_got(b"crowded", 0);
    let naive = u64::try_from(NODES * (NODES - 1)).expect("small");
    assert!(
        cost < naive,
        "one intent cost {cost} transmissions, no better than naive flooding"
    );
}

#[test]
fn status_reports_what_the_nodes_screen_needs() {
    let world = star(5);
    let hub = world.nodes[0].status(world.now_ms);

    assert_eq!(hub.links, 4);
    assert_eq!(hub.direct_peers, 4);
    assert!(hub.reachable_peers >= 4);
    assert!(!hub.offline);
    assert_eq!(hub.peer_id, world.id(0));

    // A leaf sees one neighbour directly and the rest of the star through it.
    let leaf = world.nodes[1].status(world.now_ms);
    assert_eq!(leaf.direct_peers, 1);
    assert_eq!(leaf.reachable_peers, 4, "a leaf should hear about the whole star");
}

#[test]
fn relays_are_counted_and_so_are_the_ones_that_were_not_needed() {
    // "The mesh is quiet" and "every packet is being dropped as a duplicate"
    // look identical from outside without these two counters.
    let mut world = ring(6);
    world.submit(0, b"count me");
    world.run_for(Duration::from_secs(3));

    let total: u64 = (0..6)
        .map(|node| {
            let status = world.nodes[node].status(world.now_ms);
            status.relayed + status.suppressed
        })
        .sum();

    assert!(total > 0, "nothing was relayed or suppressed in a six-node ring");
}

#[test]
fn a_gateway_is_visible_to_the_room() {
    let mut world = chain(4);
    world.nodes[2].set_gateway(true);
    world.run_for(Duration::from_secs(40));

    assert_eq!(
        world.nodes[0].status(world.now_ms).gateways,
        1,
        "a node with internet did not advertise it to the mesh"
    );
}

#[test]
fn the_simulation_is_reproducible() {
    // If this ever fails, every other test in this file became a coin flip.
    let run = || {
        let mut world = ring(6);
        world.submit(0, b"same every time");
        world.run_for(Duration::from_secs(5));
        (world.transmissions, world.intents(3))
    };

    assert_eq!(run(), run());
}

#[test]
fn a_link_that_sends_rubbish_is_dropped_rather_than_trusted() {
    // A desynchronised stream cannot be resynchronised by guessing where the
    // next length prefix starts.
    let mut world = World::new(2);
    world.connect(0, 1);
    world.start_all();
    world.run_for(Duration::from_secs(1));

    let mut engine = Engine::new(Ephemeral::from_bytes([9; 32], [9; 32]), Capabilities::none());
    let link = LinkId(0);
    let _ = engine.handle(Event::LinkUp(link), 1_000);
    let actions = engine.handle(
        Event::Bytes {
            link,
            bytes: vec![0xFF; 64],
        },
        1_000,
    );

    assert!(
        actions.iter().any(|action| matches!(action, Action::DropLink(dropped) if *dropped == link)),
        "a link sending unparseable bytes was kept"
    );
}

#[test]
fn an_unsigned_announcement_is_ignored() {
    // Announcements carry neighbour lists. An unsigned one is a way to make a
    // mesh route into a hole.
    use cabal_ble::framing::encode_frame;
    use cabal_ble::peers::Announce;
    use cabal_ble::wire::Packet;

    let mut engine = Engine::new(Ephemeral::from_bytes([1; 32], [1; 32]), Capabilities::none());
    let link = LinkId(0);
    let _ = engine.handle(Event::LinkUp(link), 1_000);

    let forged = Announce {
        key_agreement: [77; 32],
        signing: [78; 32],
        capabilities: Capabilities::none(),
        neighbours: Vec::new(),
    };
    let packet = Packet::broadcast(
        PacketKind::Announce,
        forged.peer_id(),
        1_000,
        forged.encode(),
    );

    let _ = engine.handle(
        Event::Bytes {
            link,
            bytes: encode_frame(&packet).unwrap(),
        },
        1_000,
    );

    assert!(
        engine.peers(1_000).is_empty(),
        "an unsigned announcement was accepted"
    );
}

#[test]
fn an_announcement_claiming_someone_elses_identifier_is_ignored() {
    use cabal_ble::framing::encode_frame;
    use cabal_ble::identity::Ephemeral as Id;
    use cabal_ble::peers::Announce;
    use cabal_ble::wire::Packet;

    let mut engine = Engine::new(Id::from_bytes([1; 32], [1; 32]), Capabilities::none());
    let link = LinkId(0);
    let _ = engine.handle(Event::LinkUp(link), 1_000);

    let liar = Id::from_bytes([50; 32], [50; 32]);
    let announce = Announce {
        key_agreement: liar.key_agreement_public(),
        signing: liar.signing_public(),
        capabilities: Capabilities::none(),
        neighbours: Vec::new(),
    };

    // Correctly signed, but sent under an identifier that does not follow from
    // the announced key.
    let mut packet = Packet::broadcast(
        PacketKind::Announce,
        PeerId([0xAA; 8]),
        1_000,
        announce.encode(),
    );
    liar.sign(&mut packet).unwrap();

    let _ = engine.handle(
        Event::Bytes {
            link,
            bytes: encode_frame(&packet).unwrap(),
        },
        1_000,
    );

    assert!(
        engine.peers(1_000).is_empty(),
        "a peer announced itself under somebody else's identifier"
    );
}

#[test]
fn twenty_nodes_still_converge() {
    // The size the design is actually for: a queue, a carriage, a room.
    let mut world = World::new(20);
    for index in 1..20 {
        // A rough line with occasional cross-links, which is what a moving
        // crowd looks like more than a clean topology does.
        world.connect(index - 1, index);
        if index % 4 == 0 {
            world.connect(index, index.saturating_sub(3));
        }
    }
    world.start_all();
    world.run_for(Duration::from_secs(3));

    world.submit(7, b"twenty");
    world.run_for(Duration::from_secs(6));

    let missed: Vec<usize> = (0..20)
        .filter(|&node| node != 7 && !world.intents(node).iter().any(|got| got == b"twenty"))
        .collect();

    // Exactly one node misses it, and it is the far end of the line: node 19
    // is eight hops from node 7 even using the cross-links, and the TTL is
    // seven. That is the TTL doing its job, and naming the node is the point —
    // an assertion of "most of them" would have kept passing when whole-relay
    // cancellation stranded five nodes at the other end, which is what the
    // first run of this test actually found.
    assert_eq!(
        missed,
        vec![19],
        "the intent reached a different set of nodes than the TTL allows"
    );
}

/// Sanity: the harness itself wires links symmetrically.
#[test]
fn the_harness_wires_both_directions() {
    let world = chain(2);
    let mut counts: BTreeMap<usize, usize> = BTreeMap::new();
    for (node, _) in world.wiring.keys() {
        *counts.entry(*node).or_default() += 1;
    }
    assert_eq!(counts.get(&0), Some(&1));
    assert_eq!(counts.get(&1), Some(&1));
}
