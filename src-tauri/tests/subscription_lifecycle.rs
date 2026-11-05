//! Proves streams actually stop when cancelled.
//!
//! The unit tests in `subscriptions.rs` check the registry's bookkeeping. This
//! checks the thing that leaks in production: a spawned producer that keeps
//! running after the screen that opened it is gone.
//!
//! Producers here stand in for the real log streams. What matters is the shape
//! — a task selecting on a cancellation token — not what it emits.

use cabalmesh_lib::state::AppState;
use cabalmesh_lib::subscriptions::Registry;
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::Arc;
use std::time::Duration;
use tokio_util::sync::CancellationToken;

/// A producer shaped like the real ones: emit until cancelled, then exit.
///
/// `alive` mirrors "this task is still running", so a leak is observable
/// rather than inferred.
fn spawn_producer(
    token: CancellationToken,
    alive: Arc<AtomicUsize>,
    emitted: Arc<AtomicUsize>,
) -> tokio::task::JoinHandle<()> {
    alive.fetch_add(1, Ordering::SeqCst);
    tokio::spawn(async move {
        loop {
            tokio::select! {
                // Cancellation is checked in the same select as the work, so a
                // cancelled producer stops at its next yield rather than after
                // its current backlog.
                () = token.cancelled() => break,
                () = tokio::time::sleep(Duration::from_millis(1)) => {
                    emitted.fetch_add(1, Ordering::SeqCst);
                }
            }
        }
        alive.fetch_sub(1, Ordering::SeqCst);
    })
}

/// The headline check: a hundred subscribe/cancel cycles leave nothing behind.
///
/// This is the regression that matters. A user tapping between tabs performs
/// exactly this loop, and before the registry existed each pass left a live
/// producer.
#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn a_hundred_cycles_leave_no_live_tasks() {
    let registry = Registry::new();
    let alive = Arc::new(AtomicUsize::new(0));
    let emitted = Arc::new(AtomicUsize::new(0));

    for _ in 0..100 {
        let (id, token) = registry.register("mesh-log").expect("slot available");
        let handle = spawn_producer(token, Arc::clone(&alive), Arc::clone(&emitted));

        tokio::time::sleep(Duration::from_millis(3)).await;
        registry.cancel(&id);
        handle.await.expect("producer exited cleanly");
    }

    assert_eq!(registry.len(), 0, "registry leaked entries");
    assert_eq!(alive.load(Ordering::SeqCst), 0, "producers still running");
    assert!(
        emitted.load(Ordering::SeqCst) > 0,
        "producers never ran, so the test proved nothing"
    );
}

/// Cancellation stops emission, not just bookkeeping.
///
/// Removing the entry while the task keeps producing would pass a
/// registry-length assertion and still drain the battery.
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn cancelled_producers_stop_emitting() {
    let registry = Registry::new();
    let alive = Arc::new(AtomicUsize::new(0));
    let emitted = Arc::new(AtomicUsize::new(0));

    let (id, token) = registry.register("mesh-log").unwrap();
    let handle = spawn_producer(token, Arc::clone(&alive), Arc::clone(&emitted));

    tokio::time::sleep(Duration::from_millis(20)).await;
    registry.cancel(&id);
    handle.await.unwrap();

    let after_cancel = emitted.load(Ordering::SeqCst);
    tokio::time::sleep(Duration::from_millis(30)).await;

    assert_eq!(
        emitted.load(Ordering::SeqCst),
        after_cancel,
        "producer kept emitting after cancellation"
    );
}

/// Suspension stops everything at once.
///
/// A backgrounded app must not keep producing into a webview that cannot
/// receive.
#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn suspending_cancels_every_stream() {
    let registry = Registry::new();
    let alive = Arc::new(AtomicUsize::new(0));
    let emitted = Arc::new(AtomicUsize::new(0));

    let handles: Vec<_> = (0..8)
        .map(|_| {
            let (_, token) = registry.register("mesh-log").unwrap();
            spawn_producer(token, Arc::clone(&alive), Arc::clone(&emitted))
        })
        .collect();

    tokio::time::sleep(Duration::from_millis(5)).await;
    assert_eq!(alive.load(Ordering::SeqCst), 8);

    registry.cancel_all();
    for handle in handles {
        handle.await.unwrap();
    }

    assert_eq!(alive.load(Ordering::SeqCst), 0);
    assert!(registry.is_empty());
}

/// The unmount-before-subscribe-resolves race.
///
/// Fast tab switching hits this every time: teardown runs before the invoke
/// that registers the stream has returned. Cancelling an unknown handle must
/// be harmless, and the late registration must still be cancellable.
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn tearing_down_before_registration_is_safe() {
    let registry = Registry::new();

    // Teardown arrives first, for a handle that does not exist yet.
    registry.cancel(&cabal_core::SubscriptionId::new("mesh-log-999"));
    assert!(registry.is_empty());

    // The subscribe then lands, and the frontend cancels again with the real
    // handle it finally received.
    let (id, token) = registry.register("mesh-log").unwrap();
    registry.cancel(&id);

    assert!(token.is_cancelled());
    assert!(registry.is_empty());
}

/// The limit is a tripwire for a screen that never tears down.
#[tokio::test]
async fn runaway_subscriptions_are_refused_rather_than_unbounded() {
    let registry = Registry::with_limit(4);
    for _ in 0..4 {
        registry.register("mesh-log").expect("under the limit");
    }
    assert!(
        registry.register("mesh-log").is_err(),
        "an unbounded registry would let a leaking screen exhaust the device"
    );
    assert_eq!(registry.len(), 4);
}

/// The registry lives on state from construction, not from bootstrap.
///
/// The connecting screen subscribes to the handshake log *before* services
/// exist, so a registry that arrived with bootstrap would be too late.
#[tokio::test]
async fn the_registry_is_usable_before_bootstrap() {
    let state = AppState::new();
    assert!(state.services().is_err(), "precondition: not bootstrapped");

    let (id, token) = state
        .subscriptions()
        .register("handshake")
        .expect("registry must work before services exist");

    state.subscriptions().cancel(&id);
    assert!(token.is_cancelled());
}
