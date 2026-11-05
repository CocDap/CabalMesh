# Roadmap re-baseline — 20 August 2026

The plan in [`CabalMesh-Project-Plan.md`](CabalMesh-Project-Plan.md) was scoped from an audit
written on 10 August. Ten days later it is out of date in a way that changes the order of the
work, not just the dates. This document is the analysis behind the rewrite: what the code now
does, what the outside world now looks like, and why the phases are re-sequenced.

Everything below is checked against either the repository or a named public source.

---

## 1. What changed inside the repo

`28ae9fa` — "Add guardian social recovery and a real Marketplace/Modules UI" — landed after the
audit and is large. Re-reading the code rather than the commit message:

| Claim | Checked against | Status |
|---|---|---|
| IPC surface grew from 21 to **45 commands** | `tests/snapshots/ipc_contract__command_inventory.snap` | verified |
| Guardian recovery is real: enroll, request, approve, deny, status | `src/guardian.rs`, `src/guardian_actor.rs`, `crates/cabal-guardian` | verified |
| Marketplace is wired to the **on-chain** contract, not local state | `market_buy` → `bridge.buy_listing` → `IMarketplace` (`blockchain_bridge.rs`) | verified |
| Module economy commands exist end to end | `vault_modules`, `vault_equip_module`, `vault_redeem_module`, `market_list_module`, `market_release_deal`, `market_refund_deal` | verified |
| AI intent parsing is wired into the app | `parse_intent_chat` registered in `lib.rs:372`, implemented `commands.rs:930` | verified |
| Passphrase unlock (Argon2id) and key export/import | `crates/cabal-vault/src/passphrase.rs`, `vault_export_key`, `vault_import_key` | verified |
| Test suite | 421 Rust tests (61 integration, 131 in `src/`, 229 in crates) + 49 Hardhat tests | counted |

**So two of the old plan's five phases are largely built.** Phase 1 (identity/recovery/AI
parsing) and Phase 3 (marketplace) are code-complete except for the items below.

### The gap is no longer code — it is deployment

| Gap | Evidence | Consequence |
|---|---|---|
| The voucher access-control fix exists in Solidity but is **not deployed** | `CabalMeshVoucher.sol:62` has `require(msg.sender == rewardsContract, ...)`; `contracts/deployments/fuji.json` still lists the 2026-07-22 voucher | The live contract is still the free-mint one the old plan flagged |
| `RelayRewards` is **not deployed at all** | No `relayRewards` key in `deployments/fuji.json`; `deployRelayRewards.ts` exists but unrun | Nothing can mint a module the honest way |
| Gateway relay is **not attributed** | `record_gateway_relay` exists in `blockchain_bridge.rs:1158`; the comment at line 55 says nothing calls it | The earn loop is open: relaying produces no reward |
| The recovery veto window is **not built** | `guardian.rs:12` still carries the note that the 24–48h delay is unbuilt | A stolen device has no block |
| ZK is still a stub | `zk_handler.rs` shells to `nargo`; `blockchain_bridge.rs:158` uses the signer's address "in place of a literal ZK proof" | No proof is verified anywhere |

Read together: **the single highest-value week of work in this repository is a redeploy plus one
call site**, not a new feature.

---

## 2. What changed outside the repo

### The category got validated — and contested — while we were building

- **bitchat** (Bluetooth mesh messaging, launched July 2025) has passed **3 million downloads**
  and was the **#1 app on both stores during Uganda's 101-hour internet shutdown in January
  2026**, with 400,000 downloads in days. Its published 2026 roadmap adds **Bitcoin Lightning
  payments**. ([thetechloft.com](https://www.thetechloft.com/2026/01/bitchat-app-review-jack-dorsey-bluetooth-mesh-chat.html),
  [stoic.ai](https://stoic.ai/blog/bluetooth-mesh-offline-messaging-and-crypto-adoption-why-bitchat-matters-more-than-memes/))
- **Internet shutdowns hit a record in 2025**: 313 shutdowns across 52 countries, an estimated
  **$19.7 billion** in economic loss, and not one day of the year without a shutdown somewhere.
  ([Access Now #KeepItOn 2025 report](https://www.accessnow.org/wp-content/uploads/2026/03/KeepItOn-Internet-Shutdowns-2025-Annual-Report.pdf))
- **Offline payment is becoming a requirement of money itself.** The ECB's digital euro pilot —
  36 selected providers, starting 2H 2027 — explicitly tests **offline phone-to-phone payment**;
  China's e-CNY already ships dual-offline payment; a BIS Innovation Hub survey found 49% of
  central banks call offline retail CBDC payment *vital* and another 49% *advantageous*.
  ([ECB](https://www.ecb.europa.eu/euro/digital_euro/pilot/html/ecb.faq-digital-euro-pilot.en.html),
  [Central Banking](https://www.centralbanking.com/fintech/cbdc/7954211/pboc-launches-offline-cbdc-payments),
  [BIS Project Polaris](https://www.bis.org/publ/othp64.htm))

**Reading:** the thesis is no longer speculative, which cuts both ways. Demand is proven and
spikes without warning, and the strongest adjacent player is walking toward payments from a
3-million-user messaging base. What bitchat cannot copy quickly is the part CabalMesh already
has: escrowed settlement, on-chain standing, and guardian recovery. What CabalMesh does not have
is anyone using it. That asymmetry should set the order of the work.

### Distribution is gated by paperwork with weeks of lead time

This is the part the old plan missed entirely.

| Gate | Fact | Source |
|---|---|---|
| Xcode floor | App Store Connect has rejected builds not made with **Xcode 26+ against a version-26 SDK since 28 April 2026**. This machine has 15.4. | [`docs/ios-release.md`](../ios-release.md) |
| Apple account type | Wallet apps must be shipped by a developer **enrolled as an organization**, not an individual | [Apple App Review Guidelines 3.1.5](https://developer.apple.com/app-store/review/guidelines/) |
| Apple + NFTs | Selling NFT-related services in-app requires in-app purchase; apps may let users browse collections but must not add external purchase calls to action (US rules eased in 2025 to allow links) | [Apple](https://developer.apple.com/app-store/review/guidelines/), [Crowdfund Insider](https://www.crowdfundinsider.com/2025/05/239239-apple-revises-app-store-guidelines-for-crypto-and-nfts-following-court-ruling/) |
| Google Play | Crypto wallet apps require **licensing in 15+ countries** (in force since 29 Oct 2025); apps enabling tokenized digital assets must file the **financial features declaration** | [Play policy coverage](https://myappmonitor.com/blog/google-play-cryptocurrency-exchanges-wallets-policy-update) |
| Export compliance | Determined **non-exempt, ECCN 5D992.c**, self-classifiable — but needs a compliance owner's sign-off before the first upload | [`docs/export-compliance.md`](../export-compliance.md) |

None of these are engineering. All of them take calendar time, and every one of them sits
between a finished build and a user. **They have to start on day one, in parallel, or they
become the schedule.**

### The ZK path is more concrete than the old plan assumed

Noir can generate a **Solidity verifier contract** for any EVM chain, using keccak256 for gas
efficiency. That turns Phase 4's vague "run `nargo verify`" into a deployable artifact on Fuji —
a real on-chain verification, which is what the pitch has been claiming all along.
([Noir docs](https://noir-lang.org/docs/reference/nargo_commands))

### Funding exists on a known calendar

Avalanche's **Team1 Builder Grants** opened 1 July 2026 with Mini grants up to $10,000 and
Accelerator grants up to $30,000; **infraBUIDL()** and **infraBUIDL(AI)** run milestone-based
retroactive funding on top. Field-pilot evidence is exactly what these applications score.
([Crypto Briefing](https://cryptobriefing.com/avalanche-team1-builder-grants-program/),
[Avalanche Builder Hub](https://build.avax.network/grants))

---

## 3. What the analysis changes

Three re-orderings follow directly from the findings above.

1. **Marketplace moves from Phase 3 to Phase 0.** It is built. The remaining work is a redeploy
   and one call site, and until that lands the live contract still lets anyone mint modules free.
2. **Store and compliance paperwork becomes its own track, starting immediately.** Xcode 26,
   organization enrollment, the export sign-off and the Play declarations have lead times measured
   in weeks; they cannot be discovered at the end.
3. **Hardware and marketing move from Phase 2 to last.** Pre-orders convert on field evidence,
   not renders — and the field evidence is now also what unlocks the grants. Nothing about
   ShadowBox gates any software phase.

One thing is deliberately *not* changed: the discipline of the old plan. Every phase below still
ends on a definition of done that can be demonstrated, and nothing is claimed that the code does
not do today.

---

## 4. The re-baselined plan

| # | Phase | Window | Track |
|---|---|---|---|
| 0 | Close the loop that is already built | Aug – Sep 2026 | Code |
| 1 | Clear the distribution gate | Aug – Oct 2026 (parallel, starts now) | Paperwork |
| 2 | Prove it where the network dies | Oct – Nov 2026 | Evidence |
| 3 | Make the proof real | Nov – Dec 2026 | Code |
| 4 | Mainnet and real value | Dec 2026 – Jan 2027 | Code |
| 5 | Hardware, negotiation, confidential compute | Jan – Feb 2027 | Growth / research |

### Phase 0 — Close the loop that is already built (Aug – Sep 2026)

- Redeploy `CabalMeshVoucher` carrying the access-control fix, and deploy `RelayRewards`;
  update `deployments/fuji.json` and the app's address config. **One-way: needs an explicit
  go/no-go**, low stakes today (testnet, no value locked).
- Call `record_gateway_relay` from the real gateway relay path, so relaying is attributed and the
  earn loop closes. Keep `RELAYED` honest until it is.
- Ship the 24–48h recovery delay with a veto notification — the last guardian gap.
- Ship editable intent chips on top of the already-live `parse_intent_chat`.

**Done when:** a second device earns a module from real gateway relaying and equips it with a
visible yield change, and a wiped device restores from 3-of-5 guardians with a live veto window.

### Phase 1 — Clear the distribution gate (Aug – Oct 2026, parallel from day one)

- Upgrade to Xcode 26 + SDK 26; check the macOS floor before committing an afternoon to it.
- Enroll the Apple developer account as an **organization**; wallets cannot ship from an
  individual account.
- Get a named owner to sign the ECCN 5D992.c export determination before the first upload.
- File Play's financial-features declaration and pick launch countries against the wallet
  licensing requirement.
- Decide the iOS `MARKET` surface: browse-only on iOS with web checkout, full market on Android
  and desktop, so App Review's in-app-purchase rule cannot hold the whole app hostage.

**Done when:** a signed build is accepted into TestFlight and Play closed testing, and the
offline path runs on real hardware on both.

### Phase 2 — Prove it where the network dies (Oct – Nov 2026)

- Three pilot settings: a shutdown-prone region through a civil-society partner, a market or
  festival, and a disaster-response drill.
- Instrument without breaking zero-identity: hop counts, relay density, settlement latency after
  reconnect — aggregates only, never per-user traces.
- Apply to Team1 Builder Grants and infraBUIDL() with the pilot data as the milestone evidence.

**Done when:** at least 25 settlements are completed fully offline by people outside the team,
across at least two sites, each confirming on reconnect.

### Phase 3 — Make the proof real (Nov – Dec 2026)

- Generate the Noir Solidity verifier, deploy it to Fuji, and verify a submitted bid on-chain —
  replacing the signer-address stub.
- Put `nargo` in CI. Decide the mobile proving path explicitly: a native proving library, or
  delegation to a gateway with on-chain verification.

**Done when:** a bid's proof is verified by the deployed verifier contract, and the mobile path
is documented with a chosen option rather than a gap.

### Phase 4 — Mainnet and real value (Dec 2026 – Jan 2027)

- Security review of `Escrow`, `CabalMeshVoucher`, `RelayRewards`.
- Mainnet C-Chain launch under a documented exposure cap; sender-paid relay fee model.

**Done when:** one real-value settlement completes on mainnet inside the stated cap.

### Phase 5 — Hardware, negotiation, confidential compute (Jan – Feb 2027)

- ShadowBox and the Nobody Box: renders, pre-order page, campaign — now backed by pilot footage.
- Bounded agent negotiation with price guardrails enforced in Rust, never by the model.
- FHE/MPC feasibility spike; desktop key store; the Windows/Linux BLE decision.

**Done when:** pre-orders are open with real field evidence on the page, and a feasibility
write-up exists for FHE/MPC.

---

## 5. Risks, re-scored

| Risk | Hits | Why it is scored this way | Mitigation |
|---|---|---|---|
| App Review rejects a wallet with an in-app NFT market | P1 | Apple requires IAP for NFT-related sales; our modules are bought with AVAX | Ship iOS browse-only with web checkout; keep the full market on Android and desktop |
| Xcode 26 upgrade blocks every upload | P1 | Hard floor since 28 Apr 2026; local Xcode is 15.4 | Upgrade in week one, before it is on the critical path |
| Export sign-off has no owner | P1 | Determination is written but unsigned; a wrong self-declaration is a compliance problem | Name the owner this month; treat as a two-week item |
| Play wallet licensing by country | P1 | In force since 29 Oct 2025 in 15+ countries | Choose launch countries deliberately rather than shipping globally |
| Voucher redeploy is one-way | P0 | It replaces the live Fuji address the app points at | Explicit go/no-go; testnet, no value locked |
| bitchat reaches payments first | P2 | 3M+ downloads and a published Lightning roadmap | Compete on settlement, escrow and recovery — not on chat; get field evidence early |
| No field partner identified yet | P2 | This is the largest genuine unknown in the plan | Start outreach during Phase 0, not Phase 2 |
| `nargo` still absent from CI | P3 | Unchanged since the last plan | Stand up the runner before Phase 3 opens |

---

## 6. Next 30 / 60 / 90 days

**30 days** — redeploy the voucher and `RelayRewards` behind a go/no-go; wire
`record_gateway_relay`; start the Xcode 26 upgrade and the Apple organization enrollment; name
the export-compliance owner.

**60 days** — ship the veto window and editable chips; land a signed build in TestFlight and Play
closed testing; open field-partner conversations; draft the Team1 grant application.

**90 days** — run the first two pilot sites; submit the grant with pilot data attached; scope the
Noir Solidity verifier work and stand up `nargo` in CI.

---

*Method: the repository was re-read rather than trusted, and every external claim carries its
source. When either side of that changes, this document is what gets revised — and the deck is
generated from it.*
