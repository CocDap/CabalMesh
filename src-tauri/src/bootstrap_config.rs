//! Which peers to dial when there is no one on the local network.
//!
//! # Why this exists
//!
//! mDNS finds peers in the same room. That is the whole of discovery today, so
//! two users on different networks never meet — which for a mesh product is not
//! a limitation, it is the absence of the product.
//!
//! Bootstrap peers are the answer: dial a known relay, learn about others
//! through it, and upgrade to direct connections where hole punching works.
//!
//! # The relay exists; no host runs it yet
//!
//! [`BootstrapConfig::default_relays`] is deliberately **empty**. Shipping a
//! placeholder address would produce dial failures that look like bugs, and
//! inventing one would be worse.
//!
//! The relay itself is written and tested — `crates/cabal-relay`, with a real
//! client reserving through it in `tests/reservation.rs`. What is missing is a
//! host to run it on. `docs/relay-operations.md` §6 is the one-line change
//! here once there is one, and it also says to delete
//! `no_placeholder_relay_ships` at the same time: that test exists to stop a
//! made-up address shipping before a real relay exists, and once one does its
//! job is over.
//!
//! Until then the mesh is LAN-only and says so, rather than appearing broken.

use serde::{Deserialize, Serialize};

/// Peers dialled at startup and re-dialled on resume.
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct BootstrapConfig {
    /// Multiaddrs to dial. A user override **replaces** this list rather than
    /// appending: someone pointing the app at their own relay usually means
    /// "only that one".
    #[serde(default)]
    pub relays: Vec<String>,
}

impl BootstrapConfig {
    /// The compiled-in defaults.
    ///
    /// Empty until a relay host exists. See the module docs for why a
    /// placeholder would be worse than nothing, and `docs/relay-operations.md`
    /// for exactly what to put here.
    #[must_use]
    pub fn default_relays() -> Vec<String> {
        Vec::new()
    }

    /// Loads configuration, falling back to defaults.
    ///
    /// A missing file, a missing field, or an unreadable file all yield the
    /// defaults. Config is replaceable, so it uses the forgiving load — unlike
    /// the vault, which must fail loudly.
    #[must_use]
    pub fn load(store: &cabal_store::JsonStore) -> Self {
        let mut config: Self = store.load_or(Self::default());
        if config.relays.is_empty() {
            config.relays = Self::default_relays();
        }
        config
    }

    /// Parsed multiaddrs, skipping any that do not parse.
    ///
    /// A malformed entry is logged and dropped rather than aborting startup: a
    /// typo in one address should not prevent the others from being dialled.
    #[must_use]
    pub fn parsed(&self) -> Vec<libp2p::Multiaddr> {
        self.relays
            .iter()
            .filter_map(|address| match address.parse() {
                Ok(parsed) => Some(parsed),
                Err(error) => {
                    tracing::warn!(%address, %error, "ignoring unparseable bootstrap address");
                    None
                }
            })
            .collect()
    }

    /// Whether any off-LAN discovery is possible at all.
    ///
    /// Drives the honest empty state on the nodes screen: "no peers nearby" and
    /// "no way to find peers beyond this network" are different messages.
    #[must_use]
    pub fn has_relays(&self) -> bool {
        !self.relays.is_empty()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::TempDir;

    #[test]
    fn absent_config_yields_defaults() {
        let dir = TempDir::new().unwrap();
        let store = cabal_store::JsonStore::new(dir.path().join("bootstrap.json"));
        assert_eq!(BootstrapConfig::load(&store).relays, BootstrapConfig::default_relays());
    }

    #[test]
    fn a_missing_field_still_deserializes() {
        // The relay address will move at some point, and an older config file
        // must not become unloadable when it does.
        let dir = TempDir::new().unwrap();
        let path = dir.path().join("bootstrap.json");
        std::fs::write(&path, "{}").unwrap();

        let store = cabal_store::JsonStore::new(&path);
        assert_eq!(BootstrapConfig::load(&store).relays, BootstrapConfig::default_relays());
    }

    #[test]
    fn a_user_override_replaces_rather_than_appends() {
        let dir = TempDir::new().unwrap();
        let store = cabal_store::JsonStore::new(dir.path().join("bootstrap.json"));
        store
            .save(&BootstrapConfig {
                relays: vec!["/ip4/10.0.0.1/udp/4001/quic-v1".into()],
            })
            .unwrap();

        let loaded = BootstrapConfig::load(&store);
        assert_eq!(loaded.relays.len(), 1);
        assert!(loaded.has_relays());
    }

    #[test]
    fn unparseable_addresses_are_dropped_not_fatal() {
        // One typo must not stop the other relays being dialled.
        let config = BootstrapConfig {
            relays: vec![
                "/ip4/10.0.0.1/udp/4001/quic-v1".into(),
                "not-a-multiaddr".into(),
            ],
        };
        assert_eq!(config.parsed().len(), 1);
    }

    #[test]
    fn no_placeholder_relay_ships() {
        // A fake address produces dial failures that read as bugs.
        //
        // **Delete this test when a real relay is deployed** — see
        // docs/relay-operations.md §6. It guards the gap before one exists, and
        // leaving it in place afterwards would block the real address.
        assert!(
            BootstrapConfig::default_relays().is_empty(),
            "a placeholder relay address must never ship"
        );
    }
}
