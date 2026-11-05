# CabalMesh contracts

NEAR smart contracts for CabalMesh, written in Rust with `near-sdk`.

| Crate | What it is | Deployed? |
|---|---|---|
| `rewards/` | Pays the gateway that submits a matched NEAR Intents pair, from fees the traders signed | You deploy it, steps below |
| `test-token/` | A minimal token for the sandbox tests | Never |

## How the rewards contract works

Each trader signs two intents: the swap, and a transfer of the fee to this contract. A
gateway calls `submit(payloads)`. The contract checks the pair, forwards it to the
verifier's `execute_intents`, and pays the gateway both fees from its verifier balance
once the swap has settled. If that payment fails, the amount goes to `pending` and the
gateway can retry with `claim()`.

The contract has no owner and no withdrawal function. It accepts only the exact payload
shape the app signs. Anything else is refused before it reaches the verifier, because the
verifier accepts extra fields on a transfer, and a refundable fee could otherwise drain
fees paid by other traders.

Evidence and gas figures are in [`docs/near-rewards-spike.md`](../docs/near-rewards-spike.md).

## Tools

```bash
rustup target add wasm32-unknown-unknown
cargo install cargo-near --version 0.22.0 --locked
cargo install near-cli-rs --locked
```

`rust-toolchain.toml` pins the compiler, so `rustup` installs it on first use.

## Build and test

```bash
cd contracts/rewards
cargo near build non-reproducible-wasm   # writes ../target/near/cabal_rewards/cabal_rewards.wasm
cd ..
cargo test -p cabal-rewards              # 16 unit tests, 7 sandbox tests
```

The sandbox tests run against the real verifier code. The first run fetches it from
testnet into `rewards/res/`, which is gitignored.

Do not run `cargo fmt --all` here. It follows the path dependency and reformats all of
`src-tauri`.

## Deploy to testnet

Use a testnet account only. Never use a key that holds real funds.

The commands below use two variables. Set them in your shell first:

```bash
export NEAR_ACCOUNT=you.testnet
export REWARDS=rewards.$NEAR_ACCOUNT
```

`NEAR_ACCOUNT` must be a named account with about 6 NEAR. An implicit (64 hex character)
account cannot create subaccounts.

### 1. Build

```bash
cd contracts/rewards
cargo near build non-reproducible-wasm
```

### 2. Import your key

```bash
near account import-account using-private-key
```

Run it with no arguments and answer the prompts: paste the key when asked, enter your
account, choose `testnet`, then choose to save to the keychain. A key passed as an
argument ends up in your shell history.

### 3. Create the contract account

```bash
near account create-account fund-myself $REWARDS '5 NEAR' \
  autogenerate-new-keypair save-to-keychain \
  sign-as $NEAR_ACCOUNT \
  network-config testnet sign-with-keychain send
```

The wasm is about 176 KB, which locks about 1.8 NEAR for storage. 5 NEAR leaves room for
state and gas.

### 4. Deploy and initialise

```bash
near contract deploy $REWARDS \
  use-file ../target/near/cabal_rewards/cabal_rewards.wasm \
  with-init-call new \
  json-args '{"verifier":"verifier.tidepool-sandbox.testnet","fee_token":"nep141:usdc.tidepool-sandbox.testnet","fee_per_side":"10000"}' \
  prepaid-gas '100 Tgas' attached-deposit '0 NEAR' \
  network-config testnet sign-with-keychain send
```

`10000` is 0.01 USDC per side, and must match `fee_per_side` in
`src-tauri/src/network_config.rs`.

### 5. Check it

```bash
near contract call-function as-read-only $REWARDS config \
  json-args '{}' network-config testnet now
```

This should print the verifier, fee token and fee from step 4.

### 6. Turn it on in the app

In the `TESTNET` block of `src-tauri/src/network_config.rs`, set your own account id:

```rust
rewards: Some("rewards.you.testnet"),
```

Until this is set, the app signs no fee and submits directly to the verifier.

### 7. Prove it end to end

This needs the spike keys from `scripts/near-intents-spike/probe.mjs setup`, where A holds
wNEAR, B holds USDC, and G holds NEAR for gas.

```bash
cd src-tauri
cargo test --workspace
CABALMESH_REWARDS_ID=$REWARDS cargo test --test near_live \
  the_deployed_rewards_contract_pays_the_gateway -- --ignored --nocapture
```

It settles a real pair through the contract and checks that the gateway received both
fees. It prints the transaction hash, which opens at
`https://testnet.nearblocks.io/txns/<hash>`.

To run the same thing through the app, follow
[`docs/near-two-device-checklist.md`](../docs/near-two-device-checklist.md) with a third
node as the gateway.

## Redeploying

Change the code, rebuild, and deploy `without-init-call`. State survives a redeploy, so a
change to the contract's state struct needs a migration, or a new account.

```bash
near contract deploy $REWARDS \
  use-file ../target/near/cabal_rewards/cabal_rewards.wasm \
  without-init-call \
  network-config testnet sign-with-keychain send
```

## Before mainnet

Mainnet keeps `rewards: None` and a zero fee until this is done:

- An independent audit of the contract.
- A reproducible build: `cargo near build reproducible-wasm`, which needs Docker.
- Remove the full-access key from the contract account. Whoever holds it can replace the
  code and take the fees. Without it the code can never change, so decide on upgrades first.
