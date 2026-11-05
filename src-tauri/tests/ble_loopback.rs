//! Two BLE nodes, the real runtime, real sockets, no radio.
//!
//! # What this covers that the protocol tests do not
//!
//! `crates/cabal-ble` tests the protocol with a virtual clock and links that
//! are function calls. Everything there is decided in memory.
//!
//! This exercises the other half: a tokio task per node, a real transport
//! handing back real `LinkEvent`s, engine actions actually reaching a socket,
//! bytes actually coming back as events, timers actually firing on wall-clock
//! time. It is the wiring, and the wiring is where "the protocol is correct
//! but nothing happens" lives.
//!
//! # Why loopback rather than Bluetooth
//!
//! CI has no radio, and this machine cannot drive one from a test: on macOS a
//! process without an app bundle is refused Bluetooth by TCC, and an iOS
//! simulator does not virtualise CoreBluetooth at all.
//!
//! So the seam is [`LoopbackTransport`], and what remains unproven by this
//! file is stated rather than implied: advertising, scanning, L2CAP setup, MTU
//! behaviour and radio sleep are verified on two physical machines and
//! recorded in `docs/mobile-build-verification.md`, or they are not verified.

use cabalmesh_lib::ble::{self, link::LoopbackTransport, BleEvent, BleHandle};
use cabal_ble::peers::Capabilities;
use cabal_ble::wire::PacketKind;
use std::net::SocketAddr;
use std::sync::Arc;
use std::time::Duration;
use tokio::sync::broadcast::Receiver as BleEventReceiver;

/// Long enough for announcements to cross and be processed, short enough that
/// a hang fails the suite rather than stalling it.
const SETTLE: Duration = Duration::from_millis(600);

/// An unused local port, taken by binding and releasing.
///
/// Racy in principle. In practice the window is microseconds and the
/// alternative — a fixed port — makes two test runs on one machine collide,
/// which is a far more likely failure.
fn free_port() -> u16 {
    let listener = std::net::TcpListener::bind("127.0.0.1:0").expect("bind");
    listener.local_addr().expect("addr").port()
}

fn address(port: u16) -> SocketAddr {
    format!("127.0.0.1:{port}").parse().expect("addr")
}

/// Starts a node listening on `port` and dialling `peers`.
fn node(port: u16, peers: &[u16]) -> (BleHandle, BleEventReceiver<BleEvent>) {
    let transport = LoopbackTransport::new(
        address(port),
        peers.iter().copied().map(address).collect(),
    );
    ble::spawn(
        Arc::new(transport),
        ble::fresh_identity(),
        Capabilities::none(),
    )
}

/// Waits for a condition, polling, up to a deadline.
///
/// A fixed sleep would either be slow or flaky depending on the machine; this
/// is fast when things work and still fails cleanly when they do not.
async fn until(mut check: impl FnMut() -> bool, within: Duration) -> bool {
    let deadline = tokio::time::Instant::now() + within;
    while tokio::time::Instant::now() < deadline {
        if check() {
            return true;
        }
        tokio::time::sleep(Duration::from_millis(20)).await;
    }
    check()
}

/// Collects events that have arrived so far.
fn drain(events: &mut BleEventReceiver<BleEvent>) -> Vec<BleEvent> {
    let mut out = Vec::new();
    while let Ok(event) = events.try_recv() {
        out.push(event);
    }
    out
}

#[tokio::test]
async fn two_nodes_find_each_other() {
    // The first thing a user sees: a second node appearing without either of
    // them being configured with the other's address in any protocol sense.
    let port_a = free_port();
    let port_b = free_port();

    let (alice, _alice_events) = node(port_a, &[]);
    let (bob, _bob_events) = node(port_b, &[port_a]);

    tokio::time::sleep(SETTLE).await;

    let alice_status = alice.status().await.expect("alice status");
    let bob_status = bob.status().await.expect("bob status");

    assert_eq!(alice_status.links, 1, "alice has no link to bob");
    assert_eq!(bob_status.links, 1, "bob has no link to alice");
    assert_eq!(alice_status.direct_peers, 1, "alice did not learn bob's identity");
    assert_eq!(bob_status.direct_peers, 1, "bob did not learn alice's identity");

    // One peer, not two. Bob's announcement lists alice among his neighbours,
    // so a node that does not exclude itself reports a two-node mesh as three
    // — which is what two real nodes printed before this was fixed.
    assert_eq!(alice_status.reachable_peers, 1, "alice counted herself");
    assert_eq!(bob_status.reachable_peers, 1, "bob counted himself");

    // Two nodes, two identities, and they are not the same one.
    assert_ne!(alice_status.peer_id, bob_status.peer_id);
}

#[tokio::test]
async fn a_peer_appearing_is_announced_to_the_app() {
    // The nodes screen is driven by this event, not by polling.
    let port_a = free_port();
    let port_b = free_port();

    let (_alice, mut alice_events) = node(port_a, &[]);
    let (_bob, _bob_events) = node(port_b, &[port_a]);

    let appeared = until(
        || drain(&mut alice_events)
            .iter()
            .any(|event| matches!(event, BleEvent::PeerAppeared(_))),
        Duration::from_secs(3),
    )
    .await;

    assert!(appeared, "no PeerAppeared reached the app");
}

#[tokio::test]
async fn an_intent_crosses_between_two_nodes() {
    // The whole point of the plane: a payload composed on one device arrives
    // on another with no network of any kind between them.
    let port_a = free_port();
    let port_b = free_port();

    let (alice, _alice_events) = node(port_a, &[]);
    let (_bob, mut bob_events) = node(port_b, &[port_a]);

    tokio::time::sleep(SETTLE).await;

    alice
        .broadcast(PacketKind::Intent, b"sell 1 wNEAR for 4 USDC".to_vec())
        .await
        .expect("broadcast");

    let mut received: Vec<Vec<u8>> = Vec::new();
    let arrived = until(
        || {
            for event in drain(&mut bob_events) {
                if let BleEvent::Received {
                    kind: PacketKind::Intent,
                    payload,
                    ..
                } = event
                {
                    received.push(payload);
                }
            }
            !received.is_empty()
        },
        Duration::from_secs(3),
    )
    .await;

    assert!(arrived, "the intent never arrived");
    assert_eq!(received[0], b"sell 1 wNEAR for 4 USDC");
    assert_eq!(received.len(), 1, "the intent was delivered more than once");
}

#[tokio::test]
async fn an_intent_crosses_three_nodes_by_being_relayed() {
    // A—B—C with no link between A and C. C receives only because B forwards,
    // which is the difference between a mesh and a pair of radios.
    let port_a = free_port();
    let port_b = free_port();
    let port_c = free_port();

    let (alice, _alice_events) = node(port_a, &[]);
    let (_bob, _bob_events) = node(port_b, &[port_a]);
    let (carol, mut carol_events) = node(port_c, &[port_b]);

    tokio::time::sleep(Duration::from_secs(1)).await;

    let carol_status = carol.status().await.expect("carol status");
    assert_eq!(carol_status.links, 1, "carol should only be linked to bob");

    alice
        .broadcast(PacketKind::Intent, b"relayed".to_vec())
        .await
        .expect("broadcast");

    let mut got = false;
    let arrived = until(
        || {
            for event in drain(&mut carol_events) {
                if let BleEvent::Received {
                    kind: PacketKind::Intent,
                    payload,
                    ..
                } = event
                {
                    got |= payload == b"relayed";
                }
            }
            got
        },
        Duration::from_secs(3),
    )
    .await;

    assert!(arrived, "the intent was not relayed to the far node");
}

#[tokio::test]
async fn a_directed_send_reaches_only_its_recipient_even_through_a_relay() {
    // A—B—C, no link between A and C. Guardian shares (and anything else
    // that must go to one specific peer rather than the whole mesh) need
    // this: B must forward it without ever handing it to its own app layer,
    // and C must be the only one who does.
    let port_a = free_port();
    let port_b = free_port();
    let port_c = free_port();

    let (alice, _alice_events) = node(port_a, &[]);
    let (_bob, mut bob_events) = node(port_b, &[port_a]);
    let (carol, mut carol_events) = node(port_c, &[port_b]);

    tokio::time::sleep(Duration::from_secs(1)).await;

    let carol_id = carol.status().await.expect("carol status").peer_id;

    alice
        .send_to(carol_id, PacketKind::IntentAck, b"for carol only".to_vec())
        .await
        .expect("send_to");

    let mut received: Vec<Vec<u8>> = Vec::new();
    let arrived = until(
        || {
            for event in drain(&mut carol_events) {
                if let BleEvent::Received { kind: PacketKind::IntentAck, payload, .. } = event {
                    received.push(payload);
                }
            }
            !received.is_empty()
        },
        Duration::from_secs(3),
    )
    .await;

    assert!(arrived, "the directed packet never reached carol");
    assert_eq!(received, vec![b"for carol only".to_vec()]);

    // Bob relayed it — he must not also have delivered it to his own app,
    // since the packet was never addressed to him.
    let bob_saw_it = drain(&mut bob_events)
        .iter()
        .any(|event| matches!(event, BleEvent::Received { kind: PacketKind::IntentAck, .. }));
    assert!(!bob_saw_it, "an intermediate relay delivered a directed packet to its own app");
}

#[tokio::test]
async fn a_node_two_hops_away_is_visible_without_a_link_to_it() {
    // Carol appears on Alice's nodes screen at two hops, learned from Bob's
    // announcement rather than from a radio link Alice does not have.
    let port_a = free_port();
    let port_b = free_port();
    let port_c = free_port();

    let (alice, _alice_events) = node(port_a, &[]);
    let (_bob, _bob_events) = node(port_b, &[port_a]);
    let (_carol, _carol_events) = node(port_c, &[port_b]);

    // Announcements back off to 15–30 seconds once connected, so the window
    // that matters is the initial exchange after each link comes up.
    tokio::time::sleep(Duration::from_secs(2)).await;

    let peers = alice.peers().await.expect("alice peers");
    let hops: Vec<u8> = peers.iter().map(|peer| peer.hops).collect();

    assert_eq!(peers.len(), 2, "alice sees {hops:?}, wanted one direct and one relayed");
    assert_eq!(hops, vec![1, 2], "alice mis-classified how far away a node is");

    let status = alice.status().await.expect("alice status");
    assert_eq!(status.direct_peers, 1, "a two-hop node was counted as being in the room");
    assert_eq!(status.reachable_peers, 2);
}

#[tokio::test]
async fn the_offline_switch_silences_the_node() {
    // The kill switch has to reach the antenna. A node that stops routing but
    // keeps advertising would make the promise false in the only way that
    // matters.
    let port_a = free_port();
    let port_b = free_port();

    let (alice, _alice_events) = node(port_a, &[]);
    let (_bob, mut bob_events) = node(port_b, &[port_a]);

    tokio::time::sleep(SETTLE).await;
    let _ = drain(&mut bob_events);

    alice.set_offline(true).await.expect("offline");
    alice
        .broadcast(PacketKind::Intent, b"must not leave".to_vec())
        .await
        .expect("broadcast");

    tokio::time::sleep(Duration::from_secs(1)).await;

    let leaked = drain(&mut bob_events).into_iter().any(|event| {
        matches!(event, BleEvent::Received { payload, .. } if payload == b"must not leave")
    });
    assert!(!leaked, "an offline node's intent reached a peer");

    assert!(
        alice.status().await.expect("status").offline,
        "the status did not report the node as offline"
    );
}

#[tokio::test]
async fn a_gateway_advertises_itself_to_the_room() {
    // How an offline node finds somebody who can reach the chain for it.
    let port_a = free_port();
    let port_b = free_port();

    let (alice, _alice_events) = node(port_a, &[]);
    let (bob, _bob_events) = node(port_b, &[port_a]);

    bob.set_gateway(true).await.expect("set gateway");
    tokio::time::sleep(Duration::from_secs(1)).await;

    // Bob re-announces on his own schedule; the first announcement after the
    // link came up may predate the capability being set, so allow a window.
    let status = alice.status().await.expect("alice status");
    assert_eq!(
        status.gateways, 1,
        "a node with internet did not advertise it: {status:?}"
    );
}

#[tokio::test]
async fn a_handle_survives_a_transport_that_will_not_start() {
    // Bluetooth off, or the user declined the permission. The app must keep
    // running with the IP plane, and the status must be able to say so —
    // "no peers" and "no radio" are different messages.
    let occupied = std::net::TcpListener::bind("127.0.0.1:0").expect("bind");
    let port = occupied.local_addr().expect("addr").port();

    // Binding the same port again fails, which is this transport's stand-in
    // for a radio that will not come up.
    let (handle, mut events) = node(port, &[]);

    let reported = until(
        || {
            drain(&mut events)
                .iter()
                .any(|event| matches!(event, BleEvent::Unavailable(_)))
        },
        Duration::from_secs(2),
    )
    .await;

    assert!(reported, "an unavailable radio was not reported to the app");
    assert!(handle.is_running(), "the actor died with the radio");

    let status = handle.status().await.expect("status still answers");
    assert_eq!(status.links, 0);
    assert_eq!(status.direct_peers, 0);
}

#[tokio::test]
async fn a_link_going_away_removes_the_peer() {
    let port_a = free_port();
    let port_b = free_port();

    let (alice, _alice_events) = node(port_a, &[]);
    let (bob, _bob_events) = node(port_b, &[port_a]);

    tokio::time::sleep(SETTLE).await;
    assert_eq!(alice.status().await.expect("status").links, 1);

    // Bob's transport stops, which drops the socket and therefore the link.
    bob.set_offline(true).await.expect("offline");
    tokio::time::sleep(Duration::from_secs(1)).await;

    assert_eq!(
        alice.status().await.expect("status").links,
        0,
        "the link outlived the peer"
    );
}
