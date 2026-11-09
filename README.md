# CabalMesh

> **Trade peer-to-peer with no server in the middle.** AI agents negotiate over a Bluetooth and libp2p mesh, and the deal settles atomically on **NEAR Intents**.

CabalMesh is a privacy-first desktop and mobile app (Tauri 2: Rust + React). Two people state what they want ("sell 1 wNEAR above $3.90"). Their devices find each other over the mesh, even with no internet. Local AI agents agree on a price inside each user's bounds. Each side signs its half of the swap offline, and whichever peer reaches the internet first submits both halves to the NEAR Intents verifier, which swaps the funds in one atomic step.

## How a trade settles on NEAR Intents

```
 Device A (seller)                    mesh                     Device B (buyer)
 ─────────────────                ───────────                  ────────────────
 PublicIntent: SELL wNEAR   ──────►  BLE / libp2p  ◄──────  PublicIntent: BUY wNEAR
 (no price is broadcast)                                    (no price is broadcast)
         │                                                           │
         └──────── agents negotiate inside each user's bounds ───────┘
                              agreed: 1 wNEAR @ $4.04
         │                                                           │
 user confirms, signs offline                         user confirms, signs offline
 token_diff  wNEAR −1                                 token_diff  wNEAR +1
             USDC  +4.04                                          USDC  −4.04
         │                                                           │
         └────────────── SignedDiff ──► mesh ◄── SignedDiff ─────────┘
                                         │
                   any peer with internet (a "gateway")
                                         │
                                         ▼
               verifier.execute_intents([diff_A, diff_B])
               both legs settle together, or neither does
```

Why NEAR Intents fits a mesh:

- **Signing needs no network.** A `token_diff` is a NEP-413 signed message, not a transaction. A device can sign over Bluetooth in a basement and hand it to the mesh.
- **Anyone can submit.** `execute_intents` has no access control, so the first peer that gets online can settle the deal. It cannot change the deal, because both signatures cover the exact amounts.
- **Atomic and two-way.** The verifier only applies the diffs if they net to zero, so there is no escrow window and no counterparty risk.
- **Accounts are free.** A device's NEAR account is an implicit account: the hex of its ed25519 public key. It can sign and hold a balance inside the verifier without ever being created on-chain. Only the gateway that submits needs NEAR for gas.

## Quick start

### Prerequisites

- **Rust** stable 1.91+ (`rustup update stable`)
- **Node.js** 24+

### Run

```bash
npm install
npm run tauri dev
```

The app defaults to NEAR testnet, using the Tidepool verifier and tokens compiled into it, so nothing needs deploying. To try a trade between two nodes on one machine:

```bash
scripts/dev-2-nodes.sh --near
```

The script starts node 1 with `tauri dev` and node 2 from the debug binary, with its own data directory and wallet. The two nodes find each other over mDNS. [`docs/near-two-device-checklist.md`](docs/near-two-device-checklist.md) walks through a full trade.

Other entry points:

| Command | What it does |
|---|---|
| `npm run ios:sim` | Debug build for the iOS simulator |
| `npm run android:bundle` | Android App Bundle |
| `npm run relay` | Runs the libp2p circuit relay server (`crates/cabal-relay`), see [`docs/relay-operations.md`](docs/relay-operations.md) |
| `npm run ble:check` | BLE link check between devices |

## Using the app

Splash → Unlock → Home, then the tabs Nodes, Intents, Vault and Profile.

1. **New intent.** Type what you want, for example `Sell 1 wNEAR above $3.90`. The local model turns it into a draft, and you review it before anything is broadcast.
2. **Negotiate.** Your intent tells the mesh its asset, side and size, but never your price limit. When a peer posts the opposite side at the same size, the two agents trade offers for up to 3 rounds, using the market price. Code enforces your limit on every offer. If no model is reachable, a built-in price near market is used instead.
3. **Settle.** You confirm, the app signs, and settlement happens when any node is online. With Wi-Fi off, intents still move over Bluetooth, and signed diffs wait on the mesh until a gateway appears.

## Architecture

```
┌──────────────────────── React UI (src/) ────────────────────────┐
│ screens · design system (src/ds) · generated bindings (ts-rs)    │
└───────────────────────────────┬──────────────────────────────────┘
                         Tauri commands
┌───────────────────────────────┴──────────────────────────────────┐
│ src-tauri/src                                                     │
│  mesh.rs ─ libp2p: TCP+QUIC, Noise, gossipsub, mDNS, relay, dcutr │
│  ble/    ─ BLE plane: flood routing over L2CAP                    │
│  intent_chat.rs · negotiation.rs ─ AI parsing and pricing         │
│  identity.rs ─ keys in the vault                                  │
│  near_bridge.rs ─ NEAR JSON-RPC: balances, intents, wallet moves  │
│  near_settlement.rs ─ SignedDiff book, gateway, report checks     │
└───────────────────────────────┬──────────────────────────────────┘
          I/O-free crates (src-tauri/crates, unsafe forbidden)
  cabal-core · cabal-near · cabal-ble · cabal-vault · cabal-guardian
  cabal-store · cabal-intent-inference
```

```
.
├── src/                    React 19 + Vite frontend
├── src-tauri/
│   ├── src/                app crate: commands, mesh, BLE, AI, NEAR settlement
│   ├── crates/             pure-logic crates plus cabal-relay (server binary)
│   └── tests/              integration tests (IPC contract, BLE loopback, guardian)
├── scripts/                dev, size and design-system tooling
└── docs/                   designs, status, plans
```

Keys are stored encrypted (AES-256-GCM) in the vault, bound to the device's secure element where there is one, and recoverable through guardians. See [`docs/identity-design.md`](docs/identity-design.md).

Local intent history, account snapshots and settings use SQLite. Existing JSON files are imported once and retained as backups; balances are saved only after a complete successful refresh. See [`docs/local-storage.md`](docs/local-storage.md) for the schema, migration and desktop/mobile/web storage approach.

| Platform | BLE offline plane | Secure element |
|---|---|---|
| iOS, Android, macOS | yes | Secure Enclave / Keystore |
| Windows, Linux | no, IP plane only | TPM / usually none |

## Testing

```bash
cd src-tauri && cargo test --workspace     # Rust: app + crates
npm run build                              # TypeScript type check + bundle
npm run bindings:check                     # generated bindings are up to date
cd src-tauri && cargo test --test near_live -- --ignored --test-threads=1
                                           # live Tidepool testnet, see docs/near-two-device-checklist.md
```

## Roadmap

### P1: NEAR Intents settlement

- [x] NEAR key in the vault, NEP-413 signing
- [x] `SignedDiff` over the mesh, gateway submission
- [x] Deposit and withdraw in Vault
- [x] Settled on live testnet: two ledgers, third-party gateway, two app processes over libp2p
- [x] Two Android simulators through the UI, negotiation and signatures over BLE ([evidence](docs/verification/ble-simulator-settlement/README.md))
- [ ] Two physical devices over BLE

### P2: NEAR contracts

- [x] Gateway rewards per settled pair: `contracts/rewards` deployed to testnet, proven by a headless test
- [ ] Rewards paid through the app
- [ ] Marketplace and modules on NEAR ([plan](docs/superpowers/plans/2026-11-05-p2b-near-marketplace-modules.md))

### P3: Bring your own model

- [x] Base URL, model name and API key per device, set on Profile
- [x] Presets for NEAR AI Cloud, local Ollama and OpenRouter
- [x] Key encrypted with the vault's key; never reaches the webview or the mesh
- [x] Tested against a real local Ollama
- [ ] NEAR AI Cloud and OpenRouter called with a real key
- [ ] Two devices with different providers

### P4: Agent peer trade

- [x] Function-calling agent with market price, mesh offers and position tools, up to 3 rounds of offers
- [x] Code enforces your price limit, which never leaves the device; you confirm before anything is signed
- [x] Built-in price near market when no model is reachable
- [x] Settled on testnet between two app processes using a real local Ollama
- [ ] Two devices with different providers

### P5: Local key backup

- [x] Export and import the NEAR key (`VAULT → KEYS → ADVANCED`); import verifies a named `.testnet` account
- [x] Passphrase unlock (Argon2id), opt-in
- [x] Guardian mesh unlock: sealed shares over BLE, release only on the guardian's tap
- [ ] Encrypted backup file, so a wiped device restores without a raw hex key
- [ ] 24–48h recovery delay with a veto notification
- [ ] Mobile PIN unlock, behind a retry counter

### P6: ZK Box

Prove a trade is valid without revealing what the agents keep private. No code yet; the old stub was deleted on 2026-08-27.

- [ ] **Private price limit.** Each offer carries a proof that it stays inside your floor or ceiling. The peer verifies it without ever learning the limit.
- [ ] **Proof of funds.** Prove your Intents balance covers the trade size without revealing the balance, before a peer starts negotiating.
- [ ] **Offline verification.** A gateway checks the proof attached to a `SignedDiff` over the mesh, before submitting and without an Internet connection.
- [ ] **On-chain verifier.** A NEAR contract verifies the proof at settlement.
- [ ] **On-device prover.** Proofs are generated on iOS, Android and desktop, with no server involved.

## Contributing

Fork, branch, and open a PR. New Tauri commands must be registered in five places (`commands.rs`, `generate_handler!` in `lib.rs`, `build.rs`, and both capability files), followed by `npm run bindings`.
