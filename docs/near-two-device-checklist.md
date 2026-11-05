# NEAR two-device checklist

How to prove a real trade between two CabalMesh nodes on NEAR testnet, and what has
been proven so far.

## Status

| Check | Result | Evidence |
|---|---|---|
| Settlement code, two ledgers, third-party gateway | Passed 2026-11-05 | `4ASJ1wakJi4B1VNrb8Uw4d1Eu8tqd1XAeHjZqdsodpwW` |
| Withdraw, then deposit, through the Rust encoder | Passed 2026-11-05 | `3FMM1PqszUcS3BdQ199RJj5Y1adtQqqmKiiHBgUt2AAg`, `DmeFxeWaU8UrUwrEF2WBwc34N119cffKqATPndMLfW4b` |
| Two app processes over real libp2p, commands driven by a throwaway patch | Passed 2026-11-05 | `5kWVVgaZV8THw2Dr718pt75fT5x2bUEabxuNfDr1xwee` |
| Two nodes, tapped through the UI | **Not run** | Nothing here can drive the window |
| Two physical devices, or a settlement over BLE | **Not run** | |

In the process run, A sold 0.1 wNEAR and B bought it, on one Mac with separate data
directories. The agents agreed $4.05 over the mesh, each side signed, and A submitted. B
held no NEAR, logged `NO NEAR FOR GAS`, then took A's report and confirmed its own intent
hash in the transaction logs before marking the deal settled. A's balance moved by
exactly −0.1 wNEAR and +0.405 USDC, and B's by the reverse.

## Run it through the UI

Two nodes on one machine, each with its own data directory and key:

```bash
scripts/dev-2-nodes.sh --near
```

Two real devices work the same way: they only need to reach each other over Wi-Fi or
Bluetooth. At least one node needs internet and some NEAR for gas when the pair is
submitted.

Every step below happens on testnet. Never use a key that holds real funds.

### 1. Fund both accounts

On each node, open **Vault** and copy the account from the NEAR panel. Then:

```bash
curl -X POST https://www.usetidepool.tech/v0/faucet \
  -H 'content-type: application/json' \
  -d '{"accountId":"<account>","mode":"intents","assets":["USDC","wNEAR"]}'
```

The faucet gives each address one drip per hour, which credits 40 wNEAR and 100 USDC
inside the verifier.

- [ ] After a refresh, both nodes show wNEAR and USDC rows marked `IN INTENTS. TRADABLE.`

For gas, send a little NEAR to one account, or fund a third account that runs as the
gateway. A plain transfer creates an implicit account.

- [ ] The node that will submit shows a `GAS` row for NEAR.

### 2. Try deposit and withdraw

On a node with gas:

- [ ] Withdraw 1 USDC. The `IN INTENTS` row drops by 1 and the `IN WALLET` row rises by 1.
- [ ] Deposit 1 USDC. Both return to where they started.

### 3. Trade

| Node | Compose |
|---|---|
| A | `SELL 1 wNEAR`, price above `3.90` |
| B | `BUY 1 wNEAR`, price under `4.20` |

Amounts must be equal. There are no partial fills.

- [ ] Within seconds, both Detail screens show the same `AGREED PRICE`.
- [ ] Press `SIGN YOUR SIDE` on A. It logs `SIGNED. WAITING FOR THE OTHER SIDE.`
- [ ] Press `SIGN YOUR SIDE` on B.
- [ ] Both reach Settled and show the same proof hash.
- [ ] A's `IN INTENTS` rows show wNEAR down 1 and USDC up by the agreed price. B shows the reverse.
- [ ] The hash opens at `https://testnet.nearblocks.io/txns/<hash>`.

### 4. Trade with one node offline

- [ ] Turn on `OFFLINE MODE` on B's Profile.
- [ ] Repeat step 3 until A shows `Waiting`. Nothing reaches the chain.
- [ ] Sign on B while it is offline, if a BLE or local link is still up. Otherwise bring B back first.
- [ ] Bring B back online. The pair goes out once, within about 30 seconds.
- [ ] If B missed the report, it still settles within about 30 seconds of coming back, with its intent hash as the proof.
- [ ] `ANOTHER GATEWAY SUBMITTED FIRST.` means a second gateway lost the race. No funds move twice.

### 5. What failure should look like

- [ ] A node with no NEAR holds the pair and logs `NO NEAR FOR GAS. HELD FOR A GATEWAY.`
- [ ] Cancel is refused while a signature is still live.
- [ ] A rejected pair logs `FUNDS REMAIN IN INTENTS BALANCE`.

## Automated check

Most of this runs headless against the live verifier, with the keys from
`scripts/near-intents-spike/probe.mjs setup`:

```bash
cd src-tauri
cargo test --test near_live -- --ignored --test-threads=1 --nocapture
```

It covers a third-party gateway, a device with no gas, report verification, a missed
report, a false report, replay, a mismatched pair, a stale salt, and a withdraw and deposit round trip.
It does not cover the mesh or the UI.

The faucet drips into the verifier, so if A runs out of wNEAR or B out of USDC the test
says which one and stops before it spends anything.
