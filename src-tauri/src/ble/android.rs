//! The Android radio, behind the transport seam.
//!
//! An adapter and nothing more, exactly like `corebluetooth.rs`: the radio is
//! Kotlin, the protocol is `cabal-ble`, and this converts between the plugin's
//! event channel and the async trait the runtime expects.

use super::link::{BleTransport, LinkError, LinkEvent};
use async_trait::async_trait;
use cabal_ble::LinkId;
use tauri::ipc::Channel;
use tauri::{AppHandle, Manager};
use cabal_ble::service::{PSM_UUID, SERVICE_UUID};
use tauri_plugin_cabal_ble::{from_base64, CabalBle, RadioEvent};
use tokio::sync::mpsc;

/// The Android radio.
pub struct AndroidBleTransport {
    app: AppHandle,
}

impl AndroidBleTransport {
    /// A transport bound to the running app.
    #[must_use]
    pub fn new(app: AppHandle) -> Self {
        Self { app }
    }

    fn plugin(&self) -> tauri::State<'_, CabalBle<tauri::Wry>> {
        self.app.state::<CabalBle<tauri::Wry>>()
    }
}

#[async_trait]
impl BleTransport for AndroidBleTransport {
    async fn start(&self) -> Result<mpsc::Receiver<LinkEvent>, LinkError> {
        let (tx, rx) = mpsc::channel(256);

        // The channel closure runs on whatever thread the plugin answers on,
        // so it uses the blocking sender rather than awaiting. The queue is
        // deep enough that a burst does not stall the radio.
        let channel = Channel::new(move |response| {
            let Ok(event) = serde_json::from_value::<RadioEvent>(response.deserialize()?) else {
                return Ok(());
            };
            let forwarded = match event {
                RadioEvent::Up { link } => LinkEvent::Up(LinkId(link)),
                RadioEvent::Down { link } => LinkEvent::Down(LinkId(link)),
                RadioEvent::Bytes { link, data } => match from_base64(&data) {
                    Some(bytes) => LinkEvent::Bytes {
                        link: LinkId(link),
                        bytes,
                    },
                    None => {
                        // A frame that does not decode means the boundary is
                        // desynchronised. Dropping the link is safer than
                        // guessing at bytes.
                        tracing::debug!("undecodable frame from the radio; dropping the link");
                        LinkEvent::Down(LinkId(link))
                    }
                },
                RadioEvent::Unavailable { reason } => {
                    tracing::warn!(%reason, "the radio became unusable");
                    return Ok(());
                }
            };
            let _ = tx.blocking_send(forwarded);
            Ok(())
        });

        self.plugin()
            .start(SERVICE_UUID, PSM_UUID, channel)
            .map_err(|error| LinkError::Unavailable(error.to_string()))?;

        Ok(rx)
    }

    async fn send(&self, link: LinkId, bytes: Vec<u8>) -> Result<(), LinkError> {
        self.plugin()
            .send(link.0, &bytes)
            .map_err(|_| LinkError::Closed(link))
    }

    async fn drop_link(&self, link: LinkId) {
        let _ = self.plugin().close(link.0);
    }

    async fn stop(&self) {
        let _ = self.plugin().stop();
    }

    fn describe(&self) -> &'static str {
        "bluetooth"
    }
}
