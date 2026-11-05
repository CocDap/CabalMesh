//! The CabalMesh circuit relay.
//!
//! # What this is for
//!
//! mDNS finds peers in the same room. Without a relay, that is the whole of
//! discovery — which for a mesh product is not a limitation, it is the absence
//! of the product. This process is what lets two phones on different networks
//! find each other and then, where hole punching works, stop needing it.
//!
//! # What it costs, said plainly
//!
//! It is a **single point of failure** for off-LAN discovery, and it **observes
//! which peer identifiers are online together**. For a product whose thesis is
//! zero identity, that second one is a real privacy surface. It is not
//! engineered away here and it should not be described as though it were — see
//! `docs/relay-operations.md`, which is written to be quotable in what the app
//! says about itself.
//!
//! # Running it
//!
//! ```text
//! cabal-relay --generate-key /etc/cabal-relay/relay.key   # once, then back it up
//! cabal-relay --key /etc/cabal-relay/relay.key --tcp-port 4001 --quic-port 4001
//! ```

mod identity;
mod limits;

use libp2p::futures::StreamExt;
use libp2p::{identify, noise, ping, relay, swarm::SwarmEvent, tcp, yamux, Multiaddr, SwarmBuilder};
use std::path::PathBuf;
use std::time::Duration;

/// The protocol name peers identify against.
///
/// Version it: a relay speaking a different revision fails by refusing
/// reservations, which reads as a network problem from the phone. A distinct
/// string makes the mismatch visible in the identify exchange instead.
const PROTOCOL: &str = "/cabalmesh/1.0.0";

fn main() -> Result<(), Box<dyn std::error::Error>> {
    tracing_subscriber::fmt()
        .with_env_filter(
            tracing_subscriber::EnvFilter::try_from_default_env()
                .unwrap_or_else(|_| "info,libp2p_relay=debug".into()),
        )
        .init();

    let args = Args::parse(std::env::args().skip(1))?;

    if let Some(path) = args.generate_key {
        let keypair = identity::generate(&path)?;
        // Printed on stdout, not logged: this is the value the operator has to
        // copy into the app's bootstrap address, and it should survive being
        // piped.
        println!("{}", keypair.public().to_peer_id());
        eprintln!(
            "Wrote {}. Back it up off-host before starting the relay — this peer id is \
             compiled into shipped builds and cannot be reconstructed.",
            path.display()
        );
        return Ok(());
    }

    let key_path = args.key.clone().ok_or("--key is required (or --generate-key to create one)")?;
    let keypair = identity::load(&key_path)?;
    let peer_id = keypair.public().to_peer_id();
    let limits = limits::Limits::default();

    tracing::info!(%peer_id, "relay identity loaded");
    tracing::info!(
        max_reservations = limits.max_reservations,
        max_circuits = limits.max_circuits,
        "limits applied — an unbounded relay would be an open proxy"
    );

    let runtime = tokio::runtime::Builder::new_multi_thread().enable_all().build()?;
    runtime.block_on(run(keypair, args, limits))
}

async fn run(
    keypair: libp2p::identity::Keypair,
    args: Args,
    limits: limits::Limits,
) -> Result<(), Box<dyn std::error::Error>> {
    let Args { tcp_port, quic_port, announce, .. } = args;

    let mut swarm = SwarmBuilder::with_existing_identity(keypair)
        .with_tokio()
        .with_tcp(
            tcp::Config::default().nodelay(true),
            noise::Config::new,
            yamux::Config::default,
        )?
        // Both transports, because the app dials both. A relay listening on
        // only one silently halves the peers that can reach it.
        .with_quic()
        .with_behaviour(|key| Behaviour {
            relay: relay::Behaviour::new(key.public().to_peer_id(), limits.to_relay_config()),
            // Identify is what tells a dialling peer its own observed address,
            // which is the input hole punching needs. Without it the relay
            // works and DCUtR never succeeds, so every connection stays relayed
            // — correct, and far more expensive than it should be.
            identify: identify::Behaviour::new(identify::Config::new(
                PROTOCOL.into(),
                key.public(),
            )),
            ping: ping::Behaviour::new(ping::Config::new().with_interval(Duration::from_secs(30))),
        })?
        // The swarm's own default is `Duration::ZERO`: a connection with no
        // protocol actively demanding a stream — which is the normal state of
        // a client just holding a reservation — reads as idle and gets closed
        // within a handler's keep-alive gap, regardless of ping succeeding.
        // Observed on real devices as a bootstrap connection that reports
        // `online` for ~15s and then drops it, repeating forever. The app side
        // already sets 60s for the same reason; the relay needs the same floor
        // or it is the side that hangs up first.
        .with_swarm_config(|c| c.with_idle_connection_timeout(Duration::from_secs(60)))
        .build();

    for address in [
        format!("/ip4/0.0.0.0/tcp/{tcp_port}"),
        format!("/ip4/0.0.0.0/udp/{quic_port}/quic-v1"),
        format!("/ip6/::/tcp/{tcp_port}"),
        format!("/ip6/::/udp/{quic_port}/quic-v1"),
    ] {
        match address.parse::<Multiaddr>() {
            // A host without IPv6 fails to bind the v6 listeners, which is
            // normal rather than fatal — the v4 ones still carry everything.
            Ok(parsed) => {
                if let Err(error) = swarm.listen_on(parsed) {
                    tracing::warn!(%address, %error, "could not listen");
                }
            }
            Err(error) => tracing::warn!(%address, %error, "unparseable listen address"),
        }
    }

    // **A relay must advertise an address, or every reservation fails.**
    //
    // The reservation response carries the addresses a client should publish
    // as its own circuit address. A relay with no external address returns an
    // empty set, and the client rejects the reservation with
    // `NoAddressesInReservation` — after a successful dial, a successful
    // connection, and no error on this side. It looks like it worked.
    //
    // Explicit `--announce` wins where it is given: a host reached by a DNS
    // name, or sitting behind a 1:1 NAT, binds an address that is not the one
    // clients can use.
    for address in &announce {
        match address.parse::<Multiaddr>() {
            Ok(parsed) => {
                tracing::info!(%address, "announcing");
                swarm.add_external_address(parsed);
            }
            Err(error) => tracing::warn!(%address, %error, "unparseable announce address"),
        }
    }
    let announce_observed = announce.is_empty();

    let mut shutdown = std::pin::pin!(tokio::signal::ctrl_c());

    loop {
        tokio::select! {
            _ = &mut shutdown => {
                tracing::info!("shutting down");
                return Ok(());
            }
            event = swarm.select_next_some() => match event {
                SwarmEvent::NewListenAddr { address, .. } => {
                    // The line an operator copies into the app's bootstrap
                    // list, printed whole so it does not have to be assembled
                    // by hand from a peer id and a port.
                    tracing::info!(
                        "listening: {address}/p2p/{}",
                        swarm.local_peer_id()
                    );
                    // The ticket requires the relay to run with no NAT in front,
                    // so what it binds is what clients reach. Loopback is the
                    // exception — announcing it would hand every client an
                    // address that resolves to themselves.
                    if announce_observed && !is_loopback(&address) {
                        swarm.add_external_address(address);
                    }
                }
                SwarmEvent::Behaviour(Event::Relay(relay::Event::ReservationReqAccepted { src_peer_id, .. })) => {
                    tracing::info!(peer = %src_peer_id, "reservation accepted");
                }
                SwarmEvent::Behaviour(Event::Relay(relay::Event::ReservationReqDenied { src_peer_id })) => {
                    // Worth its own line at info: a relay that has hit its
                    // limits looks identical to an unreachable one from the
                    // phone, and this is the only place the difference shows.
                    tracing::info!(peer = %src_peer_id, "reservation denied — at limit");
                }
                SwarmEvent::Behaviour(Event::Relay(relay::Event::CircuitReqDenied { src_peer_id, dst_peer_id })) => {
                    tracing::info!(src = %src_peer_id, dst = %dst_peer_id, "circuit denied — at limit");
                }
                SwarmEvent::Behaviour(Event::Relay(other)) => {
                    tracing::debug!(?other, "relay event");
                }
                SwarmEvent::Behaviour(Event::Identify(boxed)) => match *boxed {
                    identify::Event::Received { peer_id, info, .. } => {
                        // The observed address is what the peer needs for hole
                        // punching. Logged at debug because it is per-connection,
                        // but logged: a relay where every connection stays
                        // relayed is usually one whose identify reaches nobody.
                        tracing::debug!(
                            peer = %peer_id,
                            observed = %info.observed_addr,
                            protocol = %info.protocol_version,
                            "identified"
                        );
                    }
                    other => tracing::trace!(?other, "identify event"),
                },
                SwarmEvent::Behaviour(Event::Ping(ping::Event { peer, result: Err(error), .. })) => {
                    tracing::debug!(peer = %peer, %error, "ping failed");
                }
                _ => {}
            },
        }
    }
}

/// Whether an address points back at the host that published it.
///
/// A relay that announced `127.0.0.1` would hand every client a circuit
/// address resolving to the client itself.
fn is_loopback(address: &Multiaddr) -> bool {
    address.iter().any(|segment| match segment {
        libp2p::multiaddr::Protocol::Ip4(ip) => ip.is_loopback(),
        libp2p::multiaddr::Protocol::Ip6(ip) => ip.is_loopback(),
        _ => false,
    })
}

#[derive(libp2p_swarm::NetworkBehaviour)]
#[behaviour(to_swarm = "Event")]
struct Behaviour {
    relay: relay::Behaviour,
    identify: identify::Behaviour,
    ping: ping::Behaviour,
}

/// The three behaviours' events, merged.
///
/// `identify::Event` is several hundred bytes larger than the others because it
/// carries a peer's full advertised `Info`. Boxing it keeps every event on the
/// swarm's queue from being sized for the largest one — this queue carries
/// relay and ping traffic in far greater volume.
#[derive(Debug)]
enum Event {
    Relay(relay::Event),
    Identify(Box<identify::Event>),
    Ping(ping::Event),
}

impl From<relay::Event> for Event {
    fn from(event: relay::Event) -> Self {
        Self::Relay(event)
    }
}

impl From<identify::Event> for Event {
    fn from(event: identify::Event) -> Self {
        Self::Identify(Box::new(event))
    }
}

impl From<ping::Event> for Event {
    fn from(event: ping::Event) -> Self {
        Self::Ping(event)
    }
}

/// Command-line arguments.
///
/// Hand-parsed rather than with `clap`: six flags do not justify a dependency
/// on a host that should carry as little as possible.
#[derive(Debug, Default, PartialEq, Eq, Clone)]
struct Args {
    key: Option<PathBuf>,
    generate_key: Option<PathBuf>,
    tcp_port: u16,
    quic_port: u16,
    /// Addresses to advertise instead of the ones actually bound. Repeatable.
    announce: Vec<String>,
}

impl Args {
    fn parse(input: impl Iterator<Item = String>) -> Result<Self, String> {
        let mut args = Self {
            // 4001 on both, which is libp2p's convention and what the firewall
            // rules in docs/relay-operations.md open.
            tcp_port: 4001,
            quic_port: 4001,
            ..Self::default()
        };
        let mut input = input.peekable();

        while let Some(flag) = input.next() {
            let mut value = || input.next().ok_or_else(|| format!("{flag} needs a value"));
            match flag.as_str() {
                "--key" => args.key = Some(PathBuf::from(value()?)),
                "--generate-key" => args.generate_key = Some(PathBuf::from(value()?)),
                "--tcp-port" => {
                    args.tcp_port = value()?.parse().map_err(|_| "--tcp-port must be a port number".to_string())?;
                }
                "--quic-port" => {
                    args.quic_port = value()?.parse().map_err(|_| "--quic-port must be a port number".to_string())?;
                }
                "--announce" => args.announce.push(value()?),
                "--help" | "-h" => return Err(USAGE.to_string()),
                other => return Err(format!("unknown argument {other}\n\n{USAGE}")),
            }
        }

        Ok(args)
    }
}

const USAGE: &str = "\
cabal-relay — the CabalMesh circuit relay

  --generate-key <path>   Create an identity and exit. Do this once, then back
                          it up off-host: the peer id is compiled into shipped
                          builds and cannot be reconstructed.
  --key <path>            Identity to run as. Required.
  --tcp-port <port>       Default 4001.
  --quic-port <port>      Default 4001.
  --announce <multiaddr>  Advertise this instead of what is bound. Repeatable.
                          Needed when the host is reached by a name, or sits
                          behind a 1:1 NAT. Without it the bound non-loopback
                          addresses are announced.
";

#[cfg(test)]
mod tests {
    use super::*;

    fn parse(args: &[&str]) -> Result<Args, String> {
        Args::parse(args.iter().map(|s| (*s).to_string()))
    }

    #[test]
    fn ports_default_to_the_documented_ones() {
        // These are the ports docs/relay-operations.md tells an operator to
        // open. A change here without a change there is a relay behind a closed
        // firewall.
        let args = parse(&["--key", "/etc/cabal-relay/relay.key"]).unwrap();
        assert_eq!(args.tcp_port, 4001);
        assert_eq!(args.quic_port, 4001);
    }

    #[test]
    fn both_transports_can_be_moved_independently() {
        let args = parse(&["--key", "k", "--tcp-port", "4101", "--quic-port", "4102"]).unwrap();
        assert_eq!(args.tcp_port, 4101);
        assert_eq!(args.quic_port, 4102);
    }

    #[test]
    fn an_unknown_flag_is_refused_rather_than_ignored() {
        // Silently ignoring a typo'd flag means the relay runs with a limit or
        // a port the operator believes they changed.
        assert!(parse(&["--relay-port", "4001"]).is_err());
    }

    #[test]
    fn a_flag_without_its_value_is_refused() {
        assert!(parse(&["--key"]).is_err());
        assert!(parse(&["--tcp-port"]).is_err());
    }

    #[test]
    fn a_non_numeric_port_is_refused() {
        assert!(parse(&["--tcp-port", "http"]).is_err());
    }

    #[test]
    fn announce_is_repeatable() {
        // A host with both a v4 and a v6 public address needs to advertise
        // both, and a host reached by name needs the name instead of what it
        // bound.
        let args = parse(&[
            "--key", "k",
            "--announce", "/ip4/203.0.113.7/tcp/4001",
            "--announce", "/dns4/relay.example/udp/4001/quic-v1",
        ])
        .unwrap();
        assert_eq!(args.announce.len(), 2);
    }

    #[test]
    fn loopback_is_never_announced() {
        // The regression this guards: announcing 127.0.0.1 hands every client a
        // circuit address that resolves to the client itself.
        assert!(is_loopback(&"/ip4/127.0.0.1/tcp/4001".parse().unwrap()));
        assert!(is_loopback(&"/ip6/::1/udp/4001/quic-v1".parse().unwrap()));
    }

    #[test]
    fn a_routable_address_is_announced() {
        assert!(!is_loopback(&"/ip4/203.0.113.7/tcp/4001".parse().unwrap()));
        assert!(!is_loopback(&"/ip6/2001:db8::1/tcp/4001".parse().unwrap()));
        // A private address is still announced: a relay on a LAN for testing is
        // a legitimate deployment, and only the operator knows which it is.
        assert!(!is_loopback(&"/ip4/192.168.1.10/tcp/4001".parse().unwrap()));
    }
}
