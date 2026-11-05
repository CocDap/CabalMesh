// ============================================================================
// build-pitch.js — renders docs/pitch/CabalMesh-Pitch.pptx
//
// Seven slides, in the order an investor reads them:
//   1 name  2 problem  3 solution  4 market  5 go-to-market + built
//   6 traction  7 team
//
// Market figures are public and carry their source on the slide. Fields only
// the founders can supply are marked TO FILL rather than invented.
// ============================================================================
const pptxgen = require('pptxgenjs');
const path = require('path');
const { T, P, C, F, createDeck } = require('./theme');

const ROOT = path.join(__dirname, '..', '..', '..');
const asset = (...p) => path.join(ROOT, ...p);

// ---------------------------------------------------------------- content
const D = {
  cover: {
    stamp: 'INVESTOR PITCH · 2026',
    eyebrow: 'THE NOBODY NETWORK',
    name: 'CabalMesh',
    line: 'A wallet whose payments survive with no internet — sign an intent on your phone, relay it through the phones around you, settle it on Avalanche the moment any of them finds a signal.',
    footer: 'CABALMESH · AVALANCHE FUJI · github.com/kurodenjiro/Cabal-Mesh',
  },

  problem: {
    eyebrow: 'The problem',
    title: 'Every payment rail on earth assumes a live connection — so when the network drops, value stops moving.',
    stats: [
      { n: '2.2B', unit: 'PEOPLE', body: 'are still offline in 2025 — 96% of them in low- and middle-income countries.', src: 'ITU, Facts and Figures 2025' },
      { n: '3.1B', unit: 'THE USAGE GAP', body: 'live under mobile coverage they do not use — 38% of the world’s population.', src: 'GSMA, State of Mobile Internet Connectivity 2025' },
      { n: '300M', unit: 'THE COVERAGE GAP', body: 'have no mobile internet coverage available to them at all.', src: 'GSMA, State of Mobile Internet Connectivity 2025' },
    ],
    note: 'And coverage is not availability: outages, disasters, borders, crowds and dead zones take the network away from everyone else, exactly when local coordination matters most.',
  },

  solution: {
    eyebrow: 'The solution',
    title: 'CabalMesh is a zero-identity wallet that settles offline transactions through the people around you.',
    steps: [
      { n: '01', icon: 'bolt', head: 'SIGN', body: 'Compose and sign on-device. No account, no server, no session.' },
      { n: '02', icon: 'network', head: 'RELAY', body: 'Nearby phones carry the encrypted payload over Bluetooth and mesh.' },
      { n: '03', icon: 'route', head: 'GATEWAY', body: 'The first peer with internet drains the queue toward the chain.' },
      { n: '04', icon: 'cube', head: 'SETTLE', body: 'Escrow confirms on Avalanche. The proof lands; the identity never does.' },
    ],
    why: [
      { head: 'IT ALREADY WORKS', body: 'The offline path is built and demonstrated end to end — 470 automated tests, escrow live on Fuji, verified with the radio off.' },
      { head: 'PRIVACY BY SUBTRACTION', body: 'No account, no server, no stored history. There is nothing to leak because nothing is kept — only a proof settles on-chain.' },
      { head: 'THE NETWORK IS THE MOAT', body: 'Every user is a relay, and relaying earns. Density is the product: the more users in a place, the better it works there.' },
    ],
  },

  market: {
    eyebrow: 'Market',
    title: 'A large, structural need — and two compounding markets already paying for pieces of it.',
    people: [
      ['2.2B', 'offline today', 'ITU 2025'],
      ['3.1B', 'in the usage gap', 'GSMA 2025'],
      ['300M', 'with no coverage', 'GSMA 2025'],
    ],
    markets: [
      { name: 'WIRELESS MESH NETWORKING', now: '$10.4B', then: '$15.8B', cagr: '8.8% CAGR', span: [0.32, 0.62], src: 'Mordor Intelligence, 2025 → 2030' },
      { name: 'CRYPTO HARDWARE WALLETS', now: '$0.6B', then: '$1.5B', cagr: '~26% CAGR', span: [0.06, 0.28], src: 'Grand View / Mordor, 2025 → 2030' },
    ],
    beachhead: {
      label: 'OUR BEACHHEAD',
      body: 'Avalanche builders, privacy-hardware buyers, and off-grid trade communities. We size it with the Phase 2 pre-order waitlist rather than asserting a number here — the waitlist is the KPI.',
    },
    note: 'Sources — ITU Measuring Digital Development: Facts and Figures 2025 · GSMA State of Mobile Internet Connectivity 2025 · Mordor Intelligence · Grand View Research.',
  },

  gtm: {
    eyebrow: 'Go to market · what we built',
    title: 'Start where the mesh is already dense, then sell the hardware that makes it denser.',
    built: {
      label: 'BUILT AND RUNNING TODAY',
      items: [
        'Offline signing, relay queue, auto-confirm on reconnect',
        'BLE offline plane on iOS and Android',
        'Escrow live on Avalanche Fuji — real deployed address',
        'Vault (AES-256-GCM) + guardian social recovery',
        '470 automated tests · desktop and mobile app shipping',
      ],
    },
    who: {
      label: 'WHO USES IT FIRST',
      items: [
        { n: '01', head: 'Avalanche builders & grant circles', body: 'They already see the demo and read the code. Cheapest audience to reach, hardest to fool.' },
        { n: '02', head: 'Privacy & crypto-hardware buyers', body: 'A category that already buys a physical security device — ShadowBox and the Nobody Box are pre-order units.' },
        { n: '03', head: 'Off-grid, disaster & local trade', body: 'The people the offline path was written for. Slowest to reach, highest retention once it works.' },
      ],
    },
    motion: 'MOTION — mesh missions verified on-chain (not just posts) → Kaito mindshare campaign → waitlist → pre-order. Every user is a relay, so growth densifies the network instead of only adding accounts.',
  },

  built: {
    eyebrow: 'What is built · what each piece is for',
    title: 'Three pieces are built. Each removes one reason people cannot transact.',
    cards: [
      {
        kind: 'image', image: path.join(__dirname, 'assets', 'confirm-offline-dialog.png'), ratio: 990 / 1175,
        status: 'LIVE', scolor: C.live,
        name: 'OFFLINE INTENT',
        forLabel: 'WHAT IT IS FOR',
        body: 'A payment that survives the moment the network does not: signed and queued with the radio off, then broadcast and settled after reconnection — no identity attached.',
      },
      {
        kind: 'flow',
        status: 'LIVE', scolor: C.live,
        name: 'GUARDIAN RECOVERY',
        forLabel: 'WHAT IT IS FOR',
        body: 'Losing the phone stops being fatal. Three of five guardians rebuild the vault over Bluetooth, each approving by hand — and no server ever holds a key.',
        steps: ['ENROL — 5 shares, none a key', 'REQUEST — over BLE, unlinkable', 'APPROVE — a human, every time', 'VETO — 24h window to block', 'RESTORE — 3 of 5 rebuild it'],
      },
      {
        kind: 'image', image: path.join(__dirname, '..', 'src', 'deck-two-boxes.png'), ratio: 1400 / 825,
        status: 'CONCEPT · PHASE 2', scolor: C.accent,
        name: 'THE TWO BOXES',
        forLabel: 'WHAT THEY ARE FOR',
        body: 'A node and a locker — the first things we can sell. The ShadowBox relays for the neighbourhood, runs the local model and generates the proof; the Nobody Box is escrow at the front door, a parcel locker whose bolt turns when the deal settles.',
      },
    ],
    proof: 'ALL OF IT CHECKABLE — 61 automated tests · escrow live on Fuji 0xCaFF53657191d75Aa4f5C2182210302656d8B392 · demo youtu.be/Z3ooub-mnCw · code github.com/kurodenjiro/Cabal-Mesh',
  },
  team: {
    eyebrow: 'Team',
    title: 'Who is building it — and how they decide what ships.',
    founder: {
      role: 'FOUNDER · ENGINEERING',
      name: 'kurodenjiro',
      body: 'Wrote the mesh, the BLE offline plane, the vault, guardian recovery and the on-chain settlement path — 74 commits in the first 30 days.',
      note: 'Core team of two to three. Full bios on request.',
    },
    principles: {
      label: 'HOW THIS TEAM WORKS',
      items: [
        { head: 'The audit ships with the pitch', body: 'A code-read status document lists what does not work, feature by feature, with the file to check.', src: 'docs/product-status.md' },
        { head: 'The model proposes, Rust signs', body: 'AI only produces validated fields. Rust still validates, previews and signs — the safety property holds by construction.', src: 'parse_intent_chat · commands.rs' },
        { head: 'One true story over three half-built ones', body: 'Controls that imply capability we do not have get hidden rather than demoed. The offline path is the one we stake the pitch on.', src: 'CabalMesh-Project-Plan.md §4' },
      ],
    },
    proof: 'Every claim in this deck points at a file, a contract address or a video. Nothing on these seven slides is a projection.',
  },
};

// --------------------------------------------------------------- helpers
function head(slide, d, o = {}) {
  const s = slide({});
  P.eyebrow(s, d.eyebrow);
  P.title(s, d.title, { size: o.size ?? 24, w: o.w ?? 11.2, h: o.h ?? 1.1 });
  return s;
}

// ------------------------------------------------------------------ slides
function cover(slide) {
  const d = D.cover;
  const s = slide({ chrome: false });
  P.ticks(s);
  s.addText('CABALMESH', { x: T.margin, y: 0.3, w: 4, h: 0.26, fontFace: F.sans, fontSize: 9, bold: true, color: C.white, charSpacing: 3 });
  s.addText(d.stamp, { x: 7.2, y: 0.3, w: 5.4, h: 0.26, align: 'right', fontFace: F.sans, fontSize: 9, bold: true, color: C.ink2, charSpacing: 3 });
  P.rule(s, 0.62, { color: C.line });

  P.image(s, asset('src', 'ds', 'assets', 'logo', 'hero-lockup.png'), 8.5, 1.2, 4.0, 5.0, 'contain');

  s.addText(d.eyebrow, { x: T.margin, y: 1.9, w: 7, h: 0.3, fontFace: F.sans, fontSize: 10, bold: true, color: C.accent, charSpacing: 3 });
  s.addText(d.name, { x: T.margin - 0.06, y: 2.35, w: 7.6, h: 1.3, fontFace: F.sans, fontSize: 62, bold: true, color: C.white });
  P.accentBar(s, T.margin, 3.86);
  s.addText(d.line, { x: T.margin, y: 4.2, w: 7.3, h: 1.5, fontFace: F.sans, fontSize: 13.5, color: C.ink1, lineSpacingMultiple: 1.42 });
  s.addText(d.footer, { x: T.margin, y: 6.85, w: 8, h: 0.26, fontFace: F.sans, fontSize: 8.5, bold: true, color: C.ink2, charSpacing: 2 });
}

function problem(slide) {
  const d = D.problem;
  const s = head(slide, d, { size: 23, w: 11.4 });
  const w = 3.724, gapX = 0.36, top = 3.0, h = 2.66;
  d.stats.forEach((st, i) => {
    const x = T.margin + i * (w + gapX);
    P.panel(s, x, top, w, h, { line: C.lineSoft });
    P.fill(s, x, top, w, 0.028, i === 0 ? C.warn : C.accent);
    s.addText(st.n, { x: x + 0.34, y: top + 0.34, w: w - 0.68, h: 0.86, fontFace: F.sans, fontSize: 44, bold: true, color: C.white });
    P.label(s, st.unit, x + 0.34, top + 1.24, { color: C.accent, size: 8.5, w: w - 0.68 });
    P.body(s, st.body, x + 0.34, top + 1.56, w - 0.68, { size: 10, color: C.ink1, h: 0.7 });
    P.code(s, st.src, x + 0.34, top + 2.3, w - 0.68, { size: 7.4, color: C.ink2 });
  });
  P.rule(s, 6.02, { color: C.lineSoft });
  P.body(s, d.note, T.margin, 6.22, 11.5, { size: 10.5, color: C.ink2, h: 0.5 });
}

function solution(slide) {
  const d = D.solution;
  const s = head(slide, d, { size: 23, w: 11.2 });

  const w = 2.72, gapX = 0.335, top = 2.62, h = 1.72;
  d.steps.forEach((st, i) => {
    const x = T.margin + i * (w + gapX);
    P.panel(s, x, top, w, h, { line: C.lineSoft });
    s.addText(st.n, { x: x + 0.3, y: top + 0.24, w: 0.7, h: 0.26, fontFace: F.mono, fontSize: 9.5, bold: true, color: C.accent });
    P.icon(s, st.icon, x + w - 0.72, top + 0.22, 0.34);
    P.body(s, st.head, x + 0.3, top + 0.6, w - 0.6, { size: 12.5, bold: true, color: C.white, h: 0.3 });
    P.body(s, st.body, x + 0.3, top + 0.94, w - 0.6, { size: 8.8, color: C.ink1, h: 0.66 });
    if (i < 3) s.addShape('line', { x: x + w + 0.06, y: top + 0.86, w: gapX - 0.12, h: 0, line: { color: C.ink3, width: 1 } });
  });

  const wy = 4.86, ww = 3.724;
  d.why.forEach((c, i) => {
    const x = T.margin + i * (ww + 0.36);
    P.fill(s, x, wy, 0.6, 0.022, C.accent);
    P.body(s, c.head, x, wy + 0.16, ww, { size: 12, bold: true, color: C.white, h: 0.3 });
    P.body(s, c.body, x, wy + 0.52, ww - 0.2, { size: 9.6, color: C.ink1, h: 1.0 });
  });

  P.rule(s, 6.5, { color: C.lineSoft });
  P.body(s, 'None of this loop is a roadmap item — it runs today on iOS, Android and macOS, and the demo is the app, not a mock.',
    T.margin, 6.68, 11.6, { size: 10, color: C.accent, h: 0.3 });
}

function market(slide) {
  const d = D.market;
  const s = head(slide, d, { size: 23, w: 11.4 });

  // band 1 — the people
  const top = 2.48;
  P.label(s, 'WHO NEEDS IT', T.margin, top, { color: C.ink3, size: 8.5 });
  P.rule(s, top + 0.22, { color: C.line });
  d.people.forEach((p, i) => {
    const x = T.margin + i * 2.35;
    s.addText(p[0], { x, y: top + 0.34, w: 1.6, h: 0.5, fontFace: F.sans, fontSize: 26, bold: true, color: C.white });
    P.body(s, p[1], x + 0.05, top + 0.8, 2.1, { size: 9.4, color: C.ink1, h: 0.22 });
    P.code(s, p[2], x + 0.05, top + 1.05, 2.1, { size: 7.2, color: C.ink2 });
  });
  // divider between people and money
  P.vrule(s, T.margin + 7.1, top + 0.3, 0.94, { color: C.lineSoft });

  P.label(s, 'WHY THE NEED IS URGENT', T.margin + 7.5, top + 0.34, { color: C.ink3, size: 8 });
  P.body(s, 'Coverage keeps growing while use does not — the gap is behaviour, price and trust, not towers. An offline-first rail does not wait for either curve.',
    T.margin + 7.5, top + 0.58, 4.3, { size: 9.4, color: C.ink1, h: 0.7 });

  // band 2 — the money
  const my = 4.06;
  P.label(s, 'MARKETS ALREADY BEING PAID FOR', T.margin, my, { color: C.ink3, size: 8.5 });
  P.rule(s, my + 0.22, { color: C.line });
  d.markets.forEach((m, i) => {
    const y = my + 0.42 + i * 0.86;
    P.body(s, m.name, T.margin, y, 3.5, { size: 10.5, bold: true, color: C.white, h: 0.26 });
    P.code(s, m.src, T.margin, y + 0.26, 3.6, { size: 7.2, color: C.ink2 });
    // 2025 → 2030 bar
    const bx = T.margin + 3.9, bw = 4.3;
    P.fill(s, bx, y + 0.1, bw * m.span[0], 0.2, i === 0 ? '00B8CE' : '2E8494');
    s.addShape('rect', { x: bx, y: y + 0.1, w: bw * m.span[1], h: 0.2, fill: { type: 'none' }, line: { color: C.ink3, width: 1 } });
    P.code(s, m.now, bx, y + 0.36, 1.2, { size: 8, color: C.white });
    P.code(s, m.then, bx + bw * m.span[1] - 1.2, y + 0.36, 1.2, { size: 8, color: C.ink1, align: 'right' });
    P.body(s, m.cagr, bx + bw + 0.26, y + 0.08, 1.4, { size: 10.5, bold: true, color: C.accent, h: 0.26 });
  });

  // beachhead
  P.panel(s, 10.7, my + 0.42, 1.9, 1.72, { fill: C.panelUp, line: C.line });
  P.label(s, d.beachhead.label, 10.88, my + 0.6, { color: C.accent, size: 7.5, w: 1.6 });
  P.body(s, d.beachhead.body, 10.88, my + 0.84, 1.56, { size: 7.6, color: C.ink1, h: 1.2 });

  P.rule(s, 6.34, { color: C.lineSoft });
  P.code(s, d.note, T.margin, 6.52, 11.6, { size: 7.6, color: C.ink2 });
}

function gtm(slide) {
  const d = D.gtm;
  const s = head(slide, d, { size: 23, w: 11.2 });

  const top = 2.6, lw = 4.4, rw = 7.15;
  P.panel(s, T.margin, top, lw, 3.5, { line: C.lineSoft });
  P.fill(s, T.margin, top, lw, 0.028, C.live);
  P.label(s, d.built.label, T.margin + 0.34, top + 0.32, { color: C.live, size: 8.5, w: lw - 0.68 });
  d.built.items.forEach((it, i) => {
    const y = top + 0.76 + i * 0.52;
    s.addShape('line', { x: T.margin + 0.34, y: y + 0.11, w: 0.14, h: 0, line: { color: C.ink3, width: 1 } });
    P.body(s, it, T.margin + 0.6, y, lw - 0.98, { size: 9.6, color: C.ink1, h: 0.46 });
  });

  const rx = T.margin + lw + 0.34;
  P.label(s, d.who.label, rx, top + 0.06, { color: C.accent, size: 8.5, w: rw });
  d.who.items.forEach((it, i) => {
    const y = top + 0.44 + i * 1.06;
    s.addText(it.n, { x: rx, y: y + 0.02, w: 0.5, h: 0.3, fontFace: F.mono, fontSize: 11, bold: true, color: C.ink3 });
    P.body(s, it.head, rx + 0.52, y, rw - 0.52, { size: 12.5, bold: true, color: C.white, h: 0.3 });
    P.body(s, it.body, rx + 0.52, y + 0.34, rw - 0.6, { size: 9.4, color: C.ink1, h: 0.5 });
    if (i < 2) P.rule(s, y + 0.94, { x: rx, w: rw, color: C.lineSoft });
  });

  P.rule(s, 6.3, { color: C.lineSoft });
  P.body(s, d.motion, T.margin, 6.48, 11.6, { size: 9.8, color: C.accent, h: 0.5 });
}

function built(slide) {
  const d = D.built;
  const s = head(slide, d, { size: 22, w: 11.5, h: 0.62 });

  const w = 3.724, gapX = 0.36, top = 2.06, h = 4.34, pad = 0.28;
  const imgTop = top + 0.02, imgH = 2.06;

  d.cards.forEach((c, i) => {
    const x = T.margin + i * (w + gapX);
    P.panel(s, x, top, w, h, { line: C.lineSoft });

    if (c.kind === 'image') {
      const iw = imgH * c.ratio;
      P.image(s, c.image, x + (w - iw) / 2, imgTop, iw, imgH, 'contain');
    }

    if (c.kind === 'flow') {
      const fx = x + pad + 0.12, fy = imgTop + 0.16, step = 0.4;
      s.addShape('line', { x: fx, y: fy + 0.06, w: 0, h: step * 4, line: { color: C.ink3, width: 1 } });
      c.steps.forEach((st, k) => {
        const y = fy + k * step;
        const on = k < 3 || k === 4;
        s.addShape('ellipse', {
          x: fx - 0.055, y: y + 0.005, w: 0.11, h: 0.11,
          fill: { color: on ? C.live : C.accent }, line: { type: 'none' },
        });
        P.body(s, st, fx + 0.24, y - 0.03, w - pad * 2 - 0.24, { size: 8.6, color: C.ink1, h: 0.26 });
      });
    }

    if (c.kind === 'devices') {
      const bw = w - 0.04, bh = (imgH - 0.06) / 2;
      P.image(s, c.images[0], x + 0.02, imgTop, bw, bh, 'cover');
      P.image(s, c.images[1], x + 0.02, imgTop + bh + 0.06, bw, bh, 'cover');
    }

    const ty = top + imgH + 0.12;
    P.chip(s, c.status, x + pad, ty, { color: c.scolor, w: c.status.length > 6 ? 1.8 : 0.8 });
    P.body(s, c.name, x + pad, ty + 0.38, w - pad * 2, { size: 15, bold: true, color: C.white, h: 0.36 });
    P.label(s, c.forLabel, x + pad, ty + 0.78, { color: c.scolor, size: 7.5, w: w - pad * 2 });
    P.body(s, c.body, x + pad, ty + 1.0, w - pad * 2, { size: 9.2, color: C.ink1, h: 1.1 });
  });

  P.rule(s, 6.6, { color: C.lineSoft });
  P.code(s, d.proof, T.margin, 6.78, 11.7, { size: 7.6, color: C.ink2 });
}

function team(slide) {
  const d = D.team;
  const s = head(slide, d, { size: 24, w: 11.2, h: 0.6 });

  const top = 2.2, lw = 4.4, h = 3.6;
  P.panel(s, T.margin, top, lw, h, { line: C.lineSoft });
  P.fill(s, T.margin, top, lw, 0.028, C.accent);
  P.label(s, d.founder.role, T.margin + 0.36, top + 0.36, { color: C.accent, size: 8, w: lw - 0.72 });
  P.body(s, d.founder.name, T.margin + 0.36, top + 0.66, lw - 0.72, { size: 20, bold: true, color: C.white, h: 0.44 });
  P.body(s, d.founder.body, T.margin + 0.36, top + 1.26, lw - 0.72, { size: 10, color: C.ink1, h: 1.2 });
  P.rule(s, top + 2.72, { x: T.margin + 0.36, w: lw - 0.72, color: C.lineSoft });
  P.body(s, d.founder.note, T.margin + 0.36, top + 2.9, lw - 0.72, { size: 9.2, color: C.ink2, h: 0.4 });

  const rx = T.margin + lw + 0.4, rw = T.contentW - lw - 0.4;
  P.label(s, d.principles.label, rx, top + 0.02, { color: C.ink3, size: 8.5, w: rw });
  d.principles.items.forEach((it, i) => {
    const y = top + 0.4 + i * 1.14;
    s.addText(String(i + 1).padStart(2, '0'), { x: rx, y: y + 0.02, w: 0.5, h: 0.28, fontFace: F.mono, fontSize: 10.5, bold: true, color: C.ink3 });
    P.body(s, it.head, rx + 0.52, y, rw - 0.52, { size: 12.5, bold: true, color: C.white, h: 0.3 });
    P.body(s, it.body, rx + 0.52, y + 0.34, rw - 0.6, { size: 9.4, color: C.ink1, h: 0.46 });
    P.code(s, it.src, rx + 0.52, y + 0.84, rw - 0.6, { size: 7.6, color: C.ink2 });
    if (i < 2) P.rule(s, y + 1.06, { x: rx, w: rw, color: C.lineSoft });
  });

  P.rule(s, 6.2, { color: C.lineSoft });
  P.body(s, d.proof, T.margin, 6.38, 11.6, { size: 9.8, color: C.accent, h: 0.4 });
}

// -------------------------------------------------------------------- build
function main() {
  const pres = new pptxgen();
  pres.defineLayout({ name: 'CM16x9', width: T.W, height: T.H });
  pres.layout = 'CM16x9';
  pres.author = 'CabalMesh';
  pres.company = 'CabalMesh';
  pres.title = 'CabalMesh — Pitch';

  const slide = createDeck(pres, 'Pitch · 2026');
  [cover, problem, solution, market, gtm, built, team].forEach((fn) => fn(slide));

  const out = path.join(ROOT, 'docs', 'pitch', 'CabalMesh-Pitch.pptx');
  return pres.writeFile({ fileName: out }).then(() => console.log('wrote', out));
}

main().catch((e) => { console.error(e); process.exit(1); });
