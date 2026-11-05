//! A real client reserving through a real relay.
//!
//! Ticket 23's acceptance asks that two devices on different networks connect
//! through the relay and upgrade to direct where possible. Two networks cannot
//! be arranged from one machine — but the part that actually breaks silently
//! can be: **whether the relay accepts a reservation at all.**
//!
//! That is the failure worth a test. A relay running a different protocol
//! revision, or with a limit misconfigured, does not log an error and does not
//! refuse to start. It refuses reservations, and from the phone that is
//! indistinguishable from the relay being unreachable. This asserts the
//! handshake completes against the same `relay::Config` the binary ships.

use libp2p::futures::StreamExt;
use libp2p::{noise, relay, swarm::SwarmEvent, tcp, yamux, Multiaddr, SwarmBuilder};
use std::time::Duration;

/// How long the whole exchange may take before the test gives up.
///
/// Generous: this is loopback, so it completes in milliseconds, and a large
/// bound only affects how long a genuine failure takes to report.
const DEADLINE: Duration = Duration::from_secs(20);

#[tokio::test]
async fn a_client_can_reserve_a_slot_on_the_relay() {
    let (relay_peer, relay_addr, _relay) = start_relay().await;

    let mut client = SwarmBuilder::with_new_identity()
        .with_tokio()
        .with_tcp(
            tcp::Config::default().nodelay(true),
            noise::Config::new,
            yamux::Config::default,
        )
        .unwrap()
        .with_relay_client(noise::Config::new, yamux::Config::default)
        .unwrap()
        .with_behaviour(|_key, relay_behaviour| relay_behaviour)
        .unwrap()
        .build();

    // Listening on a circuit address is what asks for a reservation. The
    // address reads as "reach me through the relay".
    let circuit: Multiaddr = format!("{relay_addr}/p2p/{relay_peer}/p2p-circuit")
        .parse()
        .expect("circuit address");
    client.listen_on(circuit).expect("listen on the circuit");

    let accepted = tokio::time::timeout(DEADLINE, async {
        loop {
            if let SwarmEvent::Behaviour(relay::client::Event::ReservationReqAccepted { .. }) =
                client.select_next_some().await
            {
                return;
            }
        }
    })
    .await;

    assert!(
        accepted.is_ok(),
        "the relay never accepted a reservation — a phone would read this as the relay being unreachable"
    );
}

/// Starts a relay carrying the binary's own limits, and returns its identity
/// and first listen address.
async fn start_relay() -> (libp2p::PeerId, Multiaddr, tokio::task::JoinHandle<()>) {
    // The same defaults main.rs runs with. A test against permissive limits
    // would pass while the shipped configuration refused every reservation.
    let config = relay::Config {
        max_reservations: 512,
        max_reservations_per_peer: 4,
        reservation_duration: Duration::from_secs(3_600),
        max_circuits: 256,
        max_circuits_per_peer: 8,
        max_circuit_duration: Duration::from_secs(600),
        max_circuit_bytes: 8 * 1024 * 1024,
        ..Default::default()
    };

    let mut swarm = SwarmBuilder::with_new_identity()
        .with_tokio()
        .with_tcp(
            tcp::Config::default().nodelay(true),
            noise::Config::new,
            yamux::Config::default,
        )
        .unwrap()
        .with_behaviour(|key| relay::Behaviour::new(key.public().to_peer_id(), config))
        .unwrap()
        .build();

    let peer_id = *swarm.local_peer_id();
    // Port zero: a fixed port makes the test fail when anything else on the
    // machine happens to hold it.
    swarm
        .listen_on("/ip4/127.0.0.1/tcp/0".parse().unwrap())
        .unwrap();

    let address = tokio::time::timeout(DEADLINE, async {
        loop {
            if let SwarmEvent::NewListenAddr { address, .. } = swarm.select_next_some().await {
                return address;
            }
        }
    })
    .await
    .expect("the relay bound a port");

    // Without this the reservation is accepted and then rejected by the client
    // with `NoAddressesInReservation` — the relay has nothing to tell the peer
    // to publish. Loopback is fine here and only here, because both ends are
    // this process. The binary declines to announce loopback for exactly the
    // reason it would be wrong in production.
    swarm.add_external_address(address.clone());

    let driver = tokio::spawn(async move {
        loop {
            let _ = swarm.select_next_some().await;
        }
    });

    (peer_id, address, driver)
}
