//! The boundary between the protocol and a radio.
//!
//! # Why a trait
//!
//! Everything above this line is testable on a laptop. Everything below it
//! needs hardware that CI does not have and that an iOS simulator does not
//! emulate. Putting the seam here — one small trait, four methods — is what
//! keeps the untestable part small enough to verify by reading.
//!
//! Two implementations:
//!
//! - [`LoopbackTransport`] — links over TCP. Two real app processes on one
//!   desktop exchange real packets through the real engine, proving the
//!   runtime wiring without a radio in the room.
//! - `CoreBluetoothTransport` (macOS) — the real thing.
//!
//! # What the trait deliberately does not do
//!
//! No routing, no framing, no retries, no peer identity. A backend's whole job
//! is: tell me when a link appears or disappears, give me the bytes that
//! arrived on it, and write these bytes to it. A bug in any other behaviour
//! must be fixable in Rust with a test rather than on two devices.

use async_trait::async_trait;
use cabal_ble::LinkId;
use tokio::sync::mpsc;

/// Something that happened on the radio.
#[derive(Debug)]
pub enum LinkEvent {
    /// A link to a neighbour came up.
    Up(LinkId),
    /// A link went away.
    Down(LinkId),
    /// Bytes arrived. Any number, split anywhere.
    Bytes { link: LinkId, bytes: Vec<u8> },
}

/// Why a transport could not do something.
#[derive(Debug, thiserror::Error)]
#[non_exhaustive]
pub enum LinkError {
    /// The link is gone. Ordinary: peers walk out of range mid-write.
    #[error("link {0:?} is closed")]
    Closed(LinkId),

    /// The radio is unavailable — powered off, or permission refused.
    ///
    /// Distinguished from every other failure because it is the one the user
    /// can do something about, and the one that must never stop the app.
    #[error("bluetooth unavailable: {0}")]
    Unavailable(String),

    /// Anything else the platform reported.
    #[error("transport error: {0}")]
    Other(String),
}

/// A radio, or something standing in for one.
#[async_trait]
pub trait BleTransport: Send + Sync + 'static {
    /// Starts advertising and scanning, and returns the event stream.
    ///
    /// # Errors
    ///
    /// [`LinkError::Unavailable`] when the radio is off or permission was
    /// refused. Callers must treat that as "no BLE plane", never as "no app".
    async fn start(&self) -> Result<mpsc::Receiver<LinkEvent>, LinkError>;

    /// Writes bytes to a link.
    ///
    /// # Errors
    ///
    /// [`LinkError::Closed`] if the peer went away, which is expected rather
    /// than exceptional.
    async fn send(&self, link: LinkId, bytes: Vec<u8>) -> Result<(), LinkError>;

    /// Tears a link down.
    async fn drop_link(&self, link: LinkId);

    /// Stops the radio entirely. The offline switch must reach the antenna,
    /// not just the protocol.
    async fn stop(&self);

    /// A human-readable name for logs and the status display.
    fn describe(&self) -> &'static str;
}

/// Links over TCP, for tests and desktop development.
///
/// # What this proves and what it does not
///
/// It proves the runtime: that engine actions reach a link, that bytes come
/// back as events, that two processes running the real protocol converge. That
/// is most of what goes wrong.
///
/// It proves nothing about advertising, scanning, L2CAP channel setup, MTU
/// behaviour, or what happens when a phone's radio sleeps. Those need two
/// devices, and `docs/mobile-build-verification.md` records them as verified
/// that way or not at all.
pub struct LoopbackTransport {
    listen: std::net::SocketAddr,
    dial: Vec<std::net::SocketAddr>,
    links: std::sync::Arc<tokio::sync::Mutex<std::collections::HashMap<LinkId, LinkWriter>>>,
    /// Shared by the accept loop and every dial, because they both allocate.
    ///
    /// Two counters is the obvious implementation and it is wrong: a node that
    /// dials one peer and accepts another hands both the same identifier, the
    /// second overwrites the first in the link map, and one neighbour silently
    /// stops receiving anything. It presents as "the third node never joined".
    next_link: std::sync::Arc<std::sync::atomic::AtomicU64>,
    shutdown: tokio_util::sync::CancellationToken,
}

type LinkWriter = mpsc::Sender<Vec<u8>>;

impl LoopbackTransport {
    /// Listens on one address and dials the others.
    ///
    /// Both directions, because a BLE node is simultaneously a peripheral and
    /// a central and a transport that only did one would hide exactly the bugs
    /// that dual role causes.
    #[must_use]
    pub fn new(listen: std::net::SocketAddr, dial: Vec<std::net::SocketAddr>) -> Self {
        Self {
            listen,
            dial,
            links: std::sync::Arc::new(tokio::sync::Mutex::new(std::collections::HashMap::new())),
            next_link: std::sync::Arc::new(std::sync::atomic::AtomicU64::new(1)),
            shutdown: tokio_util::sync::CancellationToken::new(),
        }
    }

    fn allocate(&self) -> LinkId {
        LinkId(
            self.next_link
                .fetch_add(1, std::sync::atomic::Ordering::Relaxed),
        )
    }

    /// Runs one accepted or dialled socket as a link.
    async fn serve(
        stream: tokio::net::TcpStream,
        link: LinkId,
        events: mpsc::Sender<LinkEvent>,
        links: std::sync::Arc<tokio::sync::Mutex<std::collections::HashMap<LinkId, LinkWriter>>>,
        shutdown: tokio_util::sync::CancellationToken,
    ) {
        use tokio::io::{AsyncReadExt, AsyncWriteExt};

        let _ = stream.set_nodelay(true);
        let (mut reader, mut writer) = stream.into_split();
        let (outbound_tx, mut outbound_rx) = mpsc::channel::<Vec<u8>>(64);

        links.lock().await.insert(link, outbound_tx);
        if events.send(LinkEvent::Up(link)).await.is_err() {
            return;
        }

        let writing = tokio::spawn(async move {
            while let Some(bytes) = outbound_rx.recv().await {
                if writer.write_all(&bytes).await.is_err() {
                    break;
                }
            }
        });

        let mut buffer = vec![0u8; 4096];
        loop {
            tokio::select! {
                () = shutdown.cancelled() => break,
                read = reader.read(&mut buffer) => match read {
                    Ok(0) | Err(_) => break,
                    Ok(count) => {
                        if events
                            .send(LinkEvent::Bytes { link, bytes: buffer[..count].to_vec() })
                            .await
                            .is_err()
                        {
                            break;
                        }
                    }
                },
            }
        }

        links.lock().await.remove(&link);
        writing.abort();
        let _ = events.send(LinkEvent::Down(link)).await;
    }
}

#[async_trait]
impl BleTransport for LoopbackTransport {
    async fn start(&self) -> Result<mpsc::Receiver<LinkEvent>, LinkError> {
        let (events_tx, events_rx) = mpsc::channel(256);

        let listener = tokio::net::TcpListener::bind(self.listen)
            .await
            .map_err(|error| LinkError::Unavailable(error.to_string()))?;

        {
            let events = events_tx.clone();
            let links = self.links.clone();
            let shutdown = self.shutdown.clone();
            let next = self.next_link.clone();
            tokio::spawn(async move {
                loop {
                    tokio::select! {
                        () = shutdown.cancelled() => break,
                        accepted = listener.accept() => {
                            let Ok((stream, _)) = accepted else { break };
                            let link = LinkId(next.fetch_add(1, std::sync::atomic::Ordering::Relaxed));
                            tokio::spawn(Self::serve(
                                stream,
                                link,
                                events.clone(),
                                links.clone(),
                                shutdown.clone(),
                            ));
                        }
                    }
                }
            });
        }

        for address in self.dial.clone() {
            let link = self.allocate();
            let events = events_tx.clone();
            let links = self.links.clone();
            let shutdown = self.shutdown.clone();
            tokio::spawn(async move {
                // Retried rather than failed: in a two-process test neither
                // side is guaranteed to be listening first, and a peer that
                // is not there yet is the normal case on a radio too.
                for _ in 0..50 {
                    if shutdown.is_cancelled() {
                        return;
                    }
                    if let Ok(stream) = tokio::net::TcpStream::connect(address).await {
                        Self::serve(stream, link, events, links, shutdown).await;
                        return;
                    }
                    tokio::time::sleep(std::time::Duration::from_millis(100)).await;
                }
                tracing::warn!(%address, "loopback peer never came up");
            });
        }

        Ok(events_rx)
    }

    async fn send(&self, link: LinkId, bytes: Vec<u8>) -> Result<(), LinkError> {
        let writer = {
            let links = self.links.lock().await;
            links.get(&link).cloned()
        };
        let writer = writer.ok_or(LinkError::Closed(link))?;
        writer.send(bytes).await.map_err(|_| LinkError::Closed(link))
    }

    async fn drop_link(&self, link: LinkId) {
        self.links.lock().await.remove(&link);
    }

    async fn stop(&self) {
        self.shutdown.cancel();
        self.links.lock().await.clear();
    }

    fn describe(&self) -> &'static str {
        "loopback"
    }
}
