//! Lifecycle for live streams pushed to the frontend.
//!
//! # Why a registry is needed at all
//!
//! Three screens consume ordered, high-rate streams — the handshake log, the
//! mesh ticker, the proof log — so they use Tauri `Channel`s rather than the
//! event bus, which is what the docs recommend for streaming.
//!
//! **Channels do not clean themselves up.** On the frontend a channel releases
//! its callback only when the producing side sends an end message; there is no
//! unsubscribe in the JS API, and releasing a JS callback would not stop a Rust
//! task regardless. Without explicit teardown, every visit to a streaming
//! screen leaves a live producer behind, and a user tapping between tabs
//! accumulates them.
//!
//! # Cancel stops delivery, not the operation
//!
//! This distinction has money attached and is enforced structurally rather than
//! by discipline: a registered task is *only* a delivery loop. The domain
//! operation it reports on runs elsewhere and holds no token from here.
//!
//! | Stream | cancel stops | cancel does **not** stop |
//! |---|---|---|
//! | mesh log | delivery | nothing — delivery is all it does |
//! | handshake | delivery | the mesh join itself |
//! | settlement proof | delivery | **the settlement.** An in-flight on-chain settlement is never aborted by a UI navigation |
//!
//! Aborting a domain operation is a separate, explicit command. Never overload
//! cancellation with it.

use crate::error::AppError;
use cabal_core::SubscriptionId;
use std::collections::HashMap;
use std::sync::{Arc, Mutex, PoisonError};
use tokio_util::sync::CancellationToken;

/// Maximum concurrent streams.
///
/// The UI needs at most one per visible screen. A number far above that still
/// catches a subscribe-without-teardown bug long before it exhausts a phone,
/// which is the point — the limit is a tripwire, not a capacity plan.
const DEFAULT_LIMIT: usize = 32;

/// Tracks live streams so they can be stopped.
///
/// Cheap to clone; every clone shares one registry.
#[derive(Clone)]
pub struct Registry {
    inner: Arc<Mutex<HashMap<SubscriptionId, CancellationToken>>>,
    limit: usize,
    next: Arc<std::sync::atomic::AtomicU64>,
}

impl Registry {
    /// A registry with the default limit.
    #[must_use]
    pub fn new() -> Self {
        Self::with_limit(DEFAULT_LIMIT)
    }

    /// A registry with an explicit limit. Tests use small limits to reach the
    /// bound without spawning thirty-two tasks.
    #[must_use]
    pub fn with_limit(limit: usize) -> Self {
        Self {
            inner: Arc::new(Mutex::new(HashMap::new())),
            limit,
            next: Arc::new(std::sync::atomic::AtomicU64::new(0)),
        }
    }

    fn map(&self) -> std::sync::MutexGuard<'_, HashMap<SubscriptionId, CancellationToken>> {
        self.inner.lock().unwrap_or_else(PoisonError::into_inner)
    }

    /// Registers a stream and returns its handle plus the token its producer
    /// must observe.
    ///
    /// The producer is expected to select on the token and exit when it fires,
    /// dropping its `Channel` — which is what tells the frontend the stream has
    /// ended.
    ///
    /// # Errors
    ///
    /// [`AppError::TooManySubscriptions`] once the limit is reached. Returning
    /// an error rather than evicting is deliberate: silently dropping someone
    /// else's stream would surface as a screen that mysteriously stops
    /// updating.
    pub fn register(&self, kind: &str) -> Result<(SubscriptionId, CancellationToken), AppError> {
        let mut map = self.map();
        if map.len() >= self.limit {
            tracing::warn!(
                target: "cabalmesh::subscriptions",
                live = map.len(),
                limit = self.limit,
                kind,
                "subscription limit reached — a screen is probably not tearing down"
            );
            return Err(AppError::TooManySubscriptions {
                limit: u16::try_from(self.limit).unwrap_or(u16::MAX),
            });
        }

        let serial = self
            .next
            .fetch_add(1, std::sync::atomic::Ordering::Relaxed);
        let id = SubscriptionId::new(format!("{kind}-{serial}"));
        let token = CancellationToken::new();
        map.insert(id.clone(), token.clone());

        tracing::debug!(
            target: "cabalmesh::subscriptions",
            id = %id,
            live = map.len(),
            "stream registered"
        );
        Ok((id, token))
    }

    /// Stops delivery for one stream.
    ///
    /// Idempotent: cancelling an unknown or already-cancelled handle is `Ok`.
    /// The frontend races unmount against subscribe, so a teardown for a
    /// handle that never landed is normal rather than exceptional.
    pub fn cancel(&self, id: &SubscriptionId) {
        if let Some(token) = self.map().remove(id) {
            token.cancel();
            tracing::debug!(target: "cabalmesh::subscriptions", id = %id, "stream cancelled");
        }
    }

    /// Called by a producer as it exits on its own, so a finished stream does
    /// not occupy a slot until someone cancels it.
    pub fn finished(&self, id: &SubscriptionId) {
        if self.map().remove(id).is_some() {
            tracing::debug!(target: "cabalmesh::subscriptions", id = %id, "stream finished");
        }
    }

    /// Stops every live stream.
    ///
    /// Used when the app is suspended: a backgrounded app should not keep
    /// producing lines into a webview that cannot receive them.
    pub fn cancel_all(&self) {
        let mut map = self.map();
        let count = map.len();
        for (_, token) in map.drain() {
            token.cancel();
        }
        if count > 0 {
            tracing::info!(target: "cabalmesh::subscriptions", count, "all streams cancelled");
        }
    }

    /// How many streams are live. Primarily for leak assertions.
    #[must_use]
    pub fn len(&self) -> usize {
        self.map().len()
    }

    /// Whether no stream is live.
    #[must_use]
    pub fn is_empty(&self) -> bool {
        self.len() == 0
    }
}

impl Default for Registry {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn registering_yields_distinct_handles() {
        let registry = Registry::new();
        let (first, _) = registry.register("mesh-log").unwrap();
        let (second, _) = registry.register("mesh-log").unwrap();
        assert_ne!(first, second);
        assert_eq!(registry.len(), 2);
    }

    #[test]
    fn cancelling_fires_the_token_and_frees_the_slot() {
        let registry = Registry::new();
        let (id, token) = registry.register("proof").unwrap();
        assert!(!token.is_cancelled());

        registry.cancel(&id);
        assert!(token.is_cancelled());
        assert!(registry.is_empty());
    }

    #[test]
    fn cancelling_is_idempotent() {
        // The frontend races unmount against subscribe, so tearing down a
        // handle that never landed is normal.
        let registry = Registry::new();
        let (id, _) = registry.register("mesh-log").unwrap();

        registry.cancel(&id);
        registry.cancel(&id);
        registry.cancel(&SubscriptionId::new("never-existed"));
        assert!(registry.is_empty());
    }

    #[test]
    fn the_limit_is_enforced() {
        let registry = Registry::with_limit(2);
        let (first, _) = registry.register("a").unwrap();
        let _second = registry.register("b").unwrap();

        assert!(matches!(
            registry.register("c"),
            Err(AppError::TooManySubscriptions { limit: 2 })
        ));

        // Freeing a slot lets the next one through, so the limit is a bound on
        // concurrency rather than a lifetime quota.
        registry.cancel(&first);
        assert!(registry.register("c").is_ok());
    }

    #[test]
    fn a_finished_stream_frees_its_slot() {
        let registry = Registry::with_limit(1);
        let (id, _) = registry.register("handshake").unwrap();
        assert!(registry.register("handshake").is_err());

        registry.finished(&id);
        assert!(registry.register("handshake").is_ok());
    }

    #[test]
    fn cancel_all_stops_everything() {
        let registry = Registry::new();
        let tokens: Vec<_> = (0..5)
            .map(|_| registry.register("mesh-log").unwrap().1)
            .collect();

        registry.cancel_all();

        assert!(registry.is_empty());
        assert!(tokens.iter().all(CancellationToken::is_cancelled));
    }

    #[test]
    fn clones_share_one_registry() {
        let registry = Registry::new();
        let clone = registry.clone();
        let (id, _) = registry.register("mesh-log").unwrap();

        assert_eq!(clone.len(), 1);
        clone.cancel(&id);
        assert!(registry.is_empty());
    }
}
