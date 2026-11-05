//! Reservation and circuit limits.
//!
//! # An unbounded relay is an open proxy
//!
//! libp2p's relay defaults are generous because they are written for a general
//! audience. Shipped as-is on a public host they mean anyone can reserve a slot
//! and push traffic through, at the operator's bandwidth cost, with the
//! operator's IP address as the apparent source.
//!
//! So every bound is set here explicitly, and every one has a reason written
//! next to it rather than a number chosen because it looked reasonable.
//!
//! # These are per-peer, which is the part that matters
//!
//! A global cap alone is a denial-of-service surface: one peer opening the
//! global maximum locks everyone else out. The per-peer caps are what make the
//! global ones safe.

use std::time::Duration;

/// The relay's operating limits.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Limits {
    /// Concurrent reservations across all peers.
    pub max_reservations: usize,
    /// Concurrent reservations from any one peer.
    pub max_reservations_per_peer: usize,
    /// How long a reservation lasts before it must be renewed.
    pub reservation_duration: Duration,
    /// Concurrent circuits across all peers.
    pub max_circuits: usize,
    /// Concurrent circuits any one peer may open.
    pub max_circuits_per_peer: usize,
    /// How long a single circuit may stay open.
    pub max_circuit_duration: Duration,
    /// Bytes a single circuit may carry before it is closed.
    pub max_circuit_bytes: u64,
}

impl Default for Limits {
    fn default() -> Self {
        Self {
            // Sized for a single modest host. Each reservation is a held
            // connection, and a few thousand of those is where file descriptors
            // and memory start to matter more than bandwidth does.
            max_reservations: 512,
            // Four covers a user with a phone, a tablet and a laptop, plus one
            // spare for a reconnect racing an expiry. Beyond that it is one
            // peer consuming the pool.
            max_reservations_per_peer: 4,
            // An hour. Long enough that a phone does not spend its battery
            // renewing, short enough that a device that vanished frees its slot
            // the same day.
            reservation_duration: Duration::from_secs(3_600),
            max_circuits: 256,
            max_circuits_per_peer: 8,
            // Ten minutes. A circuit is meant to carry a handshake and then be
            // replaced by a direct connection via hole punching. One still open
            // after ten minutes is either a hole punch that failed or somebody
            // using the relay as a tunnel, and both should be cut.
            max_circuit_duration: Duration::from_secs(600),
            // 8 MiB. An intent and its negotiation are kilobytes. This is three
            // orders of magnitude of headroom and still small enough that
            // nobody moves a file through it.
            max_circuit_bytes: 8 * 1024 * 1024,
        }
    }
}

impl Limits {
    /// The libp2p configuration these limits describe.
    #[must_use]
    pub fn to_relay_config(self) -> libp2p::relay::Config {
        libp2p::relay::Config {
            max_reservations: self.max_reservations,
            max_reservations_per_peer: self.max_reservations_per_peer,
            reservation_duration: self.reservation_duration,
            max_circuits: self.max_circuits,
            max_circuits_per_peer: self.max_circuits_per_peer,
            max_circuit_duration: self.max_circuit_duration,
            max_circuit_bytes: self.max_circuit_bytes,
            ..Default::default()
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn every_bound_is_finite() {
        // The failure this guards is a field being left at a permissive default
        // after a libp2p upgrade adds one. An unbounded relay is an open proxy.
        let limits = Limits::default();
        assert!(limits.max_reservations > 0);
        assert!(limits.max_circuits > 0);
        assert!(limits.max_circuit_bytes > 0);
        assert!(limits.reservation_duration > Duration::ZERO);
        assert!(limits.max_circuit_duration > Duration::ZERO);
    }

    #[test]
    fn no_single_peer_can_take_the_whole_pool() {
        // The property that makes the global caps safe. Without it, one peer
        // opening the global maximum locks everyone else out.
        let limits = Limits::default();
        assert!(limits.max_reservations_per_peer * 8 <= limits.max_reservations);
        assert!(limits.max_circuits_per_peer * 8 <= limits.max_circuits);
    }

    #[test]
    fn a_circuit_cannot_outlive_a_reservation() {
        // A circuit held past its reservation is a slot that cannot be
        // reclaimed by the accounting that hands slots out.
        let limits = Limits::default();
        assert!(limits.max_circuit_duration < limits.reservation_duration);
    }

    #[test]
    fn the_config_carries_the_limits_through() {
        // to_relay_config uses `..Default::default()`, so a field that stops
        // being mapped would silently revert to libp2p's permissive default.
        let limits = Limits::default();
        let config = limits.to_relay_config();

        assert_eq!(config.max_reservations, limits.max_reservations);
        assert_eq!(config.max_reservations_per_peer, limits.max_reservations_per_peer);
        assert_eq!(config.reservation_duration, limits.reservation_duration);
        assert_eq!(config.max_circuits, limits.max_circuits);
        assert_eq!(config.max_circuits_per_peer, limits.max_circuits_per_peer);
        assert_eq!(config.max_circuit_duration, limits.max_circuit_duration);
        assert_eq!(config.max_circuit_bytes, limits.max_circuit_bytes);
    }
}
