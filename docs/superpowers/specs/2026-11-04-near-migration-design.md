# NEAR support, BYOK and agent peer trade — design decisions

Settled in a grilling session on 2026-11-04. Each line carries the question
number it was decided under, so the reasoning can be found in that session.
Plans in `docs/superpowers/plans/2026-11-04-p*.md` implement this document.

## Phase order (Q2)

1. **P1 — NEAR settlement** through NEAR Intents.
2. **P2 — NEAR contracts**: gateway rewards first, then marketplace and modules.
3. **P3 — BYOK**: OpenAI-compatible model configuration.
4. **P4 — Agent peer trade**: function-calling agent that negotiates over the mesh.

## Architecture

- **NEAR is the only chain.** This replaces Q1, Q16, Q17 and Q26 (Avalanche side by
  side), on the user's decision of 2026-11-05. `contracts/`, alloy and every EVM path
  are removed, so there is no `ChainBridge` trait and no `capabilities()`.
- Every intent carries a `chain` field (`near-testnet`, `near-mainnet`). A missing field
  or any other value does not parse: a peer's such intent is ignored, and a ledger holding
  one opens empty. Matching only pairs intents on the same chain (Q8).
- Assets come from the network's `NearConfig`. The UI offers only what settles (Q9, Q13).

## P1 — NEAR settlement

- Implicit ed25519 accounts: account id is the hex public key (Q6).
- The NEAR key is a random key in the vault; guardian recovery restores it because it
  shares the vault data key (Q10). A record with no NEAR key gains one on open, and that
  save keeps nothing else, so a key from an older build does not survive it.
- Trades settle through NEAR Intents (Q5). Testnet uses the community Tidepool
  deployment (Q11):
  - verifier `verifier.tidepool-sandbox.testnet` (`defuse` v0.4.5, unmodified upstream)
  - tokens `wnear.tidepool-sandbox.testnet` (24 decimals), `usdc.tidepool-sandbox.testnet` (6 decimals)
- All NEAR account ids live in `network_config`, never hard-coded. A `near-mainnet`
  entry (`intents.near`) sits behind the existing MAINNET toggle (Q18).
- The quote asset on NEAR is USDC: every trade is `asset ⇄ USDC` at the agreed USD price (Q13).
- First task is a spike proving two implicit accounts can deposit, sign
  `token_diff` for each other and have a third account submit `execute_intents`
  against the Tidepool verifier. If it fails, deploy `defuse` to testnet
  ourselves; only config changes (Q11). Checked on 2026-11-04: `execute_intents`
  is open to any caller, fee is 0, signatures from never-funded implicit accounts
  are accepted, and Tidepool's faucet credits tokens straight into the verifier.
- Deposit and Withdraw are explicit buttons in Vault. The composer shows the
  balance available inside the verifier and refuses amounts above it (Q14).
- Test funds are obtained by hand: the app shows the address, a copy button and
  faucet links (Q19).
- New mesh message `SignedDiff`, sent in the clear (Q15). A gateway that holds
  both signatures submits `execute_intents`. In P1 gateways submit for free (Q12, Q12b).

## P2 — NEAR contracts

- Rewards first: pay the gateway that submitted, proven on-chain; no relay-proof
  port (Q20). Then marketplace and modules (Q9).
- Standing is derived from Intents settlement events; no registry contract (Q9).

## P3 — BYOK

- OpenAI-compatible `/v1/chat/completions` only (Q3). No environment variables (Q4).
- Profile screen: Base URL, Model, API key, with presets NEAR AI Cloud, Ollama
  local, OpenRouter (Q21).
- API key lives in the vault; base URL and model in a plain file. The key never
  crosses the mesh (Q4b).
- "Test connection" also sends one probe tool call. A model that fails it gets a
  warning and the agent falls back to app-side price prefetch (Q27).

## P4 — Agent peer trade

- Real function calling (Q22) with four tools: `get_market_price`,
  `list_mesh_offers`, `get_my_position`, `propose_offer`. **No signing or transfer
  tool.** At most 5 tool calls and 30 s per decision (Q28).
- Prices come from `/v0/tokens` of the verifier in use. Offline uses the cache;
  a price older than 30 minutes makes the agent suggest-only (Q23).
- At most 3 rounds of `Offer`/`Counter`/`Accept`, each with a timeout. Code
  enforces the user's price bounds on every offer (Q24).
- The agent negotiates; the user confirms before anything is signed (Q7).
- Code filters before any model call (same chain, same asset, opposite side,
  overlapping price) and a per-hour call budget applies, default 30, editable in
  Profile (Q29).
