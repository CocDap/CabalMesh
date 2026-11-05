//! The Android Keystore half of the vault key, behind the same seam as the
//! Apple keychain.
//!
//! # Why a Tauri plugin rather than code in the app
//!
//! Android's Keystore API is Java. Reaching it from Rust means JNI, and Tauri's
//! mobile plugin system already owns that boundary — the same reasoning that
//! put the BLE radio in `tauri-plugin-cabal-ble`, and the same division of
//! labour: Kotlin talks to the platform, and every decision about what the
//! secret *means* stays in `device_binding.rs` where it can be tested on the
//! host.
//!
//! # What crosses the boundary
//!
//! Thirty-two bytes, base64, once per process. Not the vault key and not the
//! wallet: a pepper mixed into the passphrase derivation, useless on its own to
//! anyone who does not also have the key file and the passphrase.
//!
//! # Verification
//!
//! An emulator. `adb logcat` shows the plugin resolving and the vault
//! reporting a device-bound envelope; a second install on the same emulator
//! reuses the secret rather than generating a new one, which is the property
//! that would otherwise silently orphan a vault.

#[cfg(target_os = "android")]
use serde::{Deserialize, Serialize};
use tauri::{
    plugin::{Builder, TauriPlugin},
    Manager, Runtime,
};

#[cfg(target_os = "android")]
const PLUGIN_IDENTIFIER: &str = "com.cabalmesh.keystore";

/// Why a Keystore call failed.
#[derive(Debug, thiserror::Error)]
#[non_exhaustive]
pub enum Error {
    /// The platform has no Android Keystore plugin.
    #[error("the Android Keystore plugin is not available on this platform")]
    Unsupported,

    /// The plugin answered with a failure.
    #[error("{0}")]
    Plugin(String),

    /// The plugin answered, but not with 32 bytes.
    ///
    /// Its own variant because it means something different from a refusal: a
    /// short or unparseable secret would derive a key that opens nothing, and
    /// treating that as "no binding" would silently drop the layer.
    #[error("the Android Keystore returned a malformed secret")]
    Malformed,
}

// Only the Android path parses a response or sends arguments; elsewhere these
// would be dead weight the compiler is right to point at.
#[cfg(target_os = "android")]
#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
struct DeviceSecretResponse {
    secret: String,
    /// Whether the wrapping key landed in StrongBox rather than the TEE.
    /// Carried for logging: both are non-exportable, and the difference is
    /// worth recording without being worth branching on.
    #[serde(default)]
    strong_box: bool,
}

#[cfg(target_os = "android")]
#[derive(Serialize)]
struct NoArgs {}

/// Handle onto the Android Keystore.
pub struct CabalKeystore<R: Runtime>(
    #[cfg(target_os = "android")] tauri::plugin::PluginHandle<R>,
    // See `tauri-plugin-cabal-ble`: `fn() -> R` rather than `PhantomData<R>`,
    // so the type is unconditionally `Send + Sync` as managed state requires.
    #[cfg(not(target_os = "android"))] std::marker::PhantomData<fn() -> R>,
);

impl<R: Runtime> CabalKeystore<R> {
    /// The device secret for this install, created on first use.
    ///
    /// # Errors
    ///
    /// [`Error::Unsupported`] off Android, [`Error::Plugin`] with whatever
    /// Android reported, or [`Error::Malformed`] if the answer was not 32
    /// bytes.
    pub fn device_secret(&self) -> Result<Vec<u8>, Error> {
        #[cfg(target_os = "android")]
        {
            let response = self
                .0
                .run_mobile_plugin::<DeviceSecretResponse>("deviceSecret", NoArgs {})
                .map_err(|error| Error::Plugin(error.to_string()))?;

            let secret = from_base64(&response.secret).ok_or(Error::Malformed)?;
            if secret.len() != 32 {
                return Err(Error::Malformed);
            }
            tracing::info!(
                target: "cabalmesh::vault",
                strong_box = response.strong_box,
                "obtained the device secret from the Android Keystore"
            );
            Ok(secret)
        }
        #[cfg(not(target_os = "android"))]
        Err(Error::Unsupported)
    }
}

/// Decodes base64 produced by the Kotlin side.
///
/// Returns `None` for anything malformed rather than guessing. A secret that
/// half-decodes would derive a key that opens nothing, and reporting that as a
/// wrong passphrase is the failure this whole layer is careful to avoid.
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
    if !bytes.len().is_multiple_of(4) {
        return None;
    }
    let mut out = Vec::with_capacity(bytes.len() / 4 * 3);
    for chunk in bytes.chunks(4) {
        let padding = chunk.iter().filter(|&&b| b == b'=').count();
        if padding > 2 {
            return None;
        }
        let mut n = 0_u32;
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
    Builder::new("cabal-keystore")
        .setup(|app, _api| {
            #[cfg(target_os = "android")]
            {
                let handle = _api.register_android_plugin(PLUGIN_IDENTIFIER, "KeystorePlugin")?;
                app.manage(CabalKeystore(handle));
            }
            #[cfg(not(target_os = "android"))]
            {
                app.manage(CabalKeystore::<R>(std::marker::PhantomData));
            }
            Ok(())
        })
        .build()
}

/// Access to the Keystore from anywhere with an app handle.
pub trait CabalKeystoreExt<R: Runtime> {
    /// The Keystore handle.
    fn cabal_keystore(&self) -> tauri::State<'_, CabalKeystore<R>>;
}

impl<R: Runtime, T: Manager<R>> CabalKeystoreExt<R> for T {
    fn cabal_keystore(&self) -> tauri::State<'_, CabalKeystore<R>> {
        self.state::<CabalKeystore<R>>()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn base64_matches_the_standard_alphabet() {
        // Pinned against known vectors: a bespoke decoder that only agrees
        // with itself would happily mis-decode everything Kotlin produced,
        // and the result would be a device secret that is wrong by 25%.
        assert_eq!(from_base64("Zm9vYmFy").as_deref(), Some(&b"foobar"[..]));
        assert_eq!(from_base64("Zg==").as_deref(), Some(&b"f"[..]));
        assert_eq!(from_base64("Zm8=").as_deref(), Some(&b"fo"[..]));
        assert_eq!(from_base64("").as_deref(), Some(&b""[..]));
    }

    #[test]
    fn a_thirty_two_byte_secret_survives_the_boundary() {
        // The only length that ever crosses.
        let secret: Vec<u8> = (0..32_u8).map(|i| i.wrapping_mul(7)).collect();
        let encoded = {
            const ALPHABET: &[u8; 64] =
                b"ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789+/";
            let mut out = String::new();
            for chunk in secret.chunks(3) {
                let b = [chunk[0], *chunk.get(1).unwrap_or(&0), *chunk.get(2).unwrap_or(&0)];
                let n = (u32::from(b[0]) << 16) | (u32::from(b[1]) << 8) | u32::from(b[2]);
                out.push(ALPHABET[(n >> 18 & 63) as usize] as char);
                out.push(ALPHABET[(n >> 12 & 63) as usize] as char);
                out.push(if chunk.len() > 1 { ALPHABET[(n >> 6 & 63) as usize] as char } else { '=' });
                out.push(if chunk.len() > 2 { ALPHABET[(n & 63) as usize] as char } else { '=' });
            }
            out
        };

        assert_eq!(from_base64(&encoded).as_deref(), Some(secret.as_slice()));
    }

    #[test]
    fn malformed_base64_is_refused_rather_than_guessed() {
        assert_eq!(from_base64("Zm9vYmF"), None, "length not a multiple of four");
        assert_eq!(from_base64("Zm9v*mFy"), None, "character outside the alphabet");
        assert_eq!(from_base64("Z==="), None, "too much padding");
    }
}
