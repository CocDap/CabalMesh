# P1 — NEAR Intents Settlement Implementation Plan

> **Superseded in part, 2026-11-05.** The user removed EVM entirely during execution, so NEAR is the only chain. Everything below about Avalanche, `avalanche-fuji`, `ChainBridge`, `Capabilities`, and `blockchain_bridge.rs` was not built. See the spec's Architecture section.

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Two CabalMesh devices settle a matched buy/sell intent as an atomic `asset ⇄ USDC` swap on NEAR testnet through the NEAR Intents verifier, with signatures exchanged over the mesh and submitted by whichever peer is online.

**Architecture:** A new I/O-free crate `cabal-near` owns keys, NEP-413 signing, intent payloads and borsh transaction encoding. A new `src/near_bridge.rs` does JSON-RPC over the existing `reqwest`. Intents gain a `chain` field; a small `ChainBridge` trait unifies balances, address and capabilities across the existing EVM bridge and the new NEAR bridge, while settlement stays chain-specific.

**Tech Stack:** Rust (Tauri 2, tokio, reqwest 0.13 rustls), `ed25519-dalek` 2, `borsh` 1, `bs58`, `sha2`, `base64`; React 19 frontend; NEAR testnet via Tidepool's `defuse` v0.4.5 verifier.

**Spec:** `docs/superpowers/specs/2026-11-04-near-migration-design.md`

## Global Constraints

- Crates under `src-tauri/crates/` perform no I/O and must not depend on the app crate. `unsafe_code = "forbid"` workspace-wide.
- No `near-primitives` / `near-api` / `near-jsonrpc-client` dependency: they pull the full nearcore type tree into an app whose mobile size is already tracked (`scripts/android-size.sh`). Hand-encode the one transaction shape we need.
- Every new Tauri command is added in five places: `src/commands.rs`, the single `generate_handler!` in `src/lib.rs`, the command list in `build.rs`, `capabilities/desktop.json`, `capabilities/mobile.json`. Then run `npm run bindings` (exports `src/types/bindings.ts`).
- Secrets use the existing `Secret` type and never appear in `Debug`, tracing fields or mesh payloads.
- NEAR testnet values, verbatim:
  - RPC `https://rpc.testnet.fastnear.com` (fallback `https://rpc.testnet.near.org`)
  - verifier `verifier.tidepool-sandbox.testnet`
  - `wNEAR` = `wnear.tidepool-sandbox.testnet`, 24 decimals
  - `USDC` = `usdc.tidepool-sandbox.testnet`, 6 decimals (quote asset)
  - price feed `https://www.usetidepool.tech/v0/tokens`
  - faucet `POST https://www.usetidepool.tech/v0/faucet` with `{"accountId":"<hex>","mode":"intents"}` — credits 100 USDC, 40 wNEAR (and four other test tokens) straight into the verifier balance; one drip per address per mode per hour
- NEAR mainnet values, verbatim: RPC `https://rpc.mainnet.fastnear.com`, verifier `intents.near`, `wNEAR` = `wrap.near` (24), `USDC` = `17208628f84f5d6ad33f0da3bbbeb27ffcb398eac501a31bd6ad2011e36133a1` (6, **unverified**), price feed `https://1click.chaindefuser.com/v0/tokens`.
- Signed intents carry a deadline of **1 hour** after signing, matching the Avalanche escrow window.
- Verifier facts, checked live against Tidepool on 2026-11-04 (defuse source tag `defuse/v0.4.5`; probe scripts kept outside the repo):
  - `execute_intents` has no access control, only the `intents` pause flag. Any account with NEAR for gas can submit. Relayer keys are optional access keys on the verifier's own account.
  - Fee is `0` on Tidepool (`simulate_intents(...).state.fee`). Intent message is JSON `{"signer_id":"<hex>","deadline":"<RFC 3339 UTC>","intents":[{"intent":"token_diff","diff":{"nep141:<token>":"<signed decimal>"}}]}`.
  - NEP-413 hash and `MultiPayload` shape below are confirmed by a signature the verifier accepted. A never-funded implicit account can sign and hold a verifier balance; only the submitter needs native NEAR.
  - Nonce: use the versioned V1 layout `56 28 f6 c6 ‖ 00 ‖ salt[4] ‖ deadline_ns: i64 LE ‖ random[15]`, with `salt` from view `current_salt` (currently `fc4d10f1`). A 32-byte random nonce is still accepted but upstream marks it for removal. The nonce deadline must be ≥ the message deadline; use the same instant.
  - `simulate_intents` is a free view. Bad signature, bad salt and low balance panic (`"invalid signature"`, `"invalid salt"`, `"insufficient balance or overflow"`); diffs that do not net to zero do **not** panic, they return `invariant_violated: {"error":"unmatched_deltas", …}`.
  - Events: `EVENT_JSON` logs with `standard: "dip4"`, `version: "0.4.3"`, events `token_diff` and `intents_executed`.
- Default chain stays `avalanche-fuji`; nothing about the Avalanche path changes behaviour.

## Review Focus

1. **Peer on an older build sends an intent without `chain`.** Expect it to parse as `avalanche-fuji` and never match a NEAR intent. Test owned by Task 2.
2. **Both devices compute the swap amounts independently.** Expect byte-identical `token_diff` values on both sides for every price/amount, or the verifier rejects the pair. Test owned by Task 9.
3. **Implicit account has never been funded** (no NEAR for gas). Expect a "fund this account first" error carrying the account id, not a panic or an opaque RPC error. Test owned by Task 6.
4. **Verifier unreachable, `simulate_intents` rejects, or the salt rotated between signing and submit.** Expect the intent to move to `Failed` with a reason and funds to stay in the verifier balance. Test owned by Task 9.
5. **Same `SignedDiff` delivered twice (gossipsub redelivery, BLE resend).** Expect one submission only. Test owned by Task 9.

---

### Task 1: Spike — prove P2P intents on Tidepool and capture golden vectors

No app code. Its output is the evidence later tasks test against. Signing, fee, salt, faucet and simulate were already verified (Global Constraints); what is left is an on-chain submission, a deposit, a withdrawal and the fixtures.

**Files:**
- Create: `scripts/near-intents-spike/probe.mjs` (Node ≥ 20, no dependencies: `node:crypto` ed25519 + hand-rolled base58/borsh)
- Create: `docs/near-intents-spike.md`
- Create: `src-tauri/crates/cabal-near/tests/fixtures/` (`keypair.json`, `nep413_token_diff.json`, `signed_tx_ft_transfer_call.json`)

**Interfaces:**
- Produces: fixture files with fixed inputs (secret key bytes, salt, random nonce tail, deadline, block hash) and their exact expected outputs (implicit account id, `public_key` string, signature string, full `MultiPayload` JSON, base64 signed transaction). Tasks 3 and 4 assert against these byte-for-byte.

- [ ] **Step 1:** Write `probe.mjs`: three implicit accounts A (seller), B (buyer), G (gateway), keys written to a gitignored `scripts/near-intents-spike/.keys.json`. Drip A and B with the Tidepool faucet in `intents` mode. Fund G with testnet NEAR by hand (G is the only account that needs gas).
- [ ] **Step 2:** A and B sign `token_diff` (A: `wNEAR -1e24`, `USDC +4.04e6`; B: the inverse) with versioned nonces. `simulate_intents` → expect `invariant_violated: null`.
- [ ] **Step 3:** G calls `execute_intents([payloadA, payloadB])` with `GAS_EXECUTE_INTENTS`, 0 deposit. Confirm with `mt_balance_of` that balances moved; record gas burnt.
- [ ] **Step 4:** Fund A with a little NEAR. A deposits 1 `USDC` from its wallet via `ft_transfer_call(receiver_id=verifier, amount, msg="")` with 1 yocto (verifier is already storage-registered on both Tidepool tokens), then withdraws it via an `ft_withdraw` intent that A submits itself. Record gas for both.
- [ ] **Step 5:** Re-sign A's payload with fixed key, salt, nonce tail and deadline and write it to `nep413_token_diff.json`; build one `ft_transfer_call` transaction with fixed nonce and block hash and write it to `signed_tx_ft_transfer_call.json`.
- [ ] **Step 6:** Write `docs/near-intents-spike.md`: exact JSON shapes (intent message, `MultiPayload`, `simulate_intents` result, event logs), gas per call, every tx hash, verdict. Only if the verifier is paused (`is_paused("intents")`) deploy upstream `defuse/v0.4.5` to `intents.cabalmesh.testnet`, re-run, and record the new id for Task 5.
- [ ] **Step 7: Commit**

```bash
git add scripts/near-intents-spike/probe.mjs docs/near-intents-spike.md src-tauri/crates/cabal-near/tests/fixtures .gitignore
git commit -m "spike: prove peer-to-peer NEAR Intents on testnet, capture vectors"
```

---

### Task 2: `Chain` on intents, and same-chain matching

**Files:**
- Modify: `src-tauri/crates/cabal-core/src/intent.rs` (add `Chain`, add `IntentDraft.chain`)
- Modify: `src-tauri/crates/cabal-core/src/lib.rs` (re-export `Chain`)
- Modify: `src-tauri/src/negotiation.rs` (same-chain gate inside `consider`)
- Test: `src-tauri/crates/cabal-core/src/intent.rs` (tests module), `src-tauri/src/negotiation.rs` (tests module)

**Interfaces:**
- Produces:
  - `pub enum Chain { AvalancheFuji, AvalancheMainnet, NearTestnet, NearMainnet }`, `#[serde(rename_all = "kebab-case")]`, `#[default] AvalancheFuji`, `Copy + Eq + Hash`
  - `impl Chain { pub const ALL: [Self; 4]; pub const fn is_near(self) -> bool; pub const fn is_testnet(self) -> bool; pub const fn label(self) -> &'static str }`
  - `IntentDraft { …, #[serde(default)] pub chain: Chain }`

- [ ] **Step 1: Write the failing tests**

```rust
#[test]
fn draft_without_chain_parses_as_avalanche_fuji() {
    let json = r#"{"action":"buy","asset":"AVAX","condition":{"kind":"any"},"amount":"1","mode":"shark","privacy":"low"}"#;
    let draft: IntentDraft = serde_json::from_str(json).unwrap();
    assert_eq!(draft.chain, Chain::AvalancheFuji);
}

#[test]
fn chain_round_trips_as_kebab_case() {
    assert_eq!(serde_json::to_string(&Chain::NearTestnet).unwrap(), r#""near-testnet""#);
}
```

Copy the exact JSON field shapes for `condition`/`amount` from an existing serialized draft in the tests module; the point is only that `chain` is absent.

In `negotiation.rs` tests: `consider_ignores_intent_on_other_chain`: an own open `Sell wNEAR` on `NearTestnet` and an incoming `Buy wNEAR` on `AvalancheFuji` with overlapping prices produce no `DealAccepted` (assert the ledger status stays unchanged, using the existing local-TcpListener Ollama mock pattern in that file).

- [ ] **Step 2:** Run `cd src-tauri && cargo test -p cabal-core chain && cargo test --lib negotiation` → FAIL (`Chain` not found).
- [ ] **Step 3:** Implement `Chain` and the field; in `consider`, add `own.draft.chain != theirs.chain` → skip, next to the existing `asset` check. Fix every `IntentDraft { … }` literal the compiler flags by adding `chain: Chain::AvalancheFuji`.
- [ ] **Step 4:** Run `cargo test --workspace` → PASS. Run `npm run bindings` and commit the regenerated `src/types/bindings.ts`.
- [ ] **Step 5: Commit** — `git commit -m "feat(core): intents carry a chain, matching stays within it"`

---

### Task 3: `cabal-near` crate — keys, NEP-413, intent payloads

**Files:**
- Create: `src-tauri/crates/cabal-near/Cargo.toml`, `src/lib.rs`, `src/keys.rs`, `src/nep413.rs`, `src/intents.rs`
- Test: `src-tauri/crates/cabal-near/tests/vectors.rs`

**Interfaces:**
- Produces:
  - `pub struct NearKeypair` (holds `ed25519_dalek::SigningKey`, zeroized on drop, `Debug` prints only the account id)
    - `fn generate() -> Self`, `fn from_secret_bytes(&[u8; 32]) -> Self`, `fn secret_bytes(&self) -> [u8; 32]`
    - `fn account_id(&self) -> AccountId` (lowercase hex of the 32-byte public key)
    - `fn public_key_string(&self) -> String` (`"ed25519:<base58>"`)
  - `pub struct AccountId(String)` with `fn as_str(&self) -> &str`, `TryFrom<&str>` validating NEAR account id rules
  - `pub struct TokenDiff(BTreeMap<String, i128>)` keyed by `"nep141:<contract>"`; serializes amounts as decimal strings
  - `pub fn versioned_nonce(salt: [u8; 4], deadline_unix_ms: u64, random: [u8; 15]) -> [u8; 32]` — layout in Global Constraints
  - `pub fn token_diff_payload(signer: &NearKeypair, verifier: &AccountId, diff: &TokenDiff, deadline_unix_ms: u64, nonce: [u8; 32]) -> MultiPayload` — message `deadline` rendered as `YYYY-MM-DDTHH:MM:SS.mmmZ`
  - `pub struct MultiPayload` — `Serialize + Deserialize`, exact JSON from the spike doc (`standard: "nep413"`, `payload`, `public_key`, `signature`)
  - `pub fn verify(payload: &MultiPayload) -> bool` (signature check only, used by gateways before submitting)

- [ ] **Step 1: Write the failing tests** in `tests/vectors.rs`, reading the Task 1 fixtures:

```rust
#[test]
fn implicit_account_matches_fixture() {
    let f = fixture("keypair.json");
    let kp = NearKeypair::from_secret_bytes(&f.secret);
    assert_eq!(kp.account_id().as_str(), f.account_id);
    assert_eq!(kp.public_key_string(), f.public_key);
}

#[test]
fn token_diff_payload_matches_fixture_byte_for_byte() {
    let f = fixture("nep413_token_diff.json");
    let p = token_diff_payload(&f.keypair(), &f.verifier(), &f.diff(), f.deadline_ms, f.nonce);
    assert_eq!(serde_json::to_value(&p).unwrap(), f.expected_multipayload);
    assert!(verify(&p));
}

#[test]
fn tampered_diff_fails_verification() { /* flip one amount in the fixture payload message; assert !verify */ }

#[test]
fn versioned_nonce_layout() {
    let n = versioned_nonce([0xfc, 0x4d, 0x10, 0xf1], 1_000, [7; 15]);
    assert_eq!(&n[0..5], &[0x56, 0x28, 0xf6, 0xc6, 0x00]);
    assert_eq!(&n[5..9], &[0xfc, 0x4d, 0x10, 0xf1]);
    assert_eq!(i64::from_le_bytes(n[9..17].try_into().unwrap()), 1_000_000_000);
    assert_eq!(&n[17..], &[7; 15]);
}
```

- [ ] **Step 2:** Run `cargo test -p cabal-near` → FAIL (crate empty).
- [ ] **Step 3:** Implement. NEP-413 hashing: `sha256(borsh(u32: 2147484061) ++ borsh(Nep413Payload { message, nonce, recipient, callback_url: Option<String> }))`, signed with ed25519, signature rendered as `"ed25519:<base58>"`; if the spike doc shows a different encoding, the doc wins. Add the crate to the workspace member glob (already `crates/*`) and as a dependency of the app crate.
- [ ] **Step 4:** Run `cargo test -p cabal-near` → PASS.
- [ ] **Step 5: Commit** — `git commit -m "feat(near): keys, NEP-413 signing and token_diff payloads"`

---

### Task 4: `cabal-near` — FunctionCall transaction encoding

**Files:**
- Create: `src-tauri/crates/cabal-near/src/tx.rs`
- Test: `src-tauri/crates/cabal-near/tests/vectors.rs` (append)

**Interfaces:**
- Produces:
  - `pub struct FunctionCall { pub method_name: String, pub args: Vec<u8>, pub gas: u64, pub deposit: u128 }`
  - `pub fn sign_function_calls(signer: &NearKeypair, receiver: &AccountId, nonce: u64, block_hash: [u8; 32], calls: &[FunctionCall]) -> String` — base64 of borsh `SignedTransaction` (Transaction V0), ready for RPC `send_tx`
  - constants `pub const GAS_FT_TRANSFER_CALL: u64 = 100_000_000_000_000; pub const GAS_EXECUTE_INTENTS: u64 = 300_000_000_000_000;` (overwrite with the spike doc's measured values if they differ)

- [ ] **Step 1: Write the failing test** `signed_ft_transfer_call_matches_fixture`: build the call from `signed_tx_ft_transfer_call.json` inputs, assert the base64 string equals the fixture.
- [ ] **Step 2:** Run → FAIL.
- [ ] **Step 3:** Implement with `borsh` derive on private mirror structs (`Transaction`, `Action::FunctionCall` = enum index 2, `PublicKey` = `0u8` + 32 bytes, `Signature` = `0u8` + 64 bytes). Sign `sha256(borsh(tx))`.
- [ ] **Step 4:** Run → PASS.
- [ ] **Step 5: Commit** — `git commit -m "feat(near): encode and sign FunctionCall transactions"`

---

### Task 5: NEAR network config and per-chain assets

**Files:**
- Modify: `src-tauri/src/network_config.rs`
- Modify: `src-tauri/src/commands.rs:658-670` (replace `const ASSETS` / `decimals_for`)
- Test: `src-tauri/src/network_config.rs` (tests module), `src-tauri/tests/ipc_contract.rs` if it snapshots form options

**Interfaces:**
- Consumes: `Chain` (Task 2).
- Produces:
  - `pub struct AssetSpec { pub symbol: &'static str, pub code: &'static str, pub decimals: u8, pub contract: Option<&'static str> }`
  - `pub struct NearConfig { pub rpc_url: &'static str, pub verifier: &'static str, pub quote: AssetSpec, pub price_feed: &'static str }`
  - `impl Network { pub const fn near(self) -> NearConfig; pub const fn chains(self) -> [Chain; 2] }` — `Fuji → [AvalancheFuji, NearTestnet]`, `Mainnet → [AvalancheMainnet, NearMainnet]`
  - `pub fn assets(chain: Chain) -> &'static [AssetSpec]` — Avalanche: the four current rows unchanged; NEAR: `wNEAR` and `USDC` with the Global Constraints values
  - `fn decimals_for(chain: Chain, asset: &str) -> Option<u8>` in `commands.rs`
  - `NetworkConfig` gains a runtime override for the verifier id, same pattern as the existing contract overrides

- [ ] **Step 1: Write the failing tests**: `near_testnet_points_at_tidepool` (asserts verifier, both token contracts and decimals equal the Global Constraints values verbatim), `near_mainnet_points_at_intents_near`, `fuji_offers_avalanche_and_near_testnet`, `decimals_lookup_is_per_chain` (`decimals_for(NearTestnet, "AVAX") == None`, `decimals_for(NearTestnet, "wNEAR") == Some(24)`).
- [ ] **Step 2:** Run `cargo test --lib network_config` → FAIL.
- [ ] **Step 3:** Implement; `parse_draft` reads `fields.chain` (add `chain: Chain` to `IntentFields`, default from the device's network) and calls `decimals_for(chain, asset)`. `intent_form_options` returns `chains` and per-chain `assets`.
- [ ] **Step 4:** Run `cargo test --workspace` → PASS; `npm run bindings`.
- [ ] **Step 5: Commit** — `git commit -m "feat(config): NEAR networks and per-chain asset lists"`

---

### Task 6: `near_bridge.rs` — JSON-RPC I/O

**Files:**
- Create: `src-tauri/src/near_bridge.rs`
- Modify: `src-tauri/src/lib.rs` (`mod near_bridge;`)
- Modify: `src-tauri/src/error.rs` (new variant)
- Test: `src-tauri/src/near_bridge.rs` (tests with the local `TcpListener` mock pattern from `intent_chat.rs:300-380`), plus one `#[ignore]` live test

**Interfaces:**
- Consumes: `NearKeypair`, `AccountId`, `MultiPayload`, `FunctionCall`, `sign_function_calls` (Tasks 3–4); `NearConfig` (Task 5).
- Produces:
  - `pub struct TxHash(pub String)` (base58 hash as returned by RPC)
  - `pub struct NearBridge { rpc_url: String, verifier: AccountId, keypair: NearKeypair, http: reqwest::Client }`
  - `async fn verifier_balance(&self, token: &str) -> Result<u128, NearError>` (view `mt_balance_of`)
  - `async fn native_balance(&self) -> Result<u128, NearError>` (view_account)
  - `async fn deposit(&self, token: &str, amount: u128) -> Result<TxHash, NearError>` (`ft_transfer_call` to verifier, `deposit = 1` yocto)
  - `async fn withdraw(&self, token: &str, amount: u128) -> Result<TxHash, NearError>` (signs an `ft_withdraw` intent, submits via `execute_intents`)
  - `async fn current_salt(&self) -> Result<[u8; 4], NearError>` (view `current_salt`, hex string → bytes)
  - `async fn simulate(&self, payloads: &[MultiPayload]) -> Result<(), NearError>` — a panic or a non-null `invariant_violated` both become `Rejected(reason)`
  - `async fn execute_intents(&self, payloads: &[MultiPayload]) -> Result<TxHash, NearError>` (always `simulate` first)
  - `pub enum NearError { AccountNotFunded { account: String }, Rpc(String), Rejected(String), Network(reqwest::Error) }`
  - `AppError::NearAccountNotFunded { account: String }` mapped from `NearError::AccountNotFunded`

- [ ] **Step 1: Write the failing tests**: `unfunded_account_maps_to_account_not_funded` (mock returns the RPC `UNKNOWN_ACCOUNT` error body for `view_access_key`; assert `NearError::AccountNotFunded { account }` equals the keypair's id); `deposit_sends_ft_transfer_call_to_verifier` (mock captures the `send_tx` body; decode base64, assert receiver is the token contract and args contain the verifier id); `execute_intents_simulates_first` (mock records method order: `simulate_intents` before `send_tx`); `simulate_rejection_is_not_submitted` (simulate returns an error, assert no `send_tx`); `unmatched_deltas_is_rejected` (simulate returns a result with `invariant_violated: {"error":"unmatched_deltas",…}`; assert `Rejected` containing `unmatched_deltas` and no `send_tx`). Live test `live_deposit_and_balance` behind `#[ignore]` and env `CABALMESH_NEAR_LIVE_SECRET`.
- [ ] **Step 2:** Run `cargo test --lib near_bridge` → FAIL.
- [ ] **Step 3:** Implement. Nonce and block hash come from `view_access_key` + `block(finality=final)` before each send; nonce = access key nonce + 1.
- [ ] **Step 4:** Run → PASS.
- [ ] **Step 5: Commit** — `git commit -m "feat(near): JSON-RPC bridge for deposit, withdraw and execute_intents"`

---

### Task 7: NEAR key in the vault, and `ChainBridge`

**Files:**
- Modify: `src-tauri/src/blockchain_bridge.rs:57-68` (`IdentityRecord`), identity load path
- Create: `src-tauri/src/chain_bridge.rs`
- Modify: `src-tauri/src/state.rs` (`Services` holds `near: Option<Arc<Mutex<NearBridge>>>`)
- Modify: `src-tauri/src/commands.rs` (`vault_assets`, `vault_address`, `vault_export_key` read through `ChainBridge`)
- Test: `src-tauri/src/blockchain_bridge.rs` tests, `src-tauri/tests/guardian_loopback.rs` (extend)

**Interfaces:**
- Consumes: `NearKeypair` (Task 3), `NearBridge` (Task 6), `assets(chain)` (Task 5).
- Produces:
  - `IdentityRecord { …, #[serde(default)] pub near_secret: Option<Secret> }` — hex of 32 secret bytes; generated and saved on first load when `None`
  - `#[async_trait] pub trait ChainBridge: Send + Sync { fn chain(&self) -> Chain; fn address(&self) -> String; async fn balances(&self) -> Result<Vec<AssetBalance>, AppError>; fn capabilities(&self) -> Capabilities; }`
  - `pub struct Capabilities { pub assets: &'static [AssetSpec], pub marketplace: bool, pub modules: bool, pub two_way_settlement: bool, pub intents_deposit: bool }` — Avalanche: `marketplace/modules = true, two_way = false, intents_deposit = false`; NEAR: the inverse
  - `pub struct AssetBalance { pub symbol: String, pub wallet: Option<String>, pub in_intents: Option<String> }` (decimal strings)
  - `vault_export_key` returns both keys, labelled by chain

- [ ] **Step 1: Write the failing tests**: `legacy_identity_gains_near_key_once` (save a record without `near_secret`, load twice, assert the same NEAR account id both times); `identity_debug_never_prints_near_secret`; extend the guardian loopback test to assert the NEAR account id survives a guardian unlock.
- [ ] **Step 2:** Run → FAIL.
- [ ] **Step 3:** Implement; construct `NearBridge` at startup from the active identity and `Network::near()`.
- [ ] **Step 4:** Run `cargo test --workspace` → PASS.
- [ ] **Step 5: Commit** — `git commit -m "feat(identity): NEAR key in the vault and a ChainBridge for reads"`

---

### Task 8: Vault screen — NEAR address, intents balance, deposit, withdraw

**Files:**
- Modify: `src-tauri/src/commands.rs` (+ the four registration places from Global Constraints)
- Modify: `src/screens/Vault.tsx`, `src/screens/New.tsx` (chain selector, available-balance line)
- Test: `src-tauri/tests/handler_arms.rs`, `src-tauri/tests/ipc_contract.rs`, `commands.rs` tests

**Interfaces:**
- Consumes: `ChainBridge`, `NearBridge` (Task 6–7).
- Produces commands:
  - `near_account() -> NearAccountView { accountId, nativeBalance, faucetLinks: Vec<String>, explorerUrl }`
  - `near_deposit(token: String, amount: String) -> String` (tx hash)
  - `near_withdraw(token: String, amount: String) -> String`
  - `vault_assets` rows now include `in_intents` for NEAR

- [ ] **Step 1: Write the failing tests**: commands appear in the handler arm and both capability files (existing `handler_arms.rs` style); `broadcast_rejects_near_amount_above_intents_balance` (stub the bridge balance at 0.5 wNEAR, a sell draft of 1 wNEAR returns `AppError::InvalidIntent { field: "amount", .. }`).
- [ ] **Step 2:** Run → FAIL.
- [ ] **Step 3:** Implement commands and screens. Faucet links: NEAR testnet faucet page (native NEAR, needed only to deposit, withdraw or act as gateway) and `https://www.usetidepool.tech/docs/guides/faucet` (choose `intents` mode: tokens land in the verifier, ready to trade). Copy text for an unfunded account: `FUND THIS ACCOUNT WITH TESTNET NEAR FIRST` followed by the account id.
- [ ] **Step 4:** Run `cargo test --workspace` and `npm run build` → PASS. Manual: Vault shows the hex account id and a copy button.
- [ ] **Step 5: Commit** — `git commit -m "feat(vault): NEAR account, intents balance, deposit and withdraw"`

---

### Task 9: `SignedDiff` over the mesh and NEAR settlement

**Files:**
- Create: `src-tauri/crates/cabal-near/src/quote.rs` (pure amount math)
- Create: `src-tauri/src/near_settlement.rs` (`DiffBook` + settle flow)
- Modify: `src-tauri/src/mesh.rs` (dispatch `"SignedDiff"` like `"DealAccepted"`, new `MeshEvent::SignedDiff { details: String }`)
- Modify: `src-tauri/src/commands.rs` (`settle_intent` / `run_settlement` branch on `draft.chain.is_near()`)
- Modify: `src-tauri/src/lib.rs` (route `MeshEvent::SignedDiff` to `near_settlement::on_signed_diff`)
- Test: `cabal-near/src/quote.rs` tests, `near_settlement.rs` tests

**Interfaces:**
- Consumes: `token_diff_payload`, `verify`, `MultiPayload` (Task 3); `NearBridge::execute_intents` (Task 6); `NearConfig` (Task 5).
- Produces:
  - `pub fn quote_amount(base_raw: u128, base_decimals: u8, price_cents: u64, quote_decimals: u8) -> Option<u128>` — floor of `base_raw * price_cents * 10^quote_decimals / (100 * 10^base_decimals)`, `None` on overflow
  - `pub struct FeeRule { pub pips: u32 }` with `pub const NONE: FeeRule = FeeRule { pips: 0 }`; Tidepool is `NONE` (verified); on mainnet read `state.fee` from `simulate_intents`
  - `pub fn seller_diff(base: &str, quote: &str, base_raw: u128, quote_raw: u128, fee: &FeeRule) -> TokenDiff` and `buyer_diff(...)` with the same parameters
  - mesh payload `{"type":"SignedDiff","dealId":String,"chain":"near-testnet","side":"buy"|"sell","payload":MultiPayload}`
  - `pub struct DiffBook` with `fn insert(&self, deal_id: &str, side: cabal_core::Action, payload: MultiPayload) -> Option<[MultiPayload; 2]>` (only `Buy`/`Sell` accepted; returns the pair exactly once, ordered `[sell, buy]`) and `fn mark_submitted(&self, deal_id: &str) -> bool`
  - `pub async fn on_signed_diff(state: &AppState, details: &str)` — verifies the signature, inserts, and when a pair completes and `runtime_caps().online` submits via `execute_intents`, then broadcasts the existing `SettlementComplete` with the tx hash

- [ ] **Step 1: Write the failing tests**

```rust
#[test]
fn both_sides_compute_identical_diffs() {
    for (amount, price) in [(1_000_000_000_000_000_000_000_000u128, 404u64), (1, 1), (123_456_789, 99_999)] {
        let q = quote_amount(amount, 24, price, 6).unwrap();
        let s = seller_diff("nep141:w", "nep141:u", amount, q, &FeeRule::NONE);
        let b = buyer_diff("nep141:w", "nep141:u", amount, q, &FeeRule::NONE);
        for (k, v) in s.iter() { assert_eq!(b.get(k), Some(&-v)); }
    }
}
```

Plus: `quote_amount_overflow_is_none`; `diff_book_returns_pair_once` (insert sell, insert buy → `Some`; insert buy again → `None`); `diff_book_rejects_bad_signature` (via `on_signed_diff`, assert nothing inserted); `submit_failure_marks_intent_failed` (bridge `simulate` returns `Rejected`; assert ledger status `Failed { reason }` and a log line containing `FUNDS REMAIN IN INTENTS BALANCE`); `offline_device_holds_pair_until_online` (pair completes while `online = false`; flip `ConnectivityChanged { online: true }`; assert exactly one submission).
- [ ] **Step 2:** Run → FAIL.
- [ ] **Step 3:** Implement. Nonce for each payload = `versioned_nonce(salt, deadline, random)`, where `salt` is the last value from `current_salt`, persisted so a device signing offline over BLE still has one; with no salt ever seen, fall back to 32 random bytes. An `"invalid salt"` rejection marks the intent `Failed` with `SALT ROTATED, SIGN AGAIN`. `retry_queued_intents` also retries held NEAR pairs on reconnect. The Avalanche branch of `run_settlement` is untouched.
- [ ] **Step 4:** Run `cargo test --workspace` → PASS.
- [ ] **Step 5: Commit** — `git commit -m "feat(near): exchange signed diffs over the mesh and settle via Intents"`

---

### Task 10: Two-device verification and status docs

**Files:**
- Modify: `scripts/dev-2-nodes.sh` (accept `--near` to set both nodes' default chain to `near-testnet`)
- Modify: `docs/product-status.md` (NEAR rows; AI row now live since PR #15)
- Create: `docs/near-two-device-checklist.md`

- [ ] **Step 1:** Write the checklist: fund both implicit accounts; deposit 1 wNEAR on node A and 5 USDC on node B; A posts `SELL 1 wNEAR above $3.90`, B posts `BUY 1 wNEAR under $4.20`; both confirm; observe `SettlementComplete`, balances swapped in Vault; repeat with node B's network disabled until both signatures exist, then re-enable B and confirm A or B submits once.
- [ ] **Step 2:** Run it with `scripts/dev-2-nodes.sh --near`; paste tx hashes into the checklist.
- [ ] **Step 3:** Update `docs/product-status.md` with evidence paths.
- [ ] **Step 4: Commit** — `git commit -m "docs: NEAR two-device checklist and product status"`
