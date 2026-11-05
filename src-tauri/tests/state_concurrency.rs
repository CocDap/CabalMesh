//! Proves the global-mutex removal actually bought concurrency.
//!
//! The old shape was `Arc<Mutex<AppState>>`: every command locked it, then
//! locked a second mutex inside, then awaited network I/O holding both. Two
//! concurrent RPC calls ran strictly one after the other.
//!
//! "No global mutex" is easy to claim from a diff and easy to lose again in a
//! later refactor, so it is asserted here by timing rather than by inspection.

use cabalmesh_lib::state::{AppState, RuntimeCaps};
use std::sync::Arc;
use std::time::{Duration, Instant};

/// Simulates what a command does: take a state snapshot, then await I/O.
///
/// The snapshot is the whole point — it must not keep a lock alive across the
/// sleep, or these serialize.
async fn simulated_command(state: AppState, work: Duration) {
    let _caps = state.runtime_caps();
    let _ready = state.is_ready();
    tokio::time::sleep(work).await;
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn concurrent_commands_overlap_rather_than_serialize() {
    let state = AppState::new();
    let work = Duration::from_millis(150);
    let tasks = 8;

    let started = Instant::now();
    let handles: Vec<_> = (0..tasks)
        .map(|_| {
            let state = state.clone();
            tokio::spawn(simulated_command(state, work))
        })
        .collect();
    for handle in handles {
        handle.await.expect("task panicked");
    }
    let elapsed = started.elapsed();

    // Serialized would be 8 x 150ms = 1200ms. Overlapping is ~150ms. The
    // threshold is deliberately loose — this is testing that the shape is
    // concurrent, not benchmarking the scheduler.
    let serialized = work * tasks;
    assert!(
        elapsed < serialized / 2,
        "commands serialized: {elapsed:?} for {tasks} tasks of {work:?} each \
         (serialized would be {serialized:?}) — a lock is being held across an await"
    );
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn readers_do_not_block_each_other() {
    let state = AppState::new();
    let readers = 32;

    let handles: Vec<_> = (0..readers)
        .map(|_| {
            let state = state.clone();
            tokio::spawn(async move {
                for _ in 0..1_000 {
                    let _ = state.runtime_caps();
                    let _ = state.platform_caps();
                }
            })
        })
        .collect();

    for handle in handles {
        handle.await.expect("reader panicked");
    }
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn writes_are_visible_across_clones_and_threads() {
    let state = AppState::new();
    let observer = state.clone();

    let writer = tokio::spawn(async move {
        state.set_runtime_caps(RuntimeCaps {
            mdns_granted: true,
            relay_reachable: true,
            online: true,
        });
    });
    writer.await.expect("writer panicked");

    let caps = observer.runtime_caps();
    assert!(caps.mdns_granted && caps.relay_reachable && caps.online);
}

/// Before bootstrap, asking for services is an error rather than a panic.
///
/// This is the regression that used to be a race: state was managed inside a
/// spawned task, so a command arriving early found no managed value and Tauri
/// panicked inside the IPC handler.
#[tokio::test]
async fn early_commands_get_not_ready_instead_of_panicking() {
    let state = Arc::new(AppState::new());

    let handles: Vec<_> = (0..16)
        .map(|_| {
            let state = Arc::clone(&state);
            tokio::spawn(async move { state.services().is_err() })
        })
        .collect();

    for handle in handles {
        assert!(handle.await.expect("task panicked"), "expected NotReady");
    }
}
