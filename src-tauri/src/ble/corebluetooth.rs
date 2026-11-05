//! The CoreBluetooth radio, behind the transport seam.
//!
//! # What this file is
//!
//! An adapter, and deliberately nothing more. `cabal-ble-macos` owns the
//! Objective-C; `cabal_ble` owns the protocol; this converts between the
//! radio's blocking event channel and the async trait the runtime expects.
//!
//! # Why a thread rather than an async stream
//!
//! The radio's events arrive on a `std::sync::mpsc::Receiver`, which is what
//! comes out of a dispatch queue without dragging a tokio dependency into a
//! crate full of unsafe. One blocking thread forwards them; it is cheaper than
//! the alternative and it keeps the unsafe crate free of an async runtime.

use super::link::{BleTransport, LinkError, LinkEvent};
use async_trait::async_trait;
use cabal_ble::LinkId;
use cabal_ble_macos::{Config, Event, Radio, Shared};
use std::sync::Arc;
use tokio::sync::mpsc;

/// The real radio.
pub struct CoreBluetoothTransport {
    config: Config,
    /// Set once [`BleTransport::start`] has run, so [`BleTransport::send`] has
    /// something to queue into.
    state: std::sync::Mutex<Option<Started>>,
}

struct Started {
    shared: Arc<Shared>,
    radio: Radio,
}

impl CoreBluetoothTransport {
    /// A transport with the shipped service identifiers.
    #[must_use]
    pub fn new() -> Self {
        Self {
            config: Config::default(),
            state: std::sync::Mutex::new(None),
        }
    }
}

impl Default for CoreBluetoothTransport {
    fn default() -> Self {
        Self::new()
    }
}

#[async_trait]
impl BleTransport for CoreBluetoothTransport {
    async fn start(&self) -> Result<mpsc::Receiver<LinkEvent>, LinkError> {
        let (shared, events) = Shared::new();
        let radio = Radio::start(&self.config, shared.clone())
            .map_err(|error| LinkError::Unavailable(error.to_string()))?;

        let (tx, rx) = mpsc::channel(256);

        // Blocking, on its own thread: the radio's channel is a std one, and
        // bridging it with `spawn_blocking` would occupy a tokio worker for
        // the life of the process.
        std::thread::Builder::new()
            .name("cabal-ble-events".into())
            .spawn(move || {
                while let Ok(event) = events.recv() {
                    let forwarded = match event {
                        Event::Up(link) => LinkEvent::Up(LinkId(link)),
                        Event::Down(link) => LinkEvent::Down(LinkId(link)),
                        Event::Bytes { link, bytes } => LinkEvent::Bytes {
                            link: LinkId(link),
                            bytes,
                        },
                        Event::Unavailable(why) => {
                            // Not fatal to the app: the IP plane is untouched,
                            // and the runtime turns this into a status the
                            // nodes screen can render rather than an error.
                            tracing::warn!(%why, "the radio became unusable");
                            continue;
                        }
                    };
                    if tx.blocking_send(forwarded).is_err() {
                        break;
                    }
                }
            })
            .map_err(|error| LinkError::Other(error.to_string()))?;

        *self.lock() = Some(Started { shared, radio });
        Ok(rx)
    }

    async fn send(&self, link: LinkId, bytes: Vec<u8>) -> Result<(), LinkError> {
        let queued = {
            let state = self.lock();
            let Some(started) = state.as_ref() else {
                return Err(LinkError::Closed(link));
            };
            started.shared.queue(link.0, bytes)
        };

        if queued {
            Ok(())
        } else {
            // The peer walked out of range between the engine deciding to send
            // and the write reaching the radio. Ordinary, not exceptional.
            Err(LinkError::Closed(link))
        }
    }

    async fn drop_link(&self, link: LinkId) {
        if let Some(started) = self.lock().as_ref() {
            started.shared.close(link.0);
        }
    }

    async fn stop(&self) {
        if let Some(started) = self.lock().as_ref() {
            // Both, and in this order: the flag stops anything further being
            // queued, and `stop` is what makes the pump tear the radio down on
            // the queue that owns it.
            started.shared.stop();
            started.radio.stop();
        }
    }

    fn describe(&self) -> &'static str {
        "bluetooth"
    }
}

impl CoreBluetoothTransport {
    /// A poisoned lock here guards an `Option<Started>` and nothing more; a
    /// panic cannot leave it in a state worth refusing to use.
    fn lock(&self) -> std::sync::MutexGuard<'_, Option<Started>> {
        self.state
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner())
    }
}
