//! The CoreBluetooth radio.
//!
//! # What this crate is, and what it is not allowed to become
//!
//! It advertises, it scans, it opens L2CAP channels, and it moves bytes. It
//! contains no routing, no framing, no deduplication and no identity. All of
//! that is in `cabal-ble`, which has no I/O and ninety-odd tests that run in a
//! quarter of a second.
//!
//! That division is the whole design. Bluetooth cannot be driven from CI and
//! cannot be driven from a simulator, so the part that can only be verified by
//! two people standing in a room has to be small enough to verify by reading.
//! Every behaviour that could live on the other side of the line does.
//!
//! # Threading
//!
//! Everything Objective-C happens on **one serial dispatch queue**. CoreBluetooth
//! delivers its delegate callbacks there, the L2CAP streams are pumped there,
//! and no `Retained` pointer ever crosses a thread boundary.
//!
//! The Rust side touches only [`Shared`] — a mutex over plain bytes. It queues
//! outbound data and reads events; it never sees an Objective-C object. That is
//! what makes this crate's `Send`/`Sync` story trivial instead of subtle.
//!
//! # The rendezvous, in two stages
//!
//! A BLE advertisement has room for a service UUID and almost nothing else, so
//! the PSM an L2CAP channel needs cannot be advertised directly.
//!
//! 1. **GATT, once.** The peripheral publishes an L2CAP channel, learns its
//!    PSM, exposes that PSM as a read-only characteristic, and advertises the
//!    service. A central scans for the service, connects, reads two bytes, and
//!    is done with GATT forever.
//! 2. **L2CAP, for everything else.** The central opens a channel to that PSM.
//!    It is a reliable, ordered, flow-controlled byte stream, which is why
//!    `cabal-ble` needs no fragmentation layer at all.
//!
//! # Verification status
//!
//! **This code has never talked to another machine.** It compiles; nothing
//! here has been observed working. Two Macs are required and one is not
//! enough — a device does not discover its own advertisements — and a process
//! without an app bundle is refused Bluetooth by TCC, so it cannot be put
//! under `cargo test` either. `docs/mobile-build-verification.md` records it as
//! unverified, and it must not be described otherwise until somebody has run
//! it on two machines.

#![cfg_attr(not(target_vendor = "apple"), allow(dead_code))]

mod shared;

pub use shared::{Event, LinkId, Shared};

/// Why the radio could not start.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum RadioError {
    /// The platform has no CoreBluetooth.
    Unsupported,
    /// Bluetooth is off, unauthorised, or otherwise unusable. The string is
    /// what the OS said, because "unavailable" alone tells a user nothing they
    /// can act on.
    Unavailable(String),
}

impl std::fmt::Display for RadioError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Unsupported => f.write_str("CoreBluetooth is not available on this platform"),
            Self::Unavailable(why) => write!(f, "bluetooth unavailable: {why}"),
        }
    }
}

impl std::error::Error for RadioError {}

/// The identifiers every node agrees on, whatever radio it is using.
///
/// Re-exported rather than redefined: a macOS radio and an Android radio that
/// disagree here are two meshes that cannot see each other, and the symptom is
/// silence rather than an error.
pub use cabal_ble::service::{is_valid_uuid, PSM_UUID, SERVICE_UUID};

/// How the radio should identify itself.
#[derive(Debug, Clone)]
pub struct Config {
    /// The service every CabalMesh node advertises and scans for.
    pub service_uuid: String,
    /// The characteristic carrying this node's L2CAP PSM.
    pub psm_uuid: String,
    /// How often the queue drains outbound bytes and drains the streams.
    ///
    /// Polling rather than stream callbacks: it is a handful of lines instead
    /// of a C callback registered against a toll-free-bridged CFStream, and at
    /// these intervals it costs nothing measurable against a radio that tops
    /// out around 300 kbit/s.
    pub pump_interval: std::time::Duration,
}

impl Default for Config {
    fn default() -> Self {
        Self {
            // Constants, not random: every node must agree, and a build that
            // differs is a node nobody can see. Testnet and mainnet differ in
            // the last nibble so a development device cannot join a real mesh
            // by accident.
            service_uuid: SERVICE_UUID.into(),
            psm_uuid: PSM_UUID.into(),
            pump_interval: std::time::Duration::from_millis(10),
        }
    }
}

#[cfg(target_vendor = "apple")]
mod radio;

#[cfg(target_vendor = "apple")]
pub use radio::Radio;

#[cfg(not(target_vendor = "apple"))]
mod radio {
    use super::{Config, RadioError, Shared};
    use std::sync::Arc;

    /// A radio that does not exist.
    ///
    /// Present so the app compiles unchanged on Linux and Windows, and refuses
    /// at runtime rather than at link time.
    pub struct Radio;

    impl Radio {
        /// Always fails off Apple platforms.
        ///
        /// # Errors
        ///
        /// Always [`RadioError::Unsupported`].
        pub fn start(_config: &Config, _shared: Arc<Shared>) -> Result<Self, RadioError> {
            Err(RadioError::Unsupported)
        }

        /// Nothing to stop.
        pub fn stop(&self) {}
    }
}

#[cfg(not(target_vendor = "apple"))]
pub use radio::Radio;

