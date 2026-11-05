# P2 — NEAR Contracts (Gateway Rewards, then Marketplace and Modules) Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** A gateway that submits a matched NEAR Intents pair is paid on-chain for it, and the module/marketplace features exist on NEAR.

**Architecture:** A `cabal-rewards` NEAR contract sits in front of the verifier. Gateways call `rewards.submit(payloads)`; it forwards to `execute_intents` and, on success, pays the caller (`predecessor_account_id`) from fees the two traders included in their signed intents as a `transfer` to the rewards account. Because the fee is paid to the contract, not to a gateway, traders can sign it offline without knowing who will submit. Standing is read from verifier events, not stored.

**Tech Stack:** `near-sdk` 5.x (Rust), `near-workspaces` for sandbox tests, `cargo-near` for builds; app side reuses `cabal-near` from P1.

**Spec:** `docs/superpowers/specs/2026-11-04-near-migration-design.md` (P2 section). Depends on P1 being complete.

## Decisions this plan makes beyond the spec — confirm before executing

- **D1 — Fee funding.** Each trader adds a `transfer` intent of `GATEWAY_FEE_USDC = 10_000` (0.01 USDC) to the rewards account inside their own signed payload. Self-trading to farm rewards then costs the farmer exactly what it earns, so it is pointless. Alternative: a treasury-funded pool (open to farming).
- **D2 — Payout path.** The contract pays from its own verifier balance with NEP-245 `mt_transfer`, so payouts never leave the verifier. Task 1 checks this works on the deployed verifier.
- **D3 — Marketplace and modules get their own plan.** They port semantics from the deleted `Marketplace.sol`, `CabalMeshVoucher.sol` and `CabalMeshModules.sol` (see `git show e26157c:contracts/contracts/`), and those designs depend on how this rewards contract settles. Task 5 writes that plan; it is not written blind here.

## Global Constraints

- Contracts live in a new top-level Cargo workspace `contracts-near/`, separate from `src-tauri/` so the app's dependency tree never sees `near-sdk`.
- Testnet account: `rewards.cabalmesh.testnet` (create under a `cabalmesh.testnet` parent owned by the team). Mainnet id is left empty in config until audited.
- The contract never holds funds outside the verifier and has no owner withdrawal of collected fees except payout to submitters.
- App changes go through `network_config` (`NearConfig.rewards: Option<&'static str>`); when `None`, P1 behaviour (free submission straight to the verifier) is unchanged.

## Review Focus

1. **`execute_intents` fails inside the forwarded call** (bad signature, expired deadline). Expect no payout and no fee kept; the traders' fee transfer is part of the failed batch, so it reverts with it. Test owned by Task 2.
2. **Two gateways submit the same pair at once.** Expect one success, one revert from the verifier's nonce check, one payout. Test owned by Task 2.
3. **Callback runs out of gas.** Expect the trade to stand and the payout to be retryable by the same gateway through `claim(deal_nonce)`. Test owned by Task 2.
4. **Traders' payload lacks the fee transfer.** Expect `submit` to reject before forwarding, so a gateway never spends gas for nothing. Test owned by Task 2.
5. **Rewards configured but contract not deployed.** Expect the app to fall back to direct submission and log it once. Test owned by Task 4.

---

### Task 1: Spike — contract-held verifier balance and `mt_transfer`

**Files:**
- Create: `docs/near-rewards-spike.md`

- [ ] **Step 1:** On testnet, transfer 0.02 USDC into the verifier balance of a plain test account via a signed `transfer` intent; from that account call `mt_transfer(receiver_id, token_id, amount)` on the verifier with 1 yocto attached; record whether it succeeds and the gas used.
- [ ] **Step 2:** Capture the real `EVENT_JSON` logs of one `execute_intents` tx for Task 3's fixtures. Already seen in simulation: `standard: "dip4"`, `version: "0.4.3"`, event `token_diff` with `data: [{intent_hash, account_id, diff}]` and event `intents_executed` with `data: [{intent_hash, account_id, nonce}]`.
- [ ] **Step 3:** Write the findings and a go/no-go for D2. Commit `docs: rewards spike`.

---

### Task 2: `cabal-rewards` contract

**Files:**
- Create: `contracts-near/Cargo.toml` (workspace), `contracts-near/rewards/Cargo.toml`, `contracts-near/rewards/src/lib.rs`
- Test: `contracts-near/rewards/tests/sandbox.rs` (`near-workspaces`, deploys a `defuse` wasm built from the same upstream tag as P1)

**Interfaces:**
- Produces (contract methods):
  - `#[init] fn new(verifier: AccountId, fee_token: String, fee_per_side: U128)`
  - `#[payable] fn submit(&mut self, payloads: Vec<serde_json::Value>) -> Promise` — checks each payload contains a `transfer` of `fee_per_side` of `fee_token` to `env::current_account_id()`, forwards to `verifier.execute_intents`, then `on_executed(gateway, amount)`
  - `#[private] fn on_executed(&mut self, gateway: AccountId, amount: U128) -> bool` — on success `mt_transfer` to gateway; on transfer failure records `pending[gateway] += amount`
  - `fn claim(&mut self) -> Promise` — pays `pending[predecessor]`
  - `fn stats(&self) -> RewardStats { submissions: u64, paid: U128 }`

- [ ] **Step 1: Write the failing sandbox tests**: `gateway_is_paid_for_valid_pair` (gateway balance in verifier grows by `2 × fee_per_side`); `failed_execution_pays_nothing`; `double_submission_pays_once`; `payload_without_fee_is_rejected_before_forwarding` (assert verifier was never called: its nonce for the trader is unused afterwards); `failed_payout_is_claimable`.
- [ ] **Step 2:** Run `cd contracts-near && cargo test` → FAIL.
- [ ] **Step 3:** Implement. Attach `GAS_EXECUTE_INTENTS` from P1 to the forward call; keep 30 Tgas for the callback.
- [ ] **Step 4:** Run → PASS. Build with `cargo near build`, deploy to `rewards.cabalmesh.testnet`, record the tx in `docs/near-rewards-spike.md`.
- [ ] **Step 5: Commit** — `git commit -m "feat(contracts-near): gateway rewards in front of the Intents verifier"`

---

### Task 3: Standing from verifier events

**Files:**
- Create: `src-tauri/crates/cabal-near/src/events.rs`
- Modify: `src-tauri/src/standing.rs` (NEAR settlements count from events)
- Test: `cabal-near/src/events.rs` tests with the event JSON captured in Task 1

**Interfaces:**
- Produces: `pub fn settlements_for(account: &AccountId, logs: &[String]) -> u32` — parses `EVENT_JSON:` log lines in the shape recorded by Task 1.

- [ ] **Step 1:** Failing test `counts_only_diffs_signed_by_account` using two captured logs (one for the account, one not) → expects `1`.
- [ ] **Step 2:** Run → FAIL. **Step 3:** Implement; `standing.rs` adds NEAR counts from the account's settled intents' tx outcomes. **Step 4:** Run → PASS.
- [ ] **Step 5: Commit** — `git commit -m "feat(standing): count NEAR settlements from verifier events"`

---

### Task 4: App submits through rewards and includes the fee

**Files:**
- Modify: `src-tauri/src/network_config.rs` (`NearConfig.rewards`, `fee_per_side`)
- Modify: `src-tauri/crates/cabal-near/src/quote.rs` (`seller_diff`/`buyer_diff` take `Option<GatewayFee>`)
- Modify: `src-tauri/crates/cabal-near/src/intents.rs` (payload carries `[token_diff, transfer]` when a fee applies)
- Modify: `src-tauri/src/near_bridge.rs` (`submit_pair` picks `rewards.submit` or `execute_intents`)
- Test: the corresponding test modules

**Interfaces:**
- Produces: `pub struct GatewayFee { pub to: AccountId, pub token: String, pub amount: u128 }`; `async fn submit_pair(&self, pair: &[MultiPayload; 2]) -> Result<TxHash, NearError>`

- [ ] **Step 1:** Failing tests: `payload_includes_fee_transfer_when_rewards_configured`; `submit_pair_uses_rewards_contract_when_configured`; `missing_rewards_contract_falls_back_to_verifier_once` (RPC returns account-not-found for the rewards id; assert one fallback log and a direct `execute_intents`).
- [ ] **Step 2:** Run → FAIL. **Step 3:** Implement; set testnet `rewards: Some("rewards.cabalmesh.testnet")`, `fee_per_side: 10_000`. **Step 4:** Run `cargo test --workspace` → PASS.
- [ ] **Step 5:** Re-run `docs/near-two-device-checklist.md` with a third node acting as gateway; record its payout.
- [ ] **Step 6: Commit** — `git commit -m "feat(near): pay gateways through the rewards contract"`

---

### Task 5: Write the marketplace and modules plan

**Files:**
- Create: `docs/superpowers/plans/YYYY-MM-DD-p2b-near-marketplace-modules.md`

- [ ] **Step 1:** Read `Marketplace.sol`, `CabalMeshVoucher.sol`, `CabalMeshModules.sol` and `RelayRewards.sol` under `contracts/contracts/`, and `docs/module-trust-model.md`, all from commit `e26157c` (`git show e26157c:<path>`); list each behaviour (mint authority, immutable metadata, loadout slots, listing requires ownership, atomic buy, release window 259200 s).
- [ ] **Step 2:** Map each to NEAR: NEP-171 module collection minted only by `rewards.cabalmesh.testnet` at relay milestones; marketplace settles NFT ⇄ USDC atomically (via the verifier's NEP-245/NEP-171 support if Task 1 shows it, otherwise its own escrow).
- [ ] **Step 3:** Write the plan in this file's format and bring it back for review before execution.
