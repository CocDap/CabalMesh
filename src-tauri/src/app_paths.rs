//! Where this instance keeps its state.
//!
//! # Injected, not discovered
//!
//! This module previously called `dirs::data_dir()`, a desktop convention with
//! no correct answer inside a mobile app sandbox. Tauri knows the right
//! directory on every platform, so the path is now **set once at startup** from
//! the platform path resolver and read everywhere else.
//!
//! Nothing below asks the operating system where to write. If [`set`] has not
//! been called, [`data_dir`] falls back to a working-directory path and warns
//! loudly — a wrong-but-visible location beats silently writing a wallet
//! somewhere unrecoverable.

use std::fs;
use std::path::{Path, PathBuf};
use std::sync::OnceLock;

static DATA_DIR: OnceLock<PathBuf> = OnceLock::new();

/// Fixes the data directory for this process.
///
/// Called once from `run()` with the platform's resolved app-data directory,
/// before bootstrap. Later calls are ignored: two different directories in one
/// process would mean two different wallets.
///
/// `CABALMESH_DATA_DIR` overrides it on desktop, which is what lets the local
/// two-node mesh test run isolated instances side by side. There is no
/// environment to read on mobile, so the override is desktop-only.
pub fn set(dir: impl Into<PathBuf>) {
    let chosen = resolve_override().unwrap_or_else(|| dir.into());
    let _ = fs::create_dir_all(&chosen);

    if DATA_DIR.set(chosen.clone()).is_err() {
        tracing::warn!(
            target: "cabalmesh::paths",
            attempted = %chosen.display(),
            existing = %data_dir().display(),
            "data directory already fixed; ignoring"
        );
        return;
    }
    tracing::info!(target: "cabalmesh::paths", dir = %chosen.display(), "data directory set");

    #[cfg(desktop)]
    migrate_from_legacy_dir(&chosen);
}

/// Adopts state written before the directory moved.
///
/// Switching from `dirs::data_dir()/cabalmesh` to the platform's app-data
/// directory renamed the folder from `cabalmesh` to the bundle identifier. To
/// an existing installation that looks like the wallet vanished and a fresh one
/// appeared — the exact data loss this ticket exists to prevent.
///
/// The two directories are siblings on every desktop platform, so the old one
/// is found without reintroducing path discovery.
///
/// Deliberately conservative:
///
/// - **Copies, never moves.** The old directory stays as a backup. It holds
///   private keys, so removing it is the user's call, not a migration's.
/// - **Only when the destination has no identities.** A destination that
///   already has a wallet is never overwritten, whatever the source contains.
/// - **Best effort per file.** A file that fails to copy is logged and skipped
///   rather than aborting the app.
#[cfg(desktop)]
fn migrate_from_legacy_dir(current: &Path) {
    const IDENTITIES: &str = "identities.json";

    if current.join(IDENTITIES).exists() {
        return;
    }
    let Some(legacy) = current.parent().map(|parent| parent.join("cabalmesh")) else {
        return;
    };
    if legacy == current || !legacy.join(IDENTITIES).exists() {
        return;
    }

    let Ok(entries) = fs::read_dir(&legacy) else {
        return;
    };

    let mut copied = 0_usize;
    for entry in entries.flatten() {
        let source = entry.path();
        if !source.is_file() {
            continue;
        }
        let destination = current.join(entry.file_name());
        if destination.exists() {
            continue;
        }
        match fs::copy(&source, &destination) {
            Ok(_) => copied += 1,
            Err(error) => tracing::warn!(
                target: "cabalmesh::paths",
                file = %entry.file_name().to_string_lossy(),
                %error,
                "could not migrate file"
            ),
        }
    }

    tracing::info!(
        target: "cabalmesh::paths",
        from = %legacy.display(),
        to = %current.display(),
        copied,
        "adopted state from the pre-move data directory; the old one is left as a backup"
    );
}

#[cfg(desktop)]
fn resolve_override() -> Option<PathBuf> {
    std::env::var("CABALMESH_DATA_DIR")
        .ok()
        .filter(|value| !value.is_empty())
        .map(PathBuf::from)
}

#[cfg(not(desktop))]
const fn resolve_override() -> Option<PathBuf> {
    None
}

/// The directory holding this instance's state.
///
/// An unset directory falls back to `./cabalmesh-data` with a warning rather
/// than aborting, so a misconfiguration is visible and recoverable instead of
/// fatal at startup.
#[must_use]
pub fn data_dir() -> PathBuf {
    if let Some(dir) = DATA_DIR.get() {
        return dir.clone();
    }

    let fallback = PathBuf::from("./cabalmesh-data");
    tracing::warn!(
        target: "cabalmesh::paths",
        fallback = %fallback.display(),
        "data directory was never set — call app_paths::set() during setup"
    );
    let _ = fs::create_dir_all(&fallback);
    fallback
}

/// A path inside the data directory.
#[must_use]
pub fn in_data_dir(name: impl AsRef<Path>) -> PathBuf {
    data_dir().join(name)
}
