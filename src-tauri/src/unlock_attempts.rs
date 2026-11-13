//! Failed-unlock counting with exponential backoff, persisted beside the
//! vault so a restart or reboot does not reset it.
//!
//! **Not a security control** — `docs/identity-design.md`, "Retries". An
//! attacker holding a copy of the key file guesses on their own machine,
//! where nothing counts. This exists to make an over-the-shoulder or
//! borrowed-device attempt tedious, and nothing more. A wrong attempt never
//! destroys anything.

use cabal_store::DocumentStore;
use serde::{Deserialize, Serialize};
use std::path::PathBuf;

/// Wrong attempts allowed before any wait.
const FREE_ATTEMPTS: u32 = 3;
const BASE_WAIT_SECS: u64 = 5;
const MAX_WAIT_SECS: u64 = 15 * 60;

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize)]
struct State {
    failures: u32,
    /// Unix seconds before which no attempt is accepted.
    next_allowed: i64,
}

pub struct UnlockAttempts {
    store: DocumentStore,
}

impl UnlockAttempts {
    #[must_use]
    pub fn new(path: PathBuf) -> Self {
        Self { store: DocumentStore::new(path) }
    }

    fn load(&self) -> State {
        self.store.load_or(State::default())
    }

    /// `Err(seconds)` while a wait is in force.
    pub fn check(&self, now: i64) -> Result<(), u64> {
        let state = self.load();
        if now < state.next_allowed {
            Err(u64::try_from(state.next_allowed - now).unwrap_or(0).max(1))
        } else {
            Ok(())
        }
    }

    /// Records a wrong attempt; returns the wait it imposes (0 for a free one).
    pub fn record_failure(&self, now: i64) -> u64 {
        let mut state = self.load();
        state.failures = state.failures.saturating_add(1);
        let wait = wait_after(state.failures);
        state.next_allowed = now + i64::try_from(wait).unwrap_or(i64::MAX / 2);
        let _ = self.store.save(&state);
        wait
    }

    pub fn record_success(&self) {
        let _ = self.store.save(&State::default());
    }
}

fn wait_after(failures: u32) -> u64 {
    if failures < FREE_ATTEMPTS {
        return 0;
    }
    let doublings = (failures - FREE_ATTEMPTS).min(20);
    (BASE_WAIT_SECS << doublings).min(MAX_WAIT_SECS)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn backoff_is_exponential_capped_and_free_at_first() {
        let waits: Vec<u64> = (1..=12).map(wait_after).collect();
        assert_eq!(&waits[..6], &[0, 0, 5, 10, 20, 40]);
        assert_eq!(*waits.last().unwrap(), MAX_WAIT_SECS);
    }

    #[test]
    fn the_wait_survives_a_restart_and_success_clears_it() {
        let dir = tempfile::TempDir::new().unwrap();
        let path = dir.path().join("unlock_attempts.json");
        let attempts = UnlockAttempts::new(path.clone());
        for _ in 0..3 {
            attempts.record_failure(1_000);
        }
        let reopened = UnlockAttempts::new(path);
        assert_eq!(reopened.check(1_001), Err(4));
        assert!(reopened.check(1_005).is_ok());
        reopened.record_success();
        assert!(reopened.check(0).is_ok());
    }
}
