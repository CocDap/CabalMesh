//! Diagnostics setup.
//!
//! # Why this exists
//!
//! The app previously logged with `println!` and `eprintln!`. On a desktop
//! terminal that is merely untidy. On a device it is *invisible*: nothing
//! written to stdout from an iOS app reaches Console.app, and nothing from an
//! Android app reaches `logcat`. Every phase after this one is debugged on a
//! device, so without structured output the tool is guesswork.
//!
//! # Where output goes
//!
//! | Platform | Destination | How to read it |
//! |---|---|---|
//! | iOS | unified log | Console.app, or `xcrun simctl spawn booted log stream --predicate 'subsystem == "com.cabalmesh.app"'` |
//! | macOS | unified log **and** stderr | `log stream`, or the terminal |
//! | Android | logcat | `adb logcat -s cabalmesh` |
//! | Linux / Windows | stderr | the terminal |
//!
//! # Rules
//!
//! - **The app crate installs the subscriber; nothing else does.** Libraries
//!   emit through the `tracing` facade and stay silent about where it goes.
//!   A library that installs a subscriber steals that choice from every
//!   consumer, including tests.
//! - **Fields, not interpolation.** `warn!(peer = %id, "dial failed")` can be
//!   filtered and searched; `warn!("dial to {id} failed")` cannot.
//! - **Never log secrets.** Private keys, recovery phrases and raw signed
//!   transactions do not go through here. The vault redacts them at the type
//!   level; this is the second line, not the first.

use std::sync::Once;

/// Filter applied when `RUST_LOG` is unset.
///
/// `info` for this app, `warn` for everything else: libp2p and hyper are
/// extremely chatty at debug, and a device log that scrolls too fast to read
/// is the same as no log.
const DEFAULT_FILTER: &str = "cabalmesh=info,cabalmesh_lib=info,cabal_core=info,warn";

/// Subsystem/tag the platform log is grouped under.
const SUBSYSTEM: &str = "com.cabalmesh.app";

static INIT: Once = Once::new();

/// Installs the process-wide subscriber.
///
/// Idempotent, and safe to call from tests: a second call is ignored rather
/// than panicking, which the global-default subscriber would otherwise do.
pub fn init() {
    INIT.call_once(|| {
        let filter = tracing_subscriber::EnvFilter::try_from_default_env()
            .unwrap_or_else(|_| tracing_subscriber::EnvFilter::new(DEFAULT_FILTER));

        install(filter);

        tracing::info!(
            target: "cabalmesh::telemetry",
            subsystem = SUBSYSTEM,
            "diagnostics initialised"
        );
    });
}

#[cfg(target_os = "ios")]
fn install(filter: tracing_subscriber::EnvFilter) {
    use tracing_subscriber::prelude::*;

    // iOS has no stderr worth writing to — the unified log is the only channel
    // that reaches a developer looking at a running device.
    tracing_subscriber::registry()
        .with(filter)
        .with(tracing_oslog::OsLogger::new(SUBSYSTEM, "default"))
        .init();
}

#[cfg(target_os = "macos")]
fn install(filter: tracing_subscriber::EnvFilter) {
    use tracing_subscriber::prelude::*;

    // Both: the terminal for `cargo run`, the unified log for a bundled app
    // launched from Finder, where stderr goes nowhere visible.
    tracing_subscriber::registry()
        .with(filter)
        .with(tracing_subscriber::fmt::layer().with_target(true))
        .with(tracing_oslog::OsLogger::new(SUBSYSTEM, "default"))
        .init();
}

#[cfg(target_os = "android")]
fn install(filter: tracing_subscriber::EnvFilter) {
    use tracing_subscriber::prelude::*;

    tracing_subscriber::registry()
        .with(filter)
        .with(tracing_android::layer(SUBSYSTEM).expect("android logcat layer"))
        .init();
}

#[cfg(not(any(target_os = "ios", target_os = "macos", target_os = "android")))]
fn install(filter: tracing_subscriber::EnvFilter) {
    use tracing_subscriber::prelude::*;

    tracing_subscriber::registry()
        .with(filter)
        .with(tracing_subscriber::fmt::layer().with_target(true))
        .init();
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn init_is_idempotent() {
        // Installing a global default twice panics. Tests, and any future
        // second entry point, rely on this being safe.
        init();
        init();
        init();
    }
}
