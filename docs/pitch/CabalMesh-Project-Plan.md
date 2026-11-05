# CabalMesh — Project Development Plan

**Planning horizon:** Aug 2026 → Jan 2027
**Source of truth for the baseline:** [`docs/product-status.md`](../product-status.md) (code-audited, not aspirational)
**Owner:** kurodenjiro

This is the working plan, not the pitch. Every phase below is scoped from what the code
actually does today, sequenced so each phase's output unblocks the next, and written so it
can be re-verified against the repo at any time — not just trusted.

---

## 1. Baseline — where the plan starts from

Audited 2026-08-10 (`docs/product-status.md`), re-checked against the current branch.

### Works end-to-end
- Mesh networking (libp2p, mDNS, gossipsub) — `src/mesh.rs`, 9 passing tests
- BLE offline plane (iOS + Android) — `src/ble/`, `crates/cabal-ble`
- Intent lifecycle: compose → broadcast → settle → proof — `src/commands.rs`, `src/intents.rs`
- On-chain Escrow, live on Fuji — `contracts/contracts/Escrow.sol`
- Offline signing + relay queue + auto-confirm on reconnect — `src/blockchain_bridge.rs`
- Vault encryption (AES-256-GCM) — `crates/cabal-vault`
- Guardian mesh unlock — enrollment, unlock request/approve, over real (loopback) BLE transport

### UI exists, behavior does not
- **PRIVACY** (LOW/MEDIUM/HIGH) — parsed and echoed, no routing code anywhere
- **MODE** (SHARK/GHOST/PATIENT) — labels exist, no strategy differs in `mesh.rs`/`matcher.rs`
- **SWAP / STAKE** — parsed, then ignored; every action takes the Escrow path
- **USDC / WETH / BTC.b** — offered in the UI; only native AVAX balance is ever synced

### Not wired up at all
- **AI negotiation** — `SharkAgent`/`MatchAgent` exist but are called from nowhere in `commands.rs`
- **Marketplace / module cards** — `Marketplace.sol` and `CabalMeshVoucher.sol` deployed to Fuji, no command exposes them, no UI
- **ZK proving / verification** — no code at all. `zk_handler.rs` and `noir-circuit/` were deleted on 2026-08-27 because nothing called them; Phase 4 starts from an empty page
- **Confidential compute (FHE/MPC)** — no code exists
- **Recovery delay + veto** — the 24h window and push notification are designed but not built
- **Mobile PIN unlock** — blocked on the native key-store plugin (ticket 21)
- **Physical hardware** — CabalMesh is software-only today; no device has been designed or manufactured

**Takeaway driving this plan:** the offline mesh settlement path is the one differentiated,
fully-working story. Everything else is sequenced behind closing the wallet's most urgent gap
(identity backup/recovery, shipped together with reconnecting the AI) before adding surface
area — including turning the concept into physical products people can see and buy.

---

## 2. Roadmap — five phases

| # | Phase | Window | Status |
|---|---|---|---|
| 1 | Identity, recovery & AI intent parsing | Aug – Sep 2026 | In progress |
| 2 | Marketing, sales & hardware devices | Sep – Oct 2026 | Next |
| 3 | Marketplace goes live | Oct – Nov 2026 | Next |
| 4 | Harden verification & negotiation | Nov – Dec 2026 | Later |
| 5 | Confidential compute & platform hardening | Dec 2026 – Jan 2027 | Exploratory |

Windows are planned execution ranges, not fixed deadlines — phase *N+1* starts once phase
*N*'s definition of done is met, not on a calendar trigger.

---

### Phase 1 — Identity, recovery & AI intent parsing (Aug – Sep 2026, in progress)

**Goal:** Close the "lose a device, lose the funds — permanently, with no recourse" risk, and
turn "buy 10 AVAX under $25, shark mode" back into a real typed-input path — shipped together
because AI intent parsing sits directly on top of the same validated draft that guardian
recovery protects, and both are the safest, most-built pieces of the roadmap.

**Already shipped this phase:**
- Passphrase unlock (Argon2id), opt-in — `cabal_vault::PassphraseKeyProvider`
- Export / import / restore — `VAULT → KEYS → ADVANCED`
- Guardian mesh unlock — enrollment, unlinkable per-request recognition tags over BLE,
  unlock reply gated on explicit human approval — `cabal_guardian`, `src/guardian.rs`,
  `src/guardian_actor.rs`
- `parse_intent_chat` — fills the same validated fields `New.tsx` already uses, unchanged

**Key deliverables remaining:**
- 24–48h recovery delay with a veto notification — needs a background task and a
  local/push notification that survive the app being closed (real platform integration,
  cannot be verified without a physical device)
- Mobile PIN unlock path, backed by the native key-store plugin (ticket 21) — only iOS/Android
  have a hardware-enforced retry counter, so a PIN is unsafe without it
- Pixel-match the `CHOOSE GUARDIANS` / `DISTRIBUTING SHARES` screens to the one working
  flow-per-step that exists today (enroll, approve, restore)
- Guardian storage should switch protection together with `SECURITY`'s passphrase toggle
- Ship the editable "chip" UI so a wrong chat-intent parse is cheap to fix without falling
  back to the full form
- Build the recovery-assistant conversation flow (guided Q&A) for the "I lost my phone" case
- Confirm the safety property holds throughout: the model only produces `IntentFields`,
  Rust still validates, previews, and signs — the AI proposes, it never signs

**Definition of done:** A wiped device restores its wallet using 3-of-5 guardians, with a
live 24h veto window verified on a real iOS/Android device; and a user can type a
natural-language intent, see it parsed into editable chips, correct one field, and broadcast
— with no change to what Rust validates or signs.

**Blocked by:** Physical device testing for background tasks and push notifications;
ticket 21 (native key-store plugin) for the mobile PIN.

---

### Phase 2 — Marketing, sales & hardware devices (Sep – Oct 2026, next)

**Goal:** Turn the "Nobody Stack" from a software concept into tangible products people can
see, promote, and buy — building demand and a physical distribution channel ahead of the
software marketplace going live. This phase is marketing/business development, not firmware
engineering, and should not block the software phases around it.

**Key deliverables:**
- Design and produce concept renders for **ShadowBox** — one node that carries the whole Cloak
  Layer: it relays mesh traffic, runs the local model that reads an intent, and generates the
  proof, with RADIO / CRYPTO / POWER module bays matching the in-app module system
- Design and produce concept renders for **the Nobody Box** — a parcel locker whose bolt turns
  on the on-chain escrow release: the seller drops the item in, the buyer opens it, and nobody
  in between can — "prove without revealing" applied to a physical handover
- Produce marketing assets: product renders, a short demo video, landing-page copy, and pitch
  materials for promotion and pre-order
- Launch a pre-order / sales landing page and run a promotional push through community,
  socials, and hackathon channels

**Definition of done:** Both device concepts have finished renders (or a working prototype if
timeline allows), a pre-order/sales page is live, and at least one promotional campaign has
reached an audience beyond the project's current testers.

**Blocked by:** No hardware manufacturing partner identified yet; needs industrial-design
resourcing for the physical enclosure. Concept renders and the pre-order page do not depend on
manufacturing being solved first.

#### Phase 2 — marketing plan detail

**Target audience:**
- Avalanche ecosystem community (holders, builders, ecosystem grant/hackathon circles)
- Web3 builders and judges — the near-term audience that already sees the demo and the code
- Privacy/crypto-hardware enthusiasts — the audience already primed for a physical security
  key or hardware wallet, who will recognize the Nobody Box's category immediately — and the
  much larger group who already own a parcel drop box and have never trusted one
- Off-grid, disaster-response, and hyper-local-trade communities — the audience the "hyper-local
  confidential trade" use case is written for, and the most natural buyer for ShadowBox itself

**Positioning / messaging pillars:**
- *"It opens when the deal settles."* — the Nobody Box as the physical icon of escrow, and the
  one claim in the whole project that needs no explanation at all
- *"Mesh you can hold."* — ShadowBox as tangible proof the offline settlement path is real,
  not a slide
- Anchor every message in what's actually shipped (the offline mesh settlement path) — the same
  "one true story" discipline as the rest of this plan (§4), not overclaiming AI/ZK maturity the
  product doesn't have yet

**Channels:**
- Avalanche ecosystem channels — X/Twitter, Discord, ecosystem hackathon/demo days
- Crypto-hardware and privacy-tech communities — r/privacy, r/CryptoCurrency, a Hacker News
  "Show HN"-style launch post
- A short demo video building on the existing YouTube demo, styled as an unboxing/hands-on
  rather than a pitch
- A landing page with an email waitlist — capture interest first, no payment collected until a
  pre-order mechanism is deliberately decided (see budget note below)
- **Kaito AI mindshare campaign** — list CabalMesh as a Kaito Genesis/Yapper project so the
  community's own posts do the reach-building; see "Mesh missions" below for how this pairs
  with real product usage instead of empty engagement farming

**Mesh missions — turning real usage into the campaign:**

A two-track quest system, so the campaign rewards people who actually touch the product, not
just people who post about it.

*Track A — product missions (on-chain, verifiable against the app itself):*
1. Broadcast your first intent over the mesh, fully offline
2. Relay traffic as a gateway for a set amount of MB
3. Enroll guardians and complete a mesh-recovery test unlock (Phase 1's own flow)
4. Buy or equip a module NFT on the Marketplace (once Phase 3 ships)

*Track B — social missions, scored via Kaito AI:*
5. Post about CabalMesh — Kaito's AI mindshare algorithm scores the post and ranks it on a
   public leaderboard, the same mechanic Kaito runs for other crypto projects' "Yapper"
   campaigns
6. Refer a friend who completes mission 1 (mesh-verified, not just a signup)

**Reward:** crossing a mission threshold (mix of both tracks) mints a soulbound "Genesis Node"
badge NFT through the existing `CabalMeshVoucher` contract — the same non-tradable mechanic
already used for the in-app Standing Badge, so this reuses a primitive that already exists
rather than inventing a new reward contract. Kaito's leaderboard decides *who* ranks; the app's
own on-chain state decides *what counts* — Kaito never gates a mission it can't verify.

**Campaign timeline (inside the Sep–Oct 2026 window):**

| Weeks | Focus |
|---|---|
| 1–2 | Finish renders and landing-page copy (tail end of Phase 1's overlap) |
| 3–4 | Soft-launch teaser on socials/community channels; open the waitlist; list on Kaito |
| 5–6 | Full launch push — demo video, hackathon/demo-day presence, launch thread, mesh missions go live |
| 7–8 | Pre-order page live with concrete specs; mint Genesis Node badges to mission completers |

**KPIs / success signals:**
- Waitlist/pre-order signups (target to be set by the owner once the landing page is live)
- Reach/impressions on launch content across the channels above
- Kaito mindshare score / leaderboard rank, and number of mesh missions completed
- At least one mention or pickup outside owned channels (community repost, press, or a
  hackathon shoutout) — the same "beyond current testers" bar as the phase's definition of done

**Budget note:** No paid ad spend is budgeted for this phase — it runs on organic and community
channels plus hackathon presence. Paid acquisition is a decision for a later phase, once
pre-order demand from the organic push is actually validated.

---

### Phase 3 — Marketplace goes live (Oct – Nov 2026, next)

**Goal:** Turn the deployed-but-inert Marketplace and Voucher contracts into a real,
earnable in-app economy: relay traffic → earn MB → earn AVAX → buy modules → raise relay rate.

**Key deliverables:**
- **Fix first, before anything else ships:** `CabalMeshVoucher.mintVoucher` currently has no
  access control — anyone can mint themselves any module for free, right now, against the
  live Fuji contract. Redeploy with minting locked to the `RelayRewards` contract address
  (not an off-chain admin key), so a module only mints as the atomic side effect of a
  settlement the contract already verified on-chain.
- Delete the dead local-JSON relay-boost code (`apply_relay_boost` et al. in
  `blockchain_bridge.rs:841-852`) rather than reconnect it — compute the relay-yield
  multiplier on demand from verified on-chain module ownership, every time it's shown.
- Add structured on-chain fields to the voucher (`slot`, `rarity`, `effectBps`) instead of an
  off-chain `tokenURI` metadata host — keeps one read path, and keeps a module's effect
  something Rust can verify directly.
- Scope relay rewards to **gateway relaying only** for v1 (a sender-paid fee, not treasury
  emission) — BLE flood relay has no routing table and no single attributable relayer per hop;
  gateway relaying is already on-chain and attributable. Wire a real gateway relay to call
  `RelayRewards.recordGatewayRelay`.
- Ship the `MARKET` tab and `VAULT → MODULES` UI end-to-end against the live redeployed Fuji
  contracts (escrow-backed listings: AVAX locks until the module actually transfers).
- Keep `RELAYED TODAY` honest at `0` (not tracked yet) until BLE-relay attribution exists,
  rather than fabricate activity a reward can't actually attribute.

**Definition of done:** A user earns MB from real gateway relaying, watches it convert to
AVAX, and buys or equips a module NFT that visibly changes their relay yield on HOME.

**Blocked by:** The contract redeploy is a one-way action — it replaces the live Fuji address
the app points at. Needs an explicit go/no-go checkpoint, separate from writing the code
(low-stakes today: Fuji testnet, no value locked in the current contract).

---

### Phase 4 — Harden verification & negotiation (Nov – Dec 2026, later)

**Goal:** Make the ZK-proof and AI-negotiation claims the project already makes actually
true end-to-end, not just structurally present.

**Key deliverables:**
- Write the bid circuit and the proving path from scratch, in a dev/CI environment with
  `nargo` in it. The earlier stub — a three-constraint circuit and a `nargo` shell-out no
  command ever called — was deleted on 2026-08-27 rather than left to imply a capability.
  Decide the proving library up front: a shell-out cannot ship on mobile at all.
- Design the buyer/seller negotiation protocol: offer → counter-offer → accept/reject message
  types (similar in spirit to `relay_tx`/`content_request` in `mesh.rs`), with a round limit
  so two agents can't loop forever.
- Enforce price guardrails in Rust, never trusted to the model: the buyer's agent must never
  bid above the user's price ceiling, the seller's agent must never accept below their floor.
- Fix the underlying JSON-parsing reliability gaps first (llama2 via Ollama often ignores
  "respond only with JSON") — a multi-turn negotiation multiplies that risk with every round,
  so this is a prerequisite, not parallel work.

**Definition of done:** A submitted bid's ZK proof is generated and verified against a real
circuit from a command the app actually calls; two agents complete a bounded negotiation
without either breaching its guardrail.

**Blocked by:** No `nargo` in the current dev/CI environment, and no circuit to run in it. Negotiation itself is
deliberately deferred until the single-shot matching flow's JSON parsing is solid — shipping
it earlier risks an inconsistent multi-round "agreement" from a parsing slip.

---

### Phase 5 — Confidential compute & platform hardening (Dec 2026 – Jan 2027, exploratory)

**Goal:** Extend privacy beyond ZK proofs and close the remaining platform gaps. This phase
is genuinely open-ended research — the deliverable is a feasibility spike, not shipped code.

**Key deliverables:**
- Feasibility spike: FHE/MPC for negotiation-content privacy. No code exists yet anywhere in
  this codebase for this; the phase starts from a design doc, not an implementation.
- Desktop key-store integration (Secure Enclave / TPM) to replace the plain
  `0o600` file-backed vault key (`vault_key.rs::FileKeyProvider`) that every platform,
  including macOS and Windows, currently uses.
- Decide the BLE-plane story for Windows/Linux — `ble::backend::choose` returns `None`
  outside Apple/Android today, and the app silently falls back to the IP plane without
  telling the user the offline plane is off.

**Definition of done:** A feasibility write-up and a scoped follow-on plan for FHE/MPC — not
shipped code, given there is no prior art in this codebase to build on.

**Blocked by:** Depends on phases 1–4 stabilizing first. Scope and timeline are the least
certain of the five phases by design.

---

## 3. Dependencies & risks

| Risk / blocker | Impacts | Mitigation |
|---|---|---|
| No physical iOS/Android test device | Phase 1 | Background tasks and push notifications cannot be verified on an emulator — borrow or acquire test hardware before calling Phase 1 done. |
| Native key-store plugin not built (ticket 21) | Phase 1 | Scope it as its own workstream now; mobile PIN has no hardware-enforced retry counter without it. |
| No hardware manufacturing partner / industrial design resourcing | Phase 2 | Scope Phase 2 as a marketing/business-development track — concept renders and a pre-order page do not require manufacturing to be solved first. |
| No ZK code and no `nargo` in dev/CI | Phase 4 | The circuit and proving path are gone (deleted 2026-08-27, unused). Budget Phase 4 for writing them, not for wiring an existing stub, and stand up a runner with `nargo` before the work starts. |
| Contract redeploy is one-way | Phase 3 | Redeploying `CabalMeshVoucher` replaces the live Fuji address — treat it as an explicit go/no-go checkpoint, not a routine deploy. |
| Ollama/llama2 JSON output is unreliable | Phase 4 | Keep the single-shot matching flow as the fallback; multi-turn negotiation stays deferred until parsing is solid. |

---

## 4. Scope decision — what ships now vs. later

**One true story beats three half-built ones.** The offline mesh settlement path is provable
today; AI and ZK are not, yet. Scope the UI to match:

**Remove or disable now:**
- SWAP / STAKE actions — every path already resolves to the same Escrow flow
- The three unfunded assets — USDC, WETH, BTC.b never show a real balance
- PRIVACY level and MODE toggles — wire them to real behavior, or hide them until they do

**Add, in this order:**
1. Identity backup, recovery & AI intent parsing (Phase 1) — a wallet with no way to export
   its key is the most urgent gap, shipped alongside the safest, most-built piece of
   "AI negotiates"
2. Marketing, sales & hardware devices (Phase 2) — turn the concept into something people can
   see, promote, and pre-order, in parallel with the software roadmap
3. Everything else — marketplace, verification hardening, confidential compute (Phases 3–5)

---

## 5. Next 30 / 60 / 90 days

**30 days — close out Phase 1's core risk items**
- Finish the 24h recovery delay + veto notification
- Scope ticket 21 (native key-store plugin) as its own workstream
- Hide or disable SWAP/STAKE and the three unfunded assets in the UI
- Kick off concept sketches for ShadowBox and the Nobody Box

**60 days — ship the rest of Phase 1, start Phase 2 production**
- Ship the editable chip UI and QA the chat-intent flow end-to-end
- Build the recovery-assistant conversation for lost-device cases
- Finish concept renders and draft marketing/landing-page copy for both devices

**90 days — launch Phase 2, prep Phase 3**
- Launch the pre-order/sales page and run the first promotional campaign
- Draft the `CabalMeshVoucher` redeploy plan and go/no-go checklist

---

## 6. Hardware product concepts (Phase 2)

Two physical devices anchor the marketing/sales push — image-generation prompts for both are
kept alongside this plan in [`hardware-device-prompts.md`](hardware-device-prompts.md).

- **ShadowBox** — the Cloak Layer as hardware, and one machine instead of three: mesh relay,
  local model and prover in a single finned slab, with RADIO / CRYPTO / POWER module bays
  mirroring the in-app module system exactly.
- **The Nobody Box** — a parcel locker that is also the escrow: the bolt turns on the on-chain
  release, a load cell reports that something went in and something left, and there is no
  keyhole for anyone in between.

---

*This is a living plan. Every phase is checked against the code, not the pitch deck — when
`docs/product-status.md` changes, this plan is what gets revised.*
