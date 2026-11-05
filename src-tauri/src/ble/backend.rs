//! Choosing a radio.
//!
//! # Why this is a function and not a constant
//!
//! The BLE plane has to run in three situations that look nothing alike:
//!
//! - **On a phone**, over the actual radio.
//! - **On two developer machines**, before the radio backend for that platform
//!   exists, so the rest of the system can be built and demonstrated against
//!   the real protocol rather than against a mock.
//! - **In tests**, where there is no radio at all.
//!
//! One function, three answers, and the protocol above it cannot tell which it
//! got. That is the point of the [`super::link::BleTransport`] seam.

use super::link::{BleTransport, LoopbackTransport};
use std::net::SocketAddr;
use std::sync::Arc;

/// Environment variable selecting the development transport.
///
/// Format: `listen=HOST:PORT[,dial=HOST:PORT]...`
///
/// Two machines on one network, each pointed at the other, run the genuine
/// mesh protocol — announcements, flooding, deduplication, relay, the offline
/// switch — over TCP instead of over an antenna. What it does *not* exercise
/// is advertising, scanning and L2CAP setup, and calling it "BLE" in the UI
/// would be a lie, so [`BleTransport::describe`] reports `loopback` and the
/// nodes screen shows that verbatim.
pub const LOOPBACK_ENV: &str = "CABALMESH_BLE_LOOPBACK";

/// The transport this build and this environment should use.
///
/// `None` means the BLE plane does not run: no radio backend for this
/// platform, and no development transport configured. The app continues on the
/// IP plane, which is the same call `mesh.rs` makes when mDNS is refused.
#[must_use]
pub fn choose() -> Option<Arc<dyn BleTransport>> {
    if let Ok(spec) = std::env::var(LOOPBACK_ENV) {
        match parse_loopback(&spec) {
            Ok((listen, dial)) => {
                tracing::info!(%listen, peers = dial.len(), "BLE plane using the loopback transport");
                return Some(Arc::new(LoopbackTransport::new(listen, dial)));
            }
            Err(error) => {
                // Refused rather than ignored: a typo here otherwise presents
                // as "the other machine never showed up", which is the most
                // expensive possible way to find out about a typo.
                tracing::error!(%error, "{LOOPBACK_ENV} is set but unusable; the BLE plane will not start");
                return None;
            }
        }
    }

    // The real radio. Apple only here, because Android's needs a running app
    // to reach its plugin — see `choose_for_app`.
    #[cfg(target_vendor = "apple")]
    {
        tracing::info!("BLE plane using CoreBluetooth");
        return Some(Arc::new(super::corebluetooth::CoreBluetoothTransport::new()));
    }

    #[cfg(not(target_vendor = "apple"))]
    None
}

/// The transport, for a caller that has a running app.
///
/// Separate from [`choose`] because Android's radio lives behind a Tauri
/// plugin and a plugin needs an app handle to reach. Everything else is the
/// same, so this defers to `choose` on every other platform rather than
/// duplicating the loopback handling.
#[must_use]
#[allow(unused_variables)]
pub fn choose_for_app(app: &tauri::AppHandle) -> Option<Arc<dyn BleTransport>> {
    #[cfg(target_os = "android")]
    {
        if std::env::var(LOOPBACK_ENV).is_err() {
            tracing::info!("BLE plane using the Android radio");
            return Some(Arc::new(super::android::AndroidBleTransport::new(app.clone())));
        }
    }

    choose()
}

/// Parses `listen=HOST:PORT,dial=HOST:PORT,dial=HOST:PORT`.
fn parse_loopback(spec: &str) -> Result<(SocketAddr, Vec<SocketAddr>), String> {
    let mut listen = None;
    let mut dial = Vec::new();

    for field in spec.split(',').map(str::trim).filter(|f| !f.is_empty()) {
        let (key, value) = field
            .split_once('=')
            .ok_or_else(|| format!("`{field}` is not key=value"))?;
        let address: SocketAddr = value
            .parse()
            .map_err(|_| format!("`{value}` is not a host:port address"))?;
        match key {
            "listen" => listen = Some(address),
            "dial" => dial.push(address),
            other => return Err(format!("unknown field `{other}`")),
        }
    }

    listen
        .map(|listen| (listen, dial))
        .ok_or_else(|| "no `listen=` address".to_string())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_listen_address_alone_is_enough() {
        // The first machine has nobody to dial yet.
        let (listen, dial) = parse_loopback("listen=127.0.0.1:9701").unwrap();
        assert_eq!(listen.port(), 9701);
        assert!(dial.is_empty());
    }

    #[test]
    fn several_peers_can_be_dialled() {
        let (_, dial) =
            parse_loopback("listen=0.0.0.0:9701,dial=192.168.1.10:9701,dial=192.168.1.11:9701")
                .unwrap();
        assert_eq!(dial.len(), 2);
    }

    #[test]
    fn whitespace_is_tolerated() {
        assert!(parse_loopback(" listen=127.0.0.1:9701 , dial=127.0.0.1:9702 ").is_ok());
    }

    #[test]
    fn a_spec_without_a_listen_address_is_refused() {
        assert!(parse_loopback("dial=127.0.0.1:9702").is_err());
    }

    #[test]
    fn a_typo_is_refused_rather_than_ignored() {
        // Ignoring it presents as "the other machine never showed up", which
        // is the most expensive possible way to find out about a typo.
        assert!(parse_loopback("lisen=127.0.0.1:9701").is_err());
        assert!(parse_loopback("listen=127.0.0.1").is_err());
        assert!(parse_loopback("listen").is_err());
    }
}
