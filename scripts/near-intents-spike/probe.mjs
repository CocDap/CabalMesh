#!/usr/bin/env node
// Spike: prove peer-to-peer NEAR Intents on the Tidepool testnet verifier and
// capture golden vectors for the Rust `cabal-near` crate. No dependencies.
//
//   node probe.mjs setup              create keys, fund accounts, drip tokens
//   node probe.mjs swap               A and B sign, G submits execute_intents
//   node probe.mjs wallet             A deposits and withdraws USDC itself
//   node probe.mjs fixtures           write golden vectors
//   node probe.mjs fixtures --check   fail if vectors differ, and verify live
//
// Testnet keys go to .keys.json and results to .run.json, both gitignored.

import crypto from 'node:crypto';
import fs from 'node:fs';
import path from 'node:path';
import { fileURLToPath } from 'node:url';

const HERE = path.dirname(fileURLToPath(import.meta.url));
const KEYS = path.join(HERE, '.keys.json');
const RUN = path.join(HERE, '.run.json');
const FIXTURES = path.join(HERE, '../../src-tauri/crates/cabal-near/tests/fixtures');

const RPC = 'https://rpc.testnet.fastnear.com';
const HELPER = 'https://helper.testnet.near.org/account';
const FAUCET = 'https://www.usetidepool.tech/v0/faucet';
const VERIFIER = 'verifier.tidepool-sandbox.testnet';
const USDC_CONTRACT = 'usdc.tidepool-sandbox.testnet';
const WNEAR_CONTRACT = 'wnear.tidepool-sandbox.testnet';
const USDC = `nep141:${USDC_CONTRACT}`;
const WNEAR = `nep141:${WNEAR_CONTRACT}`;

const NEAR = 10n ** 24n;
const GAS_FT_TRANSFER_CALL = 100_000_000_000_000n;
const GAS_EXECUTE_INTENTS = 300_000_000_000_000n;
const NEP413_TAG = 2147484061;
const NONCE_MAGIC = '5628f6c6';

// ---------------------------------------------------------------- encoding

const B58 = '123456789ABCDEFGHJKLMNPQRSTUVWXYZabcdefghijkmnopqrstuvwxyz';

function b58encode(bytes) {
  let n = BigInt('0x' + (Buffer.from(bytes).toString('hex') || '0'));
  let out = '';
  while (n > 0n) { out = B58[Number(n % 58n)] + out; n /= 58n; }
  for (const b of bytes) { if (b === 0) out = '1' + out; else break; }
  return out;
}

function b58decode(text) {
  let n = 0n;
  for (const c of text) {
    const i = B58.indexOf(c);
    if (i < 0) throw new Error(`bad base58: ${text}`);
    n = n * 58n + BigInt(i);
  }
  let hex = n === 0n ? '' : n.toString(16);
  if (hex.length % 2) hex = '0' + hex;
  const zeros = text.match(/^1*/)[0].length;
  return Buffer.concat([Buffer.alloc(zeros), Buffer.from(hex, 'hex')]);
}

const u8 = (n) => Buffer.from([n]);
function u32(n) { const b = Buffer.alloc(4); b.writeUInt32LE(n); return b; }
function u64(n) { const b = Buffer.alloc(8); b.writeBigUInt64LE(BigInt(n)); return b; }
function u128(n) {
  const v = BigInt(n);
  return Buffer.concat([u64(v & 0xffffffffffffffffn), u64(v >> 64n)]);
}
function str(s) { const b = Buffer.from(s, 'utf8'); return Buffer.concat([u32(b.length), b]); }
const sha256 = (data) => crypto.createHash('sha256').update(data).digest();

// -------------------------------------------------------------------- keys

const PKCS8_ED25519_PREFIX = Buffer.from('302e020100300506032b657004220420', 'hex');

function keypair(seed, accountId) {
  const privateKey = crypto.createPrivateKey({
    key: Buffer.concat([PKCS8_ED25519_PREFIX, seed]), format: 'der', type: 'pkcs8',
  });
  const pub = crypto.createPublicKey(privateKey).export({ format: 'der', type: 'spki' }).subarray(-32);
  return {
    seed, privateKey, pub,
    account: accountId ?? pub.toString('hex'),
    publicKey: 'ed25519:' + b58encode(pub),
  };
}

const signBytes = (kp, data) => crypto.sign(null, data, kp.privateKey);

function loadKeys() {
  const raw = JSON.parse(fs.readFileSync(KEYS, 'utf8'));
  const out = {};
  for (const [name, k] of Object.entries(raw)) out[name] = keypair(Buffer.from(k.seed, 'hex'), k.account);
  return out;
}

// ----------------------------------------------------------------- NEP-413

function versionedNonce(saltHex, deadlineMs, tail) {
  return Buffer.concat([
    Buffer.from(NONCE_MAGIC, 'hex'), u8(0), Buffer.from(saltHex, 'hex'),
    (() => { const b = Buffer.alloc(8); b.writeBigInt64LE(BigInt(deadlineMs) * 1_000_000n); return b; })(),
    tail,
  ]);
}

function nep413Hash(message, nonce, recipient) {
  return sha256(Buffer.concat([u32(NEP413_TAG), str(message), nonce, str(recipient), u8(0)]));
}

function signIntents(kp, intents, { saltHex, deadlineMs, tail = crypto.randomBytes(15) }) {
  const deadline = new Date(deadlineMs).toISOString();
  const message = JSON.stringify({ signer_id: kp.account, deadline, intents });
  const nonce = versionedNonce(saltHex, deadlineMs, tail);
  const hash = nep413Hash(message, nonce, VERIFIER);
  return {
    hash, nonce,
    payload: {
      standard: 'nep413',
      payload: { message, nonce: nonce.toString('base64'), recipient: VERIFIER },
      public_key: kp.publicKey,
      signature: 'ed25519:' + b58encode(signBytes(kp, hash)),
    },
  };
}

// ------------------------------------------------------------- transactions

const functionCall = (method, args, gas, deposit) => Buffer.concat([
  u8(2), str(method), (() => { const a = Buffer.from(args, 'utf8'); return Buffer.concat([u32(a.length), a]); })(),
  u64(gas), u128(deposit),
]);
const transfer = (deposit) => Buffer.concat([u8(3), u128(deposit)]);

function signTx(kp, receiver, nonce, blockHash, actions) {
  const tx = Buffer.concat([
    str(kp.account), u8(0), kp.pub, u64(nonce), str(receiver), blockHash, u32(actions.length), ...actions,
  ]);
  const hash = sha256(tx);
  const signed = Buffer.concat([tx, u8(0), signBytes(kp, hash)]);
  return { base64: signed.toString('base64'), hash: b58encode(hash) };
}

// --------------------------------------------------------------------- RPC

const sleep = (ms) => new Promise((r) => setTimeout(r, ms));

async function rpc(method, params) {
  for (let attempt = 0; ; attempt++) {
    const res = await fetch(RPC, {
      method: 'POST',
      headers: { 'content-type': 'application/json' },
      body: JSON.stringify({ jsonrpc: '2.0', id: 'spike', method, params }),
    });
    const text = await res.text();
    if (res.status === 429 || res.status >= 500) {
      if (attempt >= 4) throw new Error(`${method}: HTTP ${res.status}: ${text.slice(0, 300)}`);
      await sleep(2000 * (attempt + 1));
      continue;
    }
    const json = JSON.parse(text);
    if (json.error) throw new Error(`${method}: ${JSON.stringify(json.error).slice(0, 600)}`);
    return json.result;
  }
}

async function view(account, method, args = {}) {
  const r = await rpc('query', {
    request_type: 'call_function', finality: 'final', account_id: account, method_name: method,
    args_base64: Buffer.from(JSON.stringify(args)).toString('base64'),
  });
  if (r.error) throw new Error(`${method}: ${r.error}`);
  return JSON.parse(Buffer.from(r.result).toString());
}

const mtBalance = async (account, token) => BigInt(await view(VERIFIER, 'mt_balance_of', { account_id: account, token_id: token }));
const ftBalance = async (account, contract) => BigInt(await view(contract, 'ft_balance_of', { account_id: account }));
const nativeBalance = async (account) => BigInt((await rpc('query', { request_type: 'view_account', finality: 'final', account_id: account })).amount);

async function send(kp, receiver, actions) {
  const key = await rpc('query', {
    request_type: 'view_access_key', finality: 'final', account_id: kp.account, public_key: kp.publicKey,
  });
  const block = await rpc('block', { finality: 'final' });
  const tx = signTx(kp, receiver, BigInt(key.nonce) + 1n, b58decode(block.header.hash), actions);
  const out = await rpc('send_tx', { signed_tx_base64: tx.base64, wait_until: 'FINAL' });
  const outcomes = [out.transaction_outcome, ...out.receipts_outcome];
  const gas = outcomes.reduce((sum, o) => sum + BigInt(o.outcome.gas_burnt), 0n);
  const logs = outcomes.flatMap((o) => o.outcome.logs);
  const failures = outcomes.filter((o) => o.outcome.status.Failure).map((o) => o.outcome.status.Failure);
  return { hash: out.transaction.hash, gas: gas.toString(), logs, failures, ok: 'SuccessValue' in out.status };
}

async function faucet(accountId, body) {
  const res = await fetch(FAUCET, {
    method: 'POST', headers: { 'content-type': 'application/json' }, body: JSON.stringify({ accountId, ...body }),
  });
  const json = await res.json();
  if (!res.ok) throw new Error(`faucet ${res.status}: ${JSON.stringify(json)}`);
  return json;
}

function record(step, data) {
  const run = fs.existsSync(RUN) ? JSON.parse(fs.readFileSync(RUN, 'utf8')) : {};
  run[step] = data;
  fs.writeFileSync(RUN, JSON.stringify(run, null, 2) + '\n');
}

function assert(cond, message) {
  if (!cond) throw new Error(`ASSERT FAILED: ${message}`);
  console.log(`  ok: ${message}`);
}

// ------------------------------------------------------------------- steps

async function setup() {
  if (fs.existsSync(KEYS)) throw new Error(`${KEYS} exists; delete it to start over`);
  const funderId = `cabalmesh-spike-${crypto.randomBytes(4).toString('hex')}.testnet`;
  const seeds = { A: crypto.randomBytes(32), B: crypto.randomBytes(32), G: crypto.randomBytes(32), F: crypto.randomBytes(32) };
  const keys = {
    A: keypair(seeds.A), B: keypair(seeds.B), G: keypair(seeds.G), F: keypair(seeds.F, funderId),
  };
  const saved = Object.fromEntries(Object.entries(keys).map(([n, k]) => [n, { seed: k.seed.toString('hex'), account: k.account }]));
  fs.writeFileSync(KEYS, JSON.stringify(saved, null, 2) + '\n', { mode: 0o600 });

  console.log('create funder', funderId);
  const res = await fetch(HELPER, {
    method: 'POST', headers: { 'content-type': 'application/json' },
    body: JSON.stringify({ newAccountId: funderId, newAccountPublicKey: keys.F.publicKey }),
  });
  if (!res.ok) throw new Error(`helper ${res.status}: ${(await res.text()).slice(0, 300)}`);
  await sleep(5000);
  console.log('  funder balance', (await nativeBalance(funderId)).toString());

  const fundG = await send(keys.F, keys.G.account, [transfer(2n * NEAR)]);
  const fundA = await send(keys.F, keys.A.account, [transfer(1n * NEAR)]);
  assert(fundG.ok && fundA.ok, 'funder transferred NEAR to implicit G and A');

  const dripA = await faucet(keys.A.account, { mode: 'intents', assets: ['USDC', 'wNEAR'] });
  const dripB = await faucet(keys.B.account, { mode: 'intents', assets: ['USDC', 'wNEAR'] });
  await sleep(3000);
  for (const [n, k] of [['A', keys.A], ['B', keys.B]]) {
    console.log(`  ${n} USDC`, (await mtBalance(k.account, USDC)).toString(), 'wNEAR', (await mtBalance(k.account, WNEAR)).toString());
  }
  record('setup', {
    accounts: Object.fromEntries(Object.entries(keys).map(([n, k]) => [n, k.account])),
    fundG: fundG.hash, fundA: fundA.hash,
    dripA: dripA.transactions, dripB: dripB.transactions,
  });
}

async function swap() {
  const { A, B, G } = loadKeys();
  const saltHex = await view(VERIFIER, 'current_salt');
  const deadlineMs = Date.now() + 3_600_000;
  const base = 10n ** 24n;
  const quote = 4_040_000n;
  const a = signIntents(A, [{ intent: 'token_diff', diff: { [USDC]: quote.toString(), [WNEAR]: (-base).toString() } }], { saltHex, deadlineMs });
  const b = signIntents(B, [{ intent: 'token_diff', diff: { [USDC]: (-quote).toString(), [WNEAR]: base.toString() } }], { saltHex, deadlineMs });
  const signed = [a.payload, b.payload];

  const sim = await view(VERIFIER, 'simulate_intents', { signed });
  console.log('  simulate state', JSON.stringify(sim.state));
  assert(sim.invariant_violated == null, 'simulate_intents: invariant_violated is null');

  const before = {
    aUsdc: await mtBalance(A.account, USDC), aWnear: await mtBalance(A.account, WNEAR),
    bUsdc: await mtBalance(B.account, USDC), bWnear: await mtBalance(B.account, WNEAR),
  };
  const tx = await send(G, VERIFIER, [functionCall('execute_intents', JSON.stringify({ signed }), GAS_EXECUTE_INTENTS, 0n)]);
  console.log('  tx', tx.hash, 'gas', tx.gas);
  assert(tx.ok && tx.failures.length === 0, 'G (neither A nor B) submitted execute_intents successfully');
  await sleep(2000);

  assert(await mtBalance(A.account, USDC) === before.aUsdc + quote, 'A gained USDC');
  assert(await mtBalance(A.account, WNEAR) === before.aWnear - base, 'A lost wNEAR');
  assert(await mtBalance(B.account, USDC) === before.bUsdc - quote, 'B lost USDC');
  assert(await mtBalance(B.account, WNEAR) === before.bWnear + base, 'B gained wNEAR');

  let replay;
  try { replay = await send(G, VERIFIER, [functionCall('execute_intents', JSON.stringify({ signed }), GAS_EXECUTE_INTENTS, 0n)]); }
  catch (e) { replay = { ok: false, failures: [e.message] }; }
  console.log('  replay', JSON.stringify(replay.failures).slice(0, 300));
  assert(!replay.ok, 'resubmitting the same pair is rejected');

  record('swap', {
    salt: saltHex, deadlineMs, signed, simulate: sim, tx: tx.hash, gas: tx.gas, logs: tx.logs,
    replay: { hash: replay.hash, failures: replay.failures },
  });
}

async function wallet() {
  const { A } = loadKeys();
  const drip = await faucet(A.account, { mode: 'wallet' });
  await sleep(3000);
  const amount = 1_000_000n;
  const wallet0 = await ftBalance(A.account, USDC_CONTRACT);
  const intents0 = await mtBalance(A.account, USDC);
  assert(wallet0 >= amount, 'A has USDC in its wallet');

  const args = JSON.stringify({ receiver_id: VERIFIER, amount: amount.toString(), msg: '' });
  const dep = await send(A, USDC_CONTRACT, [functionCall('ft_transfer_call', args, GAS_FT_TRANSFER_CALL, 1n)]);
  console.log('  deposit', dep.hash, 'gas', dep.gas);
  assert(dep.ok && dep.failures.length === 0, 'ft_transfer_call to verifier succeeded');
  await sleep(2000);
  assert(await mtBalance(A.account, USDC) === intents0 + amount, 'deposit credited the verifier balance');
  assert(await ftBalance(A.account, USDC_CONTRACT) === wallet0 - amount, 'deposit debited the wallet');

  const saltHex = await view(VERIFIER, 'current_salt');
  const w = signIntents(A, [{ intent: 'ft_withdraw', token: USDC_CONTRACT, receiver_id: A.account, amount: amount.toString() }], { saltHex, deadlineMs: Date.now() + 3_600_000 });
  const wd = await send(A, VERIFIER, [functionCall('execute_intents', JSON.stringify({ signed: [w.payload] }), GAS_EXECUTE_INTENTS, 0n)]);
  console.log('  withdraw', wd.hash, 'gas', wd.gas);
  assert(wd.ok && wd.failures.length === 0, 'ft_withdraw intent succeeded');
  await sleep(2000);
  assert(await mtBalance(A.account, USDC) === intents0, 'withdraw debited the verifier balance');
  assert(await ftBalance(A.account, USDC_CONTRACT) === wallet0, 'withdraw credited the wallet');

  record('wallet', {
    drip: drip.transactions?.[0], deposit: { hash: dep.hash, gas: dep.gas, args, logs: dep.logs },
    withdraw: { hash: wd.hash, gas: wd.gas, payload: w.payload, logs: wd.logs },
  });
}

// ---------------------------------------------------------------- fixtures

function buildFixtures() {
  // Fixed test-only key. It holds no funds; never reuse it.
  const seed = Buffer.from(Array.from({ length: 32 }, (_, i) => i + 1));
  const kp = keypair(seed);
  const saltHex = 'fc4d10f1';
  const tailHex = '0102030405060708090a0b0c0d0e0f';
  const deadlineMs = Date.UTC(2030, 0, 1);
  const diff = { [USDC]: '4040000', [WNEAR]: '-1000000000000000000000000' };
  const signed = signIntents(kp, [{ intent: 'token_diff', diff }], { saltHex, deadlineMs, tail: Buffer.from(tailHex, 'hex') });

  const blockHash = sha256(Buffer.from('cabalmesh fixture block'));
  const args = JSON.stringify({ receiver_id: VERIFIER, amount: '1000000', msg: '' });
  const nonce = 7n;
  const tx = signTx(kp, USDC_CONTRACT, nonce, blockHash, [functionCall('ft_transfer_call', args, GAS_FT_TRANSFER_CALL, 1n)]);

  return {
    'keypair.json': {
      secret_hex: seed.toString('hex'),
      account_id: kp.account,
      public_key: kp.publicKey,
    },
    'nep413_token_diff.json': {
      secret_hex: seed.toString('hex'),
      verifier: VERIFIER,
      diff,
      salt_hex: saltHex,
      nonce_tail_hex: tailHex,
      deadline_ms: deadlineMs,
      nonce_hex: signed.nonce.toString('hex'),
      expected_hash_hex: signed.hash.toString('hex'),
      expected_multipayload: signed.payload,
    },
    'signed_tx_ft_transfer_call.json': {
      secret_hex: seed.toString('hex'),
      signer_id: kp.account,
      receiver_id: USDC_CONTRACT,
      nonce: Number(nonce),
      block_hash: b58encode(blockHash),
      method_name: 'ft_transfer_call',
      args_json: args,
      gas: GAS_FT_TRANSFER_CALL.toString(),
      deposit: '1',
      expected_tx_hash: tx.hash,
      expected_signed_tx_base64: tx.base64,
    },
  };
}

async function fixtures(check) {
  const built = buildFixtures();
  if (!check) {
    fs.mkdirSync(FIXTURES, { recursive: true });
    for (const [name, value] of Object.entries(built)) {
      fs.writeFileSync(path.join(FIXTURES, name), JSON.stringify(value, null, 2) + '\n');
      console.log('wrote', name);
    }
    return;
  }

  for (const [name, value] of Object.entries(built)) {
    const onDisk = fs.readFileSync(path.join(FIXTURES, name), 'utf8');
    assert(onDisk === JSON.stringify(value, null, 2) + '\n', `${name} matches a fresh build`);
  }

  // The verifier checks the signature before balances, so "insufficient
  // balance" from this unfunded key proves it accepted the fixture's signature.
  const payload = built['nep413_token_diff.json'].expected_multipayload;
  let error = '';
  try { await view(VERIFIER, 'simulate_intents', { signed: [payload] }); } catch (e) { error = e.message; }
  console.log('  verifier said:', error);
  assert(error.includes('insufficient balance'), 'live verifier accepts the fixture signature');

  const tampered = structuredClone(payload);
  tampered.payload.message = tampered.payload.message.replace('4040000', '4040001');
  error = '';
  try { await view(VERIFIER, 'simulate_intents', { signed: [tampered] }); } catch (e) { error = e.message; }
  assert(error.includes('invalid signature'), 'live verifier rejects a tampered fixture');
}

// -------------------------------------------------------------------- main

const [command, flag] = process.argv.slice(2);
const commands = { setup, swap, wallet, fixtures: () => fixtures(flag === '--check') };
if (!commands[command]) {
  console.error('usage: probe.mjs setup | swap | wallet | fixtures [--check]');
  process.exit(2);
}
await commands[command]();
