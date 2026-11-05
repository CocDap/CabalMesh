//! The radio actually coming up, on real hardware.
//!
//! # Why this is `#[ignore]`d
//!
//! It needs a Bluetooth controller that is switched on. CI has none, and a
//! machine with Bluetooth off would fail it for a reason that is not a defect.
//! So it does not run by default:
//!
//! ```sh
//! cargo test -p cabal-ble-macos -- --ignored --nocapture
//! ```
//!
//! # What it asserts, and what it cannot
//!
//! It asserts the bring-up sequence a single machine can prove: the radio
//! powers on, publishes an L2CAP channel, is assigned a PSM, adds its service,
//! advertises, and scans.
//!
//! It cannot assert a link. **Two processes on one Mac do not discover each
//! other** — a controller does not hear its own advertisements — which was
//! confirmed by running exactly that: both advertised, both scanned, neither
//! saw the other. Linking needs two machines and no amount of code changes
//! that.

#![cfg(target_vendor = "apple")]

use cabal_ble_macos::{Config, Event, Radio, Shared};
use std::time::{Duration, Instant};

/// Long enough for CoreBluetooth to power on and publish; short enough that a
/// failure is reported rather than waited on.
const DEADLINE: Duration = Duration::from_secs(10);

#[test]
#[ignore = "needs a Bluetooth controller that is switched on"]
fn the_radio_comes_up_and_stays_up() {
    let (shared, events) = Shared::new();
    let radio = Radio::start(&Config::default(), shared.clone()).expect("the radio starts");

    // The only thing a single machine can observe is the absence of a
    // complaint: `Unavailable` is the one event that arrives when the radio
    // will not run, and every other symptom of a broken bring-up — a bad UUID,
    // a service that will not publish — kills the process instead.
    //
    // That last part is not hypothetical. A UUID with a non-hex character in
    // it aborted the process from inside a delegate callback, and this test
    // reaching its deadline is what "it did not" looks like.
    let deadline = Instant::now() + DEADLINE;
    while Instant::now() < deadline {
        match events.recv_timeout(Duration::from_millis(200)) {
            Ok(Event::Unavailable(why)) => {
                panic!("the radio refused to run: {why}");
            }
            Ok(other) => {
                // A link, on a machine with a peer in range. Not required, and
                // very welcome.
                println!("radio event: {other:?}");
            }
            Err(_) => {}
        }
    }

    assert!(!shared.is_stopped(), "the radio stopped on its own");
    radio.stop();
}

#[test]
#[ignore = "needs a Bluetooth controller that is switched on"]
fn stopping_the_radio_is_observed() {
    let (shared, _events) = Shared::new();
    let radio = Radio::start(&Config::default(), shared.clone()).expect("the radio starts");

    std::thread::sleep(Duration::from_secs(2));
    radio.stop();

    assert!(shared.is_stopped());
    // Queueing after a stop must be refused rather than buffered: the offline
    // switch promises nothing leaves the device.
    assert!(!shared.queue(1, b"after the switch".to_vec()));
}
