# Chat intents, marketplace, and node modules — design

> **2026-11-05:** written when the app settled on Avalanche. NEAR is now the only chain, and `contracts/`, alloy and `blockchain_bridge.rs` are gone (`identity.rs` and `near_bridge.rs` replaced it). The chat intent part still applies. Marketplace and modules wait for P2 on NEAR. Kept as a historical record; see `docs/product-status.md` for what is true today.

**Status: partially built.** Written 2026-08-10; updated 2026-08-27 once
Marketplace/Modules UI shipped. `intent-inference-runtime.md`'s always-available
local model was evaluated as an alternative to the path below and not adopted —
`parse_intent_chat` (the configured LLM, same as everywhere else in the app)
stayed the one entry point, so intent parsing has a single code path rather
than two.

**Chat intent is built** — `parse_intent_chat` fills the same form fields
`New.tsx` already validates through, unchanged; the model proposes, Rust
still decides.

**Marketplace and modules: deployed, and the UI is real.**
`CabalMeshVoucher`'s mint access-control gap this doc originally opened with
is fixed on-chain — minting is now restricted to the `RelayRewards` contract
address, and a direct call reverts. `contracts/contracts/CabalMeshVoucher.sol`
and `contracts/contracts/RelayRewards.sol` implement decisions 0–4 below, with
passing Hardhat tests (`contracts/test/{CabalMeshVoucher,Marketplace,
RelayRewards}.test.ts`), and the Rust bridge's ABI bindings match them
(`src-tauri/abi/{CabalMeshVoucher,RelayRewards}.abi.json`). `VAULT → MODULES`
and the MARKET tab are real screens against real contracts — listing, buying,
releasing, and refunding a deal all reach the chain
(`market_listings`/`market_buy`/`market_list_module`/`market_release_deal`/
`market_refund_deal`/`market_my_deals` in `commands.rs`). **Not yet wired:**
`BlockchainBridge::record_gateway_relay` exists and is untested against a real
route, but no command calls it — a real gateway relay does not yet earn a
module the way this doc describes; that needs the product decisions about fee
amount and gateway selection this doc doesn't make.

Three connected changes: composing intents by conversation instead of by form,
a marketplace for NFTs, and NFTs that measurably improve what a node earns.

## Naming, deliberately

The frozen desktop RPG UI was deleted in 0a71f59 for being off-brand. This
design does **not** reintroduce it under another name. There is no inventory, no
loot, no potions.

The metaphor is **hardware modules slotted into a node** — which is both on-brand
for an instrument/terminal aesthetic and a literal fit for a mesh device.

## The loop

```
   relay traffic for peers  ──>  earn MB  ──>  earn AVAX
            ▲                                     │
            │                                     ▼
     modules raise the rate  <──  buy / sell on MARKET
            ▲                                     │
            └──────── mint at milestones ─────────┘
```

Modules are NFTs on `CabalMeshVoucher` (ERC721, already deployed to Fuji).
Three slots:

| Slot | Example modules | Effect |
|---|---|---|
| RADIO | Relay Amplifier, Range Extender | +% relay yield, more peers counted |
| CRYPTO | Ghost Cloak, Proof Accelerator | +privacy hops, cheaper/faster settlement |
| POWER | Gateway License | earn while acting as an internet gateway |
| — | Standing Badge | **soulbound**, earned by settling, not tradable |

## Chat intent

```
┌──────────────────────────────────────────┐
│  ‹ BACK          NEW INTENT              │
├──────────────────────────────────────────┤
│                                          │
│                   ┌────────────────────┐ │
│                   │ buy 10 avax under  │ │
│                   │ 95, shark mode     │ │
│                   └────────────────────┘ │
│                                          │
│  ┌─────────────────────────────────────┐ │
│  │ ◇ PARSED · LOCAL MODEL              │ │
│  │                                     │ │
│  │  [BUY]  [10 AVAX]  [UNDER $95]      │ │
│  │  [SHARK MODE]  [PRIVACY HIGH]       │ │
│  │                                     │ │
│  │  TAP A CHIP TO CHANGE IT.           │ │
│  └─────────────────────────────────────┘ │
│                                          │
│  ┌─────────────────────────────────────┐ │
│  │ ⚠ BALANCE 8.2 AVAX — SHORT BY 1.8   │ │
│  └─────────────────────────────────────┘ │
│                                          │
├──────────────────────────────────────────┤
│  ┌────────────────────────────────────┐  │
│  │           REVIEW INTENT            │  │
│  └────────────────────────────────────┘  │
│  ┌───────────────────────────────┐ ┌───┐ │
│  │ say what you want to do…      │ │ ▶ │ │
│  └───────────────────────────────┘ └───┘ │
└──────────────────────────────────────────┘
```

Editable chips are what make a wrong parse cheap to fix without falling back to
a full form:

```
        tap [10 AVAX]
              ▼
┌─ AMOUNT ─────────────────────────────────┐
│  ┌───────────────────────┐               │
│  │  10.0          AVAX   │       [ MAX ] │
│  └───────────────────────┘               │
│  AVAILABLE 8.2 AVAX                      │
│                                          │
│  ┌──────────┐  ┌──────────────────────┐  │
│  │  CANCEL  │  │         SET          │  │
│  └──────────┘  └──────────────────────┘  │
└──────────────────────────────────────────┘
```

**The safety property from the current form is preserved exactly.** The model
only produces `IntentFields`. `parse_draft` still validates, `preview_intent`
still builds the review rows from the draft, and broadcasting still re-parses
the same fields. The AI proposes; Rust decides; the user confirms. The model
never broadcasts.

**Resolved 2026-08-12:** `propose_intent` runs the embedded model in a bounded
worker and has no app-state argument, so it cannot reach the ledger, mesh,
vault, signer, queue, or chain. Complete proposals pass through the same
`parse_draft` function used by review and broadcast before six read-only chips
are returned. Partial or ambiguous input names missing fields without defaults;
unavailable, timed-out, refused, or domain-invalid inference leaves the phrase
editable and offers the manual fields. No intent text or model output enters a
log or IPC response.

**Resolved 2026-08-12:** all six parsed chips open focus-trapped editors whose
options and precision come from Rust. Amount editing reads the latest real
snapshot through `intent_affordability`, uses fixed-point token arithmetic for
MAX and shortfall, and keeps an unknown balance distinct from a known zero.
Review rows still come only from `preview_intent`; the frontend freezes the
exact `IntentFields` accepted for preview and confirmation sends that snapshot
back through `broadcast_intent`, which re-parses it. Escape, mobile back, focus
return, radiogroup arrow keys, wrapped controls, and 200-percent type scaling
remain usable. The model still has no state-bearing or execution command.

## Marketplace

```
┌──────────────────────────────────────────┐
│  MARKET                          ⌕   ⇅   │
├──────────────────────────────────────────┤
│  [ ALL ] [ RADIO ] [ CRYPTO ] [ POWER ]  │
│                                          │
│  ┌─────────────────────────────────────┐ │
│  │ ▚▚▚  RELAY AMPLIFIER MK-II     RARE │ │
│  │ ▚▚▚  RADIO · +18% RELAY YIELD       │ │
│  │      SELLER  NODE-7F3A…C2   ★ 42    │ │
│  │      2.40 AVAX            [  BUY  ] │ │
│  └─────────────────────────────────────┘ │
│  ┌─────────────────────────────────────┐ │
│  │ ▞▞▞  GHOST CLOAK             COMMON │ │
│  │ ▞▞▞  CRYPTO · +2 HOPS, NO LATENCY   │ │
│  │      SELLER  NODE-91BE…08   ★ 31    │ │
│  │      0.85 AVAX            [  BUY  ] │ │
│  └─────────────────────────────────────┘ │
│  ┌─────────────────────────────────────┐ │
│  │ ▙▙▙  GATEWAY LICENSE         LEGEND │ │
│  │ ▙▙▙  POWER · EARN AS GATEWAY        │ │
│  │      SELLER  NODE-2C4D…AA   ★ 18    │ │
│  │      11.00 AVAX           [  BUY  ] │ │
│  └─────────────────────────────────────┘ │
│                                          │
│  ⓘ ESCROW-BACKED — AVAX LOCKS UNTIL THE  │
│    MODULE ACTUALLY TRANSFERS.            │
└──────────────────────────────────────────┘
```

`★ 42` is the seller's **real** standing — settlement count from
`src/standing.rs`, not an invented score.

`Marketplace.sol` already implements the escrow this promises: `buy()` locks
AVAX and pulls the NFT into escrow, `releaseDeal` / `refundDeal` settle it.

## Modules — `VAULT → MODULES`

```
┌──────────────────────────────────────────┐
│  VAULT                                   │
│  [ ASSETS ]  [ MODULES ]  [ KEYS ]       │
├──────────────────────────────────────────┤
│  ┌─ NODE LOADOUT ──────────────────────┐ │
│  │                                     │ │
│  │  RADIO    ▚▚▚ RELAY AMP MK-II    ⏻ │ │
│  │  CRYPTO   ▞▞▞ GHOST CLOAK        ⏻ │ │
│  │  POWER    ┈┈┈ EMPTY                 │ │
│  │                                     │ │
│  │  ───────────────────────────────    │ │
│  │  RELAY YIELD     ×1.00  →  ×1.18    │ │
│  │  PRIVACY HOPS        3  →       5   │ │
│  └─────────────────────────────────────┘ │
│                                          │
│  OWNED · 5                    [ MARKET ] │
│  ┌───────────────┐  ┌───────────────┐    │
│  │  ▚▚▚          │  │  ▞▞▞          │    │
│  │  RELAY AMP    │  │  GHOST CLOAK  │    │
│  │  RARE         │  │  COMMON       │    │
│  │  ● EQUIPPED   │  │  ● EQUIPPED   │    │
│  └───────────────┘  └───────────────┘    │
│  ┌───────────────┐  ┌───────────────┐    │
│  │  ▛▛▛          │  │  ▟▟▟          │    │
│  │  PROOF ACC.   │  │  STANDING     │    │
│  │  UNCOMMON     │  │  SOULBOUND    │    │
│  │  [ EQUIP ]    │  │  EARNED       │    │
│  └───────────────┘  └───────────────┘    │
└──────────────────────────────────────────┘
```

### Module detail

```
┌─ RELAY AMPLIFIER MK-II ──────────────────┐
│                                          │
│          ▚▚▚▚▚▚▚▚▚▚▚▚                    │
│          ▚▚▚▚▚▚▚▚▚▚▚▚        RARE        │
│          ▚▚▚▚▚▚▚▚▚▚▚▚                    │
│                                          │
│  SLOT          RADIO                     │
│  EFFECT        +18% RELAY YIELD          │
│  TOKEN ID      #1204                     │
│  CONTRACT      0x…CMV                    │
│  MINTED        2026-07-14                │
│                                          │
│  ─── WHILE EQUIPPED ───────────────────  │
│  RELAYED TODAY       412 MB              │
│  BASE EARNED         0.0081 AVAX         │
│  BONUS FROM THIS     0.0015 AVAX         │
│                                          │
│  ┌────────────┐  ┌────────────────────┐  │
│  │  UNEQUIP   │  │  LIST ON MARKET    │  │
│  └────────────┘  └────────────────────┘  │
└──────────────────────────────────────────┘
```

Showing the bonus this specific module actually produced is what makes owning it
feel real rather than decorative.

### Where earnings are visible — HOME

```
┌─ RELAY YIELD ────────────────────────────┐
│                                          │
│  TODAY           0.0096 AVAX             │
│  ▁▂▄▆█▆▄▂▁▂▄▆█                           │
│                                          │
│  RELAYED         412 MB                  │
│  BASE RATE       ×1.00                   │
│  MODULES         ×1.18    ▚▚▚ ▞▞▞        │
│  ────────────────────────────────────    │
│  EFFECTIVE       ×1.18        [ VAULT ]  │
│                                          │
└──────────────────────────────────────────┘
```

## Navigation

```
BEFORE:  HOME   INTENTS   NODES   VAULT   PROFILE

AFTER:   HOME   INTENTS   MARKET  VAULT   PROFILE
                          ▲              ▲
                          new     + MODULES tab

NODES folds into HOME, which already shows mesh status, node id and uptime.
```

Five tabs is already the limit at 390 px with the brand's tracking; a sixth
would wrap.

## Five things, now settled

The doc originally listed four open questions. Answering them honestly
surfaced a fifth, more urgent one — decision 0 — that the other four
actually depend on. All five are grounded in the contracts and Rust as they
exist today (`contracts/contracts/*.sol`, `src-tauri/src/blockchain_bridge.rs`),
not as the earlier draft assumed.

**0. `CabalMeshVoucher.mintVoucher` has no access control — this blocks
everything else.** `contracts/contracts/CabalMeshVoucher.sol:25` declares it
`external` with no modifier: anyone, from any wallet, can call it directly
against the deployed Fuji contract and mint themselves a token with any
`voucherType` and `description` string they like, for free, right now — no
app involved. This is worse than the local-file risk decision 1 below
worries about: a farming defence built entirely at the app/relay layer
still means nothing while the mint entry point itself has no gate. Nothing
in decisions 1, 3 or 4 is meaningful until this is fixed.

**Decision: redeploy the voucher contract with minting restricted to a
single on-chain caller — the reward contract from decision 3, not an
off-chain admin key.** Restricting it to a *contract address* rather than a
person keeps the reward path trustless: a module is minted only as the
atomic side effect of a settlement this contract already verified on-chain,
never by a party's own say-so, off-chain or on. `CabalMeshVoucher` is a
plain `ERC721` with no `Ownable`, no proxy — not upgradeable — so this is a
new deployment, not a migration. Low-stakes to do now: Fuji testnet, no
value locked in the current contract yet.

**1. The old relay boost was a local JSON file, and it is still live dead
code.** `apply_relay_boost` / `get_relay_boost_multiplier` /
`relay_boost_path` were not deleted with the old RPG UI (`0a71f59` removed
only the frontend caller in `src/App.tsx`) — they are still sitting in
`blockchain_bridge.rs:841-852`, unreachable from any command today, ready
to be wired back up by exactly the mistake this doc warns against.

**Decision: delete them outright**, and compute the relay-yield multiplier
on demand from verified on-chain module ownership every time it's shown —
never cache or store it as an editable local value, in a file or anywhere
else.

**2. `CabalMeshVoucher` has no slot, rarity, or effect on-chain — and no
metadata hook to add them to.** The contract has no `tokenURI` override, no
`_baseURI`, no `ERC721URIStorage` — `tokenURI()` returns an empty string for
every token today. The doc's original "recommended route," off-chain
`tokenURI` metadata, is not achievable without a contract change either, so
it is not actually cheaper than the alternative.

**Decision: structured on-chain fields instead of a metadata URI** —
`VoucherData` gains `uint8 slot`, `uint8 rarity`, `uint16 effectBps` (the
module's effect as basis points, e.g. `1800` = +18%) alongside the existing
`voucherType`/`description`. Every other contract this app reads
(`IEscrow`, `IMarketplace`) is already read as typed on-chain calls through
`sol!` bindings, not as fetched JSON from a URI a host somewhere has to keep
serving — matching that pattern keeps one read path instead of two, and
keeps a module's effect something Rust can verify directly rather than
trust a metadata host to report honestly. `tokenURI` support for external
wallet/marketplace display is worth adding later; it is not the source of
truth either way.

**3. Who pays the relay reward, and for which kind of relay specifically?**
*(the question the rest depended on.)* The app relays two different ways,
and only one of them is currently attributable to a specific node at all.
BLE mesh relaying is flood-based — a packet is copied by whichever
neighbours a thinned fanout selects (`cabal-ble/src/router.rs`), with no
routing table and no single node identifiable as "the" relay for a given
hop. **Gateway relaying is different**: a gateway submits an offline node's
signed transaction to the chain itself, so the submitting address is
already on-chain and already attributable, no new protocol needed.

**Decision: relay rewards apply to gateway relaying only, for now** — a
sender-paid fee, not treasury emission (self-sustaining, no inflation to
fund), paid atomically out of the same settlement the relayed transaction
produces. `RELAYED TODAY` / `relay_bytes` on HOME stays honest in the
meantime: it is wired to `0` today (`mesh.rs`'s counter is initialized and
read but never incremented — the `0.0096 AVAX` mock-up was decoration
against a number that was already always zero), and should keep reading as
"not tracked yet" rather than fabricate BLE-relay activity a reward can't
actually attribute. Rewarding BLE flood relay is future work that needs a
protocol change first (an attributable per-hop receipt), not a reward-layer
decision.

**4. Farming defence.** With reward scope narrowed to gateway relay
(decision 3), the sybil case narrows with it: two devices owned by one
person forwarding junk to each other over BLE earns nothing either way,
because BLE relay isn't rewarded. What remains is a gateway paying itself
by fronting its own transactions — defended against by the mechanism
already required for payment to move at all: **the fee is deducted from a
real settlement** (`Escrow`/`Marketplace` releasing to a counterparty), so
"farming" it costs the same real AVAX moving between two real addresses
that self-dealing already costs anywhere else in this app, gateway or not.
No new signature/receipt scheme (`IntentAck`, reserved in the wire format
but never implemented — `cabal-ble/src/wire.rs:143`) is needed for v1. A
minimum settlement size or a same-address check between gateway and
counterparty is worth adding if v1 shows it is actually exploited; it is
not a blocker to start.
