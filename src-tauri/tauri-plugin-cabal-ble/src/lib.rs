//! The Android BLE radio, behind the same seam as the macOS one.
//!
//! # Why a Tauri plugin rather than code in the app
//!
//! Android's BLE API is Java. Reaching it from Rust means JNI, and Tauri's
//! mobile plugin system already owns that boundary: a Kotlin class annotated
//! with `@Command`, called through `run_mobile_plugin`, with a `Channel` for
//! events coming back. Hand-rolling the JNI would be the same work with none
//! of the lifecycle handling.
//!
//! # What is in Kotlin and what is not
//!
//! Kotlin advertises, scans, runs the GATT rendezvous, opens L2CAP sockets and
//! pumps bytes. That is all. Routing, framing, deduplication and identity are
//! in `cabal-ble`, which has no I/O and is tested on the host — the same
//! division as the macOS radio, for the same reason: a bug in routing must be
//! fixable with a test rather than with two devices.
//!
//! # Verification
//!
//! Two Android emulators. The emulator has a virtual Bluetooth controller
//! (`netsim`/Rootcanal) and two instances can see each other, which is the one
//! thing two processes on a single Mac cannot do. See
//! `docs/mobile-build-verification.md`.

use serde::{Deserialize, Serialize};
use tauri::{
    plugin::{Builder, TauriPlugin},
    Manager, Runtime,
};

#[cfg(target_os = "android")]
const PLUGIN_IDENTIFIER: &str = "com.cabalmesh.ble";

/// Why a radio call failed.
#[derive(Debug, thiserror::Error)]
#[non_exhaustive]
pub enum Error {
    /// The platform has no Android BLE plugin.
    #[error("the Android BLE plugin is not available on this platform")]
    Unsupported,

    /// The plugin answered with a failure.
    #[error("{0}")]
    Plugin(String),
}

/// What the Kotlin side reports.
///
/// One enum rather than several callbacks: the Kotlin side has one `Channel`,
/// and a tagged payload keeps the two sides' idea of "what can happen" in one
/// place instead of two.
#[derive(Debug, Clone, Deserialize)]
#[serde(tag = "kind", rename_all = "camelCase")]
pub enum RadioEvent {
    /// An L2CAP channel opened.
    Up { link: u64 },
    /// A link closed.
    Down { link: u64 },
    /// Bytes arrived, base64 because JSON has no bytes.
    Bytes { link: u64, data: String },
    /// The radio will not run. Carries what Android said.
    Unavailable { reason: String },
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
struct StartArgs {
    service_uuid: String,
    psm_uuid: String,
    events: tauri::ipc::Channel<RadioEvent>,
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
struct SendArgs {
    link: u64,
    data: String,
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
struct LinkArgs {
    link: u64,
}

/// What a command answers with.
///
/// `serde_json::Value` rather than a struct, because Kotlin's
/// `invoke.resolve()` with no argument sends **null**, and a struct refuses
/// that with `invalid type: null, expected struct Empty`. The radio then
/// started, opened links, and was reported to the app as unavailable — every
/// layer working except the one sentence between them.
type Empty = serde_json::Value;

/// Handle onto the Android radio.
pub struct CabalBle<R: Runtime>(
    #[cfg(target_os = "android")] tauri::plugin::PluginHandle<R>,
    // `fn() -> R` rather than `PhantomData<R>`: the latter makes the type
    // inherit `R`'s auto traits, and Tauri's managed state requires
    // `Send + Sync`, which a bare type parameter is not. This variance-only
    // marker owns nothing and is unconditionally `Send + Sync`.
    #[cfg(not(target_os = "android"))] std::marker::PhantomData<fn() -> R>,
);

impl<R: Runtime> CabalBle<R> {
    /// Starts advertising and scanning, streaming events to `channel`.
    ///
    /// # Errors
    ///
    /// [`Error::Unsupported`] off Android, or [`Error::Plugin`] with whatever
    /// Android reported — a refused permission and a powered-off radio are
    /// different problems and the string says which.
    #[allow(unused_variables)]
    pub fn start(
        &self,
        service_uuid: &str,
        psm_uuid: &str,
        channel: tauri::ipc::Channel<RadioEvent>,
    ) -> Result<(), Error> {
        #[cfg(target_os = "android")]
        {
            self.0
                .run_mobile_plugin::<Empty>(
                    "start",
                    StartArgs {
                        service_uuid: service_uuid.to_string(),
                        psm_uuid: psm_uuid.to_string(),
                        events: channel,
                    },
                )
                .map(|_| ())
                .map_err(|error| Error::Plugin(error.to_string()))
        }
        #[cfg(not(target_os = "android"))]
        Err(Error::Unsupported)
    }

    /// Queues bytes for a link.
    ///
    /// # Errors
    ///
    /// As [`CabalBle::start`].
    #[allow(unused_variables)]
    pub fn send(&self, link: u64, data: &[u8]) -> Result<(), Error> {
        #[cfg(target_os = "android")]
        {
            self.0
                .run_mobile_plugin::<Empty>(
                    "send",
                    SendArgs {
                        link,
                        data: base64(data),
                    },
                )
                .map(|_| ())
                .map_err(|error| Error::Plugin(error.to_string()))
        }
        #[cfg(not(target_os = "android"))]
        Err(Error::Unsupported)
    }

    /// Tears a link down.
    ///
    /// # Errors
    ///
    /// As [`CabalBle::start`].
    #[allow(unused_variables)]
    pub fn close(&self, link: u64) -> Result<(), Error> {
        #[cfg(target_os = "android")]
        {
            self.0
                .run_mobile_plugin::<Empty>("close", LinkArgs { link })
                .map(|_| ())
                .map_err(|error| Error::Plugin(error.to_string()))
        }
        #[cfg(not(target_os = "android"))]
        Err(Error::Unsupported)
    }

    /// Stops the radio entirely.
    ///
    /// # Errors
    ///
    /// As [`CabalBle::start`].
    pub fn stop(&self) -> Result<(), Error> {
        #[cfg(target_os = "android")]
        {
            self.0
                .run_mobile_plugin::<Empty>("stop", ())
                .map(|_| ())
                .map_err(|error| Error::Plugin(error.to_string()))
        }
        #[cfg(not(target_os = "android"))]
        Err(Error::Unsupported)
    }
}

/// Base64, without a dependency.
///
/// JSON carries no bytes and the alternative is an array of numbers, which
/// triples the size of every frame crossing the boundary. Sixteen lines is
/// cheaper than a crate for one function used twice.
fn base64(bytes: &[u8]) -> String {
    const ALPHABET: &[u8; 64] =
        b"ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789+/";
    let mut out = String::with_capacity(bytes.len().div_ceil(3) * 4);
    for chunk in bytes.chunks(3) {
        let b = [chunk[0], *chunk.get(1).unwrap_or(&0), *chunk.get(2).unwrap_or(&0)];
        let n = (u32::from(b[0]) << 16) | (u32::from(b[1]) << 8) | u32::from(b[2]);
        out.push(ALPHABET[(n >> 18 & 63) as usize] as char);
        out.push(ALPHABET[(n >> 12 & 63) as usize] as char);
        out.push(if chunk.len() > 1 {
            ALPHABET[(n >> 6 & 63) as usize] as char
        } else {
            '='
        });
        out.push(if chunk.len() > 2 {
            ALPHABET[(n & 63) as usize] as char
        } else {
            '='
        });
    }
    out
}

/// Decodes base64 produced by the Kotlin side.
///
/// Returns `None` for anything malformed rather than guessing: a frame that
/// does not decode is a desynchronised link, and inventing bytes for it would
/// turn that into corruption further up.
#[must_use]
pub fn from_base64(text: &str) -> Option<Vec<u8>> {
    fn value(byte: u8) -> Option<u32> {
        Some(match byte {
            b'A'..=b'Z' => u32::from(byte - b'A'),
            b'a'..=b'z' => u32::from(byte - b'a') + 26,
            b'0'..=b'9' => u32::from(byte - b'0') + 52,
            b'+' => 62,
            b'/' => 63,
            _ => return None,
        })
    }

    let bytes = text.as_bytes();
    if bytes.len() % 4 != 0 {
        return None;
    }
    let mut out = Vec::with_capacity(bytes.len() / 4 * 3);
    for chunk in bytes.chunks(4) {
        let padding = chunk.iter().filter(|&&b| b == b'=').count();
        if padding > 2 {
            return None;
        }
        let mut n = 0u32;
        for &byte in chunk {
            n = (n << 6) | if byte == b'=' { 0 } else { value(byte)? };
        }
        out.push((n >> 16 & 255) as u8);
        if padding < 2 {
            out.push((n >> 8 & 255) as u8);
        }
        if padding < 1 {
            out.push((n & 255) as u8);
        }
    }
    Some(out)
}

/// Registers the plugin.
pub fn init<R: Runtime>() -> TauriPlugin<R> {
    Builder::new("cabal-ble")
        .setup(|app, _api| {
            #[cfg(target_os = "android")]
            {
                let handle = _api.register_android_plugin(PLUGIN_IDENTIFIER, "BlePlugin")?;
                app.manage(CabalBle(handle));
            }
            #[cfg(not(target_os = "android"))]
            {
                app.manage(CabalBle::<R>(std::marker::PhantomData));
            }
            Ok(())
        })
        .build()
}

/// Access to the radio from anywhere with an app handle.
pub trait CabalBleExt<R: Runtime> {
    /// The radio handle.
    fn cabal_ble(&self) -> tauri::State<'_, CabalBle<R>>;
}

impl<R: Runtime, T: Manager<R>> CabalBleExt<R> for T {
    fn cabal_ble(&self) -> tauri::State<'_, CabalBle<R>> {
        self.state::<CabalBle<R>>()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn base64_round_trips() {
        for length in 0..64usize {
            let bytes: Vec<u8> = (0..length).map(|i| (i * 7 % 251) as u8).collect();
            let encoded = base64(&bytes);
            assert_eq!(from_base64(&encoded).as_deref(), Some(bytes.as_slice()));
        }
    }

    #[test]
    fn base64_matches_the_standard_alphabet() {
        // Pinned against known vectors, because a bespoke encoder that only
        // agrees with itself would decode its own output happily and nothing
        // Kotlin produced.
        assert_eq!(base64(b""), "");
        assert_eq!(base64(b"f"), "Zg==");
        assert_eq!(base64(b"fo"), "Zm8=");
        assert_eq!(base64(b"foo"), "Zm9v");
        assert_eq!(base64(b"foob"), "Zm9vYg==");
        assert_eq!(base64(b"fooba"), "Zm9vYmE=");
        assert_eq!(base64(b"foobar"), "Zm9vYmFy");
    }

    #[test]
    fn decoding_matches_the_standard_alphabet() {
        assert_eq!(from_base64("Zm9vYmFy").as_deref(), Some(&b"foobar"[..]));
        assert_eq!(from_base64("Zg==").as_deref(), Some(&b"f"[..]));
    }

    #[test]
    fn malformed_base64_is_refused_rather_than_guessed() {
        // A frame that does not decode is a desynchronised link. Inventing
        // bytes for it turns that into corruption further up.
        assert_eq!(from_base64("Zm9vYmF"), None, "length not a multiple of four");
        assert_eq!(from_base64("Zm9v*mFy"), None, "character outside the alphabet");
        assert_eq!(from_base64("Z==="), None, "too much padding");
    }

    #[test]
    fn a_full_frame_survives_the_boundary() {
        // The size that actually crosses: a packet with a 64-byte signature.
        let frame: Vec<u8> = (0..=255u8).cycle().take(1024).collect();
        assert_eq!(from_base64(&base64(&frame)).as_deref(), Some(frame.as_slice()));
    }
}
