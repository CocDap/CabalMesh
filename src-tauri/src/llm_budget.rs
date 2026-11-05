//! How many model calls this device may make per hour.
//!
//! # Why this exists
//!
//! Negotiation runs unattended for every matching intent the mesh delivers, and
//! with a bring-your-own key each of those calls can cost the user money. Anyone
//! on the mesh could otherwise spend it by flooding intents.

use std::collections::VecDeque;
use std::sync::Mutex;
use std::time::{Duration, Instant};

const WINDOW: Duration = Duration::from_secs(3600);

/// A sliding one-hour window of model calls.
#[derive(Debug)]
pub struct LlmBudget {
    state: Mutex<Window>,
}

#[derive(Debug)]
struct Window {
    limit: u32,
    calls: VecDeque<Instant>,
}

impl LlmBudget {
    #[must_use]
    pub fn new(per_hour: u32) -> Self {
        Self { state: Mutex::new(Window { limit: per_hour, calls: VecDeque::new() }) }
    }

    /// Takes one call if the last hour has room for it.
    pub fn try_acquire(&self, now: Instant) -> bool {
        let mut window = self.state.lock().unwrap_or_else(std::sync::PoisonError::into_inner);
        while window.calls.front().is_some_and(|call| now.duration_since(*call) >= WINDOW) {
            window.calls.pop_front();
        }
        if window.calls.len() >= window.limit as usize {
            return false;
        }
        window.calls.push_back(now);
        true
    }

    /// Changes the limit. Calls already made still count.
    pub fn set_limit(&self, per_hour: u32) {
        self.state.lock().unwrap_or_else(std::sync::PoisonError::into_inner).limit = per_hour;
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn allows_exactly_limit_per_hour() {
        let budget = LlmBudget::new(30);
        let now = Instant::now();
        for call in 0..30 {
            assert!(budget.try_acquire(now), "call {call} is within the limit");
        }
        assert!(!budget.try_acquire(now));
    }

    #[test]
    fn window_slides() {
        let budget = LlmBudget::new(30);
        let start = Instant::now();
        for _ in 0..30 {
            assert!(budget.try_acquire(start));
        }
        assert!(!budget.try_acquire(start + Duration::from_secs(3599)));
        assert!(budget.try_acquire(start + Duration::from_secs(3601)));
    }

    #[test]
    fn a_refused_call_does_not_use_up_the_window() {
        let budget = LlmBudget::new(1);
        let start = Instant::now();
        assert!(budget.try_acquire(start));
        assert!(!budget.try_acquire(start + Duration::from_secs(3500)));
        assert!(budget.try_acquire(start + Duration::from_secs(3601)));
    }

    #[test]
    fn a_zero_budget_allows_nothing() {
        assert!(!LlmBudget::new(0).try_acquire(Instant::now()));
    }

    #[test]
    fn a_new_limit_applies_to_calls_already_made() {
        let budget = LlmBudget::new(2);
        let now = Instant::now();
        assert!(budget.try_acquire(now));
        assert!(budget.try_acquire(now));
        budget.set_limit(3);
        assert!(budget.try_acquire(now));
        assert!(!budget.try_acquire(now));
        budget.set_limit(1);
        assert!(!budget.try_acquire(now));
    }
}
