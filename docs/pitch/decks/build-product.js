// ============================================================================
// build-product.js — renders docs/pitch/CabalMesh-Product.pptx
//
// Audience: hackathon / grant judges.
// Discipline: every capability carries a status chip, and every LIVE claim
// carries the file it was checked against (docs/product-status.md).
// ============================================================================
const pptxgen = require('pptxgenjs');
const path = require('path');
const { T, P, C, F, createDeck } = require('./theme');

const ROOT = path.join(__dirname, '..', '..', '..');
const asset = (...p) => path.join(ROOT, ...p);

const STATUS = {
  live: { text: 'LIVE', color: C.live },
  wip: { text: 'IN PROGRESS', color: C.accent },
  plan: { text: 'PLANNED', color: C.ink2 },
};

// ---------------------------------------------------------------- content
const D = {
  cover: {
    eyebrow: 'ZERO IDENTITY · PRIVATE INTENTS',
    title: 'Private intent.\nOffline relay.\nOn-chain proof.',
    sub: 'CabalMesh is a zero-identity coordination layer. An intent is signed on-device, carried through nearby peers with no internet, and settled on Avalanche when a gateway returns.',
    footer: 'CABALMESH  ·  AVALANCHE FUJI  ·  2026',
    stamp: 'PRODUCT INTRODUCTION',
  },

  gap: {
    eyebrow: 'The gap',
    title: 'Coordination assumes a network that is not always there.',
    lead: 'Every transaction rail in use today needs two things at once: a live connection and a persistent identity. Both fail in the moments when local coordination matters most.',
    cards: [
      { icon: 'link', label: 'Connectivity is brittle', body: 'No link, no broadcast. A signed intent has nowhere to go and no path back — it simply fails.' },
      { icon: 'eye_slash', label: 'Identity is exposed', body: 'Accounts and public rails leak who acted, from where, and what they intended. The metadata is the leak.' },
      { icon: 'shield', label: 'Settlement needs a witness', body: 'A local agreement is worthless to a chain until something proves that it happened, exactly as agreed.' },
    ],
  },

  what: {
    eyebrow: 'What it is',
    title: 'One device, three layers, no identity.',
    lead: 'The stack separates local coordination from global settlement. Each layer is listed with what is running today, not what is intended.',
    layers: [
      {
        n: '01', name: 'THE CLOAK LAYER', tag: 'MESH TRANSPORT', status: 'live', icon: 'network',
        body: 'libp2p, mDNS discovery and gossipsub over IP; a BLE plane on iOS and Android for the fully offline case. Multi-hop stripping means no hop knows both ends.',
        code: 'src/mesh.rs · crates/cabal-ble · 9 passing tests',
      },
      {
        n: '02', name: 'THE SETTLEMENT LAYER', tag: 'AVALANCHE', status: 'live', icon: 'cube',
        body: 'A minimal Escrow contract, live on Fuji. Offline signing, a local relay queue, and auto-confirmation the moment a gateway appears.',
        code: 'contracts/Escrow.sol · src/blockchain_bridge.rs',
      },
      {
        n: '03', name: 'THE INVISIBLE BRAIN', tag: 'LOCAL INTELLIGENCE', status: 'wip', icon: 'brain',
        body: 'A local model parses natural language into a validated intent draft, and Rust still validates and signs it. Zero-knowledge bid proofs and agent-to-agent negotiation are Phase 4 — no proving code is in the build.',
        code: 'parse_intent_chat · commands.rs',
      },
    ],
  },

  loop: {
    eyebrow: 'The core loop',
    title: 'Compose. Relay. Reconnect. Settle.',
    lead: 'This is the whole product in four moves — and all four are observable in the running app today.',
    steps: [
      { n: '01', icon: 'bolt', head: 'COMPOSE', body: 'An intent is written and signed on-device. No account, no server, no session.' },
      { n: '02', icon: 'network', head: 'RELAY', body: 'Nearby peers forward the encrypted payload over BLE or libp2p. No hop learns the origin.' },
      { n: '03', icon: 'route', head: 'RECONNECT', body: 'The first peer holding internet drains the queued work toward the chain.' },
      { n: '04', icon: 'cube', head: 'SETTLE', body: 'Escrow confirms on Avalanche Fuji. The result lands on-chain; the identity never does.' },
    ],
    note: 'A complete, observable lifecycle — not a promise of future infrastructure.',
  },

  offline: {
    eyebrow: 'The differentiator',
    title: 'Sign with no network. Confirm when the network returns.',
    lead: 'This is the one path nothing else in the category does end to end, and it is the path we demo live.',
    steps: [
      { n: '01', head: 'SIGN OFFLINE', body: 'The transaction is signed on-device with the radio down.', code: 'sign_offline' },
      { n: '02', head: 'QUEUE LOCALLY', body: 'The signed payload waits in an encrypted local relay queue.', code: 'cabal-vault' },
      { n: '03', head: 'CARRY BY MESH', body: 'Peers in radio range move it hop by hop, still offline.', code: 'cabal-ble · mesh.rs' },
      { n: '04', head: 'DRAIN AT GATEWAY', body: 'The first peer with internet replays the queue to Avalanche.', code: 'queue replay' },
      { n: '05', head: 'CONFIRM ON-CHAIN', body: 'Escrow settles on Fuji and the app auto-confirms the intent.', code: 'deployments/fuji.json' },
    ],
    note: 'Verified across two Android emulators with the internet plane disabled — see docs/mobile-build-verification.md.',
  },

  live: {
    eyebrow: 'Evidence',
    title: 'What is running today.',
    lead: 'Audited by reading the code, not the README. Every row names the file it was checked against.',
    rows: [
      ['Mesh networking — libp2p, mDNS, gossipsub', 'src/mesh.rs · tests/ble_loopback.rs'],
      ['BLE offline plane — iOS and Android', 'src/ble/ · crates/cabal-ble'],
      ['Intent lifecycle — compose, broadcast, settle, proof', 'src/commands.rs · src/intents.rs'],
      ['On-chain Escrow, live on Avalanche Fuji', 'contracts/Escrow.sol · deployments/fuji.json'],
      ['Offline signing, relay queue, auto-confirm', 'src/blockchain_bridge.rs'],
      ['Vault encryption — AES-256-GCM, Argon2id unlock', 'crates/cabal-vault'],
      ['Guardian mesh recovery — enroll, request, approve', 'crates/cabal-guardian · src/guardian.rs'],
      ['Standing — settlement count, computed from chain', 'src/standing.rs'],
    ],
  },

  identity: {
    eyebrow: 'The privacy model',
    title: 'In this network, you are a Nobody.',
    lead: 'Privacy in depth is a subtraction, not a feature list. What is removed matters more than what is added.',
    erased: {
      label: 'ERASED',
      items: [
        'Physical location — no IP is attached to a relayed intent.',
        'Account identity — the wallet is created on-device and never registered.',
        'History — nothing is stored server-side, because there is no server.',
        'Linkability — guardian requests carry unlinkable per-request tags.',
      ],
    },
    proved: {
      label: 'PROVED',
      items: [
        'Signature validity — verified by the chain, not by us.',
        'Escrow state — AVAX locks and releases against the contract.',
        'Standing — settlement count derived from on-chain history.',
        'Key custody — hardware-backed on iOS, Android and Apple Silicon.',
      ],
    },
    note: 'No zero-knowledge proving ships today — the unused Noir stub was deleted rather than left to imply a capability. A real circuit is Phase 4, labelled, not claimed.',
  },

  guardian: {
    eyebrow: 'Recovery',
    title: 'Lose the device, keep the wallet.',
    lead: 'A wallet with no way out of it is the sharpest risk in any self-custody product. Guardian recovery closes it without ever reconstructing a key on a server.',
    flow: [
      { n: '01', head: 'ENROL', body: 'Key shares are split across five chosen guardians. No share is a key.', status: 'live' },
      { n: '02', head: 'REQUEST', body: 'A wiped device broadcasts an unlock request over BLE.', status: 'live' },
      { n: '03', head: 'APPROVE', body: 'Each guardian approves by hand. The reply is gated on a human, never automatic.', status: 'live' },
      { n: '04', head: 'VETO WINDOW', body: 'A 24–48h delay with a push notification, so a stolen device can be blocked.', status: 'wip' },
      { n: '05', head: 'RESTORE', body: 'Three of five shares rebuild the vault on the new device.', status: 'live' },
    ],
    note: 'Passphrase unlock (Argon2id) and full export / import / restore ship today under VAULT → KEYS → ADVANCED.',
  },

  arch: {
    eyebrow: 'Architecture',
    title: 'From a dark room to the chain.',
    lead: 'Three zones, one direction of travel. Nothing crosses a boundary carrying more than it must.',
    note: 'The mesh never touches the chain. The gateway never learns the origin. The chain never learns the identity.',
  },

  status: {
    eyebrow: 'Status board',
    title: 'What is live, what is not.',
    lead: 'A deck that survives questions says this out loud. One true story beats three half-built ones — the offline settlement path is the true one.',
    items: [
      { name: 'Offline mesh settlement', status: 'live', note: 'Sign, relay, confirm — end to end.' },
      { name: 'BLE plane (iOS · Android)', status: 'live', note: 'Two-emulator verification.' },
      { name: 'Escrow on Fuji', status: 'live', note: 'Real deployed address.' },
      { name: 'Vault + guardian recovery', status: 'live', note: 'Enrol, approve, restore.' },
      { name: 'AI intent parsing', status: 'wip', note: 'Rust still validates and signs.' },
      { name: 'Recovery veto window', status: 'wip', note: 'Needs device-level push.' },
      { name: 'Marketplace + modules', status: 'plan', note: 'Contracts deployed, UI next.' },
      { name: 'ZK bid proofs', status: 'plan', note: 'Circuit to be written; nargo in CI.' },
      { name: 'Agent negotiation · FHE/MPC', status: 'plan', note: 'Phase 4 and Phase 5.' },
    ],
  },

  platform: {
    eyebrow: 'Platform reality',
    title: 'Where the offline plane actually runs.',
    lead: 'The BLE plane needs a radio stack we can drive. Mobile and Apple silicon have it; desktop is scoped, not glossed over.',
    head: ['PLATFORM', 'SECURE ELEMENT', 'BLE PLANE', 'NOTE'],
    rows: [
      ['iOS', 'Secure Enclave', 'YES', 'Primary target for the offline demo.'],
      ['Android', 'Keystore / StrongBox', 'YES', 'Verified across two emulators.'],
      ['macOS', 'Secure Enclave (Apple silicon)', 'YES', 'CoreBluetooth; Bluetooth usage declared.'],
      ['Windows', 'TPM', 'NO', 'Falls back to the IP plane.'],
      ['Linux', 'Usually none', 'NO', 'Falls back to the IP plane.'],
    ],
  },

  cases: {
    eyebrow: 'Where it matters',
    title: 'Places where the internet is the missing part.',
    cards: [
      { icon: 'route', head: 'OFF-GRID AND DISASTER', body: 'A cell tower is down or saturated. Value still has to move between people standing near each other, and settle honestly later.' },
      { icon: 'store', head: 'HYPER-LOCAL TRADE', body: 'Markets, borders, festivals, campuses. Counterparties are in radio range; a payment rail that needs a data plan is not.' },
      { icon: 'lock', head: 'PRIVACY-CRITICAL WORK', body: 'Journalists, field researchers, activists. The metadata trail is the risk, so the network is built to leave none.' },
    ],
    note: 'The same primitive serves all three: a signed intent that survives without a network and proves itself when one returns.',
  },

  devices: {
    eyebrow: 'Hardware · planned',
    title: 'Two boxes the software would run on.',
    lead: 'Neither exists — no silicon selected, no board, no tooling. Both are drawn to the millimetre from workloads the app already performs, so the spec can be checked against the code instead of a mood board.',
    cards: [
      {
        image: 'deck-shadowbox.png',
        name: 'SHADOWBOX',
        role: 'MESH · MODEL · PROVER',
        spec: '220 × 160 × 70 mm · 1.9 kg · fanless',
        body: 'One node instead of three: it relays for the neighbourhood, runs the local model that reads a sentence, and generates the proof. RADIO / CRYPTO / POWER bays mirror the in-app module system.',
      },
      {
        image: 'deck-nobody.png',
        name: 'THE NOBODY BOX',
        role: 'PHYSICAL ESCROW',
        spec: '520 × 460 × 720 mm · ≈ 88 L · no keyhole',
        body: 'A parcel locker whose bolt turns on the on-chain release. The seller drops the item in, the buyer opens it, nobody in between can. A load cell reports that something left — never what it was.',
      },
    ],
    note: 'SPEC AND DRAWINGS — docs/pitch/hardware-device-prompts.md · the images above are vector renders of that geometry, not photographs',
  },

  close: {
    eyebrow: 'CabalMesh',
    title: 'Zero identity.\nPrivate intents.\nVerifiable execution.',
    lines: [
      ['DEMO', 'youtu.be/Z3ooub-mnCw'],
      ['REPOSITORY', 'github.com/kurodenjiro/Cabal-Mesh'],
      ['NETWORK', 'Avalanche Fuji — Escrow live'],
      ['STATUS', 'Offline settlement path: shipped and demonstrable'],
    ],
    footer: 'WE LEAVE NO IDENTITY, ONLY TRACES.',
  },
};

// ------------------------------------------------------------------ slides
function cover(slide) {
  const s = slide({ chrome: false });
  P.ticks(s);
  P.mesh(s, [
    [8.9, 1.35, 10.2, 2.15], [10.2, 2.15, 11.6, 1.45], [8.9, 1.35, 8.35, 2.75],
    [8.35, 2.75, 10.2, 2.15], [8.35, 2.75, 8.8, 4.5], [8.8, 4.5, 10.6, 5.1],
    [10.2, 2.15, 11.9, 3.3], [11.9, 3.3, 10.6, 5.1],
  ]);

  const d = D.cover;
  s.addText('CABALMESH', { x: T.margin, y: 0.3, w: 4, h: 0.26, fontFace: F.sans, fontSize: 9, bold: true, color: C.white, charSpacing: 3 });
  s.addText(d.stamp, { x: 7.2, y: 0.3, w: 5.4, h: 0.26, align: 'right', fontFace: F.sans, fontSize: 9, bold: true, color: C.ink2, charSpacing: 3 });
  P.rule(s, 0.62, { color: C.line });

  s.addText(d.eyebrow, { x: T.margin, y: 1.55, w: 7, h: 0.28, fontFace: F.sans, fontSize: 10, bold: true, color: C.accent, charSpacing: 3 });
  s.addText(d.title, { x: T.margin - 0.04, y: 2.05, w: 8.4, h: 2.5, fontFace: F.sans, fontSize: T.size.hero, bold: true, color: C.white, lineSpacingMultiple: 1.02 });
  P.accentBar(s, T.margin, 4.72);
  s.addText(d.sub, { x: T.margin, y: 5.0, w: 7.1, h: 1.1, fontFace: F.sans, fontSize: 12.5, color: C.ink1, lineSpacingMultiple: 1.4 });

  P.image(s, asset('src', 'ds', 'assets', 'logo', 'hero-lockup.png'), 8.55, 1.05, 3.9, 5.2, 'contain');
  s.addText(d.footer, { x: T.margin, y: 6.85, w: 7, h: 0.26, fontFace: F.sans, fontSize: 8.5, bold: true, color: C.ink2, charSpacing: 2.4 });
}

function head(slide, d, section, o = {}) {
  const s = slide({});
  P.eyebrow(s, d.eyebrow);
  P.title(s, d.title, { size: o.titleSize ?? T.size.title, w: o.titleW ?? 10.4 });
  if (d.lead) P.lead(s, d.lead, { y: o.leadY ?? 2.06, w: o.leadW ?? 9.6 });
  return s;
}

function gap(slide) {
  const d = D.gap;
  const s = head(slide, d, 'The gap');
  const w = 3.724, gapX = 0.36, top = 3.05, h = 2.7;
  d.cards.forEach((c, i) => {
    const x = T.margin + i * (w + gapX);
    P.panel(s, x, top, w, h);
    P.fill(s, x, top, w, 0.028, i === 2 ? C.accent : C.ink3);
    P.icon(s, c.icon, x + 0.42, top + 0.5, 0.42);
    P.body(s, c.label, x + 0.42, top + 1.16, w - 0.84, { size: 15, bold: true, color: C.white, h: 0.5 });
    P.body(s, c.body, x + 0.42, top + 1.76, w - 0.84, { size: 10.2, color: C.ink1, h: 0.9 });
  });
}

function what(slide) {
  const d = D.what;
  const s = head(slide, d, 'What it is');
  const top = 2.86, rowH = 1.38;
  d.layers.forEach((l, i) => {
    const y = top + i * rowH;
    P.panel(s, T.margin, y, T.contentW, 1.26, { fill: C.panel, line: C.lineSoft });
    P.fill(s, T.margin, y, 0.028, 1.26, l.status === 'live' ? C.live : C.accent);
    s.addText(l.n, { x: T.margin + 0.34, y: y + 0.3, w: 0.6, h: 0.5, fontFace: F.mono, fontSize: 16, bold: true, color: C.ink3 });
    P.icon(s, l.icon, T.margin + 1.05, y + 0.42, 0.4);
    P.body(s, l.name, T.margin + 1.72, y + 0.24, 4.2, { size: 13.5, bold: true, color: C.white, h: 0.3 });
    P.label(s, l.tag, T.margin + 1.72, y + 0.56, { w: 4, color: C.ink3 });
    P.body(s, l.body, T.margin + 5.7, y + 0.2, 4.3, { size: 9.6, color: C.ink1, h: 0.72 });
    P.code(s, l.code, T.margin + 5.7, y + 0.94, 4.6, { size: 7.8, color: C.ink2 });
    const st = STATUS[l.status];
    P.chip(s, st.text, 11.36, y + 0.24, { color: st.color, w: 1.2 });
  });
}

function loop(slide) {
  const d = D.loop;
  const s = head(slide, d, 'The core loop');
  const w = 2.72, gapX = 0.335, top = 3.05, h = 2.6;
  d.steps.forEach((st, i) => {
    const x = T.margin + i * (w + gapX);
    P.panel(s, x, top, w, h, { line: C.lineSoft });
    s.addText(st.n, { x: x + 0.34, y: top + 0.34, w: 0.8, h: 0.3, fontFace: F.mono, fontSize: 10, bold: true, color: C.accent });
    P.icon(s, st.icon, x + 0.34, top + 0.86, 0.46);
    P.body(s, st.head, x + 0.34, top + 1.5, w - 0.68, { size: 13, bold: true, color: C.white, h: 0.3 });
    P.body(s, st.body, x + 0.34, top + 1.88, w - 0.68, { size: 9.4, color: C.ink1, h: 0.8 });
    if (i < 3) {
      s.addShape('line', { x: x + w + 0.06, y: top + 1.06, w: gapX - 0.12, h: 0, line: { color: C.ink3, width: 1 } });
    }
  });
  P.rule(s, 6.2, { color: C.lineSoft });
  P.body(s, d.note, T.margin, 6.42, 11, { size: 11, color: C.accent, h: 0.3 });
}

function offline(slide) {
  const d = D.offline;
  const s = head(slide, d, 'Offline settlement');
  const top = 3.0, w = 2.19, gapX = 0.245;
  d.steps.forEach((st, i) => {
    const x = T.margin + i * (w + gapX);
    P.panel(s, x, top, w, 2.55, { fill: i === 4 ? C.panelUp : C.panel, line: i === 4 ? C.line : C.lineSoft });
    s.addText(st.n, { x: x + 0.28, y: top + 0.3, w: 0.7, h: 0.3, fontFace: F.mono, fontSize: 10, bold: true, color: i === 4 ? C.live : C.accent });
    P.body(s, st.head, x + 0.28, top + 0.74, w - 0.56, { size: 11.5, bold: true, color: C.white, h: 0.55 });
    P.body(s, st.body, x + 0.28, top + 1.3, w - 0.56, { size: 9, color: C.ink1, h: 0.72 });
    P.code(s, st.code, x + 0.28, top + 2.06, w - 0.48, { size: 7.6, color: C.ink2, h: 0.3 });
    if (i < 4) {
      s.addShape('line', { x: x + w + 0.04, y: top + 0.45, w: gapX - 0.08, h: 0, line: { color: C.ink3, width: 1 } });
    }
  });
  P.rule(s, 6.14, { color: C.lineSoft });
  P.body(s, d.note, T.margin, 6.34, 11.2, { size: 10, color: C.ink2, h: 0.4 });
}

function liveNow(slide) {
  const d = D.live;
  const s = head(slide, d, 'Evidence');
  const top = 3.12, rowH = 0.47;
  P.label(s, 'CAPABILITY', T.margin, top - 0.32, { color: C.ink3 });
  P.label(s, 'STATUS', 7.55, top - 0.32, { color: C.ink3, w: 1.2 });
  P.label(s, 'CHECKED AGAINST', 9.05, top - 0.32, { color: C.ink3, w: 4 });
  P.rule(s, top - 0.06, { color: C.line });
  d.rows.forEach((r, i) => {
    const y = top + i * rowH;
    if (i % 2 === 1) P.fill(s, T.margin, y, T.contentW, rowH - 0.04, C.panel);
    P.body(s, r[0], T.margin + 0.16, y + 0.1, 7.1, { size: 10.2, color: C.white, h: 0.3 });
    P.chip(s, 'LIVE', 7.55, y + 0.12, { color: C.live, w: 0.76 });
    P.code(s, r[1], 9.05, y + 0.14, 3.6, { size: 8, color: C.ink2 });
  });
  P.rule(s, top + d.rows.length * rowH + 0.02, { color: C.line });
}

function identity(slide) {
  const d = D.identity;
  const s = head(slide, d, 'Privacy model');
  const top = 2.95, w = 5.77, h = 2.9;
  [[d.erased, C.warn], [d.proved, C.live]].forEach((pair, ci) => {
    const [block, colr] = pair;
    const x = T.margin + ci * (w + 0.34);
    P.panel(s, x, top, w, h, { line: C.lineSoft });
    P.fill(s, x, top, w, 0.028, colr);
    P.label(s, block.label, x + 0.42, top + 0.36, { color: colr, size: 9, charSpacing: 3 });
    block.items.forEach((it, i) => {
      const y = top + 0.82 + i * 0.52;
      s.addShape('line', { x: x + 0.42, y: y + 0.11, w: 0.16, h: 0, line: { color: C.ink3, width: 1 } });
      P.body(s, it, x + 0.72, y, w - 1.14, { size: 9.8, color: C.ink1, h: 0.44 });
    });
  });
  P.body(s, d.note, T.margin, 6.28, 11.4, { size: 10, color: C.ink2, h: 0.4 });
}

function guardian(slide) {
  const d = D.guardian;
  const s = head(slide, d, 'Recovery');
  const top = 3.0, w = 2.19, gapX = 0.245;
  d.flow.forEach((st, i) => {
    const x = T.margin + i * (w + gapX);
    const st2 = STATUS[st.status];
    P.panel(s, x, top, w, 2.5, { line: C.lineSoft });
    s.addText(st.n, { x: x + 0.28, y: top + 0.28, w: 0.7, h: 0.3, fontFace: F.mono, fontSize: 10, bold: true, color: st2.color });
    P.body(s, st.head, x + 0.28, top + 0.7, w - 0.56, { size: 11.5, bold: true, color: C.white, h: 0.3 });
    P.body(s, st.body, x + 0.28, top + 1.08, w - 0.56, { size: 9, color: C.ink1, h: 0.9 });
    P.chip(s, st2.text, x + 0.28, top + 2.06, { color: st2.color, w: 1.16 });
    if (i < 4) s.addShape('line', { x: x + w + 0.04, y: top + 0.42, w: gapX - 0.08, h: 0, line: { color: C.ink3, width: 1 } });
  });
  P.rule(s, 6.1, { color: C.lineSoft });
  P.body(s, d.note, T.margin, 6.3, 11.2, { size: 10, color: C.ink2, h: 0.4 });
}

function arch(slide) {
  const d = D.arch;
  const s = head(slide, d, 'Architecture');

  const zones = [
    { x: T.margin, w: 3.5, label: 'ZONE 01 · DEVICE', title: 'ON-DEVICE', items: ['Intent composed and signed', 'Vault — AES-256-GCM', 'Secure element key custody', 'Relay queue while offline'] },
    { x: T.margin + 4.2, w: 3.5, label: 'ZONE 02 · MESH', title: 'THE MESH', items: ['BLE plane — iOS, Android', 'libp2p + mDNS + gossipsub', 'Multi-hop metadata stripping', 'No hop knows both ends'] },
    { x: T.margin + 8.4, w: 3.49, label: 'ZONE 03 · CHAIN', title: 'AVALANCHE FUJI', items: ['Gateway drains the queue', 'Escrow locks and releases AVAX', 'Settlement proof recorded', 'Standing derived on-chain'] },
  ];
  const top = 3.0, h = 2.75;
  zones.forEach((z, i) => {
    P.panel(s, z.x, top, z.w, h, { fill: i === 1 ? C.panelUp : C.panel, line: C.lineSoft });
    P.label(s, z.label, z.x + 0.36, top + 0.32, { color: i === 2 ? C.accent : C.ink3, size: 8 });
    P.body(s, z.title, z.x + 0.36, top + 0.62, z.w - 0.72, { size: 14, bold: true, color: C.white, h: 0.36 });
    z.items.forEach((it, k) => {
      P.body(s, it, z.x + 0.36, top + 1.16 + k * 0.38, z.w - 0.72, { size: 9.4, color: C.ink1, h: 0.34 });
    });
    if (i < 2) {
      const ax = z.x + z.w + 0.12;
      s.addShape('line', { x: ax, y: top + h / 2, w: 0.46, h: 0, line: { color: C.accent, width: 1 } });
      s.addText('>', { x: ax + 0.4, y: top + h / 2 - 0.16, w: 0.3, h: 0.3, fontFace: F.mono, fontSize: 12, bold: true, color: C.accent });
    }
  });
  P.rule(s, 6.28, { color: C.lineSoft });
  P.body(s, d.note, T.margin, 6.48, 11.4, { size: 10.5, color: C.accent, h: 0.3 });
}

function statusBoard(slide) {
  const d = D.status;
  const s = head(slide, d, 'Status board');
  const w = 3.724, gapX = 0.36, top = 2.92, h = 1.2, rowGap = 0.14;
  d.items.forEach((it, i) => {
    const cx = i % 3, ry = Math.floor(i / 3);
    const x = T.margin + cx * (w + gapX);
    const y = top + ry * (h + rowGap);
    const st = STATUS[it.status];
    P.panel(s, x, y, w, h, { line: C.lineSoft });
    P.fill(s, x, y, 0.028, h, st.color);
    P.body(s, it.name, x + 0.34, y + 0.2, w - 0.68, { size: 11.6, bold: true, color: C.white, h: 0.3 });
    P.body(s, it.note, x + 0.34, y + 0.52, w - 0.68, { size: 9, color: C.ink2, h: 0.3 });
    P.chip(s, st.text, x + 0.34, y + 0.84, { color: st.color, w: 1.16 });
  });
}

function platform(slide) {
  const d = D.platform;
  const s = head(slide, d, 'Platform reality');
  const top = 3.3, rowH = 0.58;
  const cols = [T.margin + 0.16, 2.9, 6.1, 7.7];
  const widths = [2.4, 3.0, 1.4, 4.6];
  d.head.forEach((hd, i) => P.label(s, hd, cols[i], top - 0.32, { color: C.ink3, w: widths[i] }));
  P.rule(s, top - 0.06, { color: C.line });
  d.rows.forEach((r, i) => {
    const y = top + i * rowH;
    if (i % 2 === 1) P.fill(s, T.margin, y, T.contentW, rowH - 0.04, C.panel);
    P.body(s, r[0], cols[0], y + 0.14, widths[0], { size: 11, bold: true, color: C.white, h: 0.3 });
    P.body(s, r[1], cols[1], y + 0.16, widths[1], { size: 9.8, color: C.ink1, h: 0.3 });
    P.chip(s, r[2], cols[2], y + 0.16, { color: r[2] === 'YES' ? C.live : C.ink3, w: 0.66 });
    P.body(s, r[3], cols[3], y + 0.16, widths[3], { size: 9.8, color: C.ink2, h: 0.3 });
  });
  P.rule(s, top + d.rows.length * rowH + 0.02, { color: C.line });
  P.body(s, 'The offline plane is scoped where the radio stack is real. Desktop keeps the IP plane — Phase 5 decides the rest.', T.margin, top + d.rows.length * rowH + 0.28, 11.4, { size: 10, color: C.ink2, h: 0.3 });
}

function cases(slide) {
  const d = D.cases;
  const s = slide({ section: 'Use cases' });
  P.eyebrow(s, d.eyebrow);
  P.title(s, d.title, { w: 10.4 });
  const w = 3.724, gapX = 0.36, top = 2.6, h = 3.3;
  d.cards.forEach((c, i) => {
    const x = T.margin + i * (w + gapX);
    P.panel(s, x, top, w, h, { line: C.lineSoft });
    P.icon(s, c.icon, x + 0.42, top + 0.5, 0.44);
    P.body(s, c.head, x + 0.42, top + 1.2, w - 0.84, { size: 14, bold: true, color: C.white, h: 0.6 });
    P.body(s, c.body, x + 0.42, top + 1.94, w - 0.84, { size: 10, color: C.ink1, h: 1.2 });
  });
  P.rule(s, 6.24, { color: C.lineSoft });
  P.body(s, d.note, T.margin, 6.44, 11.4, { size: 10.5, color: C.accent, h: 0.3 });
}

function devices(slide) {
  const d = D.devices;
  const s = head(slide, d, 'Hardware', { leadW: 11.4 });
  const w = 5.766, gapX = 0.36, top = 2.62, h = 3.62, pad = 0.34, imgH = 1.42;
  d.cards.forEach((c, i) => {
    const x = T.margin + i * (w + gapX);
    P.panel(s, x, top, w, h, { line: C.lineSoft });
    P.image(s, path.join(__dirname, '..', 'src', c.image), x + 0.02, top + 0.02, w - 0.04, imgH, 'cover');
    const ty = top + imgH + 0.2;
    P.chip(s, STATUS.plan.text, x + pad, ty, { color: STATUS.plan.color, w: 1.0 });
    P.body(s, c.name, x + pad, ty + 0.4, w - pad * 2, { size: 15, bold: true, color: C.white, h: 0.34 });
    P.label(s, c.role, x + pad, ty + 0.74, { color: C.accent, size: 8, w: w - pad * 2 });
    P.code(s, c.spec, x + pad, ty + 0.94, w - pad * 2, { size: 8.6, color: C.ink2 });
    P.body(s, c.body, x + pad, ty + 1.2, w - pad * 2, { size: 9.4, color: C.ink1, h: 1.0 });
  });
  P.rule(s, 6.44, { color: C.lineSoft });
  P.code(s, d.note, T.margin, 6.62, 11.6, { size: 7.8, color: C.ink2 });
}

function close(slide) {
  const d = D.close;
  const s = slide({ chrome: false });
  P.ticks(s);
  P.mesh(s, [
    [9.2, 1.7, 10.6, 2.5], [10.6, 2.5, 11.9, 1.9], [9.2, 1.7, 8.8, 3.1],
    [8.8, 3.1, 10.6, 2.5], [8.8, 3.1, 9.4, 4.7], [9.4, 4.7, 11.3, 5.2], [10.6, 2.5, 11.7, 3.8],
  ]);
  s.addText('CABALMESH', { x: T.margin, y: 0.3, w: 4, h: 0.26, fontFace: F.sans, fontSize: 9, bold: true, color: C.white, charSpacing: 3 });
  P.rule(s, 0.62, { color: C.line });

  P.image(s, asset('src', 'ds', 'assets', 'logo', 'minimal-mark.png'), T.margin, 1.5, 0.72, 0.72, 'contain');
  s.addText(d.title, { x: T.margin - 0.04, y: 2.5, w: 8, h: 2.3, fontFace: F.sans, fontSize: 40, bold: true, color: C.white, lineSpacingMultiple: 1.06 });
  P.accentBar(s, T.margin, 5.02);

  d.lines.forEach((l, i) => {
    const y = 5.3 + i * 0.4;
    P.label(s, l[0], T.margin, y + 0.03, { w: 1.6, color: C.ink3 });
    P.code(s, l[1], T.margin + 1.7, y, 6.4, { size: 10, color: C.ink1 });
  });
  s.addText(d.footer, { x: 7.5, y: 6.85, w: 5.1, h: 0.26, align: 'right', fontFace: F.sans, fontSize: 8.5, bold: true, color: C.ink3, charSpacing: 2.4 });
}

// -------------------------------------------------------------------- build
function main() {
  const pres = new pptxgen();
  pres.defineLayout({ name: 'CM16x9', width: T.W, height: T.H });
  pres.layout = 'CM16x9';
  pres.author = 'CabalMesh';
  pres.company = 'CabalMesh';
  pres.title = 'CabalMesh — Product Introduction';

  const slide = createDeck(pres, 'Product introduction');
  [cover, gap, what, loop, offline, liveNow, identity, guardian, arch, statusBoard, platform, cases, devices, close]
    .forEach((fn) => fn(slide));

  const out = path.join(ROOT, 'docs', 'pitch', 'CabalMesh-Product.pptx');
  return pres.writeFile({ fileName: out }).then(() => console.log('wrote', out));
}

main().catch((e) => { console.error(e); process.exit(1); });
