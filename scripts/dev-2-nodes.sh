#!/usr/bin/env bash
# Launches two local CabalMesh instances for mesh testing on one machine:
# node1 runs via `npm run tauri dev` (Vite + live reload), node2 runs the
# already-built debug binary directly with its own isolated data dir/wallet.
# Kills any previous instances first so you don't end up with duplicates
# sharing a data dir.
#
# Usage: scripts/dev-2-nodes.sh [--near]
#   --near  force both nodes onto NEAR testnet (Tidepool), even if a saved
#           config says otherwise. See docs/near-two-device-checklist.md.
set -euo pipefail

cd "$(dirname "$0")/.."
REPO_ROOT="$(pwd)"

for arg in "$@"; do
  case "$arg" in
    --near) export CABALMESH_NETWORK=testnet ;;
    *) echo "unknown argument: $arg" >&2; exit 2 ;;
  esac
done

NODE1_LOG=/tmp/tauri-dev.log
NODE2_LOG=/tmp/cabalmesh-node2.log
NODE2_DATA_DIR=/tmp/cabalmesh-node2

echo "==> Stopping any existing instances..."
# Scoped to this checkout: a Vite server from another project must survive.
pkill -f "$REPO_ROOT/node_modules/.bin/vite" 2>/dev/null || true
pkill -f "$REPO_ROOT/node_modules/.bin/tauri dev" 2>/dev/null || true
pkill -f "target/debug/cabalmesh" 2>/dev/null || true
sleep 1

echo "==> Starting node1 (npm run tauri dev)..."
rm -f "$NODE1_LOG"
(cd "$REPO_ROOT" && npm run tauri dev > "$NODE1_LOG" 2>&1 &)

echo "==> Waiting for node1 to finish bootstrapping..."
for _ in $(seq 1 60); do
  if grep -qE "System Bootstrap Complete|error\[|panicked" "$NODE1_LOG" 2>/dev/null; then
    break
  fi
  sleep 3
done
if ! grep -q "System Bootstrap Complete" "$NODE1_LOG" 2>/dev/null; then
  echo "!! node1 did not report a successful bootstrap — check $NODE1_LOG"
  tail -30 "$NODE1_LOG"
  exit 1
fi
echo "==> node1 is up."

echo "==> Starting node2 (raw binary, isolated data dir + wallet)..."
rm -f "$NODE2_LOG"
(cd "$REPO_ROOT/src-tauri" && CABALMESH_DATA_DIR="$NODE2_DATA_DIR" CABALMESH_INSTANCE_ID=-node2 ./target/debug/cabalmesh > "$NODE2_LOG" 2>&1 &)

echo "==> Waiting for node2 to finish bootstrapping..."
for _ in $(seq 1 30); do
  if grep -qE "System Bootstrap Complete|error|panicked" "$NODE2_LOG" 2>/dev/null; then
    break
  fi
  sleep 2
done
if ! grep -q "System Bootstrap Complete" "$NODE2_LOG" 2>/dev/null; then
  echo "!! node2 did not report a successful bootstrap — check $NODE2_LOG"
  tail -30 "$NODE2_LOG"
  exit 1
fi
echo "==> node2 is up."

echo ""
echo "Both nodes are running:"
echo "  node1 log: $NODE1_LOG"
echo "  node2 log: $NODE2_LOG  (data dir: $NODE2_DATA_DIR)"
echo ""
echo "Stop both with:"
echo "  pkill -f '$REPO_ROOT/node_modules/.bin/vite'; pkill -f '$REPO_ROOT/node_modules/.bin/tauri dev'; pkill -f 'target/debug/cabalmesh'"
