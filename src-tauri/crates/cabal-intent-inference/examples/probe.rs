use cabal_intent_inference::{infer_text, model_footprint_bytes, MODEL_VERSION};
use std::hint::black_box;
use std::time::Instant;

const ITERATIONS: usize = 100_000;
const PHRASES: [&str; 4] = [
    "buy 10 wnear under 95, shark mode, privacy high",
    "sell 2.5 wnear at any price, ghost mode, privacy high",
    "exchange 125 usdc above 1.01, patient mode, medium privacy",
    "stake 5 wnear at market price, patient mode, low privacy",
];

fn main() {
    let process_started = Instant::now();
    for phrase in PHRASES {
        assert!(infer_text(phrase).is_ok());
    }
    let startup = process_started.elapsed();

    let started = Instant::now();
    for index in 0..ITERATIONS {
        let phrase = PHRASES[index % PHRASES.len()];
        black_box(infer_text(black_box(phrase))).expect("benchmark phrases must remain valid");
    }
    let elapsed = started.elapsed();
    let nanos_per_inference = elapsed.as_nanos() / ITERATIONS as u128;

    println!("model={MODEL_VERSION}");
    println!("target={}-{}", std::env::consts::OS, std::env::consts::ARCH);
    println!("model_bytes={}", model_footprint_bytes());
    println!("executable_bytes={}", executable_bytes());
    println!("startup_micros={}", startup.as_micros());
    println!("iterations={ITERATIONS}");
    println!("elapsed_ms={}", elapsed.as_millis());
    println!("nanos_per_inference={nanos_per_inference}");
    println!("resident_set_bytes={}", resident_set_bytes());
}

fn executable_bytes() -> u64 {
    std::env::current_exe()
        .ok()
        .and_then(|path| path.metadata().ok())
        .map_or(0, |metadata| metadata.len())
}

#[cfg(any(target_os = "android", target_os = "linux"))]
fn resident_set_bytes() -> u64 {
    std::fs::read_to_string("/proc/self/status")
        .ok()
        .and_then(|status| {
            status.lines().find_map(|line| {
                line.strip_prefix("VmRSS:")?
                    .split_whitespace()
                    .next()?
                    .parse::<u64>()
                    .ok()
            })
        })
        .unwrap_or(0)
        .saturating_mul(1024)
}

#[cfg(any(target_os = "ios", target_os = "macos"))]
fn resident_set_bytes() -> u64 {
    std::process::Command::new("/bin/ps")
        .args(["-o", "rss=", "-p", &std::process::id().to_string()])
        .output()
        .ok()
        .filter(|output| output.status.success())
        .and_then(|output| String::from_utf8(output.stdout).ok())
        .and_then(|rss| rss.trim().parse::<u64>().ok())
        .unwrap_or(0)
        .saturating_mul(1024)
}

#[cfg(not(any(
    target_os = "android",
    target_os = "ios",
    target_os = "linux",
    target_os = "macos"
)))]
fn resident_set_bytes() -> u64 {
    0
}
