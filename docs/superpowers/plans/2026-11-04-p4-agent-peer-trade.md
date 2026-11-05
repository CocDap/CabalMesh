# P4 — Agent Peer Trade Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** When two devices hold opposite intents on the same chain and asset, their agents negotiate a price over the mesh in up to three rounds using real tool calls, and the user confirms the agreed deal before anything is signed.

**Architecture:** A pure, I/O-free negotiation state machine (`cabal-negotiation` crate) owns rounds, timeouts and bound checks. `agent.rs` (rewritten) runs one decision turn: it gives the model four tools, executes the calls the model makes, and returns one `propose_offer` or nothing. Every offer the model proposes passes through the state machine, which rejects anything outside the user's bounds. Accepted deals feed the existing `DealAccepted` → confirm → settle path from P1.

**Tech Stack:** Rust; `llm_client` and `LlmBudget` from P3; `near_bridge`/price feeds from P1.

**Spec:** `docs/superpowers/specs/2026-11-04-near-migration-design.md` (P4 section). Depends on P1 and P3.

## Decision this plan needs before executing

- **D1 — What an intent broadcasts.** Today `broadcast_intent` publishes the whole `IntentDraft`, including `condition` (the user's price limit). With limits public, a counterparty's agent simply offers at your limit and three rounds of negotiation achieve nothing. **Recommended:** broadcast a `PublicIntent { id, chain, asset, side, amount }` with no price; the user's limit stays on the device as the private bound. The pre-model filter (spec Q29) then checks chain, asset and side, and price overlap is discovered through offers. **Alternative:** keep broadcasting the limit (today's behaviour) and accept that negotiation converges at one side's limit. Task 1 is written for the recommended option.

## Global Constraints

- At most **3 rounds** per negotiation; each round times out after **60 s**; a timed-out or exhausted negotiation returns the intent to its open state.
- Per decision turn: at most **5 tool calls**, **30 s** wall clock.
- Price staleness limit **30 minutes**; a stale price makes the agent suggest-only (it may not send `Offer`, `Counter` or `Accept`; it writes a suggestion line to the intent log).
- Price feeds: the `price_feed` of the active `NearConfig` (P1 Task 5).
- No tool signs, transfers or broadcasts anything except `propose_offer`, which only queues an offer for the state machine to validate.
- Model calls go through `LlmBudget`; when the budget is exhausted or `tools_supported == Some(false)`, the turn uses the prefetch fallback (Task 3).
- Negotiation messages are sent in the clear (spec Q15) and never contain the private bound.

## Review Focus

1. **Model proposes a price outside the user's bound** (including after a prompt-injection attempt in a peer's message text). Expect the state machine to reject it and the agent to send nothing that round. Test owned by Task 1.
2. **Peer sends `Accept` for a price nobody offered, or for an expired round.** Expect it ignored. Test owned by Task 1.
3. **Both agents send `Accept` at once for different last offers.** Expect the deal to settle only when both accepts name the same price; otherwise the negotiation ends unmatched. Test owned by Task 1.
4. **Model loops on tool calls or returns malformed arguments.** Expect the turn to stop at 5 calls or 30 s with no offer sent. Test owned by Task 3.
5. **A peer floods intents to burn the user's API budget.** Expect the pre-model filter and `LlmBudget` to bound calls to the configured hourly limit. Test owned by Task 4.

---

### Task 1: `cabal-negotiation` — rounds, timeouts, bounds

**Files:**
- Create: `src-tauri/crates/cabal-negotiation/Cargo.toml`, `src/lib.rs`
- Modify: `src-tauri/crates/cabal-core/src/intent.rs` (`PublicIntent`), `src-tauri/src/commands.rs` (`publish` sends `PublicIntent`, per D1)
- Test: `src-tauri/crates/cabal-negotiation/src/lib.rs` tests

**Interfaces:**
- Produces:
  - in `cabal-core`: `pub struct PublicIntent { pub id: String, pub chain: Chain, pub asset: Box<str>, pub action: Action, pub amount: TokenAmount }` (only `Buy`/`Sell` are ever published)
  - in `cabal-negotiation`: `pub enum Side { Buy, Sell }` with `TryFrom<Action>`
  - `pub struct Bound { pub side: Side, pub limit_cents: Option<u64> }` — buyer: max; seller: min; `None` = any price
  - `pub enum Msg { Offer { neg_id: String, round: u8, price_cents: u64 }, Counter { neg_id: String, round: u8, price_cents: u64 }, Accept { neg_id: String, round: u8, price_cents: u64 } }`
  - `pub struct Negotiation` with `fn start(neg_id, own: Bound, now_ms) -> Self`, `fn propose(&mut self, price_cents: u64, now_ms: u64) -> Result<Msg, Reject>`, `fn receive(&mut self, msg: &Msg, now_ms: u64) -> Outcome`, `fn tick(&mut self, now_ms: u64) -> Outcome`
  - `pub enum Outcome { Continue, Agreed { price_cents: u64 }, Ended { reason: EndReason } }`; `pub enum EndReason { RoundsExhausted, TimedOut, Unmatched }`; `pub enum Reject { OutsideBound, NotOurTurn, Ended }`
  - constants `MAX_ROUNDS: u8 = 3`, `ROUND_TIMEOUT_MS: u64 = 60_000`

- [ ] **Step 1: Write the failing tests**

```rust
#[test]
fn seller_cannot_propose_below_floor() {
    let mut n = Negotiation::start("n".into(), Bound { side: Side::Sell, limit_cents: Some(390) }, 0);
    assert_eq!(n.propose(389, 1), Err(Reject::OutsideBound));
}

#[test]
fn messages_never_contain_the_bound() {
    let mut n = Negotiation::start("n".into(), Bound { side: Side::Sell, limit_cents: Some(390) }, 0);
    let msg = n.propose(450, 1).unwrap();
    assert!(!serde_json::to_string(&msg).unwrap().contains("390"));
}
```

Plus: `fourth_round_is_rejected`; `round_times_out_after_60s` (`tick(60_001)` → `Ended { TimedOut }`); `accept_for_unoffered_price_is_ignored`; `accept_after_end_is_ignored`; `matching_accepts_agree`; `crossed_accepts_end_unmatched`; `public_intent_has_no_price_field` (serialize, assert no `condition` key).
- [ ] **Step 2:** Run `cargo test -p cabal-negotiation` → FAIL.
- [ ] **Step 3:** Implement. Turn order: the seller opens round 1.
- [ ] **Step 4:** Run → PASS; `cargo test --workspace` (publish change) → PASS; `npm run bindings`.
- [ ] **Step 5: Commit** — `git commit -m "feat(negotiation): bounded three-round state machine; intents broadcast without a price"`

---

### Task 2: Price feed and tool implementations

**Files:**
- Create: `src-tauri/src/price_feed.rs`
- Create: `src-tauri/src/agent_tools.rs`
- Test: both files' test modules (local `TcpListener` mock serving a captured `/v0/tokens` body)

**Interfaces:**
- Consumes: `NearConfig.price_feed` (P1 Task 5), `NearBridge::verifier_balance` (P1 Task 6), the intent ledger, `Negotiation` (Task 1).
- Produces:
  - `pub struct PriceQuote { pub symbol: String, pub usd_cents: u64, pub updated_at_ms: u64, pub stale: bool }`
  - `pub struct PriceFeed` with `async fn quote(&self, symbol: &str, now_ms: u64) -> Option<PriceQuote>` — fetches when online, caches per symbol, marks `stale` past 30 min
  - `pub fn tool_specs() -> Vec<ToolSpec>` — exactly four: `get_market_price{asset}`, `list_mesh_offers{asset, side}`, `get_my_position{}`, `propose_offer{price_usd}`
  - `pub struct ToolResult { pub call_id: String, pub content: String }` (content is JSON text for the model)
  - `pub async fn execute(call: &ToolCall, ctx: &ToolContext) -> ToolResult` — `propose_offer` stores the price in `ctx.proposed` only
  - `pub struct ToolContext { pub chain: Chain, pub asset: String, pub own_intent: IntentId, pub feed: Arc<PriceFeed>, pub proposed: Mutex<Option<u64>> }`
  - `get_my_position` returns balance and side, and **never** the private bound (the bound check happens in Task 1's state machine)

- [ ] **Step 1:** Failing tests: `quote_parses_tidepool_tokens_body` (wNEAR `price` 4.04 → `usd_cents == 404`); `quote_is_stale_after_30_minutes`; `offline_returns_cached_quote_marked_stale`; `tool_specs_are_exactly_four`; `get_my_position_omits_bound`; `propose_offer_only_records` (no mesh publish happens).
- [ ] **Step 2:** Run → FAIL. **Step 3:** Implement. **Step 4:** Run → PASS.
- [ ] **Step 5: Commit** — `git commit -m "feat(agent): price feed and four negotiation tools"`

---

### Task 3: Agent decision turn

**Files:**
- Rewrite: `src-tauri/src/agent.rs` (the old PDF `SharkAgent` is removed if P3 Task 2 did not already)
- Test: `src-tauri/src/agent.rs` tests with a scripted mock chat server

**Interfaces:**
- Consumes: `LlmClient::chat` (P3 Task 2), `tool_specs`/`execute`/`ToolContext` (Task 2), `LlmSettings.tools_supported` (P3 Task 3).
- Produces: `pub async fn decide(client: &LlmClient, ctx: &ToolContext, history: &[Msg]) -> Decision`; `pub enum Decision { Propose { price_cents: u64 }, SuggestOnly { note: String }, Nothing }`. Loop: send system + history, execute returned tool calls, append results, repeat until a reply with no tool calls, 5 calls, or 30 s. Fallback when `tools_supported == Some(false)`: prefetch `get_market_price` and `get_my_position`, put them in the prompt, call `chat_json` expecting `{"price_usd": number}`.

- [ ] **Step 1:** Failing tests: `returns_propose_after_price_lookup` (mock: `get_market_price` call, then `propose_offer{4.10}`, then text → `Propose { 410 }`); `stops_after_five_tool_calls` (mock always calls `get_market_price` → `Nothing`, exactly 5 executions recorded); `stops_after_30_seconds` (mock sleeps; use `tokio::time::pause`); `malformed_arguments_yield_nothing`; `stale_price_yields_suggest_only`; `fallback_path_used_when_tools_unsupported` (assert the mock saw no `tools` field).
- [ ] **Step 2:** Run → FAIL. **Step 3:** Implement. **Step 4:** Run → PASS.
- [ ] **Step 5: Commit** — `git commit -m "feat(agent): bounded tool-calling decision turn with fallback"`

---

### Task 4: Wire negotiation into the mesh

**Files:**
- Modify: `src-tauri/src/negotiation.rs` (`consider` starts a `Negotiation` instead of a one-shot price)
- Modify: `src-tauri/src/mesh.rs` (dispatch `"Offer" | "Counter" | "Accept"` as `MeshEvent::Negotiation { details }`)
- Modify: `src-tauri/src/lib.rs` (route to `negotiation::on_message`; call `tick` every 5 s)
- Modify: `src/screens/Detail.tsx` (round log lines; `CONFIRM TO SETTLE` button appears on `Agreed`)
- Test: `src-tauri/src/negotiation.rs` tests

**Interfaces:**
- Consumes: Tasks 1–3, `LlmBudget` (P3 Task 5), P1 `settle_intent`.
- Produces: `pub async fn on_message(state: &AppState, details: &str)`; on `Outcome::Agreed` the intent moves to `Negotiating { best: Some(price) }` and logs `AGENTS AGREED AT {price} (CONFIRM TO SETTLE).` — settlement still requires the user's `settle_intent`.

- [ ] **Step 1:** Failing tests: `filter_skips_model_for_other_chain_asset_or_same_side` (mock LLM records zero calls); `flood_is_capped_by_budget` (100 matching intents, budget 30 → at most 30 model calls); `agreement_requires_user_confirmation` (after `Agreed`, assert no `SignedDiff` was published until `settle_intent` runs); `timeout_returns_intent_to_open`.
- [ ] **Step 2:** Run → FAIL. **Step 3:** Implement. **Step 4:** Run `cargo test --workspace`, `npm run build` → PASS.
- [ ] **Step 5: Commit** — `git commit -m "feat(negotiation): agents negotiate over the mesh, user confirms"`

---

### Task 5: Two-device negotiation run

**Files:**
- Modify: `docs/near-two-device-checklist.md` (add a negotiation section)

- [ ] **Step 1:** Node A: preset NEAR AI Cloud, `SELL 1 wNEAR` with private floor $3.90. Node B: preset Ollama local, `BUY 1 wNEAR` with private ceiling $4.20. Expect offers in the Detail log, agreement within 3 rounds, and no message on the mesh containing `390` or `420`.
- [ ] **Step 2:** Repeat with node B on a model whose probe reports `tools_ok = false`; expect the fallback path and still an agreement or a clean `Unmatched`.
- [ ] **Step 3:** Confirm on both; expect P1 settlement to complete. Paste tx hashes.
- [ ] **Step 4: Commit** — `git commit -m "docs: two-device agent negotiation checklist"`
