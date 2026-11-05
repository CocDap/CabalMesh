#!/usr/bin/env bash
#
# Two machines, one BLE link, one answer.
#
# Run this on both machines at the same time. It builds the node, drives the
# real radio, and prints PASS or FAIL with the reason — so completing the
# verification does not require reading a log and knowing what to look for.
#
# Usage:
#   scripts/ble-link-check.sh            # the real radio
#   scripts/ble-link-check.sh 60         # ...with a longer window
#
# Why a script and not a test: `cargo test` cannot drive the radio, because an
# unbundled process is refused Bluetooth by TCC. See docs/mobile-build-verification.md.

set -euo pipefail

WINDOW="${1:-45}"
ROOT="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
LOG="$(mktemp -t cabalmesh-ble-link)"

cleanup() {
    if [[ -n "${NODE_PID:-}" ]]; then
        kill "$NODE_PID" 2>/dev/null || true
        wait "$NODE_PID" 2>/dev/null || true
    fi
}
trap cleanup EXIT

fail() {
    echo
    echo "FAIL: $1"
    echo
    echo "Last lines:"
    tail -n 15 "$LOG" | sed 's/^/  /'
    exit 1
}

# The radio has to be on before anything else is worth trying. Checked here
# rather than left to the app, because "Bluetooth is switched off" arriving as
# a log line forty seconds in is a worse answer than one arriving now.
if ! system_profiler SPBluetoothDataType 2>/dev/null | grep -q "State: On"; then
    fail "Bluetooth is switched off on this machine. Turn it on and run again."
fi

echo "Building..."
(cd "$ROOT/src-tauri" && cargo build --quiet -p cabalmesh --example ble_node)

echo "Running the radio for ${WINDOW}s. Start this on the other machine too."
echo
RUST_LOG="${RUST_LOG:-info,cabal_ble_macos=debug}" \
    "$ROOT/src-tauri/target/debug/examples/ble_node" >"$LOG" 2>&1 &
NODE_PID=$!

for _ in $(seq "$WINDOW"); do
    sleep 1
    # Stop early on success: there is nothing to learn from the remaining
    # window once a link is up and a peer is in range.
    if grep -q "link up" "$LOG" && grep -q "in-range=1" "$LOG"; then
        break
    fi
    if ! kill -0 "$NODE_PID" 2>/dev/null; then
        fail "the node exited before the window elapsed"
    fi
done

# Each of these is a distinct place the bring-up can stop, and naming which one
# is the difference between a fix and a guess.
grep -q "scanning for nodes" "$LOG" || fail "the central never started scanning"
grep -q "published an L2CAP channel" "$LOG" || fail "no L2CAP channel was published"
grep -q "advertising" "$LOG" || fail "the peripheral never started advertising"

echo "Bring-up: scanning, L2CAP published, advertising."

if ! grep -q "link up" "$LOG"; then
    fail "no peer was found.
  - Is the other machine running this script at the same time?
  - Are they within a few metres of each other?
  - Two processes on ONE Mac cannot see each other: a controller does not hear
    its own advertisements. This needs two machines."
fi

grep -q "in-range=1" "$LOG" || fail "a link came up but no peer announced itself over it"

echo
echo "PASS: linked to a peer over BLE and exchanged announcements."
grep -E "link up|in-range=1" "$LOG" | tail -n 3 | sed 's/^/  /'
