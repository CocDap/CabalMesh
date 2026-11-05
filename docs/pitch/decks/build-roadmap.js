// ============================================================================
// build-roadmap.js — renders docs/pitch/CabalMesh-Roadmap.pptx
//
// A strategy deck: what the product becomes, and how it reaches people.
// Engineering detail (files, tickets, contract functions) deliberately lives in
// docs/pitch/CabalMesh-Project-Plan.md, not here.
// ============================================================================
const pptxgen = require('pptxgenjs');
const path = require('path');
const { T, P, C, F, createDeck } = require('./theme');

const ROOT = path.join(__dirname, '..', '..', '..');
const asset = (...p) => path.join(ROOT, ...p);

const STATUS = {
  now: { text: 'IN PROGRESS', color: C.accent },
  next: { text: 'NEXT', color: C.ink1 },
  later: { text: 'LATER', color: C.ink2 },
};

const MONTHS = ['AUG 2026', 'SEP', 'OCT', 'NOV', 'DEC', 'JAN 2027'];
const BAR = ['00E5FF', '00B8CE', '2E8494', '3C6068', '3A3A3A'];

const PHASES = [
  { n: '01', move: 'TRUST', title: 'Safe enough to keep money in', window: 'AUG – SEP 2026', status: 'now', start: 0, span: 2 },
  { n: '02', move: 'DEMAND', title: 'Something people can hold and buy', window: 'SEP – OCT 2026', status: 'next', start: 1, span: 2 },
  { n: '03', move: 'ECONOMY', title: 'A network that pays its own users', window: 'OCT – NOV 2026', status: 'next', start: 2, span: 2 },
  { n: '04', move: 'DEPTH', title: 'Privacy that survives scrutiny', window: 'NOV – DEC 2026', status: 'later', start: 3, span: 2 },
  { n: '05', move: 'REACH', title: 'Every platform, every pocket', window: 'DEC 2026 – JAN 2027', status: 'later', start: 4, span: 2 },
];

// ---------------------------------------------------------------- content
const D = {
  cover: {
    stamp: 'ROADMAP',
    eyebrow: 'PRODUCT & GROWTH STRATEGY · AUG 2026 → JAN 2027',
    title: 'Ship trust.\nSell hardware.\nLet the network\npay for itself.',
    sub: 'Three moves in six months, in the order that makes each one possible. The product earns the right to be kept, then the right to be bought, then the right to grow on its own.',
    footer: 'CABALMESH · AVALANCHE · 2026',
  },

  today: {
    eyebrow: 'Where we stand',
    title: 'One thing already works that nothing else in the category does.',
    lead: 'A payment can be signed with no network, carried by the people nearby, and settled the moment anyone finds a signal. That is the wedge. The roadmap exists to close the three gaps around it.',
    cards: [
      { label: 'PRODUCT', head: 'The wedge is real', body: 'Offline signing, mesh relay and on-chain settlement work end to end today, on phones and desktop.', tone: C.live },
      { label: 'MARKET', head: 'Nobody knows yet', body: 'No audience, no waitlist, no distribution. The best product in the category is worth nothing unheard.', tone: C.accent },
      { label: 'MONEY', head: 'Nothing to buy', body: 'No hardware to sell, no in-app economy. Usage cannot yet turn into revenue or into more nodes.', tone: C.warn },
    ],
  },

  strategy: {
    eyebrow: 'The strategy',
    title: 'Three moves, each one unlocking the next.',
    moves: [
      { n: '01', name: 'TRUST', user: 'People stop being one lost phone away from losing everything.', biz: 'Removes the objection that kills every self-custody demo, and makes a public launch defensible.' },
      { n: '02', name: 'DEMAND', user: 'The idea becomes something you can see, hold and pre-order.', biz: 'Creates the first audience and the first revenue line, and gives the campaign a physical hook.' },
      { n: '03', name: 'ECONOMY', user: 'Relaying for others earns, and earnings buy upgrades that earn more.', biz: 'Turns growth into node density instead of vanity signups — the network gets better as it gets bigger.' },
    ],
    note: 'Phases 4 and 5 deepen the moat once the loop above is running: privacy that survives scrutiny, and reach on every platform.',
  },

  timeline: {
    eyebrow: 'Timeline',
    title: 'Six months, five phases, one dependency chain.',
    note: 'Windows are execution ranges, not deadlines. Phase 2 is a marketing and business track and deliberately overlaps the software work rather than blocking it.',
  },

  p1: {
    lead: 'Nobody keeps real money in a wallet that dies with the phone. This phase makes the wallet survivable, so every later phase has something worth marketing.',
    user: {
      label: 'WHAT CHANGES FOR THE USER',
      items: [
        'Friends and family can restore your wallet — three of five, in person',
        'A stolen phone can be blocked during a delay window',
        'Keys can leave the app: export, import, restore, passphrase-protected',
        'You can type what you want instead of filling a form',
      ],
    },
    biz: {
      label: 'WHY IT MATTERS COMMERCIALLY',
      items: [
        'Removes the single objection that ends most self-custody conversations',
        'Makes a public launch defensible — losing funds is a story we cannot afford',
        'Recovery is a social act, so onboarding one user tends to bring in five',
      ],
    },
    milestone: 'MILESTONE — a wiped phone walks back into its wallet with help from five people, and a spoken sentence becomes a signed intent.',
  },

  p2: {
    lead: 'A mesh is invisible, and invisible things do not sell. Two devices turn the idea into something a person can point at — and into the first thing we can take money for.',
    products: [
      { image: 'deck-shadowbox.png', tag: 'MESH YOU CAN HOLD', name: 'SHADOWBOX', body: 'One node: it relays for the neighbourhood, runs the model that reads your sentence, and proves you can afford something without saying what you hold.' },
      { image: 'deck-nobody.png', tag: 'ESCROW YOU CAN TOUCH', name: 'THE NOBODY BOX', body: 'A parcel locker whose bolt turns on the on-chain release. The seller drops it in, the buyer opens it, and nobody in between ever could.' },
    ],
    note: 'Concept renders and a pre-order page do not wait on manufacturing. Demand is validated first, tooling second — the waitlist decides whether a batch is ever produced.',
    milestone: 'MILESTONE — both devices have finished renders, a pre-order page is live, and the first campaign has reached an audience beyond our own testers.',
  },

  marketing: {
    eyebrow: 'Marketing plan',
    title: 'Earn attention from people who can check the claim.',
    audiences: {
      label: 'WHO WE TALK TO, IN ORDER',
      items: [
        'Avalanche builders and ecosystem circles — they read code, so proof travels fast',
        'Privacy and crypto-hardware buyers — a category that already buys a physical device',
        'Off-grid, disaster and local-trade communities — most loyal once it works',
      ],
    },
    channels: {
      label: 'WHERE',
      items: ['Ecosystem channels and demo days', 'A hands-on video, not a pitch video', 'Community launch posts and AMAs', 'Kaito mindshare campaign'],
    },
    missions: {
      label: 'THE CAMPAIGN — MESH MISSIONS',
      body: 'Rewards go to people who actually use the thing: broadcast an intent offline, relay traffic as a gateway, complete a guardian recovery. Posting counts too, but usage is what the chain can verify. Crossing the threshold mints a non-tradable Genesis Node badge.',
    },
    weeks: [
      ['W 1–2', 'Renders finished, landing copy written'],
      ['W 3–4', 'Teaser, waitlist opens, campaign listed'],
      ['W 5–6', 'Launch push, demo video, missions live'],
      ['W 7–8', 'Pre-order opens, badges minted'],
    ],
  },

  p3: {
    lead: 'Growth only helps a mesh if new users become new nodes. The in-app economy makes relaying for other people the most profitable thing a user can do.',
    loop: [
      { n: '01', head: 'RELAY', body: 'Carry other people’s traffic as a gateway.' },
      { n: '02', head: 'EARN', body: 'Relayed volume converts into on-chain rewards.' },
      { n: '03', head: 'UPGRADE', body: 'Rewards buy modules that raise your relay yield.' },
      { n: '04', head: 'DENSIFY', body: 'Better yield means more relays, in more places.' },
    ],
    points: [
      'Rewards are paid by the senders who use the network, not printed by a treasury',
      'A module is owned on-chain, so its effect can be checked rather than trusted',
      'Marketplace listings are escrow-backed: value moves only when the item does',
    ],
    milestone: 'MILESTONE — a user earns from real relaying, converts it, buys an upgrade, and watches their own yield change.',
  },

  depth: {
    eyebrow: 'Phases 04 – 05 · The moat',
    title: 'Once the loop runs, deepen what cannot be copied quickly.',
    cards: [
      { n: '04', name: 'PRIVACY THAT SURVIVES SCRUTINY', body: 'Proofs verified against a real circuit, and agents that negotiate under limits enforced by the app rather than trusted to a model. The privacy claim becomes auditable instead of aspirational.', when: 'NOV – DEC 2026' },
      { n: '05', name: 'REACH ON EVERY PLATFORM', body: 'Hardware-backed key storage everywhere, and a decided answer for the desktop platforms where the offline radio plane does not run today. Research on keeping negotiation content private even from the nodes carrying it.', when: 'DEC 2026 – JAN 2027' },
    ],
    note: 'These two phases are deliberately last: they make the product harder to copy, but they do not make it easier to adopt.',
  },

  measure: {
    eyebrow: 'What we measure',
    title: 'Five numbers decide whether a phase actually landed.',
    head: ['METRIC', 'PHASE', 'THE DECISION IT DRIVES'],
    rows: [
      ['Recoveries completed by real users', 'P1', 'Whether the wallet is safe enough to market publicly'],
      ['Waitlist signups', 'P2', 'Whether a hardware batch is worth producing, and how large'],
      ['Reach beyond our own channels', 'P2', 'Whether the positioning lands, or the message needs rewriting'],
      ['Active relays per area', 'P3', 'Whether growth is producing nodes or only accounts'],
      ['Repeat settled intents per user', 'P3', 'Whether the product is used, or only tried once'],
    ],
    risks: 'WATCHING — no manufacturing partner yet (Phase 2 runs on renders and pre-orders until demand is proven) · device-level testing needed before Phase 1 is called done · the in-app economy launches on testnet first.',
  },

  close: {
    title: 'Trust, then demand,\nthen an economy.\nIn that order.',
    lines: [
      ['NOW', 'Phase 1 — make the wallet survivable'],
      ['NEXT', 'Phase 2 — devices, waitlist, first campaign'],
      ['THEN', 'Phase 3 — relaying that pays for itself'],
      ['DETAIL', 'Full execution plan available on request'],
    ],
    footer: 'A LIVING PLAN. WHAT SHIPS IS DECIDED BY WHAT THE LAST PHASE PROVED.',
  },
};

// --------------------------------------------------------------- helpers
function head(slide, d, o = {}) {
  const s = slide({});
  P.eyebrow(s, d.eyebrow);
  P.title(s, d.title, { size: o.size ?? T.size.title, w: o.w ?? 10.8, h: o.h ?? 1.0 });
  if (d.lead) P.lead(s, d.lead, { y: o.leadY ?? 2.06, w: o.leadW ?? 10.2, h: 0.7 });
  return s;
}

function phaseHead(slide, idx, lead) {
  const p = PHASES[idx];
  const s = slide({});
  P.eyebrow(s, `PHASE ${p.n} · ${p.move} · ${p.window}`);
  P.title(s, p.title, { w: 9.6, h: 0.8 });
  const st = STATUS[p.status];
  P.chip(s, st.text, 11.4, 0.88, { color: st.color, w: 1.2 });
  P.lead(s, lead, { y: 2.02, w: 10.6, h: 0.7 });
  return s;
}

function column(s, x, y, w, h, label, color, items, size = 9.8, step = 0.44) {
  P.panel(s, x, y, w, h, { line: C.lineSoft });
  P.fill(s, x, y, w, 0.028, color);
  P.label(s, label, x + 0.36, y + 0.32, { color, size: 8.5, w: w - 0.72 });
  items.forEach((it, k) => {
    const iy = y + 0.76 + k * step;
    s.addShape('line', { x: x + 0.36, y: iy + 0.11, w: 0.13, h: 0, line: { color: C.ink3, width: 1 } });
    P.body(s, it, x + 0.62, iy, w - 1.0, { size, color: C.ink1, h: step - 0.04 });
  });
}

function milestone(s, y, text) {
  P.panel(s, T.margin, y, T.contentW, 0.72, { fill: C.panelUp, line: C.line });
  P.body(s, text, T.margin + 0.32, y + 0.14, T.contentW - 0.64, { size: 9.8, color: C.white, h: 0.48 });
}

// ------------------------------------------------------------------ slides
function cover(slide) {
  const d = D.cover;
  const s = slide({ chrome: false });
  P.ticks(s);
  s.addText('CABALMESH', { x: T.margin, y: 0.3, w: 4, h: 0.26, fontFace: F.sans, fontSize: 9, bold: true, color: C.white, charSpacing: 3 });
  s.addText(d.stamp, { x: 7.2, y: 0.3, w: 5.4, h: 0.26, align: 'right', fontFace: F.sans, fontSize: 9, bold: true, color: C.ink2, charSpacing: 3 });
  P.rule(s, 0.62, { color: C.line });

  P.image(s, asset('src', 'ds', 'assets', 'logo', 'oracle-emblem.png'), 8.7, 1.5, 3.8, 4.1, 'contain');
  s.addText(d.eyebrow, { x: T.margin, y: 1.35, w: 8, h: 0.28, fontFace: F.sans, fontSize: 10, bold: true, color: C.accent, charSpacing: 2.4 });
  s.addText(d.title, { x: T.margin - 0.04, y: 1.86, w: 8, h: 3.1, fontFace: F.sans, fontSize: 40, bold: true, color: C.white, lineSpacingMultiple: 1.06 });
  P.accentBar(s, T.margin, 5.14);
  s.addText(d.sub, { x: T.margin, y: 5.42, w: 7.4, h: 1.1, fontFace: F.sans, fontSize: 12, color: C.ink1, lineSpacingMultiple: 1.4 });
  s.addText(d.footer, { x: T.margin, y: 6.9, w: 7, h: 0.26, fontFace: F.sans, fontSize: 8.5, bold: true, color: C.ink2, charSpacing: 2.2 });
}

function today(slide) {
  const d = D.today;
  const s = head(slide, d, { size: 26 });
  const w = 3.724, gapX = 0.36, top = 3.06, h = 2.9;
  d.cards.forEach((c, i) => {
    const x = T.margin + i * (w + gapX);
    P.panel(s, x, top, w, h, { line: C.lineSoft });
    P.fill(s, x, top, w, 0.028, c.tone);
    P.label(s, c.label, x + 0.36, top + 0.34, { color: c.tone, size: 8.5, w: w - 0.72 });
    P.body(s, c.head, x + 0.36, top + 0.68, w - 0.72, { size: 15, bold: true, color: C.white, h: 0.5 });
    P.body(s, c.body, x + 0.36, top + 1.32, w - 0.72, { size: 10, color: C.ink1, h: 1.2 });
  });
  P.rule(s, 6.24, { color: C.lineSoft });
  P.body(s, 'The order of the roadmap follows the order of these gaps: make it safe, make it known, make it pay.',
    T.margin, 6.44, 11.5, { size: 10.5, color: C.accent, h: 0.3 });
}

function strategy(slide) {
  const d = D.strategy;
  const s = head(slide, d, { size: 27 });
  const top = 2.72, rowH = 1.24;
  d.moves.forEach((m, i) => {
    const y = top + i * rowH;
    P.panel(s, T.margin, y, T.contentW, 1.1, { line: C.lineSoft });
    P.fill(s, T.margin, y, 0.028, 1.1, C.accent);
    s.addText(m.n, { x: T.margin + 0.34, y: y + 0.3, w: 0.6, h: 0.5, fontFace: F.mono, fontSize: 15, bold: true, color: C.ink3 });
    P.body(s, m.name, T.margin + 1.05, y + 0.36, 1.9, { size: 16, bold: true, color: C.white, h: 0.4 });
    P.label(s, 'FOR THE USER', T.margin + 3.1, y + 0.2, { color: C.ink3, size: 7.5, w: 3 });
    P.body(s, m.user, T.margin + 3.1, y + 0.42, 3.7, { size: 9.4, color: C.ink1, h: 0.5 });
    P.label(s, 'FOR THE BUSINESS', T.margin + 7.1, y + 0.2, { color: C.ink3, size: 7.5, w: 3 });
    P.body(s, m.biz, T.margin + 7.1, y + 0.42, 4.5, { size: 9.4, color: C.ink1, h: 0.5 });
  });
  P.rule(s, 6.5, { color: C.lineSoft });
  P.body(s, d.note, T.margin, 6.68, 11.5, { size: 10, color: C.ink2, h: 0.3 });
}

function timeline(slide) {
  const d = D.timeline;
  const s = slide({});
  P.eyebrow(s, d.eyebrow);
  P.title(s, d.title, { w: 10.6 });

  const gridX = 4.55, gridW = T.W - T.margin - gridX;
  const colW = gridW / MONTHS.length;
  const top = 2.66, rowH = 0.78;

  MONTHS.forEach((m, i) => {
    P.label(s, m, gridX + i * colW + 0.06, top - 0.42, { color: C.ink3, w: colW, size: 8 });
    if (i > 0) P.vrule(s, gridX + i * colW, top - 0.16, rowH * PHASES.length + 0.1, { color: C.lineSoft });
  });
  P.rule(s, top - 0.16, { color: C.line });

  PHASES.forEach((p, i) => {
    const y = top + i * rowH;
    s.addText(p.n, { x: T.margin, y: y + 0.04, w: 0.42, h: 0.3, fontFace: F.mono, fontSize: 11, bold: true, color: C.ink3 });
    P.body(s, p.move, T.margin + 0.5, y + 0.04, 1.3, { size: 10, bold: true, color: C.white, h: 0.28 });
    P.body(s, p.title, T.margin + 1.75, y + 0.05, 2.7, { size: 9.2, color: C.ink2, h: 0.28 });
    s.addShape('rect', {
      x: gridX + p.start * colW + 0.04, y: y + 0.06, w: p.span * colW - 0.16, h: 0.3,
      fill: { color: BAR[i] }, line: { type: 'none' },
    });
    const st = STATUS[p.status];
    P.label(s, st.text, gridX + p.start * colW + 0.04, y + 0.44, { color: st.color, size: 7.5, w: 2.4 });
    if (i < PHASES.length - 1) P.rule(s, y + rowH - 0.1, { color: C.lineSoft });
  });

  P.rule(s, top + PHASES.length * rowH - 0.06, { color: C.line });
  P.body(s, d.note, T.margin, top + PHASES.length * rowH + 0.14, 11.5, { size: 10, color: C.ink2, h: 0.4 });
}

function phase1(slide) {
  const d = D.p1;
  const s = phaseHead(slide, 0, d.lead);
  column(s, T.margin, 3.0, 5.77, 2.76, d.user.label, C.live, d.user.items, 9.6, 0.48);
  column(s, T.margin + 6.11, 3.0, 5.77, 2.76, d.biz.label, C.accent, d.biz.items, 9.6, 0.52);
  milestone(s, 6.08, d.milestone);
}

function phase2(slide) {
  const d = D.p2;
  const s = phaseHead(slide, 1, d.lead);
  const cardW = 5.77, top = 2.96, cardH = 3.0, imgH = 1.56;
  d.products.forEach((p, i) => {
    const x = T.margin + i * (cardW + 0.34);
    P.panel(s, x, top, cardW, cardH, { line: C.lineSoft });
    P.image(s, path.join(__dirname, '..', 'src', p.image), x + 0.02, top + 0.02, cardW - 0.04, imgH, 'cover');
    P.label(s, p.tag, x + 0.34, top + imgH + 0.2, { color: C.accent, size: 8, w: cardW - 0.68 });
    P.body(s, p.name, x + 0.34, top + imgH + 0.44, cardW - 0.68, { size: 16, bold: true, color: C.white, h: 0.34 });
    P.body(s, p.body, x + 0.34, top + imgH + 0.82, cardW - 0.68, { size: 9.6, color: C.ink1, h: 0.6 });
  });
  P.body(s, d.note, T.margin, 6.12, 11.5, { size: 9.8, color: C.ink2, h: 0.4 });
  milestone(s, 6.5, d.milestone);
}

function marketing(slide) {
  const d = D.marketing;
  const s = head(slide, d, { size: 26 });
  const top = 2.6;

  column(s, T.margin, top, 6.6, 2.14, d.audiences.label, C.accent, d.audiences.items, 9.4, 0.44);
  column(s, T.margin + 6.94, top, 4.94, 2.14, d.channels.label, C.ink2, d.channels.items, 9.4, 0.34);

  const my = top + 2.38;
  P.panel(s, T.margin, my, 6.6, 1.56, { fill: C.panelUp, line: C.line });
  P.label(s, d.missions.label, T.margin + 0.34, my + 0.22, { color: C.live, size: 8.5, w: 6 });
  P.body(s, d.missions.body, T.margin + 0.34, my + 0.5, 5.95, { size: 9.4, color: C.ink1, h: 1.0 });

  P.label(s, 'CAMPAIGN — EIGHT WEEKS', T.margin + 6.94, my + 0.02, { color: C.ink3, size: 8.5, w: 5 });
  d.weeks.forEach((r, i) => {
    const y = my + 0.32 + i * 0.34;
    P.code(s, r[0], T.margin + 6.94, y, 0.8, { size: 8.5, color: C.accent });
    P.body(s, r[1], T.margin + 7.86, y - 0.02, 4.0, { size: 9.4, color: C.ink1, h: 0.3 });
  });

  P.rule(s, 6.62, { color: C.lineSoft });
  P.body(s, 'The campaign only rewards what the network itself can verify — usage first, attention second.',
    T.margin, 6.78, 11.5, { size: 9.8, color: C.ink2, h: 0.3 });
}

function phase3(slide) {
  const d = D.p3;
  const s = phaseHead(slide, 2, d.lead);
  const w = 2.72, gapX = 0.335, top = 2.94, h = 1.5;
  d.loop.forEach((st, i) => {
    const x = T.margin + i * (w + gapX);
    P.panel(s, x, top, w, h, { line: C.lineSoft });
    s.addText(st.n, { x: x + 0.3, y: top + 0.22, w: 0.7, h: 0.26, fontFace: F.mono, fontSize: 9.5, bold: true, color: C.accent });
    P.body(s, st.head, x + 0.3, top + 0.56, w - 0.6, { size: 13, bold: true, color: C.white, h: 0.3 });
    P.body(s, st.body, x + 0.3, top + 0.92, w - 0.6, { size: 9, color: C.ink1, h: 0.5 });
    if (i < 3) s.addShape('line', { x: x + w + 0.06, y: top + 0.72, w: gapX - 0.12, h: 0, line: { color: C.ink3, width: 1 } });
  });
  d.points.forEach((pt, i) => {
    const y = 4.72 + i * 0.42;
    s.addShape('line', { x: T.margin, y: y + 0.11, w: 0.13, h: 0, line: { color: C.ink3, width: 1 } });
    P.body(s, pt, T.margin + 0.26, y, 11.3, { size: 9.8, color: C.ink1, h: 0.36 });
  });
  milestone(s, 6.16, d.milestone);
}

function depth(slide) {
  const d = D.depth;
  const s = head(slide, d, { size: 26 });
  const w = 5.77, top = 2.7, h = 3.1;
  d.cards.forEach((c, i) => {
    const x = T.margin + i * (w + 0.34);
    P.panel(s, x, top, w, h, { line: C.lineSoft });
    s.addText(c.n, { x: x + 0.36, y: top + 0.3, w: 0.8, h: 0.5, fontFace: F.mono, fontSize: 18, bold: true, color: C.panelUp === C.panelUp ? C.ink3 : C.ink3 });
    P.label(s, c.when, x + w - 2.3, top + 0.36, { color: C.ink2, size: 8, w: 2, align: 'right' });
    P.body(s, c.name, x + 0.36, top + 0.9, w - 0.72, { size: 14, bold: true, color: C.white, h: 0.6 });
    P.body(s, c.body, x + 0.36, top + 1.6, w - 0.72, { size: 9.8, color: C.ink1, h: 1.3 });
  });
  P.rule(s, 6.1, { color: C.lineSoft });
  P.body(s, d.note, T.margin, 6.28, 11.5, { size: 10, color: C.accent, h: 0.3 });
}

function measure(slide) {
  const d = D.measure;
  const s = head(slide, d, { size: 26 });
  const top = 2.8, rowH = 0.62;
  const cols = [T.margin + 0.16, 5.5, 6.5];
  const widths = [5.0, 0.8, 5.9];
  d.head.forEach((h, i) => P.label(s, h, cols[i], top - 0.3, { color: C.ink3, w: widths[i] }));
  P.rule(s, top - 0.06, { color: C.line });
  d.rows.forEach((r, i) => {
    const y = top + i * rowH;
    if (i % 2 === 1) P.fill(s, T.margin, y, T.contentW, rowH - 0.04, C.panel);
    P.body(s, r[0], cols[0], y + 0.18, widths[0], { size: 10.5, bold: true, color: C.white, h: 0.34 });
    P.code(s, r[1], cols[1], y + 0.21, widths[1], { size: 9, color: C.accent });
    P.body(s, r[2], cols[2], y + 0.18, widths[2], { size: 9.8, color: C.ink1, h: 0.34 });
  });
  P.rule(s, top + d.rows.length * rowH + 0.02, { color: C.line });
  P.body(s, d.risks, T.margin, top + d.rows.length * rowH + 0.22, 11.6, { size: 9.4, color: C.ink2, h: 0.5 });
}

function close(slide) {
  const d = D.close;
  const s = slide({ chrome: false });
  P.ticks(s);
  P.mesh(s, [
    [9.0, 1.9, 10.4, 2.7], [10.4, 2.7, 11.8, 2.1], [9.0, 1.9, 8.6, 3.3],
    [8.6, 3.3, 10.4, 2.7], [8.6, 3.3, 9.2, 4.9], [9.2, 4.9, 11.1, 5.4], [10.4, 2.7, 11.6, 4.0],
  ]);
  s.addText('CABALMESH', { x: T.margin, y: 0.3, w: 4, h: 0.26, fontFace: F.sans, fontSize: 9, bold: true, color: C.white, charSpacing: 3 });
  s.addText('ROADMAP', { x: 7.2, y: 0.3, w: 5.4, h: 0.26, align: 'right', fontFace: F.sans, fontSize: 9, bold: true, color: C.ink2, charSpacing: 3 });
  P.rule(s, 0.62, { color: C.line });

  s.addText(d.title, { x: T.margin - 0.04, y: 2.1, w: 8.2, h: 2.4, fontFace: F.sans, fontSize: 38, bold: true, color: C.white, lineSpacingMultiple: 1.08 });
  P.accentBar(s, T.margin, 4.86);
  d.lines.forEach((l, i) => {
    const y = 5.14 + i * 0.4;
    P.label(s, l[0], T.margin, y + 0.03, { w: 1.4, color: C.ink3 });
    P.body(s, l[1], T.margin + 1.5, y, 6.6, { size: 10.5, color: C.ink1, h: 0.3 });
  });
  s.addText(d.footer, { x: 5.4, y: 6.85, w: 7.2, h: 0.26, align: 'right', fontFace: F.sans, fontSize: 8, bold: true, color: C.ink3, charSpacing: 1.6 });
}

// -------------------------------------------------------------------- build
function main() {
  const pres = new pptxgen();
  pres.defineLayout({ name: 'CM16x9', width: T.W, height: T.H });
  pres.layout = 'CM16x9';
  pres.author = 'CabalMesh';
  pres.company = 'CabalMesh';
  pres.title = 'CabalMesh — Product & Growth Roadmap';

  const slide = createDeck(pres, 'Roadmap · Aug 2026 – Jan 2027');
  [cover, today, strategy, timeline, phase1, phase2, marketing, phase3, depth, measure, close]
    .forEach((fn) => fn(slide));

  const out = path.join(ROOT, 'docs', 'pitch', 'CabalMesh-Roadmap.pptx');
  return pres.writeFile({ fileName: out }).then(() => console.log('wrote', out));
}

main().catch((e) => { console.error(e); process.exit(1); });
