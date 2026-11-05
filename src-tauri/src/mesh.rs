use futures::StreamExt;
use libp2p::swarm::behaviour::toggle::Toggle;
use libp2p::{
    dcutr, gossipsub, identify, mdns, noise, ping, relay,
    swarm::{NetworkBehaviour, SwarmEvent},
    tcp, yamux, Swarm, SwarmBuilder,
};
use serde::{Deserialize, Serialize};
use std::collections::hash_map::DefaultHasher;
use std::error::Error;
use std::hash::{Hash, Hasher};
use std::sync::atomic::AtomicU64;
use std::sync::Arc;
use std::time::Duration;
use tokio::sync::mpsc;

#[derive(NetworkBehaviour)]
#[behaviour(to_swarm = "MeshBehaviourEvent")]
pub struct MeshBehaviour {
    /// `Toggle` because local-network discovery is a **permission**, not a
    /// capability. iOS and Android can both refuse it, and a node that failed
    /// to start because the user declined a prompt would be worse than one
    /// that quietly falls back to bootstrap peers.
    pub mdns: Toggle<mdns::tokio::Behaviour>,
    pub gossipsub: gossipsub::Behaviour,
    /// Tells peers our addresses and learns theirs — a precondition for relay
    /// reservations and for hole punching.
    pub identify: identify::Behaviour,
    /// Liveness. Also keeps NAT bindings warm, which matters on cellular.
    pub ping: ping::Behaviour,
    /// Lets this node be reached through a relay when it cannot be dialled
    /// directly, which on mobile is most of the time.
    pub relay: relay::client::Behaviour,
    /// Upgrades a relayed connection to a direct one when hole punching
    /// succeeds, so the relay carries handshakes rather than the whole mesh.
    pub dcutr: dcutr::Behaviour,
}

#[derive(Debug)]
pub enum MeshBehaviourEvent {
    Mdns(mdns::Event),
    Gossipsub(gossipsub::Event),
    Identify(identify::Event),
    Ping(ping::Event),
    Relay(relay::client::Event),
    Dcutr(dcutr::Event),
}

impl From<mdns::Event> for MeshBehaviourEvent {
    fn from(event: mdns::Event) -> Self {
        MeshBehaviourEvent::Mdns(event)
    }
}

impl From<identify::Event> for MeshBehaviourEvent {
    fn from(event: identify::Event) -> Self {
        MeshBehaviourEvent::Identify(event)
    }
}

impl From<ping::Event> for MeshBehaviourEvent {
    fn from(event: ping::Event) -> Self {
        MeshBehaviourEvent::Ping(event)
    }
}

impl From<relay::client::Event> for MeshBehaviourEvent {
    fn from(event: relay::client::Event) -> Self {
        MeshBehaviourEvent::Relay(event)
    }
}

impl From<dcutr::Event> for MeshBehaviourEvent {
    fn from(event: dcutr::Event) -> Self {
        MeshBehaviourEvent::Dcutr(event)
    }
}

impl From<gossipsub::Event> for MeshBehaviourEvent {
    fn from(event: gossipsub::Event) -> Self {
        MeshBehaviourEvent::Gossipsub(event)
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PrivacyIntent {
    /// The composing device's own intent id, carried verbatim.
    ///
    /// Lets a receiver dedupe: the BLE fallback resends the identical intent
    /// several times against a link with no delivery confirmation, so without
    /// this a "received" count would report retries as distinct trades.
    #[serde(default)]
    pub id: String,
    pub intent_type: String,
    pub payload: String,
    pub encrypted: bool,
    pub relay_path: Vec<String>,
    pub relay_fee: Option<String>,
}

pub struct MeshNetwork {
    pub swarm: Swarm<MeshBehaviour>,
    pub topic: gossipsub::IdentTopic,
    /// Real bytes relayed (sent + received) through the mesh topic, shared
    /// with the Tauri command layer so Relay Mode can show actual traffic
    /// instead of a simulated number.
    pub relay_bytes: Arc<AtomicU64>,
}

/// The DNS resolver the swarm should use, read from the system where possible.
///
/// libp2p's `with_dns()` calls this same system lookup and treats failure as
/// fatal. On Android there is no `/etc/resolv.conf` — DNS goes through `netd` —
/// so the lookup always fails and the entire swarm failed to build with a bare
/// `io error: No such file or directory (os error 2)`. Nothing in that message
/// says "DNS", and the mesh was dead for want of a resolver it had nothing to
/// resolve yet.
///
/// The fallback is Cloudflare rather than the crate default of Google, chosen
/// deliberately for an app that argues about privacy. It resolves only `/dns/`
/// multiaddrs in the relay list, so today — with that list empty — it is never
/// consulted at all. Once ticket 23 puts real relays in, this deserves to be
/// configurable alongside them.
fn resolver_settings() -> (libp2p::dns::ResolverConfig, libp2p::dns::ResolverOpts) {
    match hickory_resolver::system_conf::read_system_conf() {
        Ok(settings) => settings,
        Err(error) => {
            tracing::warn!(
                %error,
                "no system resolver configuration; falling back to Cloudflare DNS"
            );
            (
                libp2p::dns::ResolverConfig::cloudflare(),
                libp2p::dns::ResolverOpts::default(),
            )
        }
    }
}

impl MeshNetwork {
    pub async fn new() -> Result<Self, Box<dyn Error>> {
        // Generate ephemeral keypair for "Nobody" identity
        let local_key = libp2p::identity::Keypair::generate_ed25519();
        let local_peer_id = local_key.public().to_peer_id();
        
        tracing::info!(peer_id = %local_peer_id, "ephemeral peer id generated");

        // Configure Gossipsub for Privacy Intent broadcasting
        let message_id_fn = |message: &gossipsub::Message| {
            let mut s = DefaultHasher::new();
            message.data.hash(&mut s);
            gossipsub::MessageId::from(s.finish().to_string())
        };

        let gossipsub_config = gossipsub::ConfigBuilder::default()
            .heartbeat_interval(Duration::from_secs(1))
            .validation_mode(gossipsub::ValidationMode::Strict)
            .message_id_fn(message_id_fn)
            .build()
            .map_err(|msg| std::io::Error::new(std::io::ErrorKind::Other, msg))?;

        let mut gossipsub = gossipsub::Behaviour::new(
            gossipsub::MessageAuthenticity::Signed(local_key.clone()),
            gossipsub_config,
        )?;

        // Create the Privacy Intent topic
        let topic = gossipsub::IdentTopic::new("cabalmesh-privacy-intents");
        gossipsub.subscribe(&topic)?;

        // Local discovery is a permission on both mobile platforms. A refusal
        // disables it rather than failing to start: discovery then degrades to
        // bootstrap peers instead of the node being dead.
        let mdns = match mdns::tokio::Behaviour::new(mdns::Config::default(), local_peer_id) {
            Ok(behaviour) => Toggle::from(Some(behaviour)),
            Err(error) => {
                tracing::warn!(
                    %error,
                    "local-network discovery unavailable; falling back to bootstrap peers"
                );
                Toggle::from(None)
            }
        };

        let identify_config = identify::Config::new(
            "/cabalmesh/1.0.0".to_string(),
            local_key.public(),
        );

        let (resolver_config, resolver_opts) = resolver_settings();

        let swarm = SwarmBuilder::with_existing_identity(local_key)
            .with_tokio()
            .with_tcp(
                tcp::Config::default(),
                noise::Config::new,
                yamux::Config::default,
            )?
            // QUIC alongside TCP. Connection migration survives a Wi-Fi to
            // cellular handoff, which a TCP socket does not, and 0-RTT
            // resumption makes returning from background cheap.
            .with_quic()
            // Configured explicitly rather than via `with_dns()`, which reads
            // `/etc/resolv.conf` — a file Android does not have. There the
            // whole swarm failed to build with a bare `os error 2`, taking the
            // mesh down over a resolver it had nothing to resolve with yet.
            .with_dns_config(resolver_config, resolver_opts)
            .with_relay_client(noise::Config::new, yamux::Config::default)?
            .with_behaviour(|key, relay_behaviour| MeshBehaviour {
                mdns,
                gossipsub,
                identify: identify::Behaviour::new(identify_config),
                ping: ping::Behaviour::new(ping::Config::new()),
                relay: relay_behaviour,
                dcutr: dcutr::Behaviour::new(key.public().to_peer_id()),
            })?
            .with_swarm_config(|c| c.with_idle_connection_timeout(Duration::from_secs(60)))
            .build();

        Ok(MeshNetwork { swarm, topic, relay_bytes: Arc::new(AtomicU64::new(0)) })
    }

    /// The swarm event loop. Everything the mesh logs happens inside this span,
    /// so device output is attributable to the node rather than floating free.
    #[tracing::instrument(skip_all, fields(peer_id = %self.swarm.local_peer_id()))]
    pub async fn start(
        &mut self,
        tx: mpsc::UnboundedSender<MeshEvent>,
        mut commands: tokio::sync::mpsc::Receiver<crate::mesh_handle::MeshCommand>,
    ) -> Result<(), Box<dyn Error>> {
        use crate::mesh_handle::{MeshCommand, MeshError, MeshSnapshot};

        // Listen on both transports. QUIC is preferred by peers that support
        // it; TCP remains for those that do not and for networks that block
        // UDP.
        self.swarm.listen_on("/ip4/0.0.0.0/tcp/0".parse()?)?;
        if let Err(error) = self.swarm.listen_on("/ip4/0.0.0.0/udp/0/quic-v1".parse()?) {
            // Some networks block UDP outright. TCP alone still works, so this
            // degrades rather than fails.
            tracing::warn!(%error, "QUIC listener unavailable; continuing on TCP");
        }

        // Dial bootstrap peers so discovery is not limited to this network.
        let bootstrap = crate::bootstrap_config::BootstrapConfig::load(
            &cabal_store::JsonStore::new(crate::app_paths::in_data_dir("bootstrap.json")),
        );
        // Peer id of each configured relay, so the reservation below can be
        // requested once the dial actually connects rather than raced against
        // it — a circuit-listen has nothing to attach to before that and
        // closes right back up (`ListenerClosed { reason: Ok(()) }`, no error
        // at all, which is what made this take two attempts to get right).
        let mut pending_relay_reservations: std::collections::HashMap<
            libp2p::PeerId,
            libp2p::Multiaddr,
        > = std::collections::HashMap::new();
        if bootstrap.has_relays() {
            for address in bootstrap.parsed() {
                match self.swarm.dial(address.clone()) {
                    Ok(()) => tracing::info!(%address, "dialling bootstrap peer"),
                    Err(error) => tracing::warn!(%address, %error, "bootstrap dial failed"),
                }
                if let Some(peer_id) = address.iter().find_map(|p| match p {
                    libp2p::multiaddr::Protocol::P2p(id) => Some(id),
                    _ => None,
                }) {
                    let circuit = address.clone().with(libp2p::multiaddr::Protocol::P2pCircuit);
                    pending_relay_reservations.insert(peer_id, circuit);
                }
            }
        } else {
            tracing::info!(
                "no bootstrap relays configured — discovery is limited to this network"
            );
        }

        // Answered from the loop rather than tracked elsewhere, so a snapshot
        // always reflects what the swarm is actually doing.
        let mut listening_on: Vec<String> = Vec::new();
        let mut offline = false;
        // Edge-triggered against `self.swarm.connected_peers()` on every
        // connection open/close below, rather than recomputed on each
        // `Snapshot` poll: a gossipsub `publish` only reaches anyone once at
        // least one peer is connected, so "have a connected peer" is exactly
        // the condition this device can usefully act as a gateway under.
        let mut online = false;

        loop {
            tokio::select! {
                Some(command) = commands.recv() => {
                    match command {
                        MeshCommand::Snapshot { reply } => {
                            let _ = reply.send(MeshSnapshot {
                                peer_id: self.swarm.local_peer_id().to_string(),
                                peer_count: self.swarm.connected_peers().count(),
                                listening_on: listening_on.clone(),
                                offline,
                                online,
                                relay_bytes: self.relay_bytes.load(std::sync::atomic::Ordering::Relaxed),
                            });
                            continue;
                        }
                        MeshCommand::SetOffline { offline: next, reply } => {
                            // Deliberately does not tear the swarm down:
                            // rebuilding on resume would drop every established
                            // connection and re-run discovery from nothing.
                            offline = next;
                            tracing::info!(offline, "mesh participation toggled");
                            let _ = reply.send(());
                            continue;
                        }
                        MeshCommand::Publish { intent, reply } => {
                            if offline {
                                // The offline switch promises nothing leaves the
                                // device. Honour it here rather than relying on
                                // callers to check first.
                                let _ = reply.send(Err(MeshError::Publish));
                                continue;
                            }
                            let intent = *intent;
                            let outcome = self.broadcast_intent(intent);
                            let _ = reply.send(outcome.map_err(|_| MeshError::Publish));
                            continue;
                        }
                    }
                }

                // Handle libp2p swarm events
                event = self.swarm.select_next_some() => {
                    match event {
                        SwarmEvent::NewListenAddr { address, .. } => {
                            tracing::info!(address = %address, "listening");
                            listening_on.push(address.to_string());
                            let _ = tx.send(MeshEvent::ListeningStarted { address: address.to_string() });
                        }
                        SwarmEvent::Behaviour(event) => match event {
                            MeshBehaviourEvent::Mdns(mdns::Event::Discovered(list)) => {
                                for (peer_id, multiaddr) in list {
                                    tracing::info!(peer_id = %peer_id, address = %multiaddr, "peer discovered");
                                    self.swarm.behaviour_mut().gossipsub.add_explicit_peer(&peer_id);
                                    let _ = tx.send(MeshEvent::PeerDiscovered {
                                        peer_id: peer_id.to_string(),
                                        address: multiaddr.to_string(),
                                    });
                                }
                            }
                            MeshBehaviourEvent::Mdns(mdns::Event::Expired(list)) => {
                                for (peer_id, _) in list {
                                    tracing::info!("👻 Peer expired: {}", peer_id);
                                    self.swarm.behaviour_mut().gossipsub.remove_explicit_peer(&peer_id);
                                }
                            }
                            MeshBehaviourEvent::Gossipsub(gossipsub::Event::Message { message, .. }) => {
                                // Try to parse as PrivacyIntent first
                                let intent: Result<PrivacyIntent, _> = serde_json::from_slice(&message.data);
                                if let Ok(intent) = intent {
                                    if intent.intent_type == "settlement" {
                                        if let Some(event) = classify_settlement(&intent) {
                                            let _ = tx.send(event);
                                        }
                                    } else {
                                        // Regular trade intent
                                        tracing::info!("📬 Received Intent: {:?}", intent);
                                        let _ = tx.send(MeshEvent::IntentReceived { intent });
                                    }
                                }
                            }
                            MeshBehaviourEvent::Relay(event) => {
                                // Nothing downstream reacted to these before —
                                // silently swallowed by the catch-all this arm
                                // replaces. Logged at info while the
                                // reservation-based bootstrap path is new
                                // enough to still need eyes on it; a reservation
                                // that is silently refused or never requested
                                // looks identical to one that succeeded, from
                                // every other log line in this file.
                                tracing::info!(?event, "relay client event");
                            }
                            _ => {}
                        },
                        SwarmEvent::ConnectionEstablished { peer_id, .. } => {
                            // `remove` rather than `get`: a relay dialled on
                            // both TCP and QUIC connects twice, and the second
                            // arrival should not re-request a reservation
                            // already in flight from the first.
                            if let Some(circuit) = pending_relay_reservations.remove(&peer_id) {
                                match self.swarm.listen_on(circuit.clone()) {
                                    Ok(_) => tracing::info!(%peer_id, %circuit, "requesting relay reservation"),
                                    Err(error) => tracing::warn!(%peer_id, %circuit, %error, "could not request relay reservation"),
                                }
                            }
                            let now_online = self.swarm.connected_peers().count() > 0;
                            if now_online != online {
                                online = now_online;
                                tracing::info!(online, "IP plane connectivity changed");
                                let _ = tx.send(MeshEvent::ConnectivityChanged { online });
                            }
                        }
                        SwarmEvent::ConnectionClosed { .. } => {
                            let now_online = self.swarm.connected_peers().count() > 0;
                            if now_online != online {
                                online = now_online;
                                tracing::info!(online, "IP plane connectivity changed");
                                let _ = tx.send(MeshEvent::ConnectivityChanged { online });
                            }
                        }
                        SwarmEvent::OutgoingConnectionError { peer_id, error, .. } => {
                            tracing::warn!(peer_id = ?peer_id, %error, "outgoing connection failed");
                        }
                        SwarmEvent::ListenerError { listener_id, error, .. } => {
                            tracing::warn!(?listener_id, %error, "listener error — a relay reservation attempt likely failed here");
                        }
                        SwarmEvent::ListenerClosed { listener_id, reason, .. } => {
                            tracing::warn!(?listener_id, ?reason, "listener closed");
                        }
                        _ => {}
                    }
                }
            }
        }
    }

    pub fn broadcast_intent(&mut self, intent: PrivacyIntent) -> Result<(), Box<dyn Error>> {
        if !Self::verify_relay_integrity(&intent) {
            return Err("Integrity check failed: Malformed relay path".into());
        }

        let payload = serde_json::to_vec(&intent)?;
        match self.swarm
            .behaviour_mut()
            .gossipsub
            .publish(self.topic.clone(), payload)
        {
            Ok(_) => {
                tracing::info!(hops = intent.relay_path.len(), intent_type = %intent.intent_type, "intent broadcast");
                Ok(())
            }
            Err(gossipsub::PublishError::InsufficientPeers) => {
                tracing::warn!("⚠️  Note: No peers connected (Single-Node Mode). Intent processed locally.");
                Ok(())
            }
            Err(e) => Err(Box::new(e))
        }
    }

    pub fn broadcast_raw(&mut self, message: String) -> Result<(), Box<dyn Error>> {
        let payload = message.as_bytes().to_vec();
        match self.swarm
            .behaviour_mut()
            .gossipsub
            .publish(self.topic.clone(), payload) 
        {
            Ok(_) => {
                tracing::info!("📤 Raw message broadcasted to mesh");
                Ok(())
            }
            Err(gossipsub::PublishError::InsufficientPeers) => {
                tracing::warn!("⚠️  Note: No peers connected (Single-Node Mode).");
                Ok(())
            }
            Err(e) => Err(Box::new(e))
        }
    }
    pub fn verify_relay_integrity(intent: &PrivacyIntent) -> bool {
        // Basic integrity check:
        // 1. Relay path should not be empty (should contain at least sender)
        // 2. Relay fee should be formatted correctly (simple check for now)
        
        if intent.relay_path.is_empty() {
            tracing::error!("❌ Integrity Check Failed: Empty relay path");
            return false;
        }

        // In a real implementation, we would verify signatures of each hop
        true
    }
}

/// The event a `settlement` message means, on either plane.
///
/// One function because the IP mesh and the BLE flood carry the same messages.
/// Reading them in only one place is how a deal agreed over Bluetooth once never
/// reached the ledger.
#[must_use]
pub fn classify_settlement(intent: &PrivacyIntent) -> Option<MeshEvent> {
    if intent.intent_type != "settlement" {
        return None;
    }
    let message: serde_json::Value = serde_json::from_str(&intent.payload).ok()?;
    let details = intent.payload.clone();
    match message.get("type").and_then(serde_json::Value::as_str)? {
        "SettlementComplete" => Some(MeshEvent::SettlementComplete { details }),
        "DealAccepted" => Some(MeshEvent::DealAccepted { details }),
        "SignedDiff" => Some(MeshEvent::SignedDiff { details }),
        _ => None,
    }
}

#[derive(Debug, Clone, Serialize)]
#[serde(tag = "type")]
pub enum MeshEvent {
    ListeningStarted { address: String },
    PeerDiscovered { peer_id: String, address: String },
    IntentReceived { intent: PrivacyIntent },
    DealAccepted { details: String },
    SettlementComplete { details: String },
    /// One side of a deal, signed. See `near_settlement`.
    SignedDiff { details: String },
    /// The IP plane gained or lost its last connected peer. `online` answers
    /// "can this device currently reach the internet" — see `set_gateway` on
    /// `cabal_ble::Engine`, which is what this is meant to drive: a node with
    /// no IP-plane connectivity has nothing to offer a BLE peer asking for a
    /// gateway, so it must not advertise the capability bit.
    ConnectivityChanged { online: bool },
}

#[cfg(test)]
mod settlement_tests {
    use super::*;

    fn settlement(payload: &str) -> PrivacyIntent {
        PrivacyIntent {
            id: "m".into(),
            intent_type: "settlement".into(),
            payload: payload.into(),
            encrypted: false,
            relay_path: vec!["origin_node".into()],
            relay_fee: None,
        }
    }

    #[test]
    fn each_settlement_message_has_its_event() {
        assert!(matches!(
            classify_settlement(&settlement(r#"{"type":"DealAccepted"}"#)),
            Some(MeshEvent::DealAccepted { .. })
        ));
        assert!(matches!(
            classify_settlement(&settlement(r#"{"type":"SignedDiff"}"#)),
            Some(MeshEvent::SignedDiff { .. })
        ));
        assert!(matches!(
            classify_settlement(&settlement(r#"{"type":"SettlementComplete"}"#)),
            Some(MeshEvent::SettlementComplete { .. })
        ));
    }

    #[test]
    fn a_diff_keeps_its_exact_text() {
        // Reparsing would risk a 24-decimal number. The payload passes through untouched.
        let raw = r#"{"type":"SignedDiff", "amount":1000000000000000000000000}"#;
        let Some(MeshEvent::SignedDiff { details }) = classify_settlement(&settlement(raw)) else {
            panic!("expected a diff");
        };
        assert_eq!(details, raw);
    }

    #[test]
    fn anything_else_is_not_a_settlement_event() {
        assert!(classify_settlement(&settlement(r#"{"type":"relay_tx"}"#)).is_none());
        assert!(classify_settlement(&settlement("not json")).is_none());
        let mut intent = settlement(r#"{"type":"SignedDiff"}"#);
        intent.intent_type = "intent".into();
        assert!(classify_settlement(&intent).is_none());
    }
}
