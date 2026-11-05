//! Application state.
//!
//! # What was wrong before
//!
//! State was a single `Arc<Mutex<AppState>>`. Every command locked it, then
//! locked a second mutex inside it, then awaited network I/O while holding
//! both. Two concurrent RPC calls therefore ran strictly one after the other:
//! the app had no concurrency at all, only the appearance of it.
//!
//! It was also managed *inside a spawned task*, so any command invoked before
//! bootstrap finished raced it. That failure is worse than it sounds — a
//! missing `State<'_, T>` is a **runtime panic inside the IPC handler**, not
//! an error a command can convert. The window was small, so it looked stable.
//!
//! # The shape now
//!
//! [`AppState`] is managed synchronously at startup, before the webview can
//! invoke anything. It is cheap to clone and holds no lock of its own; the
//! services inside it arrive later.
//!
//! Commands ask for [`AppState::services`], which is `Ok` once bootstrap has
//! completed and [`AppError::NotReady`] before then. That is the same state
//! the `connecting` screen already renders as progress, so "not ready" became
//! a value instead of a panic.
//!
//! # Why `std::sync::RwLock` and not the async one
//!
//! The critical sections here clone a handful of `Arc`s and return. Nothing is
//! held across an `.await`. Tauri's own guidance is that the standard-library
//! lock is preferred in async code unless a guard genuinely spans a suspension
//! point, and paying for an async lock to guard a pointer copy is exactly the
//! case it warns about.

use crate::agent::SharkAgent;
use crate::error::AppError;
use crate::identity::IdentityStore;
use crate::near_bridge::NearBridge;
use crate::ollama_manager::OllamaManager;
use crate::subscriptions::Registry;
use serde::Serialize;
use std::sync::atomic::AtomicU64;
use std::sync::{Arc, RwLock};
use tokio::sync::Mutex;

/// The subsystems that exist only after bootstrap succeeds.
///
/// Every field is an `Arc` or a channel sender, so cloning this is a few
/// pointer copies. That is what lets a command take a snapshot and release the
/// lock immediately, rather than holding it across the network call it is
/// about to make.
#[derive(Clone)]
pub struct Services {
    /// `None` when the swarm failed to boot. Chain and vault commands
    /// still work without it, so the UI can say the mesh is down rather than
    /// appearing wholly broken.
    pub mesh: Option<crate::mesh_handle::MeshHandle>,
    /// The BLE plane. `None` when no radio backend is compiled or configured.
    ///
    /// Separate from `mesh` and separately optional, because the two planes
    /// fail independently: a phone in a basement has BLE and no internet, and
    /// a desktop with Bluetooth switched off has the reverse. Collapsing them
    /// into one "is the mesh up" would make both messages wrong.
    pub ble: Option<crate::ble::BleHandle>,
    /// Which backend is carrying the BLE plane, verbatim.
    ///
    /// Carried rather than derived so the screen can name it exactly —
    /// `loopback` is not Bluetooth, and a UI that called it that would be
    /// claiming a capability the build does not have.
    pub ble_transport: String,
    pub agent: Arc<SharkAgent>,
    pub intent_chat: Arc<crate::intent_chat::IntentChatParser>,
    pub ollama: Arc<OllamaManager>,
    pub identity: Arc<Mutex<IdentityStore>>,
    /// One client, so every NEAR call shares its connection pool.
    pub http: reqwest::Client,
    pub guardian: Arc<Mutex<crate::guardian::GuardianService>>,
    /// Unlock replies waiting on a human's approve/deny tap. See
    /// `guardian_actor::PendingApprovals`'s docs for why this exists at all.
    pub guardian_approvals: crate::guardian_actor::PendingApprovals,
    pub relay_bytes: Arc<AtomicU64>,
}

impl Services {
    /// A NEAR client for the current identity and network.
    ///
    /// Built per use rather than held: the vault may be locked when bootstrap
    /// runs, and a held client would keep signing as an identity the user has
    /// since replaced.
    ///
    /// # Errors
    ///
    /// [`AppError::VaultLocked`] while no key is available.
    pub async fn near(&self) -> Result<NearBridge, AppError> {
        let keypair = self.identity.lock().await.primary_keypair().ok_or(AppError::VaultLocked)?;
        let config = crate::network_config::NetworkConfig::current();
        let fee = config.near().gateway_fee();
        NearBridge::new(config.rpc_url(), &config.verifier(), keypair, self.http.clone())
            .map(|bridge| bridge.with_gateway_fee(fee))
            .map_err(AppError::internal)
    }
}

/// Facts fixed when the binary was compiled. Never change, so `Copy`.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct PlatformCaps {
    /// Whether a local model server can be spawned. Desktop only.
    pub local_llm: bool,
    /// Whether the mesh can keep running while backgrounded. False on iOS,
    /// which grants no general background networking.
    pub background_mesh: bool,
}

impl PlatformCaps {
    /// Resolved from compile-time configuration.
    #[must_use]
    pub const fn current() -> Self {
        Self {
            local_llm: crate::platform::CAN_SPAWN_PROCESSES,
            background_mesh: cfg!(not(target_os = "ios")),
        }
    }
}

/// State that changes while the app runs.
///
/// Kept separate from [`PlatformCaps`] on purpose. An earlier design had one
/// struct described as build-time immutable while carrying a permission grant
/// — a contradiction, because a user can revoke Local Network access from
/// Settings while the app is backgrounded. Conflating the two means that
/// revocation is never noticed and mDNS silently stops finding peers.
///
/// Re-read on resume.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct RuntimeCaps {
    /// Whether local-network discovery is permitted right now.
    pub mdns_granted: bool,
    /// Whether the bootstrap relay answered on the last attempt.
    pub relay_reachable: bool,
    /// Whether the device believes it has connectivity.
    pub online: bool,
}

impl Default for RuntimeCaps {
    /// Pessimistic: nothing is assumed granted or reachable until observed.
    /// Optimistic defaults would make the first render claim capabilities the
    /// app has not verified.
    fn default() -> Self {
        Self {
            mdns_granted: false,
            relay_reachable: false,
            online: false,
        }
    }
}

struct Inner {
    /// `None` until bootstrap completes.
    services: RwLock<Option<Services>>,
    runtime: RwLock<RuntimeCaps>,
    caps: PlatformCaps,
    /// When the process started, for uptime display.
    started: std::time::Instant,
    /// Live frontend streams. Exists from construction, not from bootstrap:
    /// the connecting screen subscribes to the handshake log *before* services
    /// are published, so the registry has to outlive that gap.
    subscriptions: Registry,
    /// Every intent composed on this device. Also outside `Services`, and for
    /// a sharper reason: an intent queued while offline exists precisely
    /// because the mesh does not, and putting the ledger behind bootstrap
    /// would make the queue unreadable in the state it was built for.
    intents: crate::intents::Ledger,
    /// Every intent seen from other peers. Outside `Services` for the same
    /// reason as `intents`: a sighting bridged in from BLE happens precisely
    /// when there is no mesh, so it cannot live behind bootstrap either.
    received: crate::intents::ReceivedLog,
    /// Signed diffs held for settlement. Outside `Services` for the same reason:
    /// a diff picked up over BLE arrives when there is no mesh.
    diff_book: crate::near_settlement::DiffBook,
}

/// Managed application state.
///
/// Cheap to clone; Tauri stores it directly, with no outer `Arc` — the
/// framework already owns it, and wrapping it again was a redundant layer.
#[derive(Clone)]
pub struct AppState {
    inner: Arc<Inner>,
}

impl AppState {
    /// Creates state with no services yet.
    ///
    /// Call and `manage` this **synchronously**, before the webview can
    /// invoke. Bootstrap fills it in afterwards via [`AppState::set_services`].
    #[must_use]
    pub fn new() -> Self {
        Self {
            inner: Arc::new(Inner {
                services: RwLock::new(None),
                runtime: RwLock::new(RuntimeCaps::default()),
                caps: PlatformCaps::current(),
                started: std::time::Instant::now(),
                subscriptions: Registry::new(),
                intents: crate::intents::Ledger::open(cabal_store::JsonStore::new(
                    crate::app_paths::in_data_dir("intents.json"),
                )),
                received: crate::intents::ReceivedLog::open(cabal_store::JsonStore::new(
                    crate::app_paths::in_data_dir("received_intents.json"),
                )),
                diff_book: crate::near_settlement::DiffBook::open(cabal_store::JsonStore::new(
                    crate::app_paths::in_data_dir("near_diffs.json"),
                )),
            }),
        }
    }

    /// Publishes the bootstrapped services. Called once, from bootstrap.
    pub fn set_services(&self, services: Services) {
        *self
            .inner
            .services
            .write()
            .unwrap_or_else(std::sync::PoisonError::into_inner) = Some(services);
        tracing::info!(target: "cabalmesh::state", "services published");
    }

    /// A snapshot of the services.
    ///
    /// Returns a clone rather than a guard so the caller holds no lock while
    /// it awaits — which is the entire reason commands no longer serialize.
    ///
    /// # Errors
    ///
    /// [`AppError::NotReady`] if bootstrap has not finished.
    pub fn services(&self) -> Result<Services, AppError> {
        self.inner
            .services
            .read()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
            .clone()
            .ok_or(AppError::NotReady { subsystem: "bootstrap" })
    }

    /// Whether bootstrap has completed.
    #[must_use]
    pub fn is_ready(&self) -> bool {
        self.inner
            .services
            .read()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
            .is_some()
    }

    /// Live frontend streams.
    ///
    /// Every stream-producing command registers here and every teardown
    /// cancels through it, so a leak is observable as a non-empty registry
    /// rather than as battery drain nobody attributes.
    #[must_use]
    pub fn subscriptions(&self) -> &Registry {
        &self.inner.subscriptions
    }

    /// Every intent composed on this device.
    ///
    /// Available before bootstrap on purpose: the intents that matter most —
    /// the ones queued while offline — are exactly the ones composed when
    /// there is no mesh to be ready.
    #[must_use]
    pub fn intents(&self) -> &crate::intents::Ledger {
        &self.inner.intents
    }

    /// Every intent seen from other peers, over the mesh or bridged from BLE.
    #[must_use]
    pub fn received(&self) -> &crate::intents::ReceivedLog {
        &self.inner.received
    }

    /// Signed diffs waiting for a route to the chain.
    #[must_use]
    pub fn diff_book(&self) -> &crate::near_settlement::DiffBook {
        &self.inner.diff_book
    }

    /// Seconds since the process started.
    #[must_use]
    pub fn uptime_seconds(&self) -> u64 {
        self.inner.started.elapsed().as_secs()
    }

    /// Compile-time capabilities.
    ///
    /// Not `const fn`: reaching through the `Arc` needs a deref coercion,
    /// which is not permitted in const context. The values themselves are
    /// still fixed at compile time — see [`PlatformCaps::current`].
    #[must_use]
    pub fn platform_caps(&self) -> PlatformCaps {
        self.inner.caps
    }

    /// Current runtime capabilities.
    #[must_use]
    pub fn runtime_caps(&self) -> RuntimeCaps {
        *self
            .inner
            .runtime
            .read()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
    }

    /// Replaces runtime capabilities. Called after bootstrap and on resume,
    /// since a permission can be revoked while the app is backgrounded.
    pub fn set_runtime_caps(&self, caps: RuntimeCaps) {
        let mut guard = self
            .inner
            .runtime
            .write()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        if *guard != caps {
            tracing::info!(
                target: "cabalmesh::state",
                mdns_granted = caps.mdns_granted,
                relay_reachable = caps.relay_reachable,
                online = caps.online,
                "runtime capabilities changed"
            );
            *guard = caps;
        }
    }
}

impl Default for AppState {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn is_not_ready_before_bootstrap() {
        let state = AppState::new();
        assert!(!state.is_ready());
        assert!(matches!(
            state.services(),
            Err(AppError::NotReady { subsystem: "bootstrap" })
        ));
    }

    #[test]
    fn asking_early_is_an_error_not_a_panic() {
        // The whole point: before, a command arriving during bootstrap panicked
        // inside the IPC handler because the managed type was absent. Now it is
        // a value the connecting screen already knows how to render.
        let state = AppState::new();
        assert!(state.services().is_err());
    }

    #[test]
    fn platform_caps_are_immutable_and_copy() {
        let state = AppState::new();
        let first = state.platform_caps();
        let second = state.platform_caps();
        assert_eq!(first, second);
    }

    #[test]
    fn runtime_caps_start_pessimistic() {
        // Optimistic defaults would have the first render claim capabilities
        // that have not been observed.
        let caps = AppState::new().runtime_caps();
        assert!(!caps.mdns_granted);
        assert!(!caps.relay_reachable);
        assert!(!caps.online);
    }

    #[test]
    fn runtime_caps_can_change_after_construction() {
        // The distinction from PlatformCaps: a user can revoke local network
        // access while the app is backgrounded, and the app must be able to
        // notice.
        let state = AppState::new();
        state.set_runtime_caps(RuntimeCaps {
            mdns_granted: true,
            relay_reachable: true,
            online: true,
        });
        assert!(state.runtime_caps().mdns_granted);

        state.set_runtime_caps(RuntimeCaps {
            mdns_granted: false,
            ..state.runtime_caps()
        });
        assert!(!state.runtime_caps().mdns_granted);
        assert!(state.runtime_caps().online);
    }

    #[test]
    fn clones_share_one_underlying_state() {
        let state = AppState::new();
        let clone = state.clone();
        state.set_runtime_caps(RuntimeCaps { online: true, ..RuntimeCaps::default() });
        assert!(clone.runtime_caps().online);
    }
}
