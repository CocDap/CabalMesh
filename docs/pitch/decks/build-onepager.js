// ============================================================================
// build-onepager.js — renders docs/pitch/CabalMesh-Project-Plan-1page.pdf
//
// One A4 portrait page: baseline, timeline, five phases, risks, next 90 days.
// Same tokens as the decks (theme.js), but its own geometry — a page is not a
// slide, so it does not borrow the slide chrome.
// ============================================================================
const pptxgen = require('pptxgenjs');
const path = require('path');
const { C, F } = require('./theme');

const ROOT = path.join(__dirname, '..', '..', '..');
const W = 8.27, H = 11.69, M = 0.46;
const CW = W - M * 2;

// ---------------------------------------------------------------- content
const META = [
  ['HORIZON', 'Aug 2026 → Jan 2027'],
  ['OWNER', 'kurodenjiro'],
  ['BASELINE', 'product-status.md'],
  ['REVISED', '2026-08-19'],
];

const BASELINE = [
  { label: 'WORKS END TO END', color: C.live, items: ['Mesh — libp2p, mDNS, gossipsub', 'BLE offline plane — iOS, Android', 'Intent lifecycle, compose to proof', 'Escrow live on Avalanche Fuji', 'Offline signing + relay queue', 'Vault — AES-256-GCM', 'Guardian mesh unlock'] },
  { label: 'UI EXISTS, BEHAVIOUR DOES NOT', color: C.accent, items: ['PRIVACY — parsed, no routing code', 'MODE — labels, no strategy differs', 'SWAP / STAKE — every path is Escrow', 'USDC · WETH · BTC.b — never funded'] },
  { label: 'NOT WIRED UP AT ALL', color: C.warn, items: ['AI negotiation — called nowhere', 'Marketplace — contracts inert', 'ZK — no proving code in the build', 'FHE / MPC — no code exists', 'Recovery delay + veto', 'Mobile PIN — blocked on ticket 21'] },
];

const MONTHS = ['AUG 26', 'SEP', 'OCT', 'NOV', 'DEC', 'JAN 27'];
const BAR = ['00E5FF', '00B8CE', '2E8494', '3C6068', '3A3A3A'];

const PHASES = [
  { n: '01', title: 'Identity, recovery & AI intent parsing', window: 'AUG – SEP 2026', status: 'IN PROGRESS', scolor: C.accent, start: 0, span: 2,
    goal: 'Close the lose-the-device risk; turn typed language back into a validated intent draft.',
    done: 'A wiped device restores from 3-of-5 guardians with a live 24h veto window on real hardware, and a typed intent parses into editable chips.' },
  { n: '02', title: 'Marketing, sales & hardware devices', window: 'SEP – OCT 2026', status: 'NEXT', scolor: C.ink1, start: 1, span: 2,
    goal: 'ShadowBox and the Nobody Box as concept products, plus a pre-order channel. Marketing/BD, not firmware.',
    done: 'Both devices have finished renders, a pre-order page is live, and one campaign reaches beyond current testers.' },
  { n: '03', title: 'Marketplace goes live', window: 'OCT – NOV 2026', status: 'NEXT', scolor: C.ink1, start: 2, span: 2,
    goal: 'Relay traffic → MB → AVAX → modules → higher relay yield. Fix mintVoucher access control first.',
    done: 'A user earns MB from real gateway relaying, converts it to AVAX, and equips a module that visibly changes relay yield on HOME.' },
  { n: '04', title: 'Harden verification & negotiation', window: 'NOV – DEC 2026', status: 'LATER', scolor: C.ink2, start: 3, span: 2,
    goal: 'Make the ZK and AI claims true end to end: write the circuit and the proving path, and enforce guardrails in Rust.',
    done: 'A bid’s proof is generated and verified against a real circuit from a command the app calls; two agents complete a bounded negotiation without breaching a guardrail.' },
  { n: '05', title: 'Confidential compute & platform hardening', window: 'DEC 2026 – JAN 2027', status: 'EXPLORATORY', scolor: C.ink3, start: 4, span: 2,
    goal: 'FHE/MPC feasibility, desktop key store, and the Windows/Linux BLE decision.',
    done: 'A feasibility write-up and a scoped follow-on plan for FHE/MPC — not shipped code.' },
];

const RISKS = [
  ['No physical iOS/Android test device', 'P1', 'Borrow test hardware before Phase 1 is called done.'],
  ['Native key-store plugin (ticket 21)', 'P1', 'Own workstream; a PIN needs a hardware retry counter.'],
  ['No manufacturing partner', 'P2', 'Phase 2 runs as marketing/BD; renders do not need it.'],
  ['Voucher redeploy is one-way', 'P3', 'Explicit go/no-go. Testnet, no value locked today.'],
  ['No ZK code, no nargo in CI', 'P4', 'The unused stub was deleted; budget Phase 4 to write the circuit, not wire one.'],
];

const NEXT = [
  ['30 DAYS', ['24h recovery delay + veto notification', 'Scope ticket 21 as its own workstream', 'Hide SWAP / STAKE and unfunded assets']],
  ['60 DAYS', ['Ship editable chips, QA the chat-intent flow', 'Recovery assistant for lost devices', 'Finish renders, draft landing-page copy']],
  ['90 DAYS', ['Launch pre-order page + first campaign', 'Run mesh missions, mint Genesis badges', 'Draft the voucher redeploy go/no-go']],
];

// ------------------------------------------------------------- primitives
const txt = (s, t, x, y, w, o = {}) => s.addText(t, {
  x, y, w, h: o.h ?? 0.16,
  fontFace: o.mono ? F.mono : F.sans, fontSize: o.size ?? 7.4,
  color: o.color ?? C.ink1, bold: o.bold ?? false, italic: o.italic ?? false,
  align: o.align ?? 'left', valign: o.valign ?? 'top',
  charSpacing: o.cs ?? 0, lineSpacingMultiple: o.ls ?? 1.22,
});
const rule = (s, y, o = {}) => s.addShape('line', {
  x: o.x ?? M, y, w: o.w ?? CW, h: 0, line: { color: o.color ?? C.lineSoft, width: 1 },
});
const fill = (s, x, y, w, h, color) => s.addShape('rect', { x, y, w, h, fill: { color }, line: { type: 'none' } });
const section = (s, label, y) => {
  fill(s, M, y + 0.045, 0.09, 0.09, C.accent);
  txt(s, label.toUpperCase(), M + 0.19, y, 5, { size: 7.5, bold: true, color: C.white, cs: 1.8 });
  rule(s, y + 0.2, { color: C.line });
};

// ------------------------------------------------------------------- page
function main() {
  const pres = new pptxgen();
  pres.defineLayout({ name: 'A4P', width: W, height: H });
  pres.layout = 'A4P';
  pres.author = 'CabalMesh';
  pres.title = 'CabalMesh — Project Plan (one page)';

  const s = pres.addSlide();
  s.background = { color: C.void };

  // registration ticks
  const a = 0.12, m = 0.22;
  [[m, m, 1, 1], [W - m, m, -1, 1], [m, H - m, 1, -1], [W - m, H - m, -1, -1]].forEach(([x, y, dx, dy]) => {
    s.addShape('line', { x, y, w: a * dx, h: 0, line: { color: C.ink3, width: 1 } });
    s.addShape('line', { x, y, w: 0, h: a * dy, line: { color: C.ink3, width: 1 } });
  });

  // ---- header
  txt(s, 'CABALMESH', M, 0.42, 3, { size: 8, bold: true, color: C.white, cs: 2.6 });
  txt(s, 'PROJECT PLAN · ONE PAGE', W - M - 3.4, 0.42, 3.4, { size: 8, bold: true, color: C.ink2, cs: 2.2, align: 'right' });
  rule(s, 0.62, { color: C.line });

  s.addImage({ path: path.join(ROOT, 'src', 'ds', 'assets', 'logo', 'minimal-mark.png'), x: W - M - 0.6, y: 0.86, w: 0.6, h: 0.6, sizing: { type: 'contain', w: 0.6, h: 0.6 } });
  txt(s, 'ONE TRUE STORY, THEN THE NETWORK AROUND IT', M, 0.86, 5.4, { size: 7.5, bold: true, color: C.accent, cs: 2 });
  txt(s, 'Six months, five phases,\none dependency chain.', M - 0.03, 1.08, 5.6, { size: 22, bold: true, color: C.white, h: 0.8, ls: 1.06 });
  txt(s, 'Every phase is scoped from what the code does today, and sequenced so each one unblocks the next. Windows are planned execution ranges: phase N+1 starts when phase N’s definition of done is met, not on a calendar trigger.',
    M, 1.88, CW - 0.85, { size: 7.8, color: C.ink1, h: 0.3, ls: 1.3 });

  // ---- meta strip
  const metaY = 2.3, cellW = CW / META.length;
  rule(s, metaY, { color: C.line });
  META.forEach((mm, i) => {
    const x = M + i * cellW;
    if (i > 0) s.addShape('line', { x, y: metaY, w: 0, h: 0.42, line: { color: C.lineSoft, width: 1 } });
    txt(s, mm[0], x + 0.12, metaY + 0.06, cellW - 0.2, { size: 6.2, bold: true, color: C.ink3, cs: 1.6 });
    txt(s, mm[1], x + 0.12, metaY + 0.19, cellW - 0.2, { size: 8, color: C.white, mono: true });
  });
  rule(s, metaY + 0.4, { color: C.line });

  // ---- baseline
  let y = 2.86;
  section(s, 'Baseline — read out of the code, not the pitch', y);
  y += 0.32;
  const bw = (CW - 0.36) / 3;
  BASELINE.forEach((b, i) => {
    const x = M + i * (bw + 0.18);
    fill(s, x, y, bw, 0.022, b.color);
    txt(s, b.label, x, y + 0.09, bw, { size: 6.4, bold: true, color: b.color, cs: 1.2 });
    b.items.forEach((it, k) => txt(s, it, x, y + 0.26 + k * 0.142, bw, { size: 6.9, color: C.ink1 }));
  });
  y += 1.32;

  // ---- timeline
  section(s, 'Timeline', y);
  y += 0.34;
  const gridX = M + 2.85, gridW = W - M - gridX, colW = gridW / MONTHS.length, rowH = 0.27;
  MONTHS.forEach((mo, i) => {
    txt(s, mo, gridX + i * colW + 0.04, y - 0.14, colW, { size: 6, bold: true, color: C.ink3, cs: 1 });
    if (i > 0) s.addShape('line', { x: gridX + i * colW, y: y + 0.02, w: 0, h: rowH * PHASES.length, line: { color: C.lineSoft, width: 1 } });
  });
  PHASES.forEach((p, i) => {
    const ry = y + 0.02 + i * rowH;
    txt(s, p.n, M, ry + 0.055, 0.42, { size: 7, bold: true, color: C.ink3, mono: true });
    txt(s, p.title, M + 0.34, ry + 0.055, 2.45, { size: 7, bold: true, color: C.white });
    fill(s, gridX + p.start * colW + 0.03, ry + 0.06, p.span * colW - 0.1, 0.15, BAR[i]);
  });
  y += 0.02 + rowH * PHASES.length + 0.16;

  // ---- phases
  section(s, 'Phases — goal and definition of done', y);
  y += 0.28;
  PHASES.forEach((p, i) => {
    const ry = y + i * 0.55;
    txt(s, p.n, M, ry - 0.02, 0.42, { size: 9, bold: true, color: C.ink3, mono: true });
    txt(s, p.title, M + 0.38, ry - 0.01, 3.4, { size: 8.2, bold: true, color: C.white });
    txt(s, p.window, M + 3.9, ry, 1.5, { size: 6.6, bold: true, color: C.ink2, cs: 1 });
    s.addShape('rect', { x: W - M - 0.92, y: ry - 0.015, w: 0.92, h: 0.15, fill: { type: 'none' }, line: { color: p.scolor, width: 1 } });
    s.addText(p.status, { x: W - M - 0.92, y: ry - 0.015, w: 0.92, h: 0.15, fontFace: F.sans, fontSize: 5.4, bold: true, color: p.scolor, align: 'center', valign: 'middle', charSpacing: 0.8 });
    txt(s, p.goal, M + 0.38, ry + 0.145, CW - 0.38, { size: 7.1, color: C.ink1, h: 0.15 });
    txt(s, `DONE WHEN — ${p.done}`, M + 0.38, ry + 0.295, CW - 0.38, { size: 6.7, color: C.ink2, h: 0.18, italic: true });
    if (i < PHASES.length - 1) rule(s, ry + 0.49);
  });
  y += PHASES.length * 0.55 - 0.05;

  // ---- risks + next 90
  section(s, 'Risks & blockers', y);
  const rightX = M + CW / 2 + 0.18, halfW = CW / 2 - 0.18;
  fill(s, rightX, y + 0.045, 0.09, 0.09, C.accent);
  txt(s, 'NEXT 30 / 60 / 90', rightX + 0.19, y, 3, { size: 7.5, bold: true, color: C.white, cs: 1.8 });
  rule(s, y + 0.2, { x: rightX, w: halfW, color: C.line });
  y += 0.28;

  RISKS.forEach((r, i) => {
    const ry = y + i * 0.275;
    txt(s, r[0], M, ry, halfW - 0.5, { size: 7.2, bold: true, color: C.white });
    txt(s, r[1], M + halfW - 0.4, ry + 0.01, 0.4, { size: 6.6, color: C.accent, mono: true, align: 'right' });
    txt(s, r[2], M, ry + 0.132, halfW, { size: 6.7, color: C.ink2, h: 0.14 });
  });

  NEXT.forEach((n, i) => {
    const ry = y + i * 0.46;
    txt(s, n[0], rightX, ry, 1.2, { size: 7, bold: true, color: i === 0 ? C.accent : C.ink2, cs: 1.4 });
    n[1].forEach((it, k) => txt(s, `— ${it}`, rightX + 0.78, ry + k * 0.128, halfW - 0.78, { size: 6.7, color: C.ink1 }));
  });

  // ---- footer
  rule(s, H - 0.62, { color: C.line });
  txt(s, 'FULL PLAN — docs/pitch/CabalMesh-Project-Plan.md  ·  AUDIT — docs/product-status.md  ·  github.com/kurodenjiro/Cabal-Mesh',
    M, H - 0.52, CW, { size: 6.5, color: C.ink2, mono: true });
  txt(s, 'A LIVING PLAN. WHEN THE AUDIT CHANGES, THE PLAN IS WHAT GETS REVISED.',
    M, H - 0.36, CW, { size: 6.1, bold: true, color: C.ink3, cs: 1.6 });

  const out = path.join(__dirname, '.build', 'CabalMesh-Project-Plan-1page.pptx');
  return pres.writeFile({ fileName: out }).then(() => console.log('wrote', out));
}

main().catch((e) => { console.error(e); process.exit(1); });
