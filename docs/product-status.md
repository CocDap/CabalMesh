# Product status — what is real, what is not

Written 2026-08-10 and updated 2026-11-05, by reading the code rather than the READMEs. Every claim
below carries the file it was checked against, so this can be re-verified
instead of trusted.

The point of this document is that CabalMesh's pitch names three big things —
mesh, AI, ZK — and only one of them is actually running. Knowing which is which
is the difference between a demo that survives questions and one that does not.

## Works, end to end

| Feature | Evidence |
|---|---|
| Mesh networking (libp2p, mDNS, gossipsub) | `src/mesh.rs`; 9 passing tests in `tests/ble_loopback.rs` |
| BLE offline plane (Apple + Android) | `src/ble/`, `crates/cabal-ble`; demonstrated across two Android emulators, see `mobile-build-verification.md` |
| Intent lifecycle: compose → broadcast → settle → proof | `src/commands.rs`, `src/intents.rs` |
| Atomic settlement on NEAR Intents (testnet) | `src/near_settlement.rs`, `tests/near_live.rs`; tx hashes in `near-two-device-checklist.md` |
| Offline signing, gateway submission, report checked against chain logs | `src/near_settlement.rs`, `crates/cabal-near` |
| Deposit and withdraw through the verifier | `src/near_bridge.rs`, `tests/near_live.rs`, Vault screen |
| Vault encryption (AES-256-GCM) | `crates/cabal-vault` |
| Standing (settlement count, real) | `src/standing.rs` |

**The offline path is the crown jewel.** Sign a `token_diff` with no network,
flood it over the mesh, watch it settle when any peer with gas reaches the
verifier. No other part of this project is as differentiated.

The settlement logic is proven against the live verifier by a headless test with
real keys. **Two real devices, driven through the UI over a real mesh, have not
been run yet.** `near-two-device-checklist.md` is that run.

## UI exists, behaviour does not

These render, accept input, and are stored — but nothing downstream reads them.

| Control | What actually happens |
|---|---|
| **PRIVACY** (LOW/MEDIUM/HIGH) | Parsed into the draft and echoed in the review dialog (`commands.rs:879`). `grep` for `PrivacyLevel` across `mesh.rs`, `ble/`, and `crates/cabal-ble/src/router.rs` returns **no routing code**. It changes nothing. |
| **MODE** (SHARK/GHOST/PATIENT) | Labels and descriptions live in `crates/cabal-core/src/intent.rs:39-58`. No hit for `ExecutionMode` in `mesh.rs` or `near_settlement.rs`. No strategy differs. |

## Not wired up at all

- ~~**AI / Ollama.**~~ **Now live (2026-11-04 update).** Intent parsing runs
  through `src/intent_chat.rs`, and peer price negotiation through
  `negotiation::consider`, called from `src/lib.rs` when a mesh intent arrives
  (PR #15). If the model fails, a deterministic midpoint is used. `SharkAgent`
  (`src/agent.rs`) is still declared in `lib.rs`/`state.rs` but is not on this path.
- **Marketplace, modules, relay rewards.** **Removed, 2026-11-05**, with every EVM
  contract. They return on NEAR in P2. Old code: `git show e26157c:contracts/`.
- **SWAP / STAKE actions.** **Removed, 2026-11-05.** The composer accepts BUY and
  SELL only, because a `token_diff` pair is exactly that.
- **ZK proving and verification.** **Removed, 2026-08-27.** `zk_handler.rs`
  and `noir-circuit/` are gone. Nothing ever called either one: no command
  reached `generate_proof` or `verify_proof`, `intent_proof` returns the
  settlement hash rather than a ZK proof, and neither path had been run against
  a real circuit because this environment has no `nargo`. Deleting it is not a
  loss of capability — the capability was never wired. Restoring the scaffolding
  is `git revert` away if the Phase-4 work starts.
- **Confidential compute (FHE/MPC).** No code exists.
- **Private Swap.** No interface exists, despite older README wording.

## Platform reality

| Platform | Secure element | BLE | Notes |
|---|---|---|---|
| iOS | Secure Enclave | yes | no ZK proving anywhere in the build |
| Android | Keystore / StrongBox | yes | same |
| macOS | Secure Enclave (Apple Silicon) | yes (CoreBluetooth) | needs `NSBluetoothAlwaysUsageDescription`, see 4dfa252 |
| Windows | TPM | **no** | BLE plane silently does not run |
| Linux | usually none | **no** | BLE plane silently does not run |

`ble::backend::choose` returns `None` outside Apple/Android, and the app falls
back to the IP plane without telling the user the offline plane is off.

## Suggested MVP cut

One true story beats three half-built ones. The offline mesh settlement path is
provable today; AI is not, and ZK no longer has code to point at.

**Remove or disable** so the UI stops implying capabilities that do not exist:
either wire PRIVACY/MODE to real behaviour or hide them.

**Add**, in order: identity backup (see `identity-design.md`), then reconnect the
AI as intent parsing (see `intent-chat-and-modules-design.md`), then everything
else.
