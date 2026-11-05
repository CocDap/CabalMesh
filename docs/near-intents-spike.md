# NEAR Intents spike: peer-to-peer settlement on Tidepool

Run on 2026-11-05 against `verifier.tidepool-sandbox.testnet` (`defuse` v0.4.5) with
`scripts/near-intents-spike/probe.mjs`.

## Verdict

**Go.** Two implicit accounts signed `token_diff` for each other and a third implicit
account, holding neither token, settled the pair on-chain. Deposit and withdrawal also
work. There is no need to deploy our own verifier.

| Claim | Result | Evidence |
|---|---|---|
| Third party can submit | Yes | `G1s5UDrB6f2phA7RdNvYnp8ehDninikrwFYqsESDVef5` |
| Both legs move atomically | Yes | `mt_balance_of` before and after; `mt_transfer` log below |
| Resubmitting the same pair fails | Yes, `nonce was already used` | `96xB8KbTmTixppcCXHPFbgoGn541EQLkp4A91wBYhNoG` |
| Deposit with `ft_transfer_call` | Yes | `HsYfWHmEivKibwFGHxvMZJnhtUtuEPKzakN4SqxutmSW` |
| Withdraw with `ft_withdraw` intent | Yes | `EbL3rP4rV1k6RqiPbsYaATXm92JxpPqaf1z2njJzjUrJ` |
| Verifier fee | 0 | `simulate_intents(...).state.fee` |

Look up any hash at `https://testnet.nearblocks.io/txns/<hash>`.

## Accounts

| Role | Account |
|---|---|
| A, seller | `b6336d522196d849252c5dbef77215710abf748ac53afc63ece79378d188b1ff` |
| B, buyer | `b66ef074474ba8b60f630afe03dafce202086ae2e568e653f64b9dd367d8c0f3` |
| G, gateway | `339c6d2483fb8cf7ed7864b224ac4606aa426d0f0415614914880af313decc93` |
| F, funder of G and A | `cabalmesh-spike-04d2eb79.testnet` |

B never held native NEAR. It traded with only a signature and a verifier balance.

Funding, all automatable with no captcha:

- `POST https://helper.testnet.near.org/account` with `{"newAccountId","newAccountPublicKey"}` creates a named `.testnet` account holding 10 NEAR. F then funded G (`2FT5Uw73xo4mHABsduqMAUSmghcqA7qS4vT2YvRWukCF`) and A (`7txReDvZWNf1LhRbEJJLqXgeqGLcZtjEHGto9vMSgepY`). A plain transfer creates an implicit account.
- `POST https://www.usetidepool.tech/v0/faucet` with `{"accountId","mode":"intents","assets":["USDC","wNEAR"]}` credits 100 USDC and 40 wNEAR inside the verifier (A `7JC5yv6vEPP4xC8atKqTDTpPuJ47SBRoBHyB7XWXLsbG`, B `A7prBUYuT4gtRhn1nhnLqSzesocUdwTsGxYHHc4eDEfL`). `"mode":"wallet"` mints into the NEP-141 wallet instead, and worked for A's implicit account.

## Gas

Gas burnt is the sum over the transaction and all its receipts.

| Call | Attached | Burnt |
|---|---|---|
| `execute_intents` (2 `token_diff`) | 300 Tgas | 14.80 Tgas |
| `ft_transfer_call` to verifier | 100 Tgas, 1 yocto | 30.58 Tgas |
| `execute_intents` (1 `ft_withdraw`) | 300 Tgas | 32.88 Tgas |

The attached values match the constants in the P1 plan and stay as they are. Unused gas
is refunded.

## JSON shapes

Intent message, the exact string that gets signed. Key order is `signer_id`, `deadline`,
`intents`:

```json
{"signer_id":"<hex>","deadline":"2030-01-01T00:00:00.000Z","intents":[{"intent":"token_diff","diff":{"nep141:usdc.tidepool-sandbox.testnet":"4040000","nep141:wnear.tidepool-sandbox.testnet":"-1000000000000000000000000"}}]}
```

Withdrawal intent:

```json
{"intent":"ft_withdraw","token":"usdc.tidepool-sandbox.testnet","receiver_id":"<hex>","amount":"1000000"}
```

`MultiPayload`, passed as `execute_intents({"signed":[...]})`:

```json
{
  "standard": "nep413",
  "payload": {
    "message": "<the string above>",
    "nonce": "<base64 of 32 bytes>",
    "recipient": "verifier.tidepool-sandbox.testnet"
  },
  "public_key": "ed25519:<base58>",
  "signature": "ed25519:<base58>"
}
```

- Hash: `sha256(u32le(2147484061) ‖ borsh_string(message) ‖ nonce[32] ‖ borsh_string(recipient) ‖ 0x00)`. The last byte is `callback_url: None`. The signature covers this hash.
- Nonce: `56 28 f6 c6 ‖ 00 ‖ salt[4] ‖ deadline_ns: i64 LE ‖ random[15]`. Salt comes from view `current_salt`, a hex string, `fc4d10f1` during this run.

`simulate_intents({"signed":[...]})`, a free view, returned:

```json
{
  "intents_executed": [{"intent_hash": "81vXJjgCx8b3pPvBYXQBY9zFDTKJ5Mz2U2vYHTyqgnRb", "account_id": "<hex>", "nonce": "<base64>"}],
  "logs": ["EVENT_JSON:..."],
  "min_deadline": "2026-11-05T05:08:07.093Z",
  "state": {"fee": 0, "current_salt": "fc4d10f1"}
}
```

A clean result has no `invariant_violated`, or has it as `null`. Unmatched diffs return
`{"error":"unmatched_deltas","unmatched_deltas":{...}}` without panicking.

## Event logs

From the settlement transaction, shortened. P2's standing reader consumes these:

```
EVENT_JSON:{"standard":"dip4","version":"0.4.3","event":"token_diff","data":[{"intent_hash":"81vX...","account_id":"b633...","diff":{"nep141:usdc.tidepool-sandbox.testnet":"4040000","nep141:wnear.tidepool-sandbox.testnet":"-1000000000000000000000000"}}]}
EVENT_JSON:{"standard":"dip4","version":"0.4.3","event":"token_diff","data":[{"intent_hash":"Hq3V...","account_id":"b66e...","diff":{"nep141:usdc.tidepool-sandbox.testnet":"-4040000","nep141:wnear.tidepool-sandbox.testnet":"1000000000000000000000000"}}]}
EVENT_JSON:{"standard":"dip4","version":"0.4.3","event":"intents_executed","data":[{"intent_hash":"81vX...","account_id":"b633...","nonce":"..."},{"intent_hash":"Hq3V...","account_id":"b66e...","nonce":"..."}]}
EVENT_JSON:{"standard":"nep245","version":"1.0.0","event":"mt_transfer","data":[{"old_owner_id":"b633...","new_owner_id":"b66e...","token_ids":["nep141:wnear.tidepool-sandbox.testnet"],"amounts":["1000000000000000000000000"]},{"old_owner_id":"b66e...","new_owner_id":"b633...","token_ids":["nep141:usdc.tidepool-sandbox.testnet"],"amounts":["4040000"]}]}
```

A deposit logs `nep141 ft_transfer` then `nep245 mt_mint` with memo `deposit`. A
withdrawal logs `dip4 ft_withdraw`, `dip4 intents_executed`, `nep245 mt_burn` with memo
`withdraw`, then `nep141 ft_transfer`.

## Golden vectors

In `src-tauri/crates/cabal-near/tests/fixtures/`, for Tasks 3 and 4:

| File | Fixed inputs | Expected output |
|---|---|---|
| `keypair.json` | `secret_hex` | `account_id`, `public_key` |
| `nep413_token_diff.json` | key, verifier, `diff`, `salt_hex`, `nonce_tail_hex`, `deadline_ms` | `nonce_hex`, `expected_hash_hex`, `expected_multipayload` |
| `signed_tx_ft_transfer_call.json` | key, receiver, `nonce`, `block_hash`, method, `args_json`, `gas`, `deposit` | `expected_tx_hash`, `expected_signed_tx_base64` |

The fixture key is public and test-only: bytes `01..20`. Never fund it.

`diff` keys are in sorted order, matching `TokenDiff`'s `BTreeMap`.

How far each vector is proven:

- `nep413_token_diff.json`: the live verifier accepted the signature and rejected a tampered copy. `node scripts/near-intents-spike/probe.mjs fixtures --check` repeats this.
- `signed_tx_ft_transfer_call.json`: its block hash is made up, so this exact transaction cannot be sent. The same encoder produced every real transaction listed above, and the chain accepted all of them.

The live check depends on salt `fc4d10f1` staying valid. If the verifier invalidates it,
the check reports `invalid salt` and the fixture needs regenerating; the Rust byte-for-byte
tests are unaffected.

## What this spike does not show

- Mainnet. The fee on `intents.near` was not checked, and its USDC id is unverified.
- The two-device flow over the mesh. That is Task 10.
- What happens after a salt rotation. Only the bad-salt rejection was observed, in the earlier probe.
- `ft_withdraw` to an account not registered on the token. A was registered by the wallet drip.

## Reproduce

```bash
cd scripts/near-intents-spike
rm -f .keys.json .run.json
node probe.mjs setup      # new keys, funds accounts; one faucet drip per address per hour
node probe.mjs swap
node probe.mjs wallet
node probe.mjs fixtures --check
```
