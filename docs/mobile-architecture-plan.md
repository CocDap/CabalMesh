# CabalMesh — `src-tauri` Mobile Architecture Plan (iOS + Android)

> **2026-11-05:** written when the app settled on Avalanche. NEAR is now the only chain, and `contracts/`, alloy and `blockchain_bridge.rs` are gone (`identity.rs` and `near_bridge.rs` replaced it). Its mobile architecture decisions still apply; its chain details do not. Kept as a historical record; see `docs/product-status.md` for what is true today.

Status: **reviewed implementation plan** — five revision passes applied 2026-08-01, see §12.
Target: **the exact compatible Tauri release set in §5.4**, centered on `tauri` 2.11.5 (repo is on 2.9.5; coordinated upgrade is a Phase 0 blocker), Rust 2024 edition, single Rust core shared by desktop + mobile.
Verified against official Tauri, Apple, Cargo, libp2p and W3C sources cited inline. The evidence ledger is [`research/tauri-mobile-plan-verification-2026-08-01.md`](./research/tauri-mobile-plan-verification-2026-08-01.md).
Scope of this document: **Rust / `src-tauri` only.** Frontend is deferred; §7 defines the command + event contract and TypeScript types the artifact UI will bind to.

---

## 0. Decisions locked

| Area | Decision |
|---|---|
| ZK proving | **Superseded 2026-08-27 — `zk_handler.rs` and the circuit were deleted, unused, so there is nothing to gate. The rest of this row describes the plan as it stood.** Out of mobile scope: `nargo` shell-out stays desktop-only behind `#[cfg(desktop)]`. A mobile Rust stub returns `AppError::Unsupported` for internal callers/tests; the mobile webview is not granted that command, so direct JS invocation is denied by ACL first. |
| LLM / agent | **Out of mobile scope.** Ollama process spawning is desktop-only. Same stub + ACL treatment. |
| Mesh transport | mDNS on LAN (**Android; iOS only with Apple's managed multicast entitlement**) + relay bootstrap (`relay` client + `dcutr` + `identify` + `ping`) + QUIC/TCP transport. Network changes recover by re-dial/rejoin/replay-dedupe; no seamless QUIC-migration promise. |
| Relay infrastructure | **Self-hosted relay, address baked in as the default**, user-overridable in Profile. See §2.7.1 — this is an infra deliverable, not just code. |
| Desktop RPG UI | **Frozen.** `src/` is left untouched and unmaintained. Mobile ships as a second Vite entry point. Consequence in §2.10 — the Rust reshape would break it, so a legacy adapter layer is required. |
| Frontend | Mobile UI planned separately in [`mobile-ui-implementation-plan.md`](./mobile-ui-implementation-plan.md). Rust exposes a stable, typed command surface derived from the 10 screens. |

Rationale for `Unsupported` over deleting the handlers: Rust/TypeScript still share one generated contract and the UI branches on `platform_caps`. Defense-in-depth is stricter: mobile Rust stubs return `Unsupported` in internal tests, while the mobile webview is not granted those three permissions and is denied by ACL if it invokes them anyway.

---

## 1. Current state audit

`src-tauri` today: 3,040 lines across 9 flat modules, 47 `#[tauri::command]`s, one `Arc<Mutex<AppState>>` global.

### 1.1 Hard blockers for a mobile build

| # | Blocker | File | Why it breaks | Fix |
|---|---|---|---|---|
| B1 | `keyring = "3.6.3"` in `Cargo.toml` | `Cargo.toml` | Desktop-only backends (Keychain/CredMan/secret-service). **Also: zero usages in the codebase.** | Delete the dep. |
| B2 | `Command::new("ollama")` | `ollama_manager.rs:21,38,62,77` | No process spawning on iOS/Android. | `#[cfg(desktop)]`; mobile uses the HTTP path only, or nothing. |
| B3 | ~~`Command::new("nargo")`~~ | ~~`zk_handler.rs:53`~~ | **Moot 2026-08-27** — the handler and circuit were deleted as unused, so no `nargo` spawn exists to gate. | — |
| B4 | `dirs::data_dir()` | `blockchain_bridge.rs:228` | Returns garbage/`.` on mobile; app sandbox path must come from the platform. | `app.path().app_data_dir()` via Tauri path API, injected at construction. |
| B5 | `dotenv::dotenv()` + 5× `std::env::var` for contract addresses | `lib.rs:630`, `blockchain_bridge.rs:209-231` | No `.env` file ships to a mobile bundle; env vars are not settable. | Layered config: compile-time defaults → `tauri.conf.json` → runtime settings store. |
| B6 | `reqwest 0.11` default features (native-tls) | `Cargo.toml` | Android has no system OpenSSL; link failure or runtime TLS failure. | `reqwest 0.12`, `default-features = false`, `features = ["json","rustls-tls"]`. Unify — lockfile currently carries **three** reqwest majors (0.11 / 0.12 / 0.13). |
| B7 | libp2p mDNS with no platform permission plumbing | `mesh.rs` | Android needs multicast reception plumbing and, once targeting Android 17 / SDK 37+, the runtime `ACCESS_LOCAL_NETWORK` permission; denial blocks raw mDNS/LAN traffic. On iOS, rust-libp2p opens raw multicast UDP sockets, so the Local Network usage string alone is insufficient: the signed app also needs Apple's managed `com.apple.developer.networking.multicast` entitlement. Without the platform grant, mDNS silently discovers nothing. | Phase 0 pins the Android target SDK and chooses the iOS entitlement-backed or relay-only path; §5.3. Android gets a small multicast-lock plugin plus the SDK-appropriate local-network permission flow. |
| B8 | TCP-only transport | `mesh.rs:1-6` | Mobile NAT + cellular + network-change churn kills long-lived TCP; no hole punching. | Add QUIC; add relay/DCUtR. |
| B9 | `capabilities/default.json` is all-platform and grants `opener:default` | `capabilities/default.json` | Capability files auto-enable unless identifiers are selected explicitly, and permissions are unioned. Merely adding a narrower mobile file would still grant the shared default to mobile. The desktop `$schema` is editor/validation metadata, not a runtime boundary. | Delete `default.json`; split into `desktop.json` + `mobile.json`; explicitly select one identifier in the base/platform configs. `windows: ["main"]` stays — mobile does have a labelled main webview. |
| B10 | `alloy = { features = ["full"] }` | `Cargo.toml` | Pulls the entire ecosystem incl. transports and test utils; cross-compile surface and binary size explode on 7 mobile targets. | Trim to `provider-http`, `signer-local`, `contract`, `rpc-types-eth`, `json-abi` (verify exact names against alloy 1.8). |

### 1.2 Structural problems (not blockers, but they will bite)

| # | Problem | Evidence | Rust rule |
|---|---|---|---|
| S1 | **Plaintext private keys on disk.** `identities.json` stores `private_key_hex` unencrypted. | `blockchain_bridge.rs:285-289`, `save_identities` | — (security) |
| S2 | **One global mutex serializes every command.** Every command does `state.lock().await` then `bridge.lock().await`, holding both across network I/O. Two concurrent RPC calls fully serialize. | `lib.rs` — all 50 commands | `anti-lock-across-await`, `own-rwlock-readers` |
| S3 | **Stringly-typed everything.** `intent_type: String` matched against `"relay_tx"`/`"settlement"`/…; addresses, ids, amounts all `String`. | `lib.rs:44-56`, `mesh.rs:41-47` | `anti-stringly-typed`, `type-newtype-ids`, `type-enum-states` |
| S4 | **Errors are `String` / `Box<dyn Error>`.** Frontend can't branch on failure kind; no source chain. | every command signature | `err-thiserror-lib`, `err-custom-type`, `err-source-chain` |
| S5 | **`println!`/`eprintln!` as logging.** Invisible on a device — no `adb logcat` / Console.app structure, no levels, no filtering. | ~40 sites | `obs-tracing-over-log`, `obs-structured-fields` |
| S6 | **God-object `BlockchainBridge`** — 1,301 lines: RPC client + wallet + 8 JSON file stores + relay queue + content store. | `blockchain_bridge.rs` | `proj-mod-by-feature` |
| S7 | **No tests except two temp-dir tests.** Nothing for the intent lifecycle. | `blockchain_bridge.rs:1182,1267` | `test-cfg-test-module`, `test-proptest-properties` |
| S8 | **Untyped events.** `emit("mesh-event", event)` + `emit("bootstrap-status", …)` with ad-hoc shapes. | `lib.rs:653`, `app_initializer.rs:86` | `type-enum-states`, `serde-enum-representation` |
| S9 | **State managed asynchronously after startup.** `app_handle.manage(state)` runs inside a spawned task, so any command invoked before bootstrap finishes panics on missing state. | `lib.rs:661,673` | `err-result-over-panic` |

---

## 2. Target architecture

### 2.1 Cargo workspace

Split the flat module pile into a workspace (`proj-workspace-large`, `proj-mod-by-feature`). Each crate is host-testable without a device.

```
src-tauri/
├── Cargo.toml                  # [workspace] + shared deps + shared lints
├── tauri.conf.json
├── tauri.android.conf.json     # platform overlays
├── tauri.ios.conf.json
├── capabilities/
│   ├── desktop.json
│   └── mobile.json
├── crates/
│   ├── cabal-core/             # domain. NO I/O, NO tauri, NO tokio
│   ├── cabal-contract/         # serialized screen/command DTOs -> ts-rs; NO tauri
│   ├── cabal-vault/            # identities, key material, encryption-at-rest
│   ├── cabal-mesh/             # libp2p actor
│   ├── cabal-chain/            # alloy RPC, contracts, offline tx queue
│   ├── cabal-store/            # typed persistence (paths injected)
│   ├── cabal-ai/               # desktop-only: ollama + agent + matcher
│   └── cabal-zk/               # desktop-only: noir
└── src/                        # the Tauri app crate (thin)
    ├── main.rs                 # 6 lines, unchanged
    ├── lib.rs                  # run() + builder wiring only
    ├── state.rs                # AppState: handles, no mega-mutex
    ├── error.rs                # AppError -> serde -> TS
    ├── events.rs               # typed event enum + emitter
    └── commands/
        ├── mod.rs
        ├── session.rs          # splash / connecting / leave
        ├── mesh.rs             # home + nodes screens
        ├── intents.rs          # intents / new / detail / settled
        ├── vault.rs            # vault screen
        ├── profile.rs          # profile screen
        └── platform.rs         # capability probe, offline toggle
```

**Dependency direction (strictly one-way):**

```
app (src/) ──> cabal-mesh ──┐
           ──> cabal-chain ─┼──> cabal-core
           ──> cabal-vault ─┤
           ──> cabal-store ─┘
           ──> cabal-contract ──> cabal-core
           ──> cabal-ai   (desktop)
           ──> cabal-zk   (desktop)
```

`cabal-core` depends on nothing but `serde`, `thiserror`, and small numeric/time crates. That is what makes the domain testable in milliseconds on the host, which matters when the alternative is a 4-minute Android link step.

`cabal-contract` owns every serialized request/response and screen view in §6 (`SessionStatus`, `MeshSnapshot`, intent/form/proof views, vault/profile rows, platform/runtime caps and nearby-node types). It depends on `cabal-core`, `serde` and `ts-rs`, but not Tauri or a service implementation. Defining this schema in Phase 1 lets UI fixtures compile against the real contract before Phase 6 wires handlers; it does **not** grant or register those future commands early.

### 2.2 `cabal-core` — the domain

Everything the UI shows becomes a type. This kills S3 and makes the artifact's state machine compiler-checked.

```rust
// crates/cabal-core/src/ids.rs
//! Opaque identifiers. Mixing an IntentId with a NodeId is a compile error.

/// A mesh peer identity. Displayed truncated as `7F3A..8C2E`.
#[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize, Deserialize, TS)]
#[serde(transparent)]
pub struct NodeId(Box<str>);

#[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize, Deserialize, TS)]
#[serde(transparent)]
pub struct IntentId(Box<str>);

#[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize, Deserialize, TS)]
#[serde(transparent)]
pub struct ProofHash(Box<str>);
```
`api-newtype-safety`, `type-newtype-ids`, `mem-boxed-slice` (`Box<str>` — these are never mutated, so `String`'s spare capacity word is dead weight across thousands of ledger rows).

```rust
// crates/cabal-core/src/intent.rs

/// What the user is trying to do — the `I WANT TO` segmented control.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, TS)]
#[serde(rename_all = "UPPERCASE")]
pub enum Action { Buy, Sell, Swap, Stake }

/// Execution strategy — the `MODE` selector.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, TS)]
#[serde(rename_all = "SCREAMING_SNAKE_CASE")]
pub enum ExecutionMode { Shark, Ghost, Patient }

impl ExecutionMode {
    /// Copy shown under the mode selector.
    #[must_use]
    pub const fn description(self) -> &'static str {
        match self {
            Self::Shark   => "Aggressive execution. Best price. Higher risk.",
            Self::Ghost   => "Maximum privacy. Longer route. Slower fill.",
            Self::Patient => "Waits for the condition. No slippage tolerance.",
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize, TS)]
#[serde(rename_all = "UPPERCASE")]
pub enum PrivacyLevel { Low, Medium, High }

/// The `CONDITION` row. `Any` carries no price, so the type forbids
/// constructing a priceless `Under`.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, TS)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum Condition {
    Under { price: UsdPrice },
    Above { price: UsdPrice },
    Any,
}
```
`type-enum-states`, `serde-enum-representation`, `const-fn`, `api-must-use`.

**Intent lifecycle** — one enum, exhaustively matched, so a new status can never silently fall through a `_` arm (`pat-exhaustive-enum`):

```rust
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, TS)]
#[serde(tag = "status", rename_all = "SCREAMING_SNAKE_CASE")]
pub enum IntentStatus {
    Draft,
    Broadcast   { route_len: u8 },
    Negotiating { bids: u8, best: Option<UsdPrice> },
    FindingRoute,
    Waiting,
    Settled     { proof: ProofHash, filled_at: UsdPrice, elapsed_ms: u32 },
    Failed      { reason: FailureReason },
    Cancelled,
}
```

Maps 1:1 onto the artifact's `status` strings (`NEGOTIATING`, `FINDING ROUTE`, `WAITING`, `SETTLED`, `FAILED`) and its `dot` colour, which the frontend derives from the variant rather than receiving as a hex string from Rust.

**Money.** The artifact renders `10 AVAX`, `1,240.00 USDC`, `$94.21`. Never `f64` — parse into fixed-point at the boundary (`api-parse-dont-validate`, `num-overflow-explicit`):

```rust
/// Token amount in an asset's smallest unit. Construction validates decimals.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub struct TokenAmount { raw: u128, decimals: u8 }

impl TokenAmount {
    /// # Errors
    /// Returns [`AmountError::Overflow`] if `units` scaled by `decimals`
    /// exceeds `u128`, and [`AmountError::TooManyDecimals`] if the string
    /// carries more precision than the asset supports.
    pub fn parse(s: &str, decimals: u8) -> Result<Self, AmountError> { /* … */ }
}

impl fmt::Display for TokenAmount { /* 1,240.00 */ }

/// Boundary view: JSON/TypeScript never receives a `u128` that can lose
/// precision in JavaScript. Construct from `TokenAmount::to_string()`.
#[derive(Debug, Clone, Serialize, TS)]
#[serde(transparent)]
pub struct FormattedAmount(Box<str>);
```
`type-display-vs-debug`, `doc-errors-section`, `conv-fromstr-parsing`.

**Transition validation** lives here and is pure, so it is proptest-able:

```rust
impl IntentStatus {
    /// Whether `next` is a legal successor. Terminal states accept nothing.
    #[must_use]
    pub const fn can_transition_to(&self, next: &Self) -> bool { /* … */ }
}
```

### 2.3 State: kill the mega-mutex (fixes S2, S9)

Today: `State<'_, Arc<Mutex<AppState>>>` → `state.lock().await` → `bridge.lock().await` → `.await` an RPC. Two locks held across network I/O.

Target: `AppState` holds **cheap clonable handles**; no outer lock exists.

```rust
// src/state.rs

/// Managed once, synchronously, before the first command can run.
#[derive(Clone)]
pub struct AppState {
    pub session: SessionHandle,   // Arc<RwLock<Session>>  — read-dominated
    pub mesh:    MeshHandle,      // mpsc sender to the swarm actor
    pub chain:   ChainHandle,     // Arc<ChainService>, interior RwLock per store
    pub vault:   VaultHandle,     // Arc<Vault>
    pub caps:    PlatformCaps,    // Copy, immutable, resolved at BUILD time
    pub runtime: RuntimeCapsHandle, // Arc<RwLock<RuntimeCaps>> + probe methods
    pub lifecycle: LifecycleHandle, // watch-backed latest-state publisher
    #[cfg(desktop)]
    pub ai: AiHandle,
    #[cfg(desktop)]
    pub zk: ZkHandle,
}

#[derive(Clone)]
pub struct RuntimeCapsHandle(Arc<RwLock<RuntimeCaps>>);

#[derive(Clone)]
pub struct LifecycleHandle { tx: watch::Sender<Lifecycle> }

impl LifecycleHandle {
    pub fn set(&self, next: Lifecycle) { let _ = self.tx.send_replace(next); }
}
```

**Static and runtime capabilities are different things and must not share a struct.** An earlier draft made `caps` build-time immutable and then had `mdns_discovery` resolved from a runtime permission grant — a contradiction. Split them:

```rust
/// Facts fixed when the binary was compiled. Never changes. `Copy`.
#[derive(Debug, Clone, Copy, Serialize, TS)]
#[serde(rename_all = "camelCase")]
pub struct PlatformCaps {
    pub zk_proving: bool,      // cfg(desktop)
    pub local_llm: bool,       // cfg(desktop)
    pub background_mesh: bool, // false on both mobile targets without a background service
}

/// State that changes while the app runs: permission grants the user can
/// change in Settings, observed discovery health, and connectivity. Probe on
/// every `Resumed` (§2.7); iOS exposes no general Local Network status API.
#[derive(Debug, Clone, Serialize, TS)]
#[serde(rename_all = "camelCase")]
pub struct RuntimeCaps {
    pub local_discovery: LocalDiscoveryState,
    pub relay_reachable: bool,
    pub online: bool,
}

#[derive(Debug, Clone, Serialize, TS)]
#[serde(tag = "state", content = "detail", rename_all = "camelCase")]
pub enum LocalDiscoveryState {
    Disabled { reason: DiscoveryDisabledReason }, // e.g. iOS relay-only build
    PermissionRequired,                           // Android 17+, before first request
    Probing,
    Ready,                                        // permission/socket ready; zero peers is valid
    Available,                                    // at least one valid local peer observed
    Denied,                                       // e.g. Android runtime permission denied
    Indeterminate,                                // raw iOS UDP: denied vs zero peers is unknowable
}

#[derive(Debug, Clone, Copy, Serialize, TS)]
#[serde(rename_all = "camelCase")]
pub enum DiscoveryDisabledReason {
    RelayOnlyBuild,
    MissingMulticastEntitlement,
    PlatformUnsupported,
}
```
The frontend fetches `platform_caps` once at boot and subscribes to `runtime_caps` changes. Conflating them means a changed permission or broken relay is never noticed until something silently stops working. Do not collapse `LocalDiscoveryState` back to `mdns_granted: bool`: [Apple TN3179](https://developer.apple.com/documentation/technotes/tn3179-understanding-local-network-privacy) says there is no general API to query Local Network access. On Android, a granted permission plus successful socket/group setup reaches `Ready` after a bounded probe even when the LAN contains zero peers; a valid peer response promotes it to `Available`. Baseline iOS raw UDP silence remains `Indeterminate`, because it can mean denial **or** simply no peers. A future native Bonjour probe may report policy denial explicitly.

- `SessionHandle` — `RwLock`, not `Mutex`: connection status and node id are read on every screen and written twice a session (`own-rwlock-readers`).
- `MeshHandle` — the libp2p `Swarm` is `!Sync` and must live in exactly one task. So mesh is an **actor**: commands send `MeshCommand` over a bounded `mpsc` and get a `oneshot` reply (`async-mpsc-queue`, `async-oneshot-response`, `async-bounded-channel` — bounded so a UI that spams "refresh nodes" applies backpressure instead of growing a queue until OOM on a 2 GB phone).

```rust
// crates/cabal-mesh/src/handle.rs
#[derive(Clone)]
pub struct MeshHandle { tx: mpsc::Sender<MeshCommand> }

enum MeshCommand {
    Publish   { intent: WireIntent,           reply: oneshot::Sender<Result<(), MeshError>> },
    Snapshot  {                               reply: oneshot::Sender<MeshSnapshot> },
    NearbyNodes {                             reply: oneshot::Sender<Vec<NodeSummary>> },
    SetOffline { offline: bool,               reply: oneshot::Sender<()> },
    Shutdown  {                               reply: oneshot::Sender<()> },
}

impl MeshHandle {
    /// # Errors
    /// [`MeshError::ActorGone`] if the swarm task has terminated.
    pub async fn snapshot(&self) -> Result<MeshSnapshot, MeshError> {
        let (tx, rx) = oneshot::channel();
        self.tx.send(MeshCommand::Snapshot { reply: tx }).await
            .map_err(|_| MeshError::ActorGone)?;
        rx.await.map_err(|_| MeshError::ActorGone)
    }
}
```

Two things the Tauri v2 state docs say directly, both of which the current code gets wrong:

> "You don't need to wrap your state in an `Arc` when using `manage`" — Tauri stores it behind one internally. Today's `Arc<Mutex<AppState>>` is a redundant layer on top of Tauri's own.

> "It is ok and often preferred to use the ordinary `Mutex` from the standard library in asynchronous code" — reserve the async mutex for guards genuinely held across `.await`. Today every command takes a `tokio::Mutex` for work that never awaits under it, paying an async lock for a synchronous read.

So: `app.manage(AppState { … })` with no outer wrapper; `std::sync::Mutex` inside the handles for short critical sections; `tokio::sync::RwLock` only where a guard really does span an await.

**Startup ordering (S9).** `manage()` must complete before the webview can invoke — and the failure mode is nastier than it looks. From the docs:

> "If you use the wrong type, ... you will get a runtime panic instead of compile time error."

A `State<'_, AppState>` lookup against state that has not been managed yet does not return an error the command can convert into `NotReady` — it panics inside the IPC handler. Today's code manages state *inside a spawned task*, so every command invoked during bootstrap is a panic waiting on a race. Restructure `run()`:

1. Synchronously build `AppState` with every subsystem in a `Connecting`/`Idle` state and call `app.manage(state)` — **inside `setup`, not inside a spawned task**.
2. Spawn bootstrap, which mutates that state and emits progress events.
3. Any command hitting an un-bootstrapped subsystem returns `AppError::NotReady`, which the frontend renders as the `connecting` screen instead of crashing.

This is exactly what the artifact's `connecting` screen already models — a progress bar plus a streaming handshake log. The current code has no way to express "not ready yet" other than a panic.

### 2.4 Errors (fixes S4)

Per-crate `thiserror` enums (`err-thiserror-lib`), one app-level enum at the command boundary that serializes to a discriminated union TS can `switch` on.

```rust
// src/error.rs
#[derive(Debug, thiserror::Error, Serialize, TS)]
#[serde(tag = "kind", content = "detail", rename_all = "snake_case")]
#[non_exhaustive]
pub enum AppError {
    #[error("subsystem not ready")]
    NotReady { subsystem: &'static str },

    #[error("not supported on this platform")]
    Unsupported { feature: &'static str },   // ← ZK + LLM on mobile land here

    #[error("mesh unreachable")]
    MeshOffline,

    #[error("invalid intent")]
    InvalidIntent { field: &'static str, reason: String },

    #[error("chain call failed")]
    Chain { message: String },

    #[error("vault locked")]
    VaultLocked,

    #[error("too many active subscriptions")]
    TooManySubscriptions { limit: u16 },

    #[error("internal error")]
    Internal { message: String },
}
```
`err-lowercase-msg`, `api-non-exhaustive`. Internal source chains are logged in full (`obs-error-chain`) and **not** returned to the webview — RPC URLs and file paths do not belong in a UI toast.

### 2.5 Events and streams (fixes S8)

Tauri v2 has **two** frontend-bound mechanisms, and the docs are explicit about which to use where:

> "Event payloads are always JSON strings making them not suitable for bigger messages." · "Channels are designed to be fast and deliver ordered data. They are used internally for streaming operations such as download progress, child process output and WebSocket messages."

Three of this app's data flows are exactly that shape — the `connecting` handshake log, the `home` mesh ticker, the `settled` verification log. Those are **`Channel<LogLine>`**, not events. The rest are low-frequency state changes and stay on the event bus.

```rust
// Each command starts or attaches to a domain operation, registers a separate
// presentation-log delivery task, and returns that delivery handle immediately.
#[tauri::command]
async fn enter_mesh(
    state: State<'_, AppState>,
    on_line: Channel<LogLine>,          // handshake log -> `connecting`
) -> Result<SubscriptionId, AppError> { … }

#[tauri::command]
async fn subscribe_mesh_log(
    state: State<'_, AppState>,
    on_line: Channel<LogLine>,          // live ticker -> `home`
) -> Result<SubscriptionId, AppError> { … }

#[tauri::command]
async fn settle_intent(
    id: IntentId,
    state: State<'_, AppState>,
    on_line: Channel<LogLine>,          // proof log -> `settled`
) -> Result<SubscriptionId, AppError> { … }
```

Channels are ordered and typed end-to-end, which the event bus is not — and a gossip feed pushed through `emit` is JSON-stringified per line for every listener.

#### 2.5.1 Channels need explicit teardown — React unmount does not end them

An earlier draft claimed a channel's lifetime tracks the screen that opened it. **It does not.** In `@tauri-apps/api` 2.9.1 (`core.js`), `Channel` frees its callback only inside `cleanupCallback()`, and that runs on exactly one trigger: Rust sending an `end` message whose index matches the next expected index. There is no public unsubscribe on the JS side, and unregistering the JS callback would not stop the Rust task anyway.

Unmanaged, leaving the `home` screen leaves a live broadcast receiver and a log-producing task per visit — a leak that compounds every time the user taps a tab.

So every stream carries an explicit lifecycle:

```rust
/// Opaque handle for a live presentation stream. Returned by every
/// stream-producing command.
#[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize, Deserialize, TS)]
#[serde(transparent)]
pub struct SubscriptionId(Box<str>);

#[tauri::command]
async fn subscribe_mesh_log(
    state: State<'_, AppState>,
    on_line: Channel<LogLine>,
) -> Result<SubscriptionId, AppError> { … }

/// Stops only the log-delivery task, drops its broadcast receiver, and closes
/// its channel. It never owns or aborts the underlying domain operation.
/// Idempotent: unsubscribing an unknown or already-closed id is Ok.
#[tauri::command]
async fn unsubscribe(id: SubscriptionId, state: State<'_, AppState>) -> Result<(), AppError>;
```

**This applies to all three streams, not just `subscribe_mesh_log`.** `enter_mesh` and `settle_intent` also spawn producers, so they too return a `SubscriptionId` immediately rather than blocking until the stream ends — otherwise the frontend has no handle to cancel and no way to render progress while the command is still pending.

**Cancel means "stop delivering", not "abort the operation".** The distinction matters and must be explicit:

| Stream | `unsubscribe` stops | `unsubscribe` does **not** stop |
|---|---|---|
| `subscribe_mesh_log` | log delivery | nothing else — it is delivery-only |
| `enter_mesh` | handshake log delivery | **the mesh join itself.** Leaving the `connecting` screen must not disconnect. |
| `settle_intent` | verification log delivery | **settlement.** An in-flight on-chain settlement is never aborted by a UI navigation — that would be a correctness bug with money attached. |

Aborting a domain operation, if ever wanted, is a separate explicit command. `cancel_intent` may transition only a still-cancellable, pre-settlement intent; once on-chain settlement has started it returns an invalid-state error. Never overload `unsubscribe` with domain cancellation.

Rules, all of them load-bearing:

- **Every stream-producing command returns a `SubscriptionId`; every React effect returns a cleanup calling `unsubscribe`.**
- **Finite delivery streams close their own channel on all three delivery exits** — natural completion, delivery error, and explicit unsubscribe. A handshake that fails at step 3 must still send `end`, or the `connecting` callback is retained forever.
- **Domain work and delivery are different tasks.** The settlement/join task writes typed state and a bounded log ring; the cancellable delivery task forwards that ring/broadcast to `Channel`. A delivery cancellation token must never be held by the transaction task.
- **The registry is bounded and self-cleaning.** Delivery registrations live in a `HashMap<SubscriptionId, DeliveryHandle>` with a per-app cap; active delivery removes its own entry after natural completion/error sends the final tail/end, and explicit unsubscribe is idempotent. A finite operation that completes while suspended retains only its bounded final tail + terminal marker until resume (with a TTL as kill-safe), then replays, closes and removes itself. Exceeding the cap returns `AppError::TooManySubscriptions` (`async-cancellation-token`, `async-bounded-channel`).
- **Suspend pauses forwarding; it does not delete registrations.** While the webview cannot receive, append only to the bounded retained tail. Resume replays that tail and continues on the **same** `SubscriptionId`; the UI must not invoke `enter_mesh` or `settle_intent` a second time.
- Integration tests: subscribe → unsubscribe → registry empty; natural completion → registry empty; unsubscribe midway through `settle_intent` → no more log delivery **and settlement still reaches a terminal state**; suspend/resume → same id resumes with no duplicate operation; concurrent/StrictMode-style repeated settlement calls → one transaction and independent delivery ids.

Everything else stays a typed event enum, adjacently tagged so TS gets a clean union.

```rust
// src/events.rs
#[derive(Debug, Clone, Serialize, TS)]
#[serde(tag = "type", content = "data", rename_all = "camelCase")]
pub enum AppEvent {
    BootstrapProgress { phase: BootPhase, message: String, progress: u8 },
    MeshStatsChanged(MeshStats),                            // home stat tiles
    PeersChanged      { nearby: Vec<NodeSummary> },         // `nodes` screen
    IntentUpdated(IntentView),                              // list + detail live update
    RuntimeCapsChanged(RuntimeCaps),                        // permissions + connectivity
    TypeScaleChanged  { scale: f32 },                       // native font setting
    Toast             { title: String, body: String, accent: ToastAccent },
}
```

The three log streams are deliberately **absent** from this enum — they are channels (above).

Channels are presentation only; they are never completion control flow. `enter_mesh` completes authoritatively through terminal `BootstrapProgress` phases (`Ready` / `Failed`) plus `session_status`; settlement completes through `IntentUpdated` reaching a terminal intent status, after which the UI calls `get_proof`. A `Toast` is supplementary feedback, never the only signal that money-moving work succeeded or failed. After foregrounding, the active screen refetches its authoritative state in case an event was not deliverable while suspended.

**Start-or-attach is an idempotency contract, not a comment.** React StrictMode, fast unmount/remount, or two windows can invoke a stream-producing command twice:

- `enter_mesh` atomically ensures one session join. Later calls attach a new delivery subscription to its retained tail/current operation; they never start a second join.
- `settle_intent` atomically keys the domain task by `IntentId`. Concurrent/repeated calls return distinct `SubscriptionId`s attached to the **same** settlement; once terminal, they replay retained tail/state and close. Transaction construction/submission also carries an idempotency key so a process-level race cannot broadcast twice.
- `subscribe_mesh_log` is attach-only by definition.

Tests reproduce concurrent calls and React StrictMode behavior: two `settle_intent(id)` invocations produce one domain invocation/transaction and two independently cancellable delivery ids; navigate away/back attaches without resubmitting. Every payload nested in `AppEvent` derives `TS` as well as `Serialize`, so the generated union in §7 is complete.

Note the prototype drives its `MESH LOG` from a client-side `setInterval` over a canned array. Real implementation: a server-push stream from the swarm actor. The frontend keeps a bounded ring buffer (the prototype shows 4 lines) and never polls.

### 2.6 Persistence + vault (fixes S1, B4)

`cabal-store` owns typed, atomic, path-injected JSON stores. `cabal-vault` owns key material.

```rust
// crates/cabal-store/src/lib.rs

/// A typed JSON document persisted atomically (write temp + rename).
pub struct JsonStore<T> { path: PathBuf, cached: RwLock<T> }
```

- **Paths injected, never discovered.** Constructor takes `&Path` from `app.path().app_data_dir()?`. `dirs` is deleted. The `CABALMESH_DATA_DIR` override stays but only under `#[cfg(desktop)]`, for the 2-node local test in `scripts/dev-2-nodes.sh`.
- **Atomic writes.** Mobile processes are killed without warning; a truncated `identities.json` is an unrecoverable wallet loss today.
- **Vault.** `identities.json` becomes `vault.enc`: AES-256-GCM (dep already present) over the identity list, with the data key held in the platform keystore — iOS Keychain (`kSecAttrAccessibleAfterFirstUnlockThisDeviceOnly`) and Android Keystore (`AndroidKeyStore`, StrongBox when available) — via a small Tauri plugin (§5.3). Desktop keeps a passphrase-derived key (Argon2id) since there is no uniform desktop keystore now that `keyring` is removed.
- **Never log key material** (`obs-no-sensitive-data`). `IdentityRecord` gets a manual `Debug` that prints `private_key_hex: <redacted>`. The current derive leaks it into any error path that formats the struct.

The artifact's vault screen has a `KEYS` tab listing `SIGNING KEY ed25519 / HELD LOCALLY. NEVER SYNCED.` and `RECOVERY PHRASE / NOT BACKED UP` — that copy is a promise the storage layer has to actually keep.

### 2.7 Mesh (fixes B7, B8)

```rust
// crates/cabal-mesh/src/behaviour.rs
#[derive(NetworkBehaviour)]
pub struct MeshBehaviour {
    pub gossipsub: gossipsub::Behaviour,
    pub mdns:      Toggle<mdns::tokio::Behaviour>,  // off when the OS denies it
    pub identify:  identify::Behaviour,
    pub ping:      ping::Behaviour,
    pub relay:     relay::client::Behaviour,        // reach peers off-LAN
    pub dcutr:     dcutr::Behaviour,                // upgrade relayed -> direct
}
```

Transport stack: **QUIC first, TCP fallback**. libp2p QUIC uses its built-in TLS 1.3 security and native multiplexed streams; the TCP fallback is upgraded with Noise + Yamux. Do not stack Noise/Yamux on QUIC.
The pinned `libp2p-quic` disables QUIC connection migration, so Wi-Fi→cellular is designed as **disconnect → re-dial bootstrap/relay → rejoin topics → replay with message-id deduplication**, not as a seamless live migration promise. Physical-device tests measure recovery time and duplicate suppression; they do not decide whether a nonexistent guarantee happened to work once.

Discovery ladder, in order:
1. mDNS on LAN (permission-gated; `Toggle` so a denial degrades instead of failing to boot).
2. Bootstrap relay multiaddrs from config → `identify` → gossipsub mesh.
3. DCUtR hole punch to promote a relayed connection to direct.

**Lifecycle — Tauri 2.11 provides this natively. No custom plugin needed.**

An earlier draft concluded "Tauri gives you nothing here" and specced a custom `lifecycle` plugin. That was true of 2.9.5 and **false of the 2.11 line we are upgrading to**. From the `tauri@2.11.0` changelog (2026-04-30):

> "Propagates the `Event::Suspended` and `Event::Resumed` events from tao when they are emitted on mobile targets."

Android maps these to `onPause`/`onResume`; iOS maps them asymmetrically to resign-active / enter-foreground. In Tauri 2.11.5 the public path is settled, so the synchronous runtime callback updates a coalescing `watch`-backed lifecycle handle. A bounded `try_send` is wrong here because dropping the one `Resumed` message can leave the app paused forever:

```rust
tauri::Builder::default()
    .build(tauri::generate_context!())?
    .run(|app, event| {
        #[cfg(mobile)]
        {
            let next = match event {
                tauri::RunEvent::WindowEvent {
                    event: tauri::WindowEvent::Suspended, ..
                } => Some(Lifecycle::Suspended),
                tauri::RunEvent::WindowEvent {
                    event: tauri::WindowEvent::Resumed, ..
                } => Some(Lifecycle::Resumed),
                _ => None,
            };

            if let Some(next) = next {
                app.state::<AppState>().lifecycle.set(next); // watch::Sender::send_replace
            }
        }

        #[cfg(desktop)]
        let _ = (app, event);
    });
```

The JavaScript names are also documented: `TauriEvent.WINDOW_SUSPENDED` / `WINDOW_RESUMED` (`tauri://suspended` / `tauri://resumed`). Keep a compile check against the exact pinned 2.11.x, but path discovery is no longer an open design question.

**One semantic question remains and is a Phase 0 device gate.** iOS `applicationWillResignActive` also fires for Control Center, Notification Center, incoming calls and lock; `applicationWillEnterForeground` only fires after true backgrounding. A transient overlay can therefore produce `Suspended` without the matching `Resumed`. Test those sequences before this event pair controls mesh/log pausing. If it is unbalanced, add a narrow symmetric UIKit bridge (`didEnterBackground`/`willEnterForeground`, or `willResignActive`/`didBecomeActive` if inactive is the intended policy) instead of guessing.

On a verified true-background transition: pause ticker forwarding, retain bounded log tails/subscription ids, and let connections idle. On resume: re-dial bootstrap, resume the existing delivery registrations, re-read runtime permissions and type scale, then emit `RuntimeCapsChanged` / `TypeScaleChanged`. The UI refetches screen state but does **not** call `settle_intent` again. Without this, returning from background either shows a permanently dead mesh or duplicates a transaction.

`relay_bytes` stays an `AtomicU64` with `Ordering::Relaxed` — a monotonic counter read for display needs no synchronization with other memory (`conc-atomic-ordering`).

#### 2.7.1 Relay bootstrap — an infra deliverable

Decision: **you host one relay; its address ships as the default.**

```rust
// crates/cabal-mesh/src/config.rs

/// Bootstrap peers dialled at startup and re-dialled on resume.
/// The compiled-in default is the project relay; a user override in
/// Profile replaces it entirely (it does not append).
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct BootstrapConfig {
    #[serde(default = "BootstrapConfig::default_relays")]
    pub relays: Vec<Multiaddr>,
}
```
`serde-default-compat` — an older config file missing the field still deserializes and picks up the current default, which matters when the relay is redeployed at a new address.

What has to exist outside this repo:

| Item | Note |
|---|---|
| Host | Small VPS. `rust-libp2p` relay server, ~30 MB RSS. |
| Ports | UDP (QUIC) + TCP, both reachable; no NAT in front. |
| Identity | A **stable** ed25519 keypair — the peer id is part of the multiaddr baked into the app. Rotating it strands every installed build. Back it up. |
| Limits | `relay::Config` reservation and circuit caps set deliberately; an unbounded relay is an open proxy. |
| Deploy | Pin the version. A relay running a different libp2p protocol revision silently fails to reserve. |

DCUtR promotes the relayed connection to direct whenever hole punching succeeds, so the relay carries handshake traffic rather than the whole mesh — but it is a **single point of failure for off-LAN discovery**, and it sees which peer ids are online together. That is a real privacy surface for a product whose thesis is zero identity, and it belongs in whatever the app says about itself.

### 2.8 Chain (fixes S6, B10)

`BlockchainBridge` (1,301 lines) splits by feature (`proj-mod-by-feature`):

| New module | Takes over |
|---|---|
| `cabal-chain::provider` | RPC client, reachability, chain cache |
| `cabal-chain::contracts` | escrow / marketplace / voucher calls (ABIs stay in `abi/`) |
| `cabal-chain::relay_queue` | pending + relayed tx queues, pruning, boost |
| `cabal-vault::identity` | identities, session keys, import/export |
| `cabal-chain::content` | content store + signature verify (desktop feature-gated — not in the mobile UI) |

Offline-first is not optional on mobile. The artifact has an explicit `OFFLINE MODE` switch on the profile screen whose toast reads *"Intents queue locally. Nothing leaves this device."* — so the queue is a first-class type, not an afterthought:

```rust
/// A transaction awaiting connectivity. Persisted; survives process death.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct QueuedTx {
    pub id: TxId,
    pub created_at: DateTime<Utc>,
    pub attempts: u8,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub last_error: Option<String>,
    pub payload: RawTx,
}
```
`serde-rename-all`, `serde-skip-empty`.

### 2.9 Observability (fixes S5)

Replace all 40-odd `println!`/`eprintln!` with `tracing` (`obs-tracing-over-log`, `obs-structured-fields`):

```rust
#[tracing::instrument(skip(self, intent), fields(intent_id = %intent.id))]
pub async fn publish(&self, intent: WireIntent) -> Result<(), MeshError> { … }
```

Subscriber wiring is platform-specific and is the only reason device debugging is tractable:
- Android → `tracing-android` / `tracing-logcat` → visible in `adb logcat`.
- iOS → OSLog via `tracing-oslog` → visible in Console.app.
- Desktop → `tracing-subscriber` + `EnvFilter` / `RUST_LOG`.

Libraries (`cabal-*`) emit only; the app crate installs the subscriber (`obs-library-facade`).

### 2.10 Keeping the frozen desktop UI alive

"Freeze desktop, don't touch it" and "reshape 50 commands into 28" are in direct conflict: `src/App.tsx` invokes the current names with the current shapes, and every one of them changes — `Result<T, String>` becomes `Result<T, AppError>`, `String` ids become newtypes, `mint_voucher`/`create_asset_listing`/`store_content` lose their home. Doing nothing means the desktop app stops working the moment Phase 2 lands, which is not what "frozen" means.

So freezing is a **commitment to maintain a compatibility layer**, not the absence of work:

```
crates/cabal-legacy/          # feature = "desktop-legacy", desktop targets only
└── src/
    ├── commands.rs           # the original 47 names, original signatures
    └── adapt.rs              # legacy shapes <-> cabal-core types
```

Rules for this crate:

- **Signatures are frozen verbatim** — including `Result<T, String>`. The adapter calls the new service, then flattens `AppError` back to a string via `Display` at the very edge. The frozen UI never sees the new error union.
- **It is the only place stringly-typed shapes are allowed** (`anti-stringly-typed` is deliberately suspended here, and nowhere else). Conversions live in `adapt.rs` as `From`/`TryFrom` impls (`api-from-not-into`, `conv-tryfrom-fallible`), so the boundary is one reviewable file rather than 47 scattered casts.
- **Registered only on desktop.** `tauri::generate_handler!` gets two lists: the 28 new handlers always, plus the 50 legacy ones under `#[cfg(all(desktop, feature = "desktop-legacy"))]`. Mobile capabilities grant only the 25 handlers used by its webview (§5.2.1).
- **Marketplace / voucher / content stay here.** They have no mobile screen; this is their home rather than deletion.
- **Snapshot-tested, not unit-tested.** `insta` over each legacy command's serialized output, captured **before** the refactor starts (Phase 1, against today's code) and asserted after. That is the only mechanical guarantee that "frozen" held.

Cost: ~1.5 days, and it grows every time a service signature moves. The honest alternative is retiring the desktop UI, which removes the crate entirely — worth revisiting once the mobile app is real.

---

## 3. Dependency changes

```toml
# src-tauri/Cargo.toml  (workspace root)
[workspace]
members = ["crates/*"]
resolver = "3"

[workspace.package]
edition      = "2024"
rust-version = "1.85"          # MSRV, tested in CI  (proj-msrv-declare)

[workspace.dependencies]
serde      = { version = "1", features = ["derive"] }
serde_json = "1"
thiserror  = "2"
tracing    = "0.1"
tokio      = { version = "1", default-features = false, features = ["rt-multi-thread", "macros", "sync", "time", "fs"] }

# TLS: rustls everywhere. Android ships no OpenSSL.        (B6)
reqwest = { version = "0.12", default-features = false, features = ["json", "rustls-tls"] }

libp2p = { version = "0.54", default-features = false, features = [
  "tokio", "quic", "tcp", "noise", "yamux",
  "gossipsub", "mdns", "identify", "ping", "relay", "dcutr", "macros",
] }

# Trimmed from "full".  Verify feature names against alloy 1.8.  (B10)
alloy = { version = "1", default-features = false, features = [
  "provider-http", "signer-local", "contract", "rpc-types-eth", "json-abi",
] }

[workspace.lints.rust]
unsafe_code       = "forbid"
missing_docs      = "warn"
unexpected_cfgs   = { level = "warn", check-cfg = ['cfg(mobile)', 'cfg(desktop)'] }

[workspace.lints.clippy]
correctness = { level = "deny",  priority = -1 }
suspicious  = { level = "warn",  priority = -1 }
perf        = { level = "warn",  priority = -1 }
unwrap_used = "warn"
expect_used = "warn"
```
`proj-workspace-deps`, `lint-workspace-lints`, `lint-deny-correctness`, `lint-cfg-check`, `unsafe-*` (nothing here needs `unsafe`, so forbid it outright).

**Removed:** `keyring` (B1, unused), `dotenv` (moved to a desktop dev-dependency), `dirs` (B4).
**Added:** `tracing`, `tracing-subscriber`, `thiserror`, `tracing-android` (android target), `tracing-oslog` (ios target), `argon2` (desktop vault KDF), `tempfile` (dev), `proptest` (dev), `insta` (dev).

Release profile (`opt-lto-release`, `opt-codegen-units`, `perf-release-profile`) — mobile binary size is a shipping constraint, not a nicety:

```toml
[profile.release]
opt-level     = "s"        # size over speed on mobile; "3" for desktop-only builds
lto           = "fat"
codegen-units = 1
panic         = "abort"
strip         = true

[profile.dev.package."*"]
opt-level = 3              # optimized deps; a debug-build libp2p is unusably slow on device
```

---

## 4. Feature gating for ZK / LLM

> **Superseded for ZK (2026-08-27).** The ZK handler and circuit were deleted as unused,
> so the `generate_zk_bid_proof` gating below describes a command that no longer exists.
> The LLM half still stands, and the pattern is the one to reuse if proving returns.

```rust
// src/commands/intents.rs

/// Generate a ZK bid proof.
///
/// # Errors
/// [`AppError::Unsupported`] on mobile — the Noir toolchain is desktop-only.
#[tauri::command]
pub async fn generate_zk_bid_proof(req: ProofRequest, state: State<'_, AppState>)
    -> Result<ZkProof, AppError>
{
    #[cfg(desktop)]
    { state.zk.prove(req).await.map_err(Into::into) }

    #[cfg(mobile)]
    { let _ = (req, state); Err(AppError::Unsupported { feature: "zk_proof" }) }
}
```

The typed name is deliberately `generate_zk_bid_proof`: the frozen 50-command legacy surface already owns `generate_zk_proof` with a different signature. Tauri cannot register two invoke handlers under one command name; the legacy wrapper keeps its name, while the new contract gets an unambiguous one.

And a single probe so the frontend can hide affordances rather than discover failure by tapping:

```rust
#[derive(Debug, Clone, Copy, Serialize, TS)]
#[serde(rename_all = "camelCase")]
pub struct PlatformCaps {
    pub zk_proving: bool,
    pub local_llm: bool,
    pub background_mesh: bool,  // false on iOS + Android until a background service exists
}
```
Permission grants and connectivity live in `RuntimeCaps`, not here — see §2.3.

**Correction — Cargo has no "target-conditional workspace members".** An earlier draft claimed that; it does not exist. `members = ["crates/*"]` defines membership unconditionally, and Tauri's custom `cfg(desktop)`/`cfg(mobile)` are set by `tauri-build` at compile time — far too late to influence dependency resolution.

The mechanism that does work is **target-specific dependencies** on the app crate:

```toml
# src-tauri/Cargo.toml — cabal-ai and cabal-zk stay ordinary workspace members
[target.'cfg(any(target_os = "macos", target_os = "windows", target_os = "linux"))'.dependencies]
cabal-ai = { path = "crates/cabal-ai" }
cabal-zk = { path = "crates/cabal-zk" }
```

Consequences to plan around, not discover:

- **They are still resolved**, into the shared dependency graph and `Cargo.lock` — Cargo's resolver is target-agnostic. What changes is that they are **not compiled or linked** for a mobile target. That is the real win (binary size, link time, cross-compile surface), but it does not shrink the lockfile, and a dependency that fails to *resolve* on any platform still breaks every platform. [Cargo resolver](https://doc.rust-lang.org/cargo/reference/resolver.html).
- Call sites still need `#[cfg(desktop)]`, because the `use` statements do not exist on mobile.
- **`cargo test --workspace` still builds and tests them on the host** — membership is unconditional. That is fine (they are desktop crates being tested on a desktop) but it means workspace-wide CI timings do not shrink.
- CI must run `cargo build --target aarch64-linux-android -p cabalmesh` to actually prove the exclusion; a host build proves nothing.

Refs: [Cargo workspaces](https://doc.rust-lang.org/cargo/reference/workspaces.html), [target-specific dependencies](https://doc.rust-lang.org/cargo/reference/specifying-dependencies.html).

---

## 5. Mobile platform configuration

### 5.1 Tauri config

**Correction on `app.windows`.** An earlier draft asserted it "applies to desktop only". The config reference does not say that, and 2.11 explicitly *adds* mobile monitor APIs. The accurate statement: the **geometry and decoration fields** (`width`, `height`, `resizable`, `decorations`, `titleBarStyle`, …) have no meaning on mobile, while `label` and `url` are still honoured — mobile has a labelled main webview, which is why capabilities keep `windows: ["main"]` (§5.2). Treat any specific field as desktop-only unless verified on device.

**Selecting the mobile frontend — `frontendDist` is a directory, not an entry file.** The reference is explicit:

> "The path to the application assets … When a path relative to the configuration file is provided, it is read recursively and all files are embedded in the application binary. Tauri then looks for an `index.html` and serves it as the default entry point."

So the two-entry idea of emitting `index.html` + `mobile.html` into one `dist/` and "pointing `frontendDist` at the mobile entry" **cannot work** — Tauri would load `index.html`, i.e. the desktop UI, on the phone. Build into two directories instead, each with its own `index.html`:

```
dist-desktop/index.html
dist-mobile/index.html
```
```jsonc
// tauri.conf.json          → "frontendDist": "../dist-desktop"
// tauri.ios.conf.json      → "frontendDist": "../dist-mobile"
// tauri.android.conf.json  → "frontendDist": "../dist-mobile"
```

Platform overlays use the documented filenames and merge into `tauri.conf.json` by **JSON Merge Patch (RFC 7396)**: `tauri.ios.conf.json`, `tauri.android.conf.json`. Vite emits both directories; the UI plan §3.2 covers the build wiring.

```jsonc
// tauri.conf.json
{
  "bundle": {
    "iOS": {
      // minimumSystemVersion already defaults to "14.0" — pinned explicitly
      // so a Tauri default change can't move our mDNS floor silently.
      "minimumSystemVersion": "14.0",
      "infoPlist": "./Info.plist"
      // developmentTeam intentionally omitted: supply it via the
      // APPLE_DEVELOPMENT_TEAM env var so a Team ID never lands in git.
    },
    "android": {
      "minSdkVersion": 24,            // also the Tauri default (Android 7.0)
      "autoIncrementVersionCode": true
    }
  }
}
```

iOS 14 is the floor because the Local Network permission prompt (needed for mDNS) arrived there — and it happens to match Tauri's own default.

Tauri's `bundle.android` schema exposes `minSdkVersion`, but not `targetSdkVersion`. After `android init`, Phase 0 therefore pins and reviews `compileSdk` / `targetSdk` in the generated, version-controlled `src-tauri/gen/android/app/build.gradle.kts`; it must not silently inherit whatever the installed Android Gradle Plugin happens to choose. The exact target is recorded with the SDK/NDK toolchain. This matters because the local-network permission contract changes at target SDK 37 (§5.3).

### 5.2 Capabilities (B9)

Allowed `platforms` values are `linux`, `macOS`, `windows`, `iOS`, `android` — **case-sensitive** exactly as written. Desktop and mobile reference different generated schemas:

**Capability files are auto-enabled unless the config names identifiers explicitly, and a window in several capabilities receives the *union* of their permissions.** So leaving today's `capabilities/default.json` in place while adding platform files does not scope anything — mobile would inherit `opener:default` from the shared file regardless of any `platforms` key on the new ones.

Therefore: **delete `default.json`**, ship one capability per platform, and name them explicitly in config.

```jsonc
// capabilities/mobile.json
{
  "$schema": "../gen/schemas/mobile-schema.json",
  "identifier": "mobile",
  "platforms": ["iOS", "android"],
  "windows": ["main"],
  "permissions": [
    "core:event:allow-listen",
    "core:event:allow-unlisten"
    // Phase 3 adds "type-scale:allow-get-scale" when that plugin exists.
    // Phase 6 appends the generated allow-* permissions for the exact
    // 25-command mobile webview surface; never add future commands early.
  ]
}
```
```jsonc
// capabilities/desktop.json
{
  "$schema": "../gen/schemas/desktop-schema.json",
  "identifier": "desktop",
  "platforms": ["linux", "macOS", "windows"],
  "windows": ["main"],
  "permissions": [
    "core:default",
    "opener:default"
    // Phase 0 appends generated allow-* permissions for all 47 current
    // commands before AppManifest is enabled. Phase 6 adds relevant new ones.
  ]
}
```

Select the identifiers explicitly; the overlay array replaces the base array under JSON Merge Patch:

```jsonc
// tauri.conf.json (base / desktop)
{ "app": { "security": { "capabilities": ["desktop"] } } }

// tauri.ios.conf.json and tauri.android.conf.json
{ "app": { "security": { "capabilities": ["mobile"] } } }
```

Two corrections to an earlier draft:

- **`core:event:default` and `core:path:default` were redundant beside `core:default`.** The umbrella already bundles app, event, image, menu, path, resources, tray, webview and window defaults. Mobile therefore drops the umbrella and grants only what current frontend imports use. The planned bundle only listens/unlistens; `core:event:default` would additionally authorize frontend `emit`/`emit_to`, so the baseline is the two granular permissions above. Add another core permission only with an import/call inventory proving it is needed. [Core permission table](https://v2.tauri.app/reference/acl/core-permissions/).
- `windows: ["main"]` is retained — mobile does have a labelled main webview.

The generated desktop/mobile schemas improve validation and editor completion, but `$schema` does not authorize or deny anything at runtime. Explicit capability selection plus the permission list is the security boundary.

### 5.2.1 Security hardening — currently unconfigured

The UI plan reasons about a "strict CSP" blocking CDN fetches. **There is no CSP.** `src-tauri/tauri.conf.json` ships `"csp": null` and `"withGlobalTauri": true`, and no phase configured either. That gap is now Phase 0 work:

```jsonc
// tauri.conf.json → app.security
{
  "csp": {
    "default-src": "'self'",
    "script-src":  "'self'",
    "style-src":   "'self' 'unsafe-inline'",   // DS uses inline style objects
    "img-src":     "'self' data:",             // glyphs ship in the Vite bundle
    "font-src":    "'self' data:",             // self-hosted woff2 only
    "connect-src": "'self' ipc: http://ipc.localhost"
  },
  "devCsp": {
    "default-src": "'self'",
    "script-src":  "'self'",
    "style-src":   "'self' 'unsafe-inline'",
    "img-src":     "'self' data:",
    "font-src":    "'self' data:",
    "connect-src": "'self' ipc: http://ipc.localhost ws:"
  },
  "freezePrototype": true
}
```

- **Set `app.withGlobalTauri: false`** (it is a sibling of `app.security`, not a key inside the snippet above). The frontend already imports `@tauri-apps/api`; removing the convenience global is defense-in-depth and API hygiene. Capabilities/AppManifest remain the authority boundary.
- **Do not add the Avalanche RPC host to webview CSP.** Chain requests stay in Rust and are not governed by the webview's `connect-src`. Add a host only if a future, reviewed architecture deliberately moves that network call into frontend code.
- **HMR is development-only authority.** Production `csp` has no WebSocket source. `devCsp` adds `ws:` solely so the device can reach Vite HMR on 1421; never copy that source into production. Tauri documents `devCsp` as the policy injected during development. [CSP](https://v2.tauri.app/security/csp/) · [SecurityConfig](https://v2.tauri.app/reference/config/#securityconfig).
- **Least privilege for the app's own commands — and mind the ordering trap.** `tauri_build::AppManifest::commands` generates `allow-*`/`deny-*` permissions, but a generated permission does nothing until a capability *references* it. Declaring a manifest without granting is how you lock yourself out.

  The trap: Phase 0 runs against **today's 50 commands** (the legacy adapter does not exist until Phase 1), while the 28 new handlers do not exist yet. So Phase 0 must inventory and grant the **50 current commands** in `capabilities/desktop.json`, or the desktop app starts failing IPC the moment the manifest lands. Phase 6 adds all 28 handlers to the manifest, but grants mobile only the **25 commands its webview uses**; the three ZK/LLM handlers remain ungranted mobile stubs. Never grant future commands speculatively.

  Concretely: Phase 0 `desktop.json` grants the current 47; Phase 0 mobile launches only a static no-IPC probe page. After Phase 6, desktop grants the relevant new commands plus the 50 legacy commands, while mobile grants the exact 25-command UI surface. Nothing is granted on both unless a real screen calls it. This matters more after 2.11.1, not less — that release closed the "no AppManifest ⇒ no ACL for remote origins" hole, and relying on the old behaviour was never safe. Refs: [AppManifest 2.6.3](https://docs.rs/tauri-build/2.6.3/tauri_build/struct.AppManifest.html), [Permissions](https://v2.tauri.app/security/permissions/).
- **Validate every IPC input in Rust.** `IntentDraft`, `IntentId`, addresses and amounts parse into validated types at the boundary (`api-parse-dont-validate`); nothing downstream accepts a raw `String` from the webview.

**Upgrade Tauri to the 2.11 line before `tauri android init`.** The repo is on 2.9.5. `tauri@2.11.1` (2026-05-06) carries two security fixes that both bear on this app:

1. ACL checks are now enforced for IPC from remote origins **even with no `AppManifest` configured** — previously custom commands could bypass access control entirely.
2. `.localhost` suffix handling corrected on **Windows and Android**: a remote site could previously be misclassified as local when it matched a registered custom scheme (e.g. `http://app.evil.com/` reading as local when an `app` protocol exists).

Refs: [Tauri CSP](https://v2.tauri.app/security/csp/), [Capabilities](https://v2.tauri.app/security/capabilities/), [v2.11.1](https://v2.tauri.app/release/tauri/v2.11.1/).

### 5.3 Native permissions

**iOS — `src-tauri/Info.plist`**, referenced from `bundle.iOS.infoPlist` and merged with Tauri's generated one. Editing the file under `src-tauri/gen/apple/` instead would be overwritten by `tauri ios init`.

```xml
<key>NSLocalNetworkUsageDescription</key>
<string>CabalMesh discovers nearby nodes on your local network. No identity is attached.</string>
<key>NSBonjourServices</key>
<array><string>_p2p._udp</string></array>

<!-- Export compliance: DO NOT hardcode a value yet. See note below. -->
```

Those plist declarations explain the access and declare the one service rust-libp2p advertises; they do **not** authorize its raw multicast socket. `libp2p-mdns` joins `224.0.0.251:5353`, so an iOS build that enables it also needs this restricted entitlement in the signed entitlements file and provisioning profile:

```xml
<key>com.apple.developer.networking.multicast</key>
<true/>
```

Apple must approve that managed entitlement. Phase 0 therefore chooses one of three honest paths before LAN discovery work begins:

1. **Preferred:** request/obtain the entitlement and use rust-libp2p mDNS.
2. **Schedule-expanding fallback:** prototype a native Bonjour/Network.framework bridge and prove wire compatibility; this adds a fourth mobile plugin.
3. **Release fallback:** disable iOS mDNS, report `LocalDiscoveryState::Disabled { reason: RelayOnlyBuild }`, and use bootstrap/relay. Never ship a UI that claims local discovery while packets are blocked.

[Apple TN3179](https://developer.apple.com/documentation/technotes/tn3179-understanding-local-network-privacy) is the Phase 0 source of truth; entitlement review lead time is external and not included in engineering days.

**Export compliance is a release gate, not a config line.** An earlier draft set `ITSAppUsesNonExemptEncryption` to `true` merely because the app ships AES-256-GCM and Noise. That inference is invalid: the key asks whether the encryption is **non-exempt**. Standard algorithms implemented outside Apple's OS prove neither exemption nor non-exemption, and some distribution (including France) may require additional documentation. If the classified answer is non-exempt, Apple says a compliance code is typically also required; do not pre-fill either value.

Sequence it properly:
1. Work App Store Connect's export-compliance questionnaire against what this app actually does (AES-256-GCM at rest, Noise in transit, ZK tooling excluded from mobile).
2. Determine exempt vs non-exempt from that outcome.
3. Only then set the key — and add `ITSEncryptionExportComplianceCode` when App Store Connect requires it.

Blocking a release on an unanswered questionnaire is normal; shipping a wrong self-declaration is a compliance problem. Refs: [ITSAppUsesNonExemptEncryption](https://developer.apple.com/documentation/bundleresources/information-property-list/itsappusesnonexemptencryption), [Export compliance overview](https://developer.apple.com/help/app-store-connect/manage-app-information/overview-of-export-compliance).
Without the complete approved configuration for the chosen path — usage declaration plus the multicast entitlement/profile for raw rust-libp2p mDNS, or the relevant Bonjour declaration for a native bridge — iOS can yield zero peers without a useful application error. The Phase 0 entitlement/provisioning probe exists to prevent that silent failure from reaching Phase 4.

**Android — `src-tauri/gen/android/app/src/main/AndroidManifest.xml`.** This tree is version-controlled (the repo `.gitignore` excludes only `src-tauri/gen/schemas`), so edits persist; re-running `tauri android init` is the thing to be careful with, not ordinary builds.

```xml
<uses-permission android:name="android.permission.INTERNET"/>
<uses-permission android:name="android.permission.ACCESS_NETWORK_STATE"/>
<uses-permission android:name="android.permission.CHANGE_WIFI_MULTICAST_STATE"/>
<uses-permission android:name="android.permission.ACCESS_WIFI_STATE"/>
```

The local-network declaration is **conditional on the pinned target SDK**, not a permission pasted into every manifest:

- **Target SDK ≤ 36:** `INTERNET` still grants ordinary LAN access. Do **not** declare or request `ACCESS_LOCAL_NETWORK`. To exercise Android 16's opt-in restriction before raising the target, use a debug-only manifest overlay declaring `NEARBY_WIFI_DEVICES`, enable `RESTRICT_LOCAL_NETWORK` with `adb shell am compat enable RESTRICT_LOCAL_NETWORK <package>`, reboot, then test both deny and grant. Remove/disable that test overlay unless another Wi-Fi API genuinely needs it.
- **Target SDK ≥ 37 (Android 17+):** raw UDP multicast and direct LAN sockets are blocked by default. Add `<uses-permission android:name="android.permission.ACCESS_LOCAL_NETWORK"/>`, expose `PermissionRequired` before the first just-in-time request, and request it **before** starting local discovery. After a refusal or later revocation, report `Denied` and do not prompt in a loop; relay connectivity remains available. Because the Rust swarm needs broad raw-socket discovery rather than one user-selected service, Android's `NsdManager` picker is not a drop-in escape hatch; adopting it would be a separate native-discovery design with wire-compatibility proof.

This split follows Android's [Local Network Protection guidance](https://developer.android.com/privacy-and-security/local-network-permission): SDK 36 uses `NEARBY_WIFI_DEVICES` only for the Android 16 opt-in test, while SDK 37+ uses the new runtime `ACCESS_LOCAL_NETWORK` permission. UDP denial normally surfaces as `EPERM`; that is a permission signal, not “zero peers.”

The Kotlin Tauri plugin still acquires a non-reference-counted `WifiManager.MulticastLock` while the foreground mesh is active and releases it on verified pause/shutdown. Android's newer automatic multicast handling is documented for `NsdManager`; rust-libp2p opens raw UDP sockets, so do not assume that optimization applies. Test raw receive on both a pre-T-extension-7 device and a current Android device before considering removal. The [MulticastLock contract](https://developer.android.com/reference/android/net/wifi/WifiManager.MulticastLock) also makes the battery constraint explicit: never hold it merely because the app process exists.

Platform setup and discovery probes feed `RuntimeCaps.local_discovery` (§2.3), rerun on every verified `Resumed`. Android reports the applicable runtime permission, lock and `EPERM` state directly. Baseline iOS raw UDP cannot distinguish a denied Local Network privilege from “no peers answered”, so it reports `Indeterminate` rather than inventing a denial reason; a native Bonjour bridge can surface a specific policy-denied error. The `nodes` screen renders the honest state and always reports relay reachability separately.

### 5.3.1 The plugins this app needs

**Three in the baseline** — down from four, since 2.11 supplies lifecycle natively (§2.7). All follow the documented mobile-plugin shape (`@TauriPlugin` + `@Command` + `invoke.resolve` on Android, `Plugin` subclass + `@objc` on iOS, `run_mobile_plugin("name", payload)` from Rust). A narrow symmetric iOS lifecycle bridge or native Bonjour bridge is added only if its Phase 0 gate fails; either is separately scoped fallback work, not a generic lifecycle plugin kept “just in case.”

| Plugin | Called by | Reaches the webview? | Android | iOS |
|---|---|---|---|---|
| `multicast-lock` | **Rust only** (mesh actor) | ❌ no capability grant | target-aware Local Network request/settings route + `WifiManager.MulticastLock` | no-op |
| `keystore` | **Rust only** (vault) | ❌ no capability grant | `AndroidKeyStore`, StrongBox when available | Keychain, `kSecAttrAccessibleAfterFirstUnlockThisDeviceOnly` |
| `type-scale` | frontend at boot; Rust on resume | ✅ `type-scale:allow-get-scale` | `Resources.getConfiguration().fontScale` | `UIFontMetrics` scale ratio for current content-size category |

**Of the native plugins, only `type-scale` gets a direct webview grant.** An earlier draft listed `keystore:allow-unwrap-key` and `multicast-lock:*` in the capability permissions — that would hand the webview the ability to unwrap vault key material and toggle radio state. Both are Rust-internal implementation details invoked through `run_mobile_plugin`, never through plugin IPC. The webview reaches local-network UX only through the narrow, state-validated app command in §6. Granting a native-plugin permission the frontend does not need is the definition of over-grant, and this one is severe.

Contract rules:

- `type-scale:allow-get-scale` is added to `capabilities/mobile.json` when the plugin lands in Phase 3. It authorizes the frontend's initial direct read; without it the invocation promise rejects with an ACL error and the UI must catch that error and fall back to scale 1.
- **`type-scale` has two deliberate callers, not conflicting owners.** The frontend reads once at boot to avoid a flash at the wrong size. Rust re-reads through its plugin handle on the verified runtime `Resumed` path (§2.7) and emits `TypeScaleChanged`; the root `--type-scale` updates live. No plugin subscribe permission is needed — the push travels on the existing typed event bus.
- On Android SDK 37+, the frontend calls the granted app command `nearby_nodes(LocalAccessAction)`; Rust alone calls the `multicast-lock` plugin. `Observe` never prompts, `Request` is accepted only from `PermissionRequired`, and `OpenSettings` only from `Denied`. The latter opens Android's app-details settings natively, not through `tauri-plugin-opener`, so mobile still needs no opener permission. Plugin results toggle mDNS and update `RuntimeCaps`. This keeps native permission/settings methods off the webview ACL while preserving explicit user initiation.
- Each plugin defines a no-op desktop implementation so `PlatformCaps` is the only place platform branching lives.

### 5.4 Toolchain gaps on this machine

Re-verified 2026-08-01. This table has now been wrong twice; treat it as a snapshot and re-run the checks rather than trusting it.

| Need | Status |
|---|---|
| iOS targets: `aarch64-apple-ios`, `x86_64-apple-ios`, `aarch64-apple-ios-sim` | ✅ all three installed |
| Cocoapods | ✅ `/opt/homebrew/bin/pod` |
| `src-tauri/gen/apple` | ✅ **exists** — `ios init` has been run |
| macOS host | ⚠️ **14.6.1 — cannot install Xcode 26** (requires macOS 15.6+) |
| Xcode | ⚠️ **15.4 — cannot ship** (see release blocker below) |
| Android targets ×4 | ❌ none installed |
| `JAVA_HOME`, `ANDROID_HOME`, `NDK_HOME` | ❌ all unset |
| Android Studio: SDK Platform, Platform-Tools, **NDK (Side by side)**, Build-Tools, Command-line Tools | ❌ absent |
| `src-tauri/gen/android` | ❌ absent |
| `cargo-tauri` | ❌ **not installed** — only the npm CLI is present |
| Tauri release set | ⚠️ current: Rust 2.9.5 / build 2.5.3 / JS API 2.9.1 / npm CLI 2.9.6 / opener 2.5.3. Target exact pins are below. |
| Rust | ⚠️ `1.99.0-nightly`. Replace it with an exact stable channel in `rust-toolchain.toml`; `channel = "stable"` is floating, not a pin. |

**Exact compatible target pins (official release index, checked 2026-08-01):**

```toml
# Cargo.toml
tauri = { version = "=2.11.5", features = [] }
tauri-plugin-opener = "=2.5.4" # remove entirely if the frozen desktop stops using it

[build-dependencies]
tauri-build = { version = "=2.6.3", features = [] }
```

```jsonc
// package.json — bare versions, no ^/~
"@tauri-apps/api": "2.11.1",
"@tauri-apps/cli": "2.11.4",
"@tauri-apps/plugin-opener": "2.5.4" // remove if no JS import remains
```

These packages version independently: `tauri-build` and plugins do **not** acquire a fictitious 2.11 version. Tauri core and the JS API stay on the same 2.11 minor; each official plugin's Rust/npm pair must match exactly. Cargo needs the leading `=` because a bare `"2.11.5"` is caret-compatible, not an exact manifest pin. Refs: [official release index](https://v2.tauri.app/release/) · [updating dependencies](https://v2.tauri.app/develop/updating-dependencies/) · [CLI 2.11.4](https://v2.tauri.app/release/@tauri-apps/cli/v2.11.4/).

**Release blocker — macOS + Xcode.** Since 2026-04-28, App Store Connect uploads require Xcode 26+ and the iOS 26 SDK. Xcode 26 requires macOS 15.6+, while this host is on macOS 14.6.1 with Xcode 15.4. Xcode 15.4 remains useful for the Phase 0 iOS 17.5 simulator/cross-compile probe, but this machine **must upgrade macOS and Xcode** before it can validate the iOS 26 SDK or upload. This is a Phase 0 release-path decision, not a Phase 7 surprise. [Apple upload requirement](https://developer.apple.com/news/upcoming-requirements/?id=02032026a) · [Xcode system requirements](https://developer.apple.com/support/xcode/).

**Command form.** `cargo-tauri` is not installed, so the canonical form in this repo is:
```bash
npm run tauri -- ios dev
npm run tauri -- android build --apk --split-per-abi
```
Do not mix this with `cargo tauri …` in docs or scripts.

Remaining Android setup:
```bash
rustup target add aarch64-linux-android armv7-linux-androideabi i686-linux-android x86_64-linux-android
# Android Studio → SDK Manager: SDK Platform, Platform-Tools, NDK (Side by side), Build-Tools, Command-line Tools
export JAVA_HOME="/Applications/Android Studio.app/Contents/jbr/Contents/Home"
export ANDROID_HOME="$HOME/Library/Android/sdk"
export NDK_HOME="$ANDROID_HOME/ndk/<version>"
```

Sequencing consequence: **the iOS cross-compile go/no-go can run today** — `src-tauri/gen/apple` already exists. If `alloy`/`libp2p` fail to build for `aarch64-apple-ios`, that is discoverable within the hour, before any Android or Xcode-upgrade time is spent.

`src-tauri/gen/apple` was generated by CLI 2.9.6. After the coordinated 2.11 upgrade, back it up, rerun the 2.11 generator, and review the diff before reconciling project changes; do not assume generated Xcode files are version-neutral or overwrite signing edits blindly.

### 5.5 Dev loop

`vite.config.ts` already reads `TAURI_DEV_HOST` and binds the server + HMR to it — the piece a physical iOS device needs. **One bug there:** HMR reuses `PORT || 1420`, the same port as the dev server, where the Tauri template uses `1421`. Fix before the first device run or HMR will not attach.

```bash
npm run tauri -- android dev            # emulator or attached device
npm run tauri -- ios dev 'iPhone 15'    # named simulator
npm run tauri -- ios dev --open --host  # physical device, drive from Xcode
```
The CLI process must stay running when using `--open`.

---

## 6. Command surface (screen → Rust)

Reshaped from 47 ad-hoc commands to an exact **28-handler contract**: 25 screen/shared commands plus 3 feature commands. Everything returns `Result<T, AppError>`.

| Screen | Command | Signature |
|---|---|---|
| splash | `session_status` | `() -> SessionStatus` |
| splash | `create_anonymous_node` | `() -> NodeIdentity` |
| connecting | `enter_mesh` | `(on_line: Channel<LogLine>) -> SubscriptionId` — returns immediately, streams the handshake log; also emits `BootstrapProgress` |
| home | `mesh_snapshot` | `() -> MeshSnapshot` (node id, uptime, conn state, 3 stat tiles) |
| home | `subscribe_mesh_log` | `(on_line: Channel<LogLine>) -> SubscriptionId` — replays the retained tail, then streams live |
| intents | `list_intents` | `(filter: IntentFilter) -> Vec<IntentView>` — filter = Active / Pending / History |
| new | `intent_form_options` | `() -> FormOptions` (assets, conditions, modes + descriptions, privacy levels) |
| new | `preview_intent` | `(draft: IntentDraft) -> IntentPreview` — the confirm-dialog rows, server-computed |
| new | `broadcast_intent` | `(draft: IntentDraft) -> IntentId` |
| detail | `get_intent` | `(id: IntentId) -> IntentDetail` |
| detail | `cancel_intent` | `(id: IntentId) -> ()` |
| detail | `settle_intent` | `(id: IntentId, on_line: Channel<LogLine>) -> SubscriptionId` — returns immediately; streams the verification log |
| all | `unsubscribe` | `(id: SubscriptionId) -> ()` — idempotent; cancel semantics in §2.5.1 |
| settled | `get_proof` | `(id: IntentId) -> ProofView` |
| nodes | `nearby_nodes` | `(action: LocalAccessAction) -> NearbyNodesView` — `Observe` by default; `Request` / `OpenSettings` only from the matching Android state; see below and §6.1 |
| nodes | `inspect_node` | `(id: NodeId) -> NodeDetail` |
| vault | `vault_assets` | `() -> Vec<VaultRow>` |
| vault | `vault_identities` | `() -> Vec<VaultRow>` |
| vault | `vault_keys` | `() -> Vec<VaultRow>` |
| vault | `vault_total_value` | `() -> FormattedAmount` — only called on reveal; decimal string, never JSON `u128` |
| profile | `profile_summary` | `() -> ProfileView` (node id, settled count + delta, member since) |
| profile | `set_offline_mode` | `(offline: bool) -> ()` |
| profile | `leave_mesh` | `() -> ()` — shreds session, returns to splash |
| all | `platform_caps` | `() -> PlatformCaps` — static build facts only |
| all | `runtime_caps` | `() -> RuntimeCaps` — permission + connectivity state; re-read after resume |
| feature-gated | `generate_zk_bid_proof`, `analyze_content`, `match_intent` | real implementations on desktop; registered `Unsupported` stubs on mobile, but intentionally **not granted** to the mobile webview (direct JS receives ACL denial, not the stub error) |

`LocalAccessAction` is a constrained screen action, not a generic native-plugin escape hatch:

```rust
#[derive(Debug, Clone, Copy, Deserialize, TS)]
#[serde(rename_all = "camelCase")]
pub enum LocalAccessAction { Observe, Request, OpenSettings }

#[derive(Debug, Clone, Serialize, TS)]
#[serde(rename_all = "camelCase")]
pub struct NearbyNodesView {
    pub nodes: Vec<NodeSummary>,
    pub local_discovery: LocalDiscoveryState,
}
```

Rust rejects an action that does not match the current state (`Request` after denial, `OpenSettings` before denial). That makes ordinary renders side-effect-free and provides the promised Settings recovery path without adding a 26th mobile command or granting `opener:default`.

### 6.1 "Nearby" has no source — do not ship the prototype's kilometres

The prototype lists peers at `1.2 km`, `2.4 km`, `3.1 km`. A libp2p peer has a peer id and a multiaddr; it has **no coordinates**, and this app requests no location permission (`NSLocationWhenInUseUsageDescription` / `ACCESS_FINE_LOCATION` appear nowhere in §5.3, deliberately — asking for location in a zero-identity product would be self-refuting).

Turning canned kilometres into a rendered field would be a fabricated measurement, and the brand's own copy rules demand exact numbers. `NodeSummary` therefore carries what the mesh actually knows:

```rust
#[derive(Debug, Clone, Serialize, TS)]
#[serde(rename_all = "camelCase")]
pub struct NodeSummary {
    pub id: NodeId,
    pub latency_ms: Option<u16>,     // from libp2p ping
    pub hops: u8,                    // 1 = direct, >1 = relayed
    pub discovery_sources: Vec<DiscoverySource>, // Mdns | Bootstrap | Mesh
    pub connection: ConnectionKind,  // DirectQuic | DirectTcp | Relayed
    pub liveness: Liveness,          // Ok | Failed { slashed: bool }
    pub stake: Option<Box<str>>,
}

#[derive(Debug, Clone, Copy, Serialize, TS)]
#[serde(rename_all = "camelCase")]
pub enum DiscoverySource { Mdns, Bootstrap, Mesh }

#[derive(Debug, Clone, Copy, Serialize, TS)]
#[serde(rename_all = "camelCase")]
pub enum ConnectionKind { DirectQuic, DirectTcp, Relayed }
```

Discovery and connection are orthogonal: a peer can be found through mDNS, connected over direct QUIC, then redialled through a relay. Never collapse those facts back into a single `Transport` enum.

The `nodes` row then reads `41ms · DIRECT` or `RELAYED · 2 HOPS` where the prototype read `1.2 km` — same visual slot, same `DataTable` column, real data. Signal bars derive from `latency_ms`, not from a canned `bars` integer.

If genuine physical proximity is wanted later, it needs a location permission and a stated privacy trade-off — a product decision, not a formatting one.

**Deliberately not commands:** the prototype's `totalValue` masking (`✱✱✱✱✱`) and every colour constant. Masking is presentation; colours are design tokens. Rust returns the value and the semantic status, never a hex string — the current code returns `dot: BLUE` style data, and that would hard-code the palette into the backend.

**Not retired — relocated.** The 20 marketplace/voucher/content commands (`mint_voucher`, `create_asset_listing`, `store_content`, …) have no screen in this design, but the frozen desktop UI calls them. They move to `cabal-legacy` (§2.10) with signatures unchanged, registered only under `#[cfg(all(desktop, feature = "desktop-legacy"))]`. The mobile handler never sees them.

So the invoke surface is two lists, not one:

| List | Count | Registered on |
|---|---|---|
| Screen/shared commands | 25 | all targets; granted to the mobile webview |
| ZK/LLM feature commands | 3 | all targets; mobile Rust stubs exist, but mobile capability does not grant them |
| Legacy compatibility commands | 50 | desktop + `desktop-legacy` feature |

---

## 7. TypeScript contract (generated incrementally)

Generate `src/types/bindings.ts` from Rust with **ts-rs** (`#[derive(TS)]` beside `Serialize`) from Phase 1 onward: core types **and the complete screen DTO schema** in Phase 1, then errors/events in Phase 2. Phase 6 wires those types to handlers and treats an unexpected binding diff as contract drift. One deterministic output grows with the backend; UI fixtures never invent duplicate domain or boundary types.

```ts
export type IntentStatus =
  | { status: "DRAFT" }
  | { status: "BROADCAST";    route_len: number }
  | { status: "NEGOTIATING";  bids: number; best: string | null }
  | { status: "FINDING_ROUTE" }
  | { status: "WAITING" }
  | { status: "SETTLED";      proof: string; filled_at: string; elapsed_ms: number }
  | { status: "FAILED";       reason: FailureReason }
  | { status: "CANCELLED" };

// Log lines are NOT here — they arrive over Channel<LogLine> (§2.5).
export type AppEvent =
  | { type: "bootstrapProgress";   data: { phase: BootPhase; message: string; progress: number } }
  | { type: "meshStatsChanged";    data: MeshStats }
  | { type: "peersChanged";        data: { nearby: NodeSummary[] } }
  | { type: "intentUpdated";       data: IntentView }
  | { type: "runtimeCapsChanged";  data: RuntimeCaps }
  | { type: "typeScaleChanged";    data: { scale: number } }
  | { type: "toast";               data: { title: string; body: string; accent: ToastAccent } };
```

The frontend maps `status` → dot colour and `accent` → toast colour from its own token file. Design tokens observed in the artifact (`#FFFFFF / #BEBEBE / #7A7A7A / #3A3A3A`, accents `#00E5FF` blue, `#FF3B3B` red, `#9BFF00` green, Pixel Operator + IBM Plex Mono) belong there, not in Rust.

---

## 8. Testing

| Layer | Approach |
|---|---|
| `cabal-core` | Unit tests in `#[cfg(test)] mod tests` (`test-cfg-test-module`, `test-use-super`). **Proptest** the `IntentStatus` transition table — terminal states accept nothing, no cycle re-enters `Draft` (`test-proptest-properties`). |
| `TokenAmount` | Proptest round-trip `parse ↔ Display`; explicit overflow and precision-loss cases (`num-overflow-explicit`). |
| `cabal-store` | RAII temp-dir fixtures (`test-fixture-raii`); crash-during-write → previous file intact. |
| `cabal-mesh` | Two in-process swarms on a memory transport; assert gossip delivery and peer events. `#[tokio::test]` (`test-tokio-async`). |
| `cabal-chain` | Trait-abstract the provider, mock with `mockall` (`test-mockall-mocking`, `test-mock-traits`) — no live RPC in CI. |
| Command layer | Integration tests in `tests/` (`test-integration-dir`) against a `MockAppState`. |
| Serialization | `insta` snapshots of every `AppEvent` and `AppError` variant (`test-snapshot-testing`) — a shape change that would silently break the TS union fails CI instead. |
| Device | Manual smoke matrix: iOS simulator, one physical iPhone, one physical Android, two devices on one LAN for mesh, airplane-mode for offline queue. |

CI starts with `cargo fmt --check`, `cargo clippy --all-targets -- -D warnings` and `cargo test --workspace`. The cross-target matrix then covers **all 7 Rust targets**: the 4 Android targets on a runner with the pinned SDK/NDK, and `aarch64-apple-ios`, `aarch64-apple-ios-sim`, `x86_64-apple-ios` on macOS 15.6+ with Xcode 26 (`lint-rustfmt-check`). Rust target checks alone do not compile Kotlin/Gradle or Swift/Xcode, so CI also performs one full Android debug APK build (`npm run tauri -- android build --debug --apk --ci`) and one iOS simulator native build (`npm run tauri -- ios build --debug --target aarch64-sim --ci`). Signed store artifacts remain a protected release job.

---

## 9. Phased execution

Each phase ends green — compiling, tests passing, desktop app still runnable. No phase leaves the tree broken.

**Ordering correction.** An earlier draft put the legacy adapter at Phase 4b while Phase 2 already rewrote every command signature from `Result<T, String>` to `Result<T, AppError>`. That would have left the frozen desktop UI broken across Phases 2–4 — directly contradicting the guarantee above. **The adapter now lands at the end of Phase 1, before the first signature change.** Every refactor after it runs behind 50 intact handlers.

### Phase 0 — Unblock the build + security baseline (3–3.5 days)
- **Upgrade from Tauri core 2.9.5 to an exact compatible release set** before Android init and before regenerating iOS: pin each independently versioned component (`tauri`, `tauri-build`, JS API, npm CLI and official plugins) to the verified set in §5.4. Regenerate/reconcile `src-tauri/gen/apple` under that CLI instead of treating the 2.9.6 output as immutable.
- **Configure security that the plans have been assuming exists**: explicit CSP, `withGlobalTauri: false`, `freezePrototype: true`.
- **Capabilities: delete `default.json`**, add `desktop.json` + `mobile.json` with explicit identifiers named in config (auto-enable + permission union otherwise defeats the split — §5.2).
- **AppManifest must grant today's 50 commands, not tomorrow's 28.** The legacy adapter does not exist until Phase 1; declaring a manifest without inventorying the current handlers locks the desktop app out of its own IPC (§5.2.1). Mobile uses a temporary static **no-IPC probe page** in this phase; do not grant current desktop commands merely to make that page launch.
- **Decide both Apple paths now.** This host must move from macOS 14.6.1/Xcode 15.4 to macOS 15.6+/Xcode 26 before release. Separately, request the managed multicast entitlement or lock the initial iOS product to relay-only (§5.3–5.4). External approval lead time is tracked outside engineering days.
- **Run the lifecycle semantics gate on a physical iPhone.** A minimal Rust log probe records Tauri suspend/resume while opening/dismissing Control Center and Notification Center, handling an incoming-call/lock interruption, and truly backgrounding/foregrounding. The result chooses built-in events or the narrow symmetric UIKit bridge before either controls mesh/channel pausing (§2.7).
- Remove `keyring`; unify `reqwest` to 0.12 + rustls; trim `alloy` features.
- Put Ollama/nargo behind `#[cfg(desktop)]`; inject an app sandbox path and non-`.env` mobile config far enough to make the unrefactored probe safe. The full service extraction still belongs to later phases.
- Add `rust-toolchain.toml` with an **exact** stable version validated by the probe.
- Fix the HMR port collision in `vite.config.ts` (§5.5).
- Use `npm run tauri --` consistently; `cargo-tauri` is not installed (§5.4).
- **iOS first — `src-tauri/gen/apple` already exists**, so the cross-compile probe is answerable today.
- Then Android: 4 targets, Studio + SDK + NDK, `JAVA_HOME`/`ANDROID_HOME`/`NDK_HOME`, `android init`. Commit `src-tauri/gen/android`; explicitly pin and record generated `compileSdk` / `targetSdk`. If the release targets ≤36, add the debug-only `NEARBY_WIFI_DEVICES` overlay and run Android 16's `RESTRICT_LOCAL_NETWORK` compatibility mode now. If it targets 37+, install an API-37 emulator, record that `ACCESS_LOCAL_NETWORK` is the selected path, and make its blocked/granted proof a Phase 4 exit gate when the native plugin exists (§5.3).
- **Done when:** the Rust/native graph cross-compiles for one iOS device target and all four Android targets; static no-IPC mobile probes launch on a simulator + emulator; the Android target-SDK/local-network path is recorded and **either** the ≤36 compatibility probe reaches blocked/granted **or** the ≥37 permission proof is explicitly gated in Phase 4; the physical-iPhone lifecycle matrix has a recorded built-in-vs-bridge decision; the desktop app still starts and its 47 current calls pass the new ACL. This is the honest go/no-go — if `alloy` or `libp2p` will not cross-compile, everything after is wasted effort, so it gets found on day one. Shipping remains blocked until Xcode 26 and export compliance are resolved.

### Phase 1 — Workspace + domain + legacy adapter (4 days)
- **First, before touching anything:** capture the frozen-desktop contract (§2.10).
  **Snapshot the serialized shapes, not live output.** Many of the 50 commands need a live RPC, Ollama, `nargo` or a real mesh; snapshotting their runtime output is neither reproducible nor CI-safe. Instead: `insta` over each command's *serialized request/response types* driven by fixtures and mocked services. That is the contract the frozen UI actually depends on.
- Create the workspace; extract `cabal-core` with ids, `Action`, `ExecutionMode`, `Condition`, `PrivacyLevel`, `IntentStatus`, `TokenAmount`, `UsdPrice`.
- Add Tauri-free `cabal-contract` and define the full serialized 25-command screen/shared DTO surface from §6 now—including `SessionStatus`, `SubscriptionId`/`LogLine`, mesh/intent/form/proof/vault/profile views, `PlatformCaps`, `RuntimeCaps`, `LocalAccessAction`, `NearbyNodesView` and node discovery/connection types. Types existing does not register handlers or relax Phase 0 ACL ordering.
- Establish the `ts-rs` build step and emit core plus all screen DTOs to the canonical `src/types/bindings.ts`. UI Phases B–D consume this artifact; do not wait for command implementations in Phase 6.
- Proptest the transition table and amount parsing.
- **Build `cabal-legacy` now** (§2.10): 50 frozen signatures including `Result<T, String>`, `adapt.rs` conversions, `desktop-legacy` feature gating. At this point it is a pass-through — which is exactly why it is cheap to write here and expensive to write later.
- **Done when:** `cargo test -p cabal-core -p cabal-contract` passes; the untouched desktop UI runs against the workspace build; legacy contract snapshots are committed and green; every UI fixture DTO exists in a deterministic `src/types/bindings.ts`.

Everything from Phase 2 onward changes services *behind* this adapter. The snapshot suite is the regression gate on every subsequent phase.

### Phase 2 — Errors, events, tracing (2 days)
- `AppError` + per-crate `thiserror` enums; new command signatures return `Result<T, AppError>`. The legacy adapter flattens back to `String` at its edge, so the frozen UI sees no change.
- `AppEvent` enum + emitter; delete ad-hoc `emit` calls.
- Extend the existing binding generation with `AppError`, `AppEvent` and their payload types; `SubscriptionId`/`LogLine` already come from Phase 1's command contract. This is incremental generation, not a second handwritten contract.
- **Generic channel-delivery registry** (§2.5.1): `SubscriptionId`, idempotent `unsubscribe`, cancellation tokens, bounded/self-cleaning registry, and synthetic subscribe/unsubscribe/natural-completion tests. The three screen-command integrations do not exist until Phase 6.
- Swap all `println!`/`eprintln!` for `tracing`; wire platform subscribers.
- insta snapshots for every event/error variant.
- **Done when:** `adb logcat` and Console.app show structured spans from a device; legacy snapshots and regenerated error/event bindings stay green; the generic registry leaks nothing after 100 subscribe/unsubscribe and natural-completion cycles.

### Phase 3 — State + storage (2.5 days)
- Split `AppState` into handles; remove the global `Mutex`.
- Move `manage()` synchronously into `setup`; add `NotReady`.
- Extract `cabal-store` with injected paths + atomic writes; delete `dirs`.
- Extract `cabal-vault`; encrypt `identities.json` → `vault.enc`; redacted `Debug`.
- Keystore and type-scale plugins for iOS + Android. Only after `type-scale` exists, add its one frontend permission; the frontend reads at boot and Rust rereads on resume (§5.3.1).
- **Done when:** no plaintext key on disk; concurrent commands no longer serialize (assert with a two-slow-RPC test).

### Phase 4 — Mesh for mobile (3.5 days)
- Extract `cabal-mesh`; convert to the actor + `MeshHandle`.
- Add QUIC, `identify`, `ping`, `relay`, `dcutr`; `Toggle`-wrap mDNS. Implement Wi-Fi/cellular recovery as disconnect → redial → rejoin → replay/dedupe; do not rely on QUIC migration.
- Bootstrap multiaddr config; verified lifecycle pause/resume with retained subscription ids.
- Implement the internal `RuntimeCapsHandle` and lifecycle probes here, alongside the mesh state they observe. Native permission/socket/relay changes update the Phase 1 contract types before any IPC wrapper exists; Phase 6 only exposes the handle and command over IPC.
- Native permissions (Info.plist, conditional Android manifest/runtime flow, multicast-lock plugin). For Android target SDK 37+, implement `nearby_nodes(Request/OpenSettings)` through the Rust-only plugin, request `ACCESS_LOCAL_NETWORK` before raw LAN discovery, and map denial/revocation to `LocalDiscoveryState::Denied`; for ≤36, never request that permission and keep the Android 16 compat probe. On iOS, enable mDNS only when the signed provisioning profile contains Apple's multicast entitlement; otherwise this release is relay-only.
- **Deploy the relay** (§2.7.1): VPS, stable keypair backed up, reservation/circuit limits set, version pinned, address baked into `BootstrapConfig::default_relays`.
- **Done when:** two physical devices connect on different networks through the relay; the internal caps handle proves bounded Android transitions `PermissionRequired → Probing → Ready` with zero peers and `Available` after a response, plus deny/revoke/settings-return while relay remains usable; Android devices discover on LAN with the applicable permission granted; entitled iOS builds also discover on LAN (or the release is explicitly marked iOS relay-only). A Wi-Fi→cellular test reconnects, rejoins and suppresses duplicate replay. For target SDK 37+, this is also the permission proof deferred by Phase 0.

### Phase 5 — Chain split + offline queue (2 days)
- Break `blockchain_bridge.rs` into `provider` / `contracts` / `relay_queue`; move identity to `cabal-vault`; feature-gate `content` + marketplace.
- Config layering replaces `dotenv` + `env::var`.
- Offline queue drains on reconnect, with a test that survives process kill.
- **Done when:** airplane mode → queue intent → restore network → auto-settles.

### Phase 6 — Command surface + bindings (2.5 days)
- Implement the exact 28-handler contract: 25 screen/shared commands + 3 ZK/LLM handlers with mobile `Unsupported` stubs. Add all to AppManifest; grant only the 25 used commands to `mobile.json`.
- Wire the already-generated Phase 1 request/response DTOs into the command implementations. `src/types/bindings.ts` remains the single canonical output; its screen DTO diff should be empty unless a reviewed contract change is intentional.
- Add `PlatformCaps`; expose the Phase 4 `RuntimeCapsHandle` through `runtime_caps` plus typed `RuntimeCapsChanged` / `TypeScaleChanged` events. Do not reimplement probes at the IPC layer.
- Wire `enter_mesh`, `subscribe_mesh_log`, and `settle_intent` to the generic delivery registry. Prove explicit unsubscribe does not abort join/settlement, natural completion self-cleans, suspend/resume retains the same id, and concurrent/StrictMode-style repeated calls attach to one domain operation/transaction.
- **Done when:** every screen has a real-data command; the 25 mobile grants exactly match the frontend invoke inventory; `src/types/bindings.ts` regenerates deterministically including every `AppEvent` payload; all stream lifecycle/idempotency tests pass.

### Phase 7 — Harden + ship (2.5–3 days)
- Size pass (`opt-level = "s"`, LTO, strip). Measure the real artifacts:
  - AAB → `src-tauri/gen/android/app/build/outputs/bundle/universalRelease/app-universal-release.aab`
  - IPA → `src-tauri/gen/apple/build/arm64/$APPNAME.ipa`
- Per-ABI APKs for size comparison: `npm run tauri -- android build --apk --split-per-abi`.
- Release builds: `npm run tauri -- android build --aab` · `npm run tauri -- ios build --export-method app-store-connect`.
- Android keystore + signing config; Play's first upload **must be done manually in the console** so it can verify the signature and bundle identifier.
- App Store upload via `xcrun altool --upload-app --type ios --file … --apiKey $APPLE_API_KEY_ID --apiIssuer $APPLE_API_ISSUER`.
- Cold-start profiling; background/resume soak.
- CI matrix for all 7 Rust targets **plus** a full Android debug APK and iOS simulator-native build (§8); protected signed release jobs use Xcode 26.

**Total ≈ 22–23 working days** of Rust/native work; the phase ranges above now add to that number. The range includes the legacy adapter, contract snapshots, relay deployment, three planned mobile plugins, Tauri/security migration, exact command ACL work, and subscription lifecycle. It excludes the mobile frontend, deferred ZK/LLM implementations, macOS/Xcode installation time, Apple multicast-entitlement review lead time, and any fallback native Bonjour or symmetric-lifecycle bridge selected after device/entitlement testing.

---

## 10. Risks

| Risk | Likelihood | Impact | Mitigation |
|---|---|---|---|
| `alloy` or `libp2p` fails to cross-compile for Android | Medium | Blocking | Phase 0 proves it before any refactor. Fallback: move chain calls behind a thin HTTP JSON-RPC client and drop `alloy` from the mobile build. |
| iOS kills the mesh in background | **High** | Design-level | iOS grants no general background networking. `background_mesh: false`; the UI must treat backgrounding as a disconnect and reconnect on resume — the artifact's offline banner already covers this state, so it is a product truth to honour, not a bug to fix. |
| Built-in iOS lifecycle pair is asymmetric for transient overlays | Medium | Delivery/log forwarding can remain paused after Control Center or an interruption | Phase 0 device-tests the four UIKit sequences in §2.7 before the pair controls pausing; use a narrow symmetric bridge if events do not balance. |
| Local-network access unavailable on iOS | Medium | Degraded | Relay bootstrap keeps the mesh functional; raw-UDP silence remains `Indeterminate`, not a fabricated denial. |
| Apple multicast entitlement is not approved | Medium | iOS rust-libp2p mDNS cannot work | Request in Phase 0; ship iOS relay-only if unavailable. Native Bonjour is a separately estimated fourth-plugin fallback, not an invisible scope increase. |
| Android target SDK reaches 37 without `ACCESS_LOCAL_NETWORK` handling | Medium | Every raw LAN/mDNS socket is blocked | Phase 0 pins the target; Phase 4 declares/requests only for SDK 37+, maps denial/revocation to runtime caps, and proves relay fallback. Android 16 compat mode tests the path before the bump. |
| Wi-Fi→cellular breaks the QUIC connection | High | Temporary mesh disconnect | Expected path: redial relay, rejoin topics, replay with message-id dedupe. Physical-device recovery test in Phase 4; no seamless-migration claim. |
| Binary size (libp2p + alloy + Tauri) | Medium | Store friction | Trimmed features, `opt-level="s"`, LTO, strip, per-ABI Android splits. Budget: < 40 MB per ABI. |
| Vault migration corrupts existing desktop wallets | Low | **Severe** | One-shot migration writes `vault.enc` then verifies a full decrypt round-trip before unlinking `identities.json`; keep a `.bak` for one release. |
| Estimate slips on device debugging | Medium | Schedule | Phase 2 (tracing on device) lands early precisely so later phases are debuggable. |
| Frozen desktop UI silently breaks anyway | **High** | Desktop demo dies | Adapter + contract snapshots both land in Phase 1, before the first signature change. If either is deferred, "frozen" is a wish. |
| Channel subscriptions leak on navigation | **High** | Grows unbounded; drains battery | §2.5.1 lifecycle is mandatory, not a polish item. React unmount does **not** end a Tauri channel. Guarded by the 100-cycle leak test in Phase 2. |
| Shipping with `csp: null` + `withGlobalTauri: true` | **High** | Full IPC surface exposed to any injected script | Both fixed in Phase 0, before mobile init — not deferred to hardening. |
| Legacy adapter accrues drift and becomes a second product | Medium | Ongoing tax | Adapter is append-only and never gains features. Revisit retiring the desktop UI once the mobile app is real. |
| Relay is a single point of failure and a metadata observer | Medium | Availability + privacy | Off-LAN discovery dies with the host; the relay sees co-online peer ids. Mitigate availability with a second relay in the default list; the privacy surface needs disclosing, not engineering away. |
| Relay peer id rotated or lost | Low | **Severe** | Every shipped build hardcodes it. Back up the keypair off-host; treat it like a signing key. |

---

## 11. Decisions taken

| # | Question | Resolution |
|---|---|---|
| 1 | ZK/LLM — gated or deleted? | **Gated.** Desktop-only implementations; mobile Rust stubs return `Unsupported`, frontend hides the affordance via `platform_caps`, and mobile ACL does not grant direct webview invocation. |
| 2 | Relay bootstrap node — who runs it? | **Self-hosted, address compiled in as the default**, user-overridable. Infra spec in §2.7.1; deployment is Phase 4. |
| 5 | Marketplace/voucher/content commands | **Kept**, relocated to `cabal-legacy`, desktop-only registration. Required anyway by the frozen desktop UI. |
| — | Desktop RPG UI | **Frozen** — which makes the §2.10 adapter mandatory, not optional. |
| — | Confirm-dialog copy | **Two strings, chosen by connection state** (ticket 04, 2026-08-03). Online: *"This intent broadcasts to the mesh and settles on-chain. No identity is attached."* Offline: *"Queued locally. Broadcast and settlement follow reconnection. No identity is attached."* The prototype's single string claimed offline execution, which the queue-then-drain architecture does not do. Both live in Rust alongside the review rows so the dialog cannot describe a path the command will not take. |
| — | Reputation score | **Removed and replaced** (ticket 39, 2026-08-03). Ticket 03 had shipped a mock derived from the peer identifier; it is deleted. The field is now `INTENTS SETTLED` — a lifetime count from the intent ledger, with the last seven days compared against the seven before. Renamed as well as redefined: a count labelled "score" would be the same overclaim in a different place. `src-tauri/src/standing.rs`. |

### Defaulted without asking (change if wrong)

- **Networks: Avalanche Fuji as the shipping default, mainnet available by config.** Contract addresses move from bare `env::var` to a per-network table compiled in, overridable at runtime. A testnet default is the safe choice for a build that is still moving; mainnet is one config flip.

### Still open

1. **The product's one-line positioning still says "executed offline."** Ticket 04 fixed the dialog and flagged this: the tagline in `src/ds/BRAND.md` makes the same claim the dialog string was retired for. It ships nowhere in the app today, so it is flagged rather than rewritten — the positioning line is the design owner's, not a dialog string.

---

## 12. Revision log — 2026-08-01 review

External review flagged six High findings. All applied; two of them were places where this document was simply wrong.

| # | Finding | Where fixed |
|---|---|---|
| 12.1 | `frontendDist` is a **directory**, not an entry file — two HTML files in one `dist/` still loads `index.html` (desktop) on mobile. Also: `app.windows` is not wholly desktop-only. | §5.1 — dual `dist-desktop` / `dist-mobile` outputs |
| 12.2 | React unmount does **not** end a Tauri `Channel`; there is no public JS unsubscribe, and Rust producers keep running. Prior claim was false. | §2.5.1 — `SubscriptionId` + explicit `unsubscribe` + leak test |
| 12.3 | Plans assumed a strict CSP; project ships `csp: null` and `withGlobalTauri: true`, and no phase configured it. Tauri 2.9.5 also predates two relevant security fixes. | §5.2.1 + Phase 0 |
| 12.4 | Cargo has **no** "target-conditional workspace members". Prior claim was invented. | §4 — target-specific dependencies |
| 12.5 | Legacy adapter was scheduled at Phase 4b while Phase 2 already broke every signature — the frozen desktop would have been broken for three phases. Live-output snapshots were also unrunnable (need RPC/Ollama/nargo/mesh). | Phase 1 — adapter moved earlier; snapshots now fixture/mock-driven |
| 12.6 | Toolchain table was wrong: iOS targets **are** installed and Cocoapods **is** present. | §5.4 |
| 12.7 | Plugin section said "Three" over a four-row table; capability examples granted no plugin permissions; `type-scale` was a boot-time-only read. | §5.3.1, §5.2 |
| 12.8 | TS `AppEvent` union still listed `handshakeLine`/`meshLogLine` after they moved to channels. `NEARBY` kilometres have no data source. | §7, §6.1 |

### Second review pass — same day

| # | Finding | Where fixed |
|---|---|---|
| 12.9 | **Tauri 2.11.0 propagates mobile `Suspended`/`Resumed`.** The "Tauri gives you nothing here" conclusion was correct for 2.9.5 and wrong for the version we are upgrading to. Custom `lifecycle` plugin dropped — 4 plugins → 3. | §2.7, §5.3.1 |
| 12.10 | AppManifest in Phase 0 would have been declared against the *future* 24 commands while the *current* 47 were still live and the adapter did not exist yet — locking desktop out of its own IPC. | §5.2.1, Phase 0 |
| 12.11 | Leaving `default.json` in place defeats the platform split: capabilities auto-enable and a window gets the **union** of permissions, so mobile would inherit `opener:default`. | §5.2 |
| 12.12 | `core:event:default` + `core:path:default` are already inside `core:default` — the "least privilege" list was redundant. | §5.2 |
| 12.13 | `keystore:allow-unwrap-key` and `multicast-lock:*` were granted to the webview though only Rust calls them — exposing key unwrapping to any injected script. | §5.3.1 |
| 12.14 | "Never resolve on mobile" overstated Cargo: target-specific deps **are** resolved into the graph and lockfile; they are not compiled or linked. | §4 |
| 12.15 | `ITSAppUsesNonExemptEncryption: true` was inferred from "we use AES" — the key asks about *non-exempt* encryption, and a `true` classification typically also requires compliance documentation/code that has not been obtained. | §5.3 |
| 12.16 | Toolchain table stale again: `src-tauri/gen/apple` now exists, `cargo-tauri` is **not** installed (npm CLI 2.9.6 only). **Apple requires Xcode 26 + iOS 26 SDK for uploads since 2026-04-28** — Xcode 15.4 cannot ship. | §5.4, Phase 0 |
| 12.17 | Only `subscribe_mesh_log` had a lifecycle; `enter_mesh`/`settle_intent` returned `()`. Cancel semantics were also undefined — critically, whether it aborts settlement. | §2.5.1, §6 |
| 12.18 | Contract drift: `get_platform_caps` vs `platform_caps`; `bindings.ts` vs `src/types/bindings.ts`; `caps` declared build-time immutable while `mdns_discovery` came from a runtime grant; CI said 4+2 targets against a stated 7. | §2.3, §6, §7, §8 |

### Third consistency and primary-source pass — same day

| # | Finding | Where fixed |
|---|---|---|
| 12.19 | The first stream signatures still returned `()`; log channels also lacked an authoritative completion contract, and pausing deleted ids that mounted React effects would never reacquire. | §2.5–2.5.1 — all return `SubscriptionId`; domain tasks are separate from delivery; typed state/events drive completion; suspend retains ids/tails. |
| 12.20 | Tauri 2.11.5 already documents the exact Rust variants and JS event names. The real uncertainty is iOS's asymmetric resign-active/enter-foreground mapping. | §2.7 — exact `RunEvent::WindowEvent` match plus Phase 0 device gate. |
| 12.21 | Capability examples still granted mobile `core:default`; `$schema` was described as runtime policy; explicit capability selection was missing. | §5.2 — event-only core baseline, explicit `desktop`/`mobile` selection, schema wording corrected. |
| 12.22 | The command table contains 25 screen/shared + 3 feature handlers, not “~24”; feature stubs and mobile ACL behavior were ambiguous. | §5.2.1, §6, Phase 6 — exact 28-handler manifest and exact 25-command mobile grant. |
| 12.23 | `RuntimeCapsChanged` / `TypeScaleChanged` were promised in prose but absent from Rust/TS unions; type-scale ownership was contradictory. | §2.5, §5.3.1, §7 — both events added; frontend reads at boot, Rust rereads on resume. |
| 12.24 | `libp2p-mdns` uses raw multicast UDP, requiring Apple's managed multicast entitlement; the advertised service is `_p2p._udp` only. | B7, §5.3, Phase 0/4 — entitlement gate with relay-only fallback. |
| 12.25 | QUIC was incorrectly described as Noise+Yamux and as seamlessly migrating across networks. The pinned transport uses TLS 1.3/native streams and disables migration. | §2.7, Phase 4 — explicit disconnect/redial/rejoin/replay-dedupe recovery. |
| 12.26 | Xcode 26 is not installable on this host's macOS 14.6.1; remaining commands still used unavailable `cargo tauri`; native CI covered Rust targets only. | §5.4–5.5, §8, Phase 0/7 — definite OS upgrade, npm CLI throughout, native Android/iOS build jobs. |
| 12.27 | Phase estimates summed to 21 days while the headline said 22–23, and work was scheduled before its commands/plugins existed. | §9 — ranges now total 22–23; generic registry precedes Phase 6 integrations; type-scale permission lands with its Phase 3 plugin. |
| 12.28 | `RuntimeCaps.mdns_granted: bool` implied iOS exposes a permission query. Apple provides no general Local Network status API; raw UDP silence is ambiguous. | §2.3, §5.3 — `LocalDiscoveryState` includes `Indeterminate`; probes rerun on resume and never fabricate “denied”. |
| 12.29 | React StrictMode or route re-entry could invoke `settle_intent` twice; delivery teardown alone does not prevent a duplicate transaction. `AppEvent` also lacked `TS`. | §2.5 — atomic start-or-attach by `IntentId`, transaction idempotency key, duplicate-call tests, and `Serialize + TS` across the event graph. |
| 12.30 | Lifecycle code referenced a missing state handle, and the required physical-iPhone semantics gate was absent from Phase 0. | §2.3, §2.7, Phase 0 — watch-backed `LifecycleHandle` plus recorded overlay/lock/background matrix. |
| 12.31 | New typed `generate_zk_proof` collided with the frozen legacy command of the same invoke name; ACL behavior was described as though JS reached the mobile stub. | §4, §6 — typed command renamed `generate_zk_bid_proof`; mobile JS denial vs internal `Unsupported` made explicit. |
| 12.32 | Tauri packages were described as though all shared a 2.11 version; `tauri-build` and plugins use independent release lines. | §5.4 — exact official compatible pins for core, build, API, CLI and opener, with Cargo/npm exact constraints. |
| 12.33 | Production CSP would also have applied to development without a WebSocket allowance, silently breaking the newly fixed device HMR path. | §5.2.1 — separate `devCsp` allows `ws:` only in development; production remains closed. |

### Fourth primary-source pass — same day

| # | Finding | Where fixed |
|---|---|---|
| 12.34 | Android's local-network model changed after the earlier review: Android 16 offers an opt-in restriction using temporary `NEARBY_WIFI_DEVICES`; Android 17 enforces `ACCESS_LOCAL_NETWORK` for apps targeting SDK 37+, including raw mDNS/LAN sockets. Tauri config pins only the minimum SDK, so the generated target SDK also needs an explicit decision. | B7, §2.3, §5.1, §5.3, Phase 0/4, risks — target-aware declarations, grant/deny/revoke tests and relay fallback. |

### Final cross-plan consistency pass — same day

| # | Finding | Where fixed |
|---|---|---|
| 12.35 | A granted Android socket on an empty LAN had no terminal state: `Available` required a peer, so `Probing` could last forever. | §2.3, Phase 4 — bounded `Ready` state separates successful setup/zero peers from `Available` with an observed peer. |
| 12.36 | UI fixture phases needed screen DTOs before the Phase 6 binding step existed. | §1, §7, Phase 1/2/6 — Tauri-free `cabal-contract` defines and generates the complete screen schema in Phase 1; Phase 2 adds errors/events and Phase 6 wires stable DTOs to handlers. |
| 12.37 | Runtime permission state was required in Phase 4 but its probes were scheduled in Phase 6; the promised Settings CTA also had no command or ACL route. | §5.3.1, §6, Phase 4/6 — `RuntimeCapsHandle` moves beside mesh/native state; constrained `nearby_nodes(LocalAccessAction)` supplies request/settings actions through Rust without opener permission or a new command. |
| 12.38 | `NodeSummary.transport` mixed how a peer was discovered with how it is currently connected. | §6.1 — orthogonal `discovery_sources` and `connection` fields. |
