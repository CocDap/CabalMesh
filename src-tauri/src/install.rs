//! When this installation first ran.
//!
//! # Why this needs its own file
//!
//! `MEMBER SINCE` had no source. The obvious candidates are all wrong:
//!
//! - **The mesh identity** is ephemeral by design — a fresh keypair every
//!   launch, which is the "Nobody" identity the product is built around. It has
//!   no history to have a start date.
//! - **A file's creation time** does not survive a backup and restore, or a
//!   reinstall from a cloud backup, and reports the restore date as though the
//!   user joined then.
//! - **The vault** only exists once a wallet does, which is later than the
//!   moment the user actually joined.
//!
//! So the timestamp is written explicitly the first time it is asked for, and
//! never again. It is the honest answer to "since when has this device been
//! part of the mesh", which is what the row claims.
//!
//! # First read, not first launch
//!
//! Writing it during bootstrap would mean the file is created by code paths
//! that have nothing to do with membership — a build probe that launches and
//! quits would leave one. Writing it when the profile screen first asks keeps
//! the value meaning what it says.

use serde::{Deserialize, Serialize};

/// The persisted record.
#[derive(Debug, Clone, Copy, Serialize, Deserialize)]
struct Install {
    /// Unix milliseconds of the first read.
    first_seen_ms: u64,
}

/// When this installation was first seen, in unix milliseconds.
///
/// Writes the value on the first call and returns the stored one thereafter. A
/// write that fails is logged and the in-memory value returned, so a read-only
/// filesystem degrades to "since this launch" rather than to an error on a
/// screen that is otherwise fine.
#[must_use]
pub fn first_seen_ms(now_ms: u64) -> u64 {
    first_seen_in(
        &cabal_store::JsonStore::new(crate::app_paths::in_data_dir("install.json")),
        now_ms,
    )
}

/// The same, against an explicit store.
///
/// Split out so it is testable: `app_paths` is a process-wide `OnceLock`, so a
/// test that set it would depend on running before every other test that does.
fn first_seen_in(store: &cabal_store::JsonStore, now_ms: u64) -> u64 {
    if let Ok(existing) = store.load::<Install>() {
        return existing.first_seen_ms;
    }

    let record = Install { first_seen_ms: now_ms };
    if let Err(error) = store.save(&record) {
        tracing::warn!(
            target: "cabalmesh::install",
            %error,
            "could not record the first-run timestamp; member-since will move"
        );
    }
    record.first_seen_ms
}

/// Formats a unix-millisecond timestamp as the board writes a date: `2026.08.03`.
///
/// Dots rather than slashes, and year-first, because the brand's numbers are
/// unambiguous by construction — `03/08` is two different dates depending on
/// where the reader is.
///
/// Implemented here rather than with `chrono` because the app crate does not
/// depend on it outside tests, and a civil-date conversion is a dozen lines of
/// arithmetic with no timezone question to get wrong: this is deliberately UTC.
#[must_use]
pub fn format_date(ms: u64) -> String {
    let (year, month, day) = civil_from_days((ms / 86_400_000) as i64);
    format!("{year:04}.{month:02}.{day:02}")
}

/// Days since the unix epoch to a civil date.
///
/// Howard Hinnant's `civil_from_days`, which is the standard formulation and
/// exact for every date this will ever see.
const fn civil_from_days(days: i64) -> (i64, u32, u32) {
    // Shift the era so March is month 1, which makes the leap day the last day
    // of the year and removes every special case.
    let z = days + 719_468;
    let era = if z >= 0 { z } else { z - 146_096 } / 146_097;
    let doe = (z - era * 146_097) as u64;
    let yoe = (doe - doe / 1_460 + doe / 36_524 - doe / 146_096) / 365;
    let y = yoe as i64 + era * 400;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
    let mp = (5 * doy + 2) / 153;
    let d = (doy - (153 * mp + 2) / 5 + 1) as u32;
    let m = if mp < 10 { mp + 3 } else { mp - 9 } as u32;
    (if m <= 2 { y + 1 } else { y }, m, d)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn dates_are_year_first_and_zero_padded() {
        // 2026-08-03T00:00:00Z
        assert_eq!(format_date(1_785_715_200_000), "2026.08.03");
        // The epoch itself, which is the value a failed clock read produces.
        assert_eq!(format_date(0), "1970.01.01");
    }

    #[test]
    fn leap_days_are_not_off_by_one() {
        // 2024-02-29T00:00:00Z — the case a naive days-per-month table gets
        // wrong, and the reason this uses the era formulation.
        assert_eq!(format_date(1_709_164_800_000), "2024.02.29");
        // The day after, to catch an off-by-one that only shows at the boundary.
        assert_eq!(format_date(1_709_251_200_000), "2024.03.01");
    }

    #[test]
    fn a_century_boundary_is_a_leap_year_only_every_four_hundred() {
        // 2000 was a leap year; 1900 was not. The two cases a simple
        // `year % 4` check gets wrong in opposite directions.
        assert_eq!(format_date(951_782_400_000), "2000.02.29");
    }

    #[test]
    fn the_first_seen_timestamp_is_written_once_and_then_kept() {
        let dir = tempfile::tempdir().unwrap();
        let store = cabal_store::JsonStore::new(dir.path().join("install.json"));

        let first = first_seen_in(&store, 1_000_000);
        // A later call must not move the date, or "member since" would read as
        // today on every launch.
        let second = first_seen_in(&store, 9_999_999);

        assert_eq!(first, 1_000_000);
        assert_eq!(second, 1_000_000);
    }

    #[test]
    fn a_fresh_install_survives_a_restart() {
        // The value has to come back from disk, not from a process-lifetime
        // cache — otherwise it resets on every launch, which is the bug.
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("install.json");

        let written = first_seen_in(&cabal_store::JsonStore::new(&path), 1_785_715_200_000);
        let reread = first_seen_in(&cabal_store::JsonStore::new(&path), 9_999_999_999_999);

        assert_eq!(written, reread);
        assert_eq!(format_date(reread), "2026.08.03");
    }
}
