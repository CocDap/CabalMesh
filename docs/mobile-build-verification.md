# Mobile build verification record

> **2026-11-05:** written when the app settled on Avalanche. NEAR is now the only chain, and `contracts/`, alloy and `blockchain_bridge.rs` are gone (`identity.rs` and `near_bridge.rs` replaced it). Entries about alloy, Fuji and AVAX describe that older build. Kept as a historical record; see `docs/product-status.md` for what is true today.

Running log of what has actually been proven to build and run, as opposed to what the plans assume. Updated per ticket.

---

## BLE plane — protocol and runtime **GO**, radio **not started** (2026-08-09)

`crates/cabal-ble` and `src/ble/`. Design in `docs/ble-mesh-design.md`.

**Proven by test, on the host, in CI — 105 tests, under three seconds.**

| Check | Where | Result |
|---|---|---|
| Wire format round-trips; every truncation is an error and never a panic | `wire.rs` | ✅ |
| A TTL decrement does not invalidate a signature | `wire.rs`, `identity.rs` | ✅ |
| Reserved flags, unknown kinds, lying lengths refused before allocating | `wire.rs`, `framing.rs` | ✅ |
| A packet split at every possible byte boundary still reassembles | `framing.rs` | ✅ |
| Flood reaches every node in a chain, star and ring — exactly once | `tests/mesh_simulation.rs` | ✅ |
| A flood terminates, and costs less than naive flooding in a full mesh | `tests/mesh_simulation.rs` | ✅ |
| Partition heals when one link returns | `tests/mesh_simulation.rs` | ✅ |
| Unsigned and identity-spoofing announcements are ignored | `tests/mesh_simulation.rs` | ✅ |
| The offline switch stops every transmission | both suites | ✅ |
| Two and three real nodes, real sockets, real timers, over `LoopbackTransport` | `tests/ble_loopback.rs` | ✅ |

**Proven by running two processes** — `cargo run -p cabalmesh --example ble_node`. Two nodes discovered each other, exchanged intents both ways, and reported matching link counts.

**Proven on the simulator, against a real peer process on the host.** A throwaway build opening on the nodes screen, launched with `SIMCTL_CHILD_CABALMESH_BLE_LOOPBACK=listen=127.0.0.1:9702,dial=127.0.0.1:9701` against `ble_node` on port 9701. The nodes screen rendered the peer's real identifier at `RADIO · DIRECT`, and the `OFFLINE PLANE` panel rendered `LINKS 1 · IN RANGE 1`. Screenshot only — the throwaway build was reverted, and no interaction was tested, because this machine cannot tap a simulator.

**The CoreBluetooth radio — written, reached, and not yet linked.**

`crates/cabal-ble-macos`. Dual role, GATT rendezvous for the PSM, L2CAP for everything after. All Objective-C on one serial dispatch queue; the Rust side touches only a mutex over bytes.

| Check | Result |
|---|---|
| Compiles; fourteen unit tests | ✅ |
| `backend::choose()` selects it on Apple platforms | ✅ |
| The delegate is reached; both managers report their state | ✅ |
| **Radio powers on, publishes an L2CAP channel, is assigned a PSM** | ✅ **on hardware** |
| **Service added; advertising started; scanning started** | ✅ **on hardware** |
| Asserted by test rather than eyeballed (`--ignored`) | ✅ |
| Two machines link and exchange a packet | ⏸ **blocked on a second Mac** |

The run:

```
BLE plane using CoreBluetooth
transport: bluetooth
BLE plane up transport="bluetooth" peer_id=711f9251a8435f4f
scanning for nodes
published an L2CAP channel psm=192
advertising
```

Every step a single machine can reach. The PSM is assigned by CoreBluetooth, so `psm=192` is the OS answering rather than a constant being echoed.

**One bug, found only by running.** The service UUID contained an `H`, which is not a hexadecimal digit. `CBUUID::UUIDWithString` does not return an error for that — it raises an Objective-C exception that killed the process from inside a delegate callback, with a stack trace naming KVO rather than the constant. No test and no type caught it; `is_valid_uuid` and four tests now do.

## Two devices linked over BLE — **GO** (2026-08-10)

Two Android emulators, `com.cabalmesh.ble` (`tauri-plugin-cabal-ble`), real BLE through the emulator's virtual controller.

| Check | Result |
|---|---|
| Both nodes publish an L2CAP channel and are assigned a PSM | ✅ `psm=139` / `psm=171` |
| Both advertise and both scan | ✅ |
| GATT rendezvous: discover, connect, read the PSM | ✅ `discovered a node; connecting` |
| **L2CAP channels open in both directions** | ✅ `link up 1`, `link up 2` on each |
| The Rust engine runs on the radio | ✅ `BLE plane up transport="bluetooth"` |
| **Announcements cross; each node identifies the other** | ✅ |
| The nodes screen shows it | ✅ screenshot |

The two nodes, from their own logs:

```
emulator-5554  BLE plane up transport="bluetooth" peer_id=7f9985898179db94
emulator-5556  BLE plane up transport="bluetooth" peer_id=4e78f1fe4396b85c
```

and from each other's nodes screen:

| | 5554 | 5556 |
|---|---|---|
| OFFLINE PLANE | `7f99..db94` · BLUETOOTH | `4e78..b85c` · BLUETOOTH |
| LINKS / IN RANGE / REACHABLE | 2 / 1 / 1 | 2 / 1 / 1 |
| NEARBY NODES | `4e78..b85c` — RADIO · DIRECT | `7f99..db94` — RADIO · DIRECT |

Each node names the other by the identifier that node logged for itself. Nothing here is a fixture.

**`LINKS 2` for one peer is correct, not a leak.** Both nodes scan and both connect, so each pair ends up with one outbound and one inbound channel. `SUPPRESSED 8` on the same screen is the consequence and the proof: every announcement arrives twice and the router drops the second, which is exactly what `router::Router::receive` is for.

**Two bugs, both found only by running:**

1. **A reconnect storm.** The GATT connection is dropped once the PSM has been read — that *is* the normal end of the rendezvous — and the disconnect handler released the device for reconnection. Every scan result then opened another channel: two nodes reached nine links in under a second. An address is now released only when a link goes down or the GATT exchange fails before a channel was attempted.

2. **A one-word boundary mismatch.** Kotlin's `invoke.resolve()` with no argument sends `null`; the Rust side expected a struct and refused it with `invalid type: null, expected struct Empty`. The radio started, advertised, scanned and opened links — and the app was told it was unavailable. Every layer worked except the sentence between them.

### The iOS Simulator cannot do this

It has no Bluetooth at all — CoreBluetooth is not virtualised, so no amount of code makes a simulator a second BLE device. The Android emulator does have a virtual controller (`netsim`/Rootcanal): `dumpsys bluetooth_manager` reports `state: ON` with addresses `BB:BB:BB:00:00:01` and `...:02`, and the two instances see each other.

```sh
# Two emulators, then:
adb -s emulator-5554 install -r -g app-universal-debug.apk
adb -s emulator-5554 shell pm grant com.cabalmesh.app android.permission.BLUETOOTH_SCAN
# ...ADVERTISE, CONNECT; repeat for emulator-5556
adb -s <device> logcat -s CabalBle:V
```

`-g` grants what is declared; the three Bluetooth permissions are runtime ones on API 31+ and a missing grant throws `SecurityException` at the first scan rather than failing to install.

Build with **JDK 21**. Gradle 8.14.3 cannot read the class files JDK 26 emits and fails with `Unsupported class file major version 70` before it reaches anything project-specific.

### Verifying the link between two Macs

```sh
# Bluetooth on, on both machines. Then, on each:
cargo run -p cabalmesh --example ble_node
```

No argument means the real radio; an argument means the loopback transport. Expect on both, additionally: `link up`, then `in-range=1`, then the peer's `hello from the terminal #N` lines.

**One machine cannot do it, and this was tested rather than assumed.** Two `ble_node` processes were run on one Mac: both advertised, both scanned, neither ever saw the other. A controller does not hear its own advertisements. No amount of code changes that.

`cargo test` cannot do it either — an unbundled process is refused Bluetooth by TCC — which is why the bring-up test is `#[ignore]`d and run explicitly:

```sh
cargo test -p cabal-ble-macos -- --ignored --nocapture
```

**`LoopbackTransport` is TCP.** It reports itself as `loopback` everywhere it surfaces, and the nodes screen prints that verbatim, precisely so no screenshot of it can be mistaken for Bluetooth. The radio reports `bluetooth`.

Background behaviour, MTU behaviour under load, and battery cost remain entirely unmeasured.

**Three bugs the tests found that reading did not**, recorded because each was invisible until something ran:

1. Whole-relay cancellation on a duplicate stranded five of twenty nodes in a line. Cancellation is now per-link.
2. Fanout subsetting dropped a third of a star's leaves. Only links another neighbour also covers may now be thinned.
3. A node counted **itself** as a two-hop peer, because every neighbour advertises it in their own neighbour list. Found by reading `reachable=2` from two real nodes where one was correct.

A fourth was found only by building for the simulator: `ble_status` was registered in the desktop invoke-handler arm and not the mobile one. It compiled, it was granted by the ACL, and it failed only on device. `tests/handler_arms.rs` now compares the two arms.

---

## iOS cross-compile — **GO** (2026-08-02, ticket 07)

The question the probe existed to answer: do `alloy` and `libp2p` cross-compile for arm64 iOS? If not, the dependency strategy has to change before any refactoring is worth starting.

**They do.** No patches, no forks, no vendored C.

| Check | Result |
|---|---|
| `cargo build --lib --target aarch64-apple-ios` | ✅ 1m 25s |
| `cargo build --lib --target aarch64-apple-ios-sim` | ✅ 58s |
| `alloy` 1.8.3 full contract/provider/signer stack | ✅ |
| `libp2p` 0.54.1 — tcp, mdns, noise, yamux, gossipsub | ✅ |
| `ring` (rustls backend) | ✅ |
| App bundle builds, installs, launches on iOS 17.5 simulator | ✅ |
| Process alive after launch, renders UI | ✅ |

Verified against the current unrefactored code, on Tauri 2.11.5 with the trimmed dependency set from ticket 02.

**What it does not prove.** The simulator renders the frozen desktop UI, because the mobile UI does not exist yet — panels overlap, text is clipped, the layout is plainly built for a wide window. That is expected and is the whole point of tickets 26–36. This probe answers "can it build and run", not "does it look right".

It also does not prove anything about the App Store: the installed Xcode is 15.4, and Apple has required Xcode 26 with a version-26 SDK for App Store Connect uploads since 2026-04-28. Simulator work is unaffected. See ticket 37.

### Repeatable simulator builds

`tauri ios build --target aarch64-sim` fails on a **second** run with:

```
failed to rename app .../cabalmesh_iOS.xcarchive/Products/Applications/CabalMesh.app:
Directory not empty (os error 66)
```

The message names the source path, but the non-empty directory is the *destination* — `build/arm64-sim/CabalMesh.app` left by the previous run. The Xcode build itself succeeds; only Tauri's post-build move fails, which makes it easy to misread as a compile error.

Use `npm run ios:sim`, which clears the previous output first. `npm run ios:clean-build` on its own if the state needs resetting.

### Install and launch by hand

```bash
npm run ios:sim
xcrun simctl install booted src-tauri/gen/apple/build/arm64-sim/CabalMesh.app
xcrun simctl launch booted com.cabalmesh.app
xcrun simctl io booted screenshot shot.png
```

---

## Security baseline — enforced and proven (2026-08-02, ticket 06)

The project shipped `csp: null` and `withGlobalTauri: true`, with the app's own
commands outside the ACL entirely. All three are now closed.

| Check | Result |
|---|---|
| Explicit CSP configured (`default-src 'self'`, no CDN sources) | ✅ |
| Webview renders identically under CSP — fonts, images, sprites | ✅ |
| `withGlobalTauri: false`; no `window.__TAURI__` usage anywhere in the frontend | ✅ |
| `freezePrototype: true` | ✅ |
| Shared `default.json` deleted; per-platform capabilities with explicit `platforms` | ✅ |
| AppManifest declares all **50** current commands (audit said 47 — the codebase grew) | ✅ |
| Desktop grants all 50; IPC works end to end | ✅ |
| Mobile grants `core:default` only — no app command reachable | ✅ |

`connect-src` is deliberately tight (`'self' ipc:`). The webview makes no
external requests: `src/avalanche-settlement.ts` would need the RPC host
allowlisted, but nothing imports it — it is dead code. Chain calls happen in
Rust, outside the webview's CSP.

### The ACL is genuinely enforced, not decorative

Removing a single permission and rebuilding proves the boundary is live rather
than nominally configured:

| Build | Wallet address in UI |
|---|---|
| `allow-get-identity` granted | `0xC24c...0B2e` shown in balance pill and onboarding chip |
| `allow-get-identity` removed | **absent from both**, everything else renders normally |
| restored | shown again |

One permission removed denied exactly one command and nothing else.

### Mobile grants nothing on purpose

An earlier pass granted mobile all 50 commands, reasoning that mobile still
serves the desktop frontend so it needs them. That was wrong: it hands a
surface with no screens the full command set — private-key export and raw
transaction submission included — so that a placeholder UI does not look
broken. Convenience during development is not a reason to widen an authority
boundary.

The mobile build's job is to prove the graph compiles, links, launches and
renders. IPC-dependent fields coming up empty is correct behaviour, not a
defect. The surface opens per screen from ticket 29 onward.

---

## Frozen IPC contract — baselined (2026-08-02, ticket 09)

`src-tauri/tests/ipc_contract.rs`, 23 snapshots, **0.02s**, no network, no
external binaries, no device.

Shapes rather than live output: most of the 50 commands need a reachable
Avalanche RPC, a running Ollama, the `nargo` binary or a live mesh, so their
runtime output is neither reproducible nor CI-safe. What the frozen UI depends
on is the serialized shape — field names, casing, enum tagging, how
optionality is represented — and that is what is pinned, from fixtures.

Covered: identity and wallet, marketplace and vouchers, deals, transaction
results, the relay queue, content, matching, ZK proofs, all 10 `MeshEvent`
variants, the two hand-built `serde_json::Value` payloads that no type
protects, and the 50-command inventory.

**Verified that it actually catches breakage.** Adding a single
`#[serde(rename)]` to one field failed exactly one snapshot; reverting went
green again. It detects the class of change that otherwise produces
`undefined` in the webview and no Rust error at all.

### Two things the baseline exposed

**Casing is inconsistent across the boundary.** `TxResult::Queued` serializes
`queueId` in camelCase, while its sibling `QueuedTx` uses `raw_tx_hex` and
`tx_hash` in snake_case. Both are now pinned. Whatever the reshaped API
settles on, the compatibility adapter has to keep emitting these exact spellings
for the frozen UI.

**Five modules became `pub`.** `agent`, `blockchain_bridge`, `matcher`, `mesh`
and `zk_handler` were private, which is a fiction: every type in them already
serializes to the webview, so they were public API in everything but the
keyword.

---

## Domain crate — extracted and property-tested (2026-08-02, ticket 10)

`src-tauri/crates/cabal-core`. 29 unit tests + 16 property tests, **0.07s**.

The constraint that makes it worth having: `serde` and `thiserror`, nothing
else. No `tauri`, `tokio`, `reqwest`, `alloy` or `libp2p`. That is why roughly
four thousand generated cases run in fifty milliseconds instead of behind a
multi-minute cross-compile and link. If something in there needs I/O, it
belongs in a different crate.

| Invariant | Why it matters |
|---|---|
| Terminal states never transition | A settled intent that could re-settle is money moving twice |
| Nothing returns to `Draft` | Broadcasting is irreversible |
| Only `Negotiating` may repeat | Every other self-loop is meaningless |
| Every live state can be cancelled | Otherwise the UI shows a cancel button that does nothing |
| Settlement requires routing | Settling from `Broadcast` means settling through a path never found |
| Active and terminal are disjoint | The two predicates drive different affordances |
| Amounts round-trip through display | A value the user typed and the app silently changed is a bug |
| Separators never change value | Users paste back exactly what the UI showed |
| Parsing arbitrary input never panics | Everything from the webview is hostile until parsed |
| Addition overflows rather than wraps | A wrapped total is a plausible-looking wrong balance |
| Mixing assets always fails | Adding AVAX to USDC is a bug, not a saturating op |
| USD always renders two decimals | The brand's number rules are exact, never approximate |

Two bugs the tests caught during writing:

- `NodeId::truncated` guarded on **byte** length while slicing by character,
  so a nine-character CJK identifier (27 bytes) was abbreviated when it should
  have been left whole.
- `prop_assert!` stringifies its expression into a format string, so an inline
  struct literal's braces break compilation. Struct values must be bound to
  locals first.

Verified after extraction: full workspace tests green including the 23 IPC
contract snapshots, clippy clean on the new crate, desktop app builds and
reaches the mesh, and the workspace still cross-compiles for `aarch64-apple-ios`.

---

## Legacy compatibility seam — in place (2026-08-02, ticket 11)

`src-tauri/src/legacy/` holds the 50 frozen commands, gated on
`cfg(all(desktop, feature = "desktop-legacy"))`. Handler registration is split
by surface: desktop gets the legacy arm, mobile gets an empty one.

| Check | Result |
|---|---|
| 50 commands moved with signatures byte-identical | ✅ |
| 23 IPC contract snapshots still green | ✅ |
| Builds with the feature on and off | ✅ |
| Legacy symbols present with gate on (40) / absent with gate off (1) | ✅ |
| Desktop builds, launches, completes bootstrap, reaches the mesh | ✅ |
| iOS build excludes legacy entirely and still launches | ✅ |

### A module, not a crate — and why

The plan called for a `cabal-legacy` crate. Not viable yet: these commands take
`State<'_, Arc<Mutex<AppState>>>` and return types that still live in the app
crate. A separate crate would either depend on the app crate — a cycle — or
need those types extracted first, which is tickets 17–24.

A feature-gated module gives the same seam today: one place to review, one flag
to disable, no leakage into the new surface. Extracting the crate becomes
mechanical once the services move.

### Desktop windows cannot be screenshotted from here

`screencapture` returns only the wallpaper and menu bar for this app. Verified
identical on the pre-ticket-11 baseline by stashing the change and recapturing,
so it is **not a regression** — it is the signature of missing Screen Recording
permission (macOS TCC). Simulator screenshots are unaffected because `simctl`
does not go through that path.

Consequence for every desktop-side ticket: visual verification is unavailable
until Screen Recording is granted to the terminal. Desktop claims here rest on
process liveness, bootstrap logs, the snapshot suite and symbol inspection —
all mechanical, none visual. Where a ticket needs a human eye on the desktop
window, that is called out rather than assumed.

---

## Error taxonomy — typed and redacting (2026-08-02, ticket 12)

`src-tauri/src/error.rs`. `AppError` serializes as a discriminated union tagged
on `kind`, so the frontend switches on a variant and renders its own on-voice
copy. The variant is the contract; the sentence is not.

Before, every command returned `Result<T, String>` built from `e.to_string()`,
which has two costs. The frontend could only display prose — no branching, no
on-voice copy, no localisation. And it leaked: an I/O error's `Display`
contains the filesystem path, a transport error's contains the RPC URL, and
both travelled to the webview.

**Redaction is enforced by test, not by convention.** `no_variant_leaks_infrastructure_detail`
serializes every variant — including one built from an error containing a vault
path, an RPC URL and a hex key — and asserts none of `/Users`, `http`, `://`,
`.network`, `0xdeadbeef` or `vault.enc` survives. That is the test that fails
if someone later adds a `message: String` field "just for debugging".

`AppError::Chain` deliberately has no message field at all, only a `retryable`
flag. `AppError::Internal` is unit-shaped: `AppError::internal(source)` logs
the real error and returns a variant carrying none of it.

37 legacy call sites now flatten through `legacy::adapt::flatten_error` rather
than inline `e.to_string()`, making the compatibility seam real rather than
notional. The 23 frozen-contract snapshots are unchanged by that move, which is
the proof it is behaviour-preserving — the whole requirement for a
compatibility layer.

Test count across the workspace: **84**.

---

## Diagnostics on device — working (2026-08-02, ticket 13)

79 `println!`/`eprintln!` calls became `tracing`. On a desktop terminal those
were merely untidy; on a device they were **invisible** — nothing written to
stdout from an iOS app reaches Console.app.

**Proven on the iOS simulator**, not merely configured:

```
CabalMesh: [com.cabalmesh.app:default] diagnostics initialised  subsystem="com.cabalmesh.app"
CabalMesh: [com.cabalmesh.app:default] Checking connection...  phase="PHASE_1_SYNC" progress=10
CabalMesh: [com.cabalmesh.app:default] ephemeral peer id generated  peer_id=12D3KooWEtgdSP1H…
CabalMesh: [com.cabalmesh.app:default] listening  address=/ip4/192.168.2.111/tcp/59365
```

Read it with:

```sh
xcrun simctl spawn booted log stream --predicate 'subsystem == "com.cabalmesh.app"'
```

| Platform | Destination |
|---|---|
| iOS | unified log — Console.app or `simctl … log stream` |
| macOS | unified log **and** stderr (a Finder-launched bundle has no visible stderr) |
| Android | logcat, `adb logcat -s cabalmesh` — untested until ticket 08 |
| Linux / Windows | stderr |

Severity was preserved rather than flattened: the codebase used emoji as
severity markers, so ❌ became `error`, ⚠️/🚨 became `warn`, bare `eprintln!`
became `warn`, and the rest `info`.

Spans on `sync_state`, `create_escrow` and the mesh event loop mean every line
inside them is attributable:

```
INFO sync_state{wallet_address_override="…" rpc=https://…}: fetching native AVAX balance
```

`skip(self)` keeps the bridge — which holds signers — out of span fields.

Default filter is `cabalmesh=info,…,warn`: libp2p and alloy at debug scroll a
device log faster than it can be read, which is the same as no log.
`RUST_LOG` overrides.

`AppError::internal` now records the full `source()` chain as
`outer: middle: root`, since the root cause is the useful part and is exactly
what `Display` on the outermost error discards. **Logs may contain paths and
URLs; return values may not** — that asymmetry is the design, not an
oversight.

Test count: **85**.

---

## State and capabilities — reshaped (2026-08-02, ticket 14)

The global `Arc<Mutex<AppState>>` is gone. Every command used to lock it, lock
a second mutex inside, then await network I/O holding both: two concurrent RPC
calls ran strictly one after the other. Commands now take a cheap `Services`
snapshot and release the lock before awaiting anything.

**Asserted by timing, not by inspection** — `tests/state_concurrency.rs` runs
8 tasks of 150 ms each and fails if the total approaches the 1,200 ms a
serialized run would take. "No global mutex" is easy to claim from a diff and
easy to lose again in a later refactor.

State is now managed **synchronously**, before the webview exists. Previously it
was managed inside a spawned task, so a command arriving during bootstrap found
nothing managed — and a missing `State<'_, T>` is a panic inside the IPC
handler, not an error a command can convert. It is now `AppError::NotReady`,
which is the state the connecting screen already renders as progress.

`PlatformCaps` (build-time, `Copy`) and `RuntimeCaps` (permissions,
connectivity) are separate types. An earlier design had one struct described as
build-time immutable while carrying a permission grant — a contradiction,
because a user can revoke Local Network access while the app is backgrounded.
Conflating them means that revocation is never noticed and mDNS silently stops
finding peers.

### How state resolution was actually verified

The mock runtime cannot prove it: the ACL runs *before* state resolution and
`mock_context` has no resolved capabilities, so every invoke is refused before a
command body runs. `tests/ipc_wiring.rs` was rewritten to assert what it does
prove — that the ticket 06 ACL is enforced on the real invoke path.

Proof came from running the app instead. Bootstrap and the frontend both call
`sync_state`, distinguishable by argument:

```
sync_state{wallet_address_override="ignored_override"}   <- bootstrap
sync_state{wallet_address_override=""}                   <- frontend, over IPC
```

The second line is a frontend command resolving `State<'_, AppState>` and
executing. That is the regression this ticket was most at risk of.

### A trap worth knowing about

**The debug binary is built with `cfg(dev)`, so it loads `devUrl`, not the
bundled frontend.** Run `target/debug/cabalmesh` without Vite serving port 1420
and the webview loads nothing — bootstrap logs look perfect while *no* frontend
command ever runs.

That cost real time here: it looks exactly like a broken IPC layer or a
regression from the CSP work. Before concluding the frontend is broken, check
that `npm run dev` is running.

### A `cfg` bug the desktop build could not catch

`kill_switch` lives in an always-compiled module but called
`crate::legacy::adapt::flatten_error`, and `legacy` is `cfg(desktop)`-gated. It
compiled on desktop and failed only on `aarch64-apple-ios-sim`. `error::flatten`
is now the unconditional home and `legacy` delegates to it.

This is the argument for cross-compiling every ticket rather than at the end.

Test count: **98**.

---

## Stream lifecycle — leak-proof (2026-08-02, ticket 15)

Tauri `Channel`s do not clean themselves up. A channel releases its frontend
callback only when the producing side sends an end message; there is no
unsubscribe in the JS API, and releasing a JS callback would not stop a Rust
task regardless. Without explicit teardown, every visit to a streaming screen
leaves a live producer, and a user tapping between tabs accumulates them.

`src/subscriptions.rs` registers each stream against a cancellation token.
`commands::unsubscribe` is the first command on the reshaped surface — returning
`AppError`, not `String` — and is registered on **both** platform arms.

**The headline check runs 100 subscribe/cancel cycles with real spawned
producers** and asserts the registry is empty *and* no task is still alive.
That loop is exactly what tab-switching performs.

Cancellation is verified to stop *emission*, not just bookkeeping: removing a
registry entry while the task kept producing would pass a length assertion and
still drain the battery.

### Cancel stops delivery, not the operation

Enforced structurally rather than by discipline — a registered task is *only* a
delivery loop, and the operation it reports on holds no token from here.

| Stream | cancel stops | cancel does **not** stop |
|---|---|---|
| mesh log | delivery | nothing; delivery is all it does |
| handshake | delivery | the mesh join |
| settlement proof | delivery | **the settlement** |

That last row is why this is structural. A UI navigation must never cancel a
transaction.

### Two cases that only show up in practice

**Unmount before subscribe resolves.** Fast tab switching runs teardown before
the registering invoke has returned. Cancelling an unknown handle is `Ok`, and
the late registration is still cancellable.

**Suspension.** `cancel_all` stops every stream at once, so a backgrounded app
is not producing into a webview that cannot receive.

The limit (32) is a tripwire, not a capacity plan: the UI needs one stream per
visible screen, so hitting it means a screen is not tearing down. Exceeding it
returns a typed error rather than evicting someone else's stream, which would
surface as a screen that mysteriously stops updating.

The registry lives on state from **construction**, not from bootstrap — the
connecting screen subscribes to the handshake log before services are
published.

Test count: **113**.

---

## Storage — crash-safe and sandbox-correct (2026-08-02, ticket 17)

`crates/cabal-store`. 11 tests. Two problems fixed.

**Paths were discovered, not injected.** Storage called `dirs::data_dir()`, a
desktop convention with no correct answer inside a mobile app sandbox, and no
way for a caller to correct it. The path now comes from Tauri's platform
resolver, is set once at startup, and is read everywhere else. `dirs` is
removed from the dependency tree.

**Writes were not atomic.** A mobile process is killed without warning, and a
truncated `identities.json` is an unrecoverable wallet loss, not a cache miss.
Writes now encode first, then go to a sibling temporary file, flush, `sync_all`,
and rename over the target. Each step is load-bearing: encoding first means a
serialization failure cannot truncate good data; `sync_all` before the rename
means a power loss cannot make the rename visible while the contents are not;
and the temporary file is a sibling so the rename stays on one filesystem,
where it is atomic.

`load` fails loudly on corruption; `load_or` falls back to a default. The split
is deliberate and documented at the call site: caches and queues cost a refresh,
a wallet must never silently become empty.

### The migration this uncovered

Moving to the platform directory renamed the folder from `cabalmesh` to
`com.cabalmesh.app`. Caught at runtime — the app generated a **brand-new
identity** while the real wallet sat orphaned in the old location:

```
🆕 Generating NEW Identity 'Genesis Fox'
   Identity: 0x6db089712FE0264a0ff2B7fE0Baa5F81189204C9    <- fresh, empty
   (real wallet 0xfF8dd6db… still in ~/…/Application Support/cabalmesh)
```

That is precisely the data loss this ticket exists to prevent, introduced by
the fix for it.

The directories are siblings on every desktop platform, so the old one is found
without reintroducing path discovery. Migration is conservative: it **copies
rather than moves** (the old directory holds private keys — deleting it is the
user's call), runs **only when the destination has no identities**, and is best
effort per file.

Verified by clearing the new directory and relaunching:

```
adopted state from the pre-move data directory … copied=3
Identity: 0xfF8dd6dbB7B97b44044573cFE843dE1F463637a9    <- original wallet
```

Test count: **124**.

---

## Vault — private keys encrypted at rest (2026-08-02, ticket 18)

Private keys were **plaintext hex in a JSON file**. Anything that could read
the app's data directory — another process, a backup, a synced folder, a
support bundle — had the wallet. Meanwhile the vault screen's own copy promises
*"HELD LOCALLY. NEVER SYNCED"*.

`crates/cabal-vault`: AES-256-GCM, fresh random nonce per write, key supplied by
a pluggable `KeyProvider`.

Verified on the real wallet:

| Check | Result |
|---|---|
| `identities.json` removed, `vault.enc` written | ✅ |
| Private key absent from the ciphertext (grep = 0) | ✅ |
| Key file mode `-rw-------` | ✅ |
| Restart loads the same address `0xfF8dd6db…` | ✅ |
| Wrong key / tampered byte → refused, not garbage | ✅ (GCM authenticates) |
| Identical plaintext → different ciphertext | ✅ (nonce reuse would break GCM) |

**Migration verifies before it destroys.** It writes the vault, decrypts it
back, compares, and only then unlinks the plaintext. A failure leaves the old
file exactly where it was — proven by a test using a provider that always fails.
After ticket 17's near-miss, the ordering here is not incidental.

Two paths now fail loudly rather than helpfully: a vault that exists but will
not decrypt, and a corrupt key file. Both previously would have fallen through
to "generate a fresh wallet", which turns a recoverable problem into permanent
loss of funds.

`Secret` redacts in both `Debug` and `Display`, so the real accident — logging a
*struct* that happens to contain a key, not the key itself — is prevented by
type rather than by memory.

### What is not done

**The device key store is not wired.** iOS Keychain and Android Keystore need
the native plugin. Until then mobile uses the same file-backed key inside the
app sandbox — meaningfully protected (no other app can read it) but not
hardware-backed, and it warns at startup. Desktop has no uniform key store since
`keyring` was removed, so it uses the same mechanism: a real improvement over
plaintext, and honest about being weaker than a keychain.

Test count: **145**.

---

## Presentation contracts — generated, not hand-written (2026-08-02, ticket 16)

`src-tauri/src/bindings.rs` → `src/types/bindings.ts`, via `npm run bindings`.
`npm run bindings:check` fails on uncommitted drift. Regeneration is
deterministic — verified by regenerating and diffing.

**No colour crosses the boundary.** The prototype passed colours as data
(`dot: BLUE`, `deltaColor: GREEN`), which hard-codes the palette into Rust and
makes a design change a backend release. Rust now sends a semantic *tone* whose
domain matches the design system's props exactly — `StatusTone`, `LogTone`,
`DeltaTone`, `ToastTone` — and the hex mapping lives in the design system alone.

Guarded by test: every presentation payload is asserted to contain no `#`, no
palette hex, and no colour name.

**Numbers are formatted once, in Rust.** The brand demands exact separated
figures and forbids approximations, so `separated()` is the single
implementation rather than one per screen.

`StatTile::plain` omits the delta entirely rather than emitting `+0.0%` — a
fabricated trend for an unmeasured figure would violate the same exactness
rule. `INTENTS SETTLED` uses exactly this shape until a prior week exists to
compare against — a percentage change from zero is undefined, not infinite.

Test count: **150**.

---

## Mesh actor — bounded and answerable (2026-08-02, ticket 19)

`libp2p::Swarm` is not `Sync`, so it already lived in one task. What it lacked
was a handle: requests went in on an **unbounded** channel with no reply.

Two consequences, both fixed:

**An unbounded send to a dead receiver succeeded silently.** Callers believed
intents were broadcast when they had been dropped. `MeshHandle::publish` now
awaits the actor's acknowledgement, and a stopped actor is
`MeshError::ActorGone` rather than a false success.

**A caller that outran the actor grew the queue until the process died.** On a
2 GB phone that arrives quickly. The channel is bounded at 32 — a request
queue, not a buffer — so the same situation becomes backpressure. Asserted by
test: a full queue refuses rather than growing.

`snapshot()` and `set_offline()` are answered from inside the event loop, so
they reflect what the swarm is actually doing rather than state tracked
alongside it. `set_offline` deliberately does **not** tear the swarm down:
rebuilding on resume would drop every established connection and re-run
discovery from nothing. It also refuses publishes while offline, honouring the
switch's promise that nothing leaves the device at the actor rather than
trusting each caller to check.

Verified with two live nodes: mDNS discovery, presence broadcast on discovery,
and the full frontend → `MeshHandle` → gossipsub path.

Test count: **155**.

---

## Transport — mobile-viable (2026-08-02, ticket 20)

TCP with mDNS is a room-sized mesh. Two users on different networks never meet,
which for a mesh product is not a limitation but the absence of the product.

**QUIC alongside TCP.** Verified listening on both:

```
listening address=/ip4/192.168.1.122/tcp/65118
listening address=/ip4/192.168.1.122/udp/61023/quic-v1
```

QUIC earns its place on mobile specifically: connection migration survives a
Wi-Fi→cellular handoff that kills a TCP socket, and 0-RTT resumption makes
returning from background cheap. A network that blocks UDP degrades to TCP with
a warning rather than failing.

Added `identify` (a precondition for relay reservations and hole punching),
`ping` (liveness, and keeps NAT bindings warm on cellular), `relay` client, and
`dcutr` to upgrade a relayed connection to direct — so the relay carries
handshakes rather than the whole mesh.

**mDNS is now `Toggle`-wrapped.** Local discovery is a *permission* on both
mobile platforms, not a capability. A node that failed to start because the user
declined a prompt would be worse than one that quietly falls back to bootstrap
peers.

### No placeholder relay ships

`BootstrapConfig::default_relays()` is deliberately **empty**, asserted by test.
A fake address produces dial failures that read as bugs, and inventing one would
be worse. Until ticket 23 deploys a relay, the app logs

```
no bootstrap relays configured — discovery is limited to this network
```

and says so honestly rather than appearing broken. A user override *replaces*
the list rather than appending — pointing the app at your own relay usually
means only that one. A missing config field still deserializes, because the
relay address will move at some point and an older file must not become
unloadable when it does.

Verified: two nodes discover each other over mDNS with both transports live;
iOS cross-compiles with QUIC, relay and dcutr.

Test count: **160**.

---

## iOS local-network permission — in the built app (2026-08-02, ticket 21)

`src-tauri/Info.plist`, merged via `bundle.iOS.infoPlist`. Verified present in
the **built** bundle, not merely in the source:

```
"NSBonjourServices" => [ "_p2p._udp", "_p2p._tcp" ]
"NSLocalNetworkUsageDescription" => "CabalMesh discovers nearby nodes on your local network. No identity is attached."
```

It lives in `src-tauri/` rather than `gen/apple/` because that tree is
regenerated by `tauri ios init`, which would silently drop the keys — and their
absence does not error. iOS just stops delivering mDNS, discovery returns zero
peers, and every other part of the mesh looks healthy.

`minimumSystemVersion` pinned to 14.0 (also Tauri's default) — the floor where
the local-network prompt exists. `minSdkVersion` 24 and
`autoIncrementVersionCode` set for Android.

Export compliance is deliberately **not** declared. Ticket 05 answers the
questionnaire and sets the value from its outcome; guessing `true` obliges a
compliance code we do not hold.

### Android half is prepared, not applied

`src-tauri/gen/android/` does not exist until ticket 08, and there is no
pre-init hook. `docs/android-permissions.md` holds the exact manifest block and
the multicast-lock plugin, to apply immediately after that init.

Worth knowing before then: Android drops multicast silently without a
`WifiManager.MulticastLock`. Manifest permissions **and** the runtime lock are
both required; either alone leaves discovery dead with no error. A green build
proves nothing here — the check is two physical devices discovering each other.

Test count: **160**.

---

## Lifecycle — native, no plugin (2026-08-02, ticket 22)

An earlier plan specified a custom lifecycle plugin. It was written against
2.9, where nothing propagated. **2.11 emits `WindowEvent::Suspended` and
`WindowEvent::Resumed` on mobile**, so the plugin was obsolete before it was
written — and the plugin count dropped from four to three.

Verified on the simulator by backgrounding and returning:

```
[com.cabalmesh.app:default] suspended  cancelled_streams=0
[com.cabalmesh.app:default] resumed
```

Confirmed against the Tauri 2.11.5 source, since the changelog names the tao
events rather than the public path:

| | Suspended | Resumed |
|---|---|---|
| Android | `Activity.onPause` | `Activity.onResume` (first ignored) |
| iOS | `applicationWillResignActive` | `applicationWillEnterForeground` |
| Desktop | not emitted | not emitted |

### The iOS asymmetry is real, and shapes what teardown may do

Those two callbacks are **not** mirror images. `willResignActive` fires for
transient interruptions — control centre, notification shade, incoming call, a
permission prompt — while `willEnterForeground` fires only after genuinely
leaving the background.

So pulling down the notification shade delivers `Suspended` with **no matching
`Resumed`** until the app is actually backgrounded and returned to. Anything
torn down on suspend must therefore be cheap to lose and rebuilt on demand, not
only on resume — otherwise glancing at a notification silently kills the mesh
for the rest of the session.

Teardown is bounded accordingly: cancel live streams, stop mesh participation.
Both re-establish on the next subscribe or publish, so the transient case
self-heals. The swarm is deliberately left running.

Resume re-reads runtime capabilities rather than assuming them: a user can
revoke Local Network access from Settings while backgrounded, and an app that
kept believing it was granted would silently find no peers and blame the
network. Streams are **not** restarted — the frontend re-subscribes when screens
remount, and guessing would recreate streams nothing is listening to.

Test count: **162**.

---

## Chain config and offline queue (2026-08-02, tickets 24 + 25)

**Contract addresses came from bare `std::env::var` with no fallback.** On
desktop a dotfile made that work. On mobile there is no environment to read and
no file to load, so every address resolved to `None` and every contract call
failed — with an error that looked like a chain problem rather than a
configuration one.

Now a compiled-in table keyed by network, overridable at runtime, with the
desktop environment layer retained for the local two-node test.

**The default is Fuji, not mainnet.** This build is still moving and the
contracts are unaudited; a wrong default here spends real money rather than
displaying something wrong. Undeployed contracts are `None` rather than a
placeholder, so the failure reads "not configured" instead of surfacing as a
chain error.

### The queue drains itself now

The existing path relied on a *peer* with Relay Mode picking transactions up —
which never happens for a user who is simply alone and offline. `drain_pending`
is the self-service path: when the device regains connectivity it submits its
own queue.

Retries are bounded at 5. Retrying forever would drain the battery
re-broadcasting a transaction the chain will never accept, since a bad nonce or
an underpriced fee does not improve by being tried again. The attempt count is
**persisted**, so a relaunch does not reset the counter and restart the loop.

### Extending the frozen type without breaking it

`QueuedTx` gained `attempts`, and the frozen desktop UI must see exactly the
shape it always saw. `#[serde(default, skip_serializing_if)]` means an untried
entry serializes identically to before, and a queue file written before the
field existed still loads — real installations have one, and failing to read it
would drop transactions the user is waiting on.

**The 23 contract snapshots stayed green through the change**, which is the
proof rather than the intention.

Test count: **167**.

---

## Frontend build split (2026-08-02, ticket 26)

Two output **directories**, not two entry files in one:

```
index.html            -> dist-desktop/   (frozen RPG UI, Tailwind + framer-motion)
src/mobile-entry/     -> dist-mobile/    (design-system UI, neither)
```

`frontendDist` names a directory and Tauri always serves the `index.html`
inside it, so the original two-entries-in-one-folder design would have loaded
the **desktop UI on the phone**.

**Verified by screenshot, not by inspection.** iOS now renders the mobile shell
— no RPG sprites, no Tailwind. Desktop still bundles the RPG UI with its
Tailwind CSS intact.

| | dist-mobile | dist-desktop |
|---|---|---|
| size | 196 KB | 1.9 MB |
| Tailwind in CSS | none emitted | present |

### Three traps avoided

**Vite resolves `outDir` relative to `root`.** With a nested mobile root, a bare
`dist-mobile` lands in `src/mobile-entry/dist-mobile` and the Tauri overlay
points at nothing. Both outputs use absolute paths.

**Both `beforeDevCommand` and `beforeBuildCommand` are overridden** in each
platform overlay. Overriding only the build command means `tauri ios dev`
starts the desktop root and serves the desktop UI — the dev-mode twin of the
`frontendDist` bug, and just as easy to misread as a routing problem.

**Tailwind's `content` glob is scoped to the frozen tree**, never `src/**`.
Tailwind compiles to the raw px and hex values the adherence lint forbids, so
one generated utility reaching a design-system surface reintroduces exactly the
collision this split prevents.

Also fixed the HMR port: it reused the dev-server port, so HMR never attached on
a physical device. Now 1421, matching the Tauri template.

The mobile viewport carries `viewport-fit=cover` and deliberately **no**
`user-scalable=no` — locking zoom fails WCAG 1.4.4, and an accidental pinch is a
far smaller cost than an unusable app.

---

## Design system vendored (2026-08-02, ticket 27)

`src/ds/` — 25 component files, 32 exports, 162 tokens, 14 glyphs.
**Verified rendering on the simulator**: Silkscreen wordmark at `0.42em`
tracking, IBM Plex Mono body, pure black ground, safe area respected. No
network involved.

### Fonts self-hosted — the CDN would have been fatal

The delivered `fonts.css` opened with an `@import` from Google Fonts. Under this
project's CSP a Tauri webview cannot fetch that, and the premise is *operating
offline* — first launch without network would render in fallback
`ui-monospace`, losing the brand's most defining property exactly when it
matters.

Latin subset only: 4 faces, **32 KB**, against the 7 weights × 8 subsets the CDN
import pulled. `grep` for `fonts.googleapis` in the built CSS returns 0.

### The un-bundling broke the app first

Splitting the bundle left `Object.assign(__ds_scope, …)` — the bundle's own
registration — in 6 form components. `__ds_scope` no longer exists, so it threw
a `ReferenceError` at module load and took down **the entire bundle**, not just
those components. The device showed a black screen with correct tokens and no
content, which reads as a CSS problem rather than a JS one.

`scripts/unbundle-ds.py` now strips it and **hard-fails** if any scaffolding
survives, rather than emitting code that throws at runtime. Bundle also dropped
242 KB → 195 KB, since the broken code was dead weight.

### Glyphs resampled

264×264 board crops rendered at 20px is a 4.4× downscale, and `pixelated` is
correct for *up*scaling and destructive downscaling — thin strokes drop out and
shimmer while scrolling. `scripts/resample-ds-assets.py` produces exact
@1x/@2x/@3x variants with a Lanczos filter, so the browser never resizes.

### Types from the adherence lint

The design system shipped no `.d.ts`, but its lint config encodes the same
information as regexes. `scripts/generate-ds-types.py` turns 25 components and
22 enumerated prop domains into types, so `<Button tone="blue">` is a compile
error rather than a lint warning nobody reads.

All three scripts are in the repo, so replacing the vendored tree is a command
rather than an archaeology exercise.

---

## Shell and accessibility foundation (2026-08-02, ticket 28)

Verified on the simulator: header with the Silkscreen wordmark clear of the
status bar, real protocol glyphs in the tab bar, `HOME` selected with the white
underline at full opacity against 0.34 for the rest, and the home indicator
region reclaimed.

### Accessibility decisions that were cheap now and expensive later

**Type scale is unclamped.** `--type-scale` comes from the OS setting with no
ceiling. An earlier revision capped it at 130%, which was a decision to fail
WCAG 1.4.4 rather than a resolution of it — the criterion does not forbid a 9px
base, it forbids a layout that cannot grow.

**Nothing has a fixed height.** Header, tab bar and rows use `minHeight`. A
fixed height clips descenders the moment the scale rises, which is exactly what
supporting 200% requires avoiding.

**The tab bar leads with glyphs.** Five long uppercase labels in 390px at 200%
is roughly 2.5× the available width. The board treats glyphs as primary, so the
icon carries meaning and `aria-label` carries the name; labels clip rather than
wrap. No overflow menu — that would hide primary destinations.

**Roles and states are explicit.** `role="tablist"`/`role="tab"` with
`aria-selected`, because the white underline is a *visual* selected state that
reaches no assistive technology.

**Hover is gated to real pointers.** `:hover` sticks after a tap in a mobile
webview, and the brand's hover inverts a button — leaving it stuck white-on-black.

**Screen state is typed so illegal navigation is unrepresentable.** `detail` and
`settled` carry an `IntentId` in the type, so "open detail with nothing loaded"
cannot be expressed. Hardware back binds to the same `back()` the header uses,
so the two cannot disagree.

### Two bugs the device caught that inspection would not

**Glyphs rendered as broken images.** `Icon` builds `{basePath}/{name}.png` and
the base path was missing its `icons/` segment.

**`Icon` has no srcset**, so whatever sits at the plain filename is what every
density gets — and that was the 264×264 original. `scripts/stage-ds-assets.py`
now puts the **@3x variant** at the plain name: at 20 CSS px on a 3× device that
is 60 device pixels from a 60px source, exactly 1:1 with no resampling, which is
what `pixelated` is actually good at. Originals stay in `src/ds/assets` so
resampling can be redone.

---

## First screens live (2026-08-02, ticket 29)

**Splash** and **connecting**, both verified on the simulator.

Splash renders the minimal logo mark, the wordmark in Silkscreen at `0.42em`
tracking with its matching text-indent (the board specifies both together —
tracking alone pushes it off-centre), the primary and ghost buttons, and the
board's copy verbatim: *"Zero identity. Private intents."*, *"The nobody
network."*

Connecting is the first **real end-to-end stream**: Rust command → Tauri
`Channel` → React → the design system's `Terminal`.

```
CONNECTING TO MESH
NO IDENTITY IS ATTACHED.
[████████░░░░░░░░░░░░]  40%
> INITIALIZING EPHEMERAL NODE...
> GENERATING ONE-TIME KEYPAIR...
> NO IDENTITY WRITTEN.
> ▌
```

Tones map correctly (`dim` vs `out`), the meter advances in `steps()` rather
than easing, and the caret blinks on the 3-step cycle. The final `ok` line
advances to home — observed by screenshotting at 6s and finding the app already
there.

The line buffer is bounded at 200. A live feed into an unbounded array is a slow
OOM on a 2 GB device, and the prototype's `setInterval` over a canned array
hides that entirely.

**Mobile's IPC surface opened for the first time**, and only by three:
`unsubscribe`, `session_status`, `enter_mesh`. That is the per-screen grant
discipline working — nothing speculative.

---

## Home screen, on real data (2026-08-02, ticket 30)

Mesh status panel with corner registration ticks, live stat tiles, and the log
ticker — every figure from the running mesh, none canned.

```
MESH STATUS
■ MESH UNREACHABLE · OPERATING OFFLINE
NODE ID     12D3..W2CE
UPTIME              0M

NETWORK NODES   0
RELAYED BYTES   0
REPUTATION SCORE —
```

### The reputation tile renders an em dash

> **Historical — the field no longer exists.** Ticket 39 replaced it with
> `INTENTS SETTLED`, a real count. Kept because the reasoning below is why the
> em dash was right at the time, and that reasoning is what eventually produced
> a real signal instead of a plausible-looking number.

Ticket 03 has not resolved where a reputation score would come from. The
prototype shows `87.6 (+5.3%)`, which is a constant. Rendering it would be a
fabricated trust signal in a product whose entire pitch is proving things, and
the brand's own copy rules demand exact figures.

`StatTile::plain` omits the delta rather than emitting `+0.0%`, for the same
reason. **This screen is finished; the number is not.** It becomes real the
moment ticket 03 names a source.

### A bug the device caught

`NODE ID` first rendered `/ip4..8299` — a truncated *listen address*. The
extraction looked for a `/p2p/` component, which listen addresses do not carry,
so it silently fell through to the address itself. The peer id now comes from
the swarm directly.

That is the kind of thing that type-checks, runs, and produces plausible-looking
output — and only a screenshot catches it.

---

## Android — toolchain, build, and four device-only bugs (2026-08-03, tickets 08 + 21)

The suspicion recorded here — that "the iOS result is encouraging but does not
transfer" — was right, and understated. Android found **four** defects that
every desktop and iOS build had been green through.

### Toolchain

Installed headlessly; no Android Studio involved.

| Component | Version |
|---|---|
| Command-line tools | 11076708 |
| Platform | android-34, compiled against 36 |
| Build-Tools | 34.0.0 |
| NDK | 27.0.12077973 |
| Platform-Tools | 37.0.1 |
| JDK | **Temurin 21.0.12** |
| Gradle | 8.14.3 (vendored by the generated project) |

**The JDK version is not a free choice.** The machine's only JDK was Temurin 26,
and Gradle 8.14.3 rejects it outright:

```
BUG! exception in phase 'semantic analysis' in source unit '_BuildScript_'
Unsupported class file major version 70
```

Class file 70 is Java 26. A 21 LTS was installed into
`~/Library/Java/JavaVirtualMachines/` — user-level, no sudo — and `JAVA_HOME`
points at it for Android builds only.

The four Rust triples are in `rust-toolchain.toml`, so rustup installs them
without anyone remembering to.

### Bug 1 — rustls has no trust store on Android

The first HTTPS request panicked:

```
thread 'tokio-runtime-worker' panicked at rustls-platform-verifier-0.7.0/src/android.rs:90:10:
Expect rustls-platform-verifier to be initialized
```

reqwest 0.13 wires `rustls-platform-verifier` into **every** rustls feature —
`rustls`, `rustls-no-provider`, both — and there is no webpki-roots alternative
to select. So it cannot be avoided, only initialized. That needs two halves: the
verifier's Kotlin component in the Gradle build, and a Rust call handing it a
`Context`. Either alone still panics.

Two things about this were only learnable by running it:

- **`ndk_context` is never populated in a Tauri app.** It is the documented way
  to reach the JVM from Rust, and it does not work here: tao's Android glue
  keeps the VM and activity in private state and never calls
  `initialize_android_context`. `android_context()` panics with "android context
  was not initialized" — it cannot even be null-checked. The Context is
  therefore pushed *in* from Kotlin at plugin load (`TlsPlugin`), which is the
  first moment one exists.
- **`latest.release` does not resolve** against the bundled Maven repository,
  which ships no `maven-metadata.xml`. The version is read from `cargo metadata`
  alongside the path, so neither is written down twice.

The panic was on a background thread, which is the nasty part: the app launched,
the mesh came up, and only the balance was missing. It read as a network
problem.

Verified fixed — `✅ [Bridge] Fetched balance for 0x04465C31…` over real HTTPS
to `api.avax-test.network`.

### Bug 2 — the swarm would not build, for want of `/etc/resolv.conf`

```
Mesh Failed: io error: No such file or directory (os error 2)
```

libp2p's `.with_dns()` reads the system resolver configuration and treats
failure as fatal. Android has no `/etc/resolv.conf` — DNS goes through `netd` —
so the **entire swarm** failed to construct. Nothing in that message says DNS,
and the mesh was dead over a resolver it had nothing to resolve with: the relay
list is empty until ticket 23.

Now read explicitly, with a fallback (Cloudflare, not the crate default of
Google — this app argues about privacy). Verified: swarm active, listening on
TCP and QUIC.

### Bug 3 — `env(safe-area-inset-*)` is not the whole story on Android

The tab labels rendered underneath the gesture pill.

Android's WebView reports **only a display cutout** through
`env(safe-area-inset-*)`. The status bar and the gesture pill are *window
insets*, which it does not surface at all, so `safe-area-inset-bottom` is `0px`
on every phone without a notch. iOS reports the home indicator there; Android
does not.

`MainActivity` now publishes the real insets as `--android-inset-*`, which
`mobile.css` folds in with `max()`. Each platform contributes what it knows and
nothing downstream is special-cased.

**The first attempt looked like it worked and did not.** Insets arrive during
the first layout pass, *before Tauri navigates*, so `evaluateJavascript` ran
against `about:blank` and the inline style was discarded by the navigation that
followed — indistinguishable from insets that never applied. Diagnosed by having
the injected script return `document.readyState + location.href`:

```
CabalMeshInsets: top=51 bottom=24 readyState="complete about:blank"
```

Registered as a document-start script instead, so it runs for the real document
and every reload after it.

### Bug 4 — Gradle's inset listener hands back a `View`, not a `WebView`

Caught by the compiler (`Unresolved reference: evaluateJavascript`), noted only
because it is the one of the four that a build could catch.

### Multicast lock — the other half of ticket 21

The manifest permissions are declared and the lock is acquired when the mesh
starts participating, released on suspend. Confirmed on the emulator:

```
cabalmesh::state: runtime capabilities changed mdns_granted=true relay_reachable=false online=false
```

`mdns_granted` had never been `true` on any platform before this — nothing wrote
it. `LocalNetwork` is deliberately three-valued so iOS is not made to *claim* a
permission it has no API to query: `NotApplicable` leaves the flag alone rather
than asserting a grant nobody checked.

Not proven here: two physical devices discovering each other. An emulator is one
host on a virtual network, so a green multicast lock is a granted permission, not
a working discovery.

### What the screens look like

Splash, home, and profile render correctly — Silkscreen, protocol glyphs, the
blood-red `AVALANCHE FUJI` badge with `TEST FUNDS ONLY.`, real peer id
`12D3..9gPP` matching the log line, safe areas respected top and bottom.

`REPUTATION SCORE` and `MEMBER SINCE` are em dashes for the same reason they are
on iOS: ticket 03 has not named a source.

**Superseded 2026-08-03.** Both now render, and both are real. The reputation
score is gone entirely — ticket 39 replaced it with `INTENTS SETTLED`, a count
from the intent ledger (`src-tauri/src/standing.rs`). Member-since is a real
first-run timestamp (`src-tauri/src/install.rs`). The screenshots above predate
both.

---

## Compose and confirm on iOS (2026-08-03, tickets 33 + 34)

Built for `aarch64-apple-ios-sim` and run on an iPhone 15 simulator, iOS 17.5.
The app launches clean:

```
ledger opened  count=0
Booting Libp2p Swarm...  phase="PHASE_3_NETWORK" progress=70
System Bootstrap Complete. Mesh Swarm Active.
no bootstrap relays configured — discovery is limited to this network
```

### What could not be verified this way, and why

**This environment has no way to tap.** `simctl` has no input command, `idb` is
not installed, and AppleScript is refused assistive access (`osascript is not
allowed assistive access. (-1719)`). So the screens below were reached by
building **throwaway variants** that open on the target screen — reverted
immediately, and the reverted tree is what is committed.

That means the following are asserted by tests and by reading, not by a finger
on glass, and should be checked on a real device before release:

- the focus trap and focus restore in `shell/ModalDialog.tsx`
- the back gesture closing the dialog without also leaving the screen
- the keyboard-avoidance offset, which needs a real on-screen keyboard
- everything in ticket 34 downstream of a peer accepting an intent

### Compose — every option from Rust

![New intent at 100%](screenshots/new-intent-100.png)

`BUY SELL SWAP STAKE`, `AVAX USDC WETH BTC.b`, and the three conditions all
arrive from `intent_form_options`. Nothing on this screen is a frontend
constant.

### The segmented controls wrap at 200%

![New intent at 200%](screenshots/new-intent-200.png)

`--type-scale: 2`, forced — the native content-size plugin is not built yet, so
`preferredContentSizeCategory` does not reach the webview and `simctl ui
content_size` cannot move it.

`BUY SELL SWAP` wrap to a second row carrying `STAKE`; `AVAX USDC` wrap to
`WETH BTC.b`. **Every option stays reachable and the page does not scroll
sideways**, which is the criterion. The tab bar's labels clip rather than wrap,
as ticket 28 decided — the glyph carries the meaning and `aria-label` carries
the name.

### The confirm dialog, against real Rust

![Confirm intent, offline](screenshots/confirm-intent-offline.png)

Every row here was computed by `preview_intent` from the parsed draft:
`BUY AVAX`, `UNDER $95.00`, `1.5 AVAX`, `SHARK MODE`, `LOW`.

The closing line is ticket 04's **offline** string — *"Queued locally. Broadcast
and settlement follow reconnection. No identity is attached."* — which is
correct, because the simulator has no mesh peers, and the button says
`QUEUE LOCALLY` rather than `BROADCAST`. The prose and the verb come from the
same answer, so they cannot disagree.

This is the string the prototype got wrong. Seeing the accurate one appear
*because* the mesh is genuinely unreachable is the whole point of that ticket.

---

## Canonical module MARKET browsing — **GO, release tuple unavailable** (2026-08-12, intent-market ticket 10)

The five-destination shell now exposes `HOME · INTENTS · MARKET · VAULT ·
PROFILE`; node ID, mesh state, uptime, the network map, BLE figures, and nearby
peer rows remain on HOME. MARKET accepts only an active listing from the
reviewed module/market pair after rechecking token existence, current seller
ownership, marketplace eligibility, and approval at one pinned accepted block.
Card identity, slot, rarity, and typed effect come from immutable
`CabalMeshModules.assetData`, never the listing description.

The current Fuji module collection, replacement marketplace, standing registry,
SOURCE_ROLE grants, and independent RPC quorum have not been published as one
reviewed release tuple. The production screen therefore renders `CANONICAL
MARKET UNAVAILABLE`; environment and data-file overrides cannot activate the
buyer-facing catalog. That is the intended fail-closed state, not a legacy
voucher fallback.

| Gate | Result |
|---|---|
| Browser integration at 390 px and forced 200% type: five tabs remain reachable and single-line; HOME diagnostics remain visible | ✅ 3 Playwright flows |
| Canonical catalog: lossless IDs/prices, ALL/RADIO/CRYPTO/POWER, search, all four deterministic sorts, verified/unknown standing | ✅ |
| Loading, empty, unconfigured deployment, offline, RPC rejection, stale listing, malformed metadata, and refresh recovery | ✅ |
| `cargo test --workspace` including 132 app tests, 14 standing tests, 9 BLE loopback tests, IPC/ACL/concurrency suites | ✅ |
| Solidity contracts | ✅ 95 tests |
| TypeScript + Vite production build | ✅ |
| Workspace Clippy | ✅; existing warnings only |
| Desktop debug application | ✅ `src-tauri/target/debug/cabalmesh` |
| iOS 17.5 arm64 simulator, clean unsigned debug bundle | ✅ `src-tauri/gen/apple/build/arm64-sim/CabalMesh.app` |
| Android arm64 debug APK, JDK 21 + NDK 27 | ✅ `src-tauri/gen/android/app/build/outputs/apk/universal/debug/app-universal-debug.apk` |

Known build warnings remain unchanged: the unresolved `glitch.png` runtime
texture, the `.app` bundle-identifier advisory, duplicate Android INTERNET
permission, Java 8 source/target deprecation, and existing dead-code warnings.

---

## Unequipped module seller listing — **GO, release tuple unavailable** (2026-08-12, intent-market ticket 11)

VAULT module detail now derives listing eligibility from accepted canonical
collection and marketplace state. Exact AVAX text remains exact wei across IPC;
token approval, listing, and cancellation have independent pending and confirmed
states; blanket approval skips only the redundant approval transaction. A live
listing is visible in MARKET only after ownership and eligibility are rechecked.
An unsold cancellation returns the module to listable/equippable state, while
marketplace custody after payment presents the deal release/refund rules and
refuses seller cancellation.

The reviewed Fuji module collection and marketplace replacement are still not
published as a release tuple, so the production listing controls fail closed as
`DEPLOYMENT UNAVAILABLE`. Browser integration uses the same IPC shapes with
dynamic accepted-state mocks to exercise the complete seller lifecycle without
substituting the legacy voucher market.

| Gate | Result |
|---|---|
| Browser integration: unequip → approve → exact list → MARKET → cancel → re-equip; blanket approval + paid deal; rejected transaction refresh | ✅ 3 Playwright flows |
| Exact price, lossless IDs, owner/equipped/legacy/soulbound/revoked/stale/deal classifiers and IPC snapshots | ✅ 17 module unit tests; 25 IPC tests |
| Solidity owner, approval, blanket, duplicate, cancel, cancelled-buy, paid-deal and verified module lifecycle | ✅ 96 tests |
| `cargo test --workspace`, including 138 app tests, relay reservation, and 9 BLE loopback tests | ✅ |
| TypeScript + Vite production build; generated TypeScript bindings | ✅ |
| Workspace Clippy | ✅; existing warnings only |
| Desktop debug application | ✅ `src-tauri/target/debug/cabalmesh` |
| iOS 17.5 arm64 simulator, clean unsigned debug bundle | ✅ `src-tauri/gen/apple/build/arm64-sim/CabalMesh.app` |
| Android arm64 debug APK, JDK 21 + NDK 27 | ✅ `src-tauri/gen/android/app/build/outputs/apk/universal/debug/app-universal-debug.apk` |

Known build warnings remain unchanged: unresolved `glitch.png`, the `.app`
bundle-identifier advisory, duplicate Android INTERNET permission, Java 8
source/target deprecation, and existing dead-code/unused-mut warnings.

---

## Escrow-backed module purchase — **GO, release tuple unavailable** (2026-08-12, intent-market ticket 12)

MARKET now preflights one exact listing from accepted state and presents the
canonical module, seller, integer-wei price, network-fee estimate, total, wallet
balance, and the actual settlement model before BUY is enabled. A confirmed buy
retires the listing and creates a custody-verified MY MODULE DEALS entry. Buyer,
seller, deadline, consent, and status determine the visible actions: buyer
release, buyer cancellation request, seller refund after consent, or release by
either involved party after auto-release. Terminal release/refund views are
accepted only when ERC-721 ownership agrees with the contract status.

The reviewed Fuji module collection and replacement marketplace remain absent,
so production quote/deal controls fail closed. Dynamic browser integration
exercised the same IPC shapes without activating the legacy voucher market.

| Gate | Result |
|---|---|
| Browser integration: exact quote + buy + release; buyer cancellation request remains pending and releasable; seller mutual refund with no early release | ✅ 3 Playwright flows |
| Purchase/deal classifiers: exact large IDs, integer price/fee/total/shortfall, stale/self/inactive, roles, deadline, consent, custody and terminal actions | ✅ 22 module unit tests; 27 IPC tests |
| Solidity canonical module purchase, release, auto-release, mutual refund, relist and invalid-action guards | ✅ 97 tests |
| `cargo test --workspace`, including 143 app tests, relay reservation, and 9 BLE loopback tests | ✅ |
| Generated TypeScript bindings | ✅ 62 export tests |
| TypeScript + Vite production build | ✅ |
| Workspace Clippy | ✅; existing warnings only |
| Desktop debug application | ✅ `src-tauri/target/debug/cabalmesh` |
| iOS 17.5 arm64 simulator, clean unsigned debug bundle | ✅ `src-tauri/gen/apple/build/arm64-sim/CabalMesh.app` |
| Android arm64 debug APK, JDK 21 + NDK 27 | ✅ `src-tauri/gen/android/app/build/outputs/apk/universal/debug/app-universal-debug.apk` |

Known build warnings remain unchanged: unresolved `glitch.png`, the `.app`
bundle-identifier advisory, duplicate Android INTERNET permission, Java 8
source/target deprecation, Gradle 9 deprecation notices, and existing
dead-code/unused-mut warnings.
