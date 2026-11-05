//! The BLE plane: the engine, a radio, and a clock.
//!
//! # What lives here and what does not
//!
//! `cabal_ble` is the protocol and has no I/O at all. This module is the other
//! half: it owns a tokio task, holds the engine, turns radio callbacks into
//! engine events, and executes the actions the engine returns.
//!
//! The split is deliberate and it is the reason the protocol has ninety tests
//! that run in a quarter of a second. Everything genuinely hard — flood
//! termination, deduplication, partition healing, the offline gate — is
//! decided in a crate with no sockets in it. What is left here is plumbing,
//! and plumbing is verifiable by reading.
//!
//! # Why an actor
//!
//! Same reason `mesh_handle` is one: a single event loop with one owner, and
//! everything else talking to it by message. The channel is bounded so a UI
//! that spams a request applies backpressure instead of growing a queue on a
//! phone with two gigabytes of memory.

pub mod backend;
pub mod link;

/// The CoreBluetooth radio. Apple platforms only.
#[cfg(target_vendor = "apple")]
pub mod corebluetooth;

/// The Android radio, through the Kotlin plugin.
#[cfg(target_os = "android")]
pub mod android;

use cabal_ble::engine::{Action, Engine, Event, MeshStatus};
use cabal_ble::peers::{Capabilities, KnownPeer};
use cabal_ble::wire::PacketKind;
use cabal_ble::{Ephemeral, LinkId, PeerId};
use link::{BleTransport, LinkEvent};
use std::sync::Arc;
use std::time::Duration;
use tokio::sync::{broadcast, mpsc, oneshot};

/// Queue depth for requests to the BLE actor.
const COMMAND_QUEUE: usize = 32;

/// Queue depth for timers the engine asked for.
///
/// Generous relative to the command queue: a busy mesh schedules a relay per
/// packet, and dropping one silently loses a forward.
const TIMER_QUEUE: usize = 512;

/// Why a BLE request failed.
#[derive(Debug, thiserror::Error)]
#[non_exhaustive]
pub enum BleError {
    /// The actor is not running — usually because the radio was unavailable
    /// at startup, which is a state the app is expected to survive.
    #[error("the BLE plane is not running")]
    NotRunning,

    /// The actor accepted the request and then stopped without answering.
    #[error("the BLE actor dropped the request without answering")]
    NoReply,
}

/// A request to the BLE actor.
#[derive(Debug)]
enum Command {
    Status(oneshot::Sender<MeshStatus>),
    Peers(oneshot::Sender<Vec<KnownPeer>>),
    Submit {
        kind: PacketKind,
        payload: Vec<u8>,
        /// `None` floods it; `Some` sends it to one peer. See
        /// `BleHandle::send_to` — the engine has supported this since
        /// `Event::Submit` was written, but nothing above it could reach the
        /// capability until guardian shares needed directed delivery.
        recipient: Option<PeerId>,
        reply: oneshot::Sender<()>,
    },
    SetOffline {
        offline: bool,
        reply: oneshot::Sender<()>,
    },
    SetGateway {
        gateway: bool,
        reply: oneshot::Sender<()>,
    },
}

/// Cheap, clonable access to the BLE actor.
#[derive(Clone, Debug)]
pub struct BleHandle {
    tx: mpsc::Sender<Command>,
    events: broadcast::Sender<BleEvent>,
}

impl BleHandle {
    /// Current mesh status.
    ///
    /// # Errors
    ///
    /// [`BleError::NotRunning`] if the actor stopped, [`BleError::NoReply`] if
    /// it stopped mid-request.
    pub async fn status(&self) -> Result<MeshStatus, BleError> {
        let (reply, answer) = oneshot::channel();
        self.tx
            .send(Command::Status(reply))
            .await
            .map_err(|_| BleError::NotRunning)?;
        answer.await.map_err(|_| BleError::NoReply)
    }

    /// Every reachable peer.
    ///
    /// # Errors
    ///
    /// As [`BleHandle::status`].
    pub async fn peers(&self) -> Result<Vec<KnownPeer>, BleError> {
        let (reply, answer) = oneshot::channel();
        self.tx
            .send(Command::Peers(reply))
            .await
            .map_err(|_| BleError::NotRunning)?;
        answer.await.map_err(|_| BleError::NoReply)
    }

    /// Floods a payload to the mesh.
    ///
    /// # Errors
    ///
    /// As [`BleHandle::status`].
    pub async fn broadcast(&self, kind: PacketKind, payload: Vec<u8>) -> Result<(), BleError> {
        let (reply, answer) = oneshot::channel();
        self.tx
            .send(Command::Submit {
                kind,
                payload,
                recipient: None,
                reply,
            })
            .await
            .map_err(|_| BleError::NotRunning)?;
        answer.await.map_err(|_| BleError::NoReply)
    }

    /// Sends a payload to exactly one peer, routed hop by hop rather than
    /// flooded.
    ///
    /// # Errors
    ///
    /// As [`BleHandle::status`].
    pub async fn send_to(&self, peer: PeerId, kind: PacketKind, payload: Vec<u8>) -> Result<(), BleError> {
        let (reply, answer) = oneshot::channel();
        self.tx
            .send(Command::Submit {
                kind,
                payload,
                recipient: Some(peer),
                reply,
            })
            .await
            .map_err(|_| BleError::NotRunning)?;
        answer.await.map_err(|_| BleError::NoReply)
    }

    /// Stops or resumes the radio.
    ///
    /// # Errors
    ///
    /// As [`BleHandle::status`].
    pub async fn set_offline(&self, offline: bool) -> Result<(), BleError> {
        let (reply, answer) = oneshot::channel();
        self.tx
            .send(Command::SetOffline { offline, reply })
            .await
            .map_err(|_| BleError::NotRunning)?;
        answer.await.map_err(|_| BleError::NoReply)
    }

    /// Declares whether this node can reach the internet.
    ///
    /// # Errors
    ///
    /// As [`BleHandle::status`].
    pub async fn set_gateway(&self, gateway: bool) -> Result<(), BleError> {
        let (reply, answer) = oneshot::channel();
        self.tx
            .send(Command::SetGateway { gateway, reply })
            .await
            .map_err(|_| BleError::NotRunning)?;
        answer.await.map_err(|_| BleError::NoReply)
    }

    /// Whether the actor is still accepting requests.
    #[must_use]
    pub fn is_running(&self) -> bool {
        !self.tx.is_closed()
    }

    /// A fresh stream of events, independent of any other subscriber's.
    ///
    /// More than one part of the app needs to watch BLE traffic — the
    /// frontend forwarder, and anything (like the guardian actor) that reacts
    /// to a specific packet kind — and an `mpsc` channel can only ever be
    /// drained by one of them. `broadcast` gives each subscriber its own
    /// queue instead.
    #[must_use]
    pub fn subscribe(&self) -> broadcast::Receiver<BleEvent> {
        self.events.subscribe()
    }
}

/// Queue depth per event subscriber, before a slow one starts missing events
/// rather than blocking the actor. Generous relative to how bursty BLE
/// traffic actually gets in a session.
const EVENT_QUEUE: usize = 256;

/// What the BLE plane hands up to the rest of the app.
#[derive(Debug, Clone)]
pub enum BleEvent {
    PeerAppeared(String),
    PeerGone(String),
    /// A payload delivered to this node.
    Received {
        from: PeerId,
        kind: PacketKind,
        payload: Vec<u8>,
    },
    /// The radio could not be started. The app continues on the IP plane.
    Unavailable(String),
}

/// Starts the BLE plane.
///
/// Returns a handle and the stream of things worth telling the UI about.
/// Call [`BleHandle::subscribe`] for additional independent streams.
///
/// A radio that will not start is **not** an error here. Bluetooth being off,
/// or the user declining the permission, must leave the app running with the
/// IP plane intact — the same call the mDNS behaviour makes in `mesh.rs`, for
/// the same reason: a node that failed to start because somebody tapped
/// "Don't Allow" is worse than one that quietly does less.
pub fn spawn(
    transport: Arc<dyn BleTransport>,
    identity: Ephemeral,
    capabilities: Capabilities,
) -> (BleHandle, broadcast::Receiver<BleEvent>) {
    let (command_tx, command_rx) = mpsc::channel(COMMAND_QUEUE);
    let (event_tx, event_rx) = broadcast::channel(EVENT_QUEUE);

    tokio::spawn(run(transport, identity, capabilities, command_rx, event_tx.clone()));

    (BleHandle { tx: command_tx, events: event_tx }, event_rx)
}

/// The actor loop.
async fn run(
    transport: Arc<dyn BleTransport>,
    identity: Ephemeral,
    capabilities: Capabilities,
    mut commands: mpsc::Receiver<Command>,
    events: broadcast::Sender<BleEvent>,
) {
    let mut engine = Engine::new(identity, capabilities);

    let mut radio = match transport.start().await {
        Ok(radio) => radio,
        Err(error) => {
            tracing::warn!(%error, "BLE plane unavailable; continuing on the IP plane");
            let _ = events.send(BleEvent::Unavailable(error.to_string()));
            // The actor stays alive so status calls answer "no links" rather
            // than "not running": the UI has to distinguish "no peers" from
            // "no radio", and a dead actor cannot say which.
            drain_without_radio(&mut engine, &mut commands).await;
            return;
        }
    };

    tracing::info!(transport = transport.describe(), peer_id = %engine.id(), "BLE plane up");

    let (timer_tx, mut timer_rx) = mpsc::channel::<Event>(TIMER_QUEUE);
    apply(
        &transport,
        &events,
        &timer_tx,
        engine.start(),
    )
    .await;

    loop {
        let event = tokio::select! {
            Some(command) = commands.recv() => {
                match command {
                    Command::Status(reply) => {
                        let _ = reply.send(engine.status(now_ms()));
                        continue;
                    }
                    Command::Peers(reply) => {
                        let _ = reply.send(engine.peers(now_ms()));
                        continue;
                    }
                    Command::Submit { kind, payload, recipient, reply } => {
                        let _ = reply.send(());
                        Event::Submit { kind, payload, recipient }
                    }
                    Command::SetOffline { offline, reply } => {
                        if offline {
                            // The switch has to reach the antenna, not just
                            // the protocol. Stopping only the engine would
                            // leave the radio advertising, which is precisely
                            // the promise the switch makes about not doing.
                            transport.stop().await;
                        }
                        let _ = reply.send(());
                        Event::SetOffline(offline)
                    }
                    Command::SetGateway { gateway, reply } => {
                        engine.set_gateway(gateway);
                        let _ = reply.send(());
                        continue;
                    }
                }
            }
            Some(link_event) = radio.recv() => match link_event {
                LinkEvent::Up(link) => Event::LinkUp(link),
                LinkEvent::Down(link) => Event::LinkDown(link),
                LinkEvent::Bytes { link, bytes } => Event::Bytes { link, bytes },
            },
            Some(timer) = timer_rx.recv() => timer,
            else => break,
        };

        let actions = engine.handle(event, now_ms());
        apply(&transport, &events, &timer_tx, actions).await;
    }

    transport.stop().await;
    tracing::info!("BLE plane down");
}

/// Answers status requests when there is no radio.
///
/// Without this the handle reports `NotRunning`, which the UI cannot tell from
/// a crash — and "Bluetooth is off" is a thing the user can act on.
async fn drain_without_radio(engine: &mut Engine, commands: &mut mpsc::Receiver<Command>) {
    while let Some(command) = commands.recv().await {
        match command {
            Command::Status(reply) => {
                let _ = reply.send(engine.status(now_ms()));
            }
            Command::Peers(reply) => {
                let _ = reply.send(Vec::new());
            }
            Command::Submit { reply, .. } => {
                let _ = reply.send(());
            }
            Command::SetOffline { reply, .. } | Command::SetGateway { reply, .. } => {
                let _ = reply.send(());
            }
        }
    }
}

/// Executes what the engine asked for.
async fn apply(
    transport: &Arc<dyn BleTransport>,
    events: &broadcast::Sender<BleEvent>,
    timers: &mpsc::Sender<Event>,
    actions: Vec<Action>,
) {
    for action in actions {
        match action {
            Action::Send { link, bytes } => {
                if let Err(error) = transport.send(link, bytes).await {
                    // A peer walking out of range mid-write is ordinary. The
                    // engine finds out through the link-down event, not here.
                    tracing::debug!(?link, %error, "send failed");
                }
            }
            Action::ScheduleRelay { key, delay } => {
                schedule(timers, delay, Event::RelayDue(key));
            }
            Action::ScheduleAnnounce { delay } => {
                schedule(timers, delay, Event::AnnounceDue);
            }
            Action::ScheduleExpiry { delay } => {
                schedule(timers, delay, Event::ExpiryDue);
            }
            Action::Deliver {
                from,
                kind,
                payload,
            } => {
                let _ = events.send(BleEvent::Received { from, kind, payload });
            }
            Action::PeerAppeared(peer) => {
                let _ = events.send(BleEvent::PeerAppeared(peer.to_string()));
            }
            Action::PeerGone(peer) => {
                let _ = events.send(BleEvent::PeerGone(peer.to_string()));
            }
            Action::DropLink(link) => {
                transport.drop_link(link).await;
            }
        }
    }
}

/// Fires an event back into the loop after a delay.
fn schedule(timers: &mpsc::Sender<Event>, delay: Duration, event: Event) {
    let timers = timers.clone();
    tokio::spawn(async move {
        if !delay.is_zero() {
            tokio::time::sleep(delay).await;
        }
        // `send` rather than `try_send`: a full timer queue means the loop is
        // behind, and waiting is better than dropping a forward.
        let _ = timers.send(event).await;
    });
}

/// Milliseconds since the Unix epoch.
///
/// The engine takes time as a parameter precisely so that this function exists
/// in exactly one place, and so that tests never call it.
fn now_ms() -> u64 {
    use std::time::{SystemTime, UNIX_EPOCH};
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| u64::try_from(d.as_millis()).unwrap_or(u64::MAX))
        .unwrap_or(0)
}

/// A fresh identity for this session.
///
/// Generated from the OS entropy source and never written anywhere. See
/// `cabal_ble::identity` for why there is no load-from-disk counterpart.
#[must_use]
pub fn fresh_identity() -> Ephemeral {
    Ephemeral::from_bytes(rand::random(), rand::random())
}

/// Where a link id came from, for the nodes screen.
#[must_use]
pub fn link_label(link: LinkId) -> String {
    format!("link-{}", link.0)
}
