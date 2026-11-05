// ============================================================================
// build.js — renders CabalMesh-Project-Plan.pptx
//
//   theme.js    design tokens + drawing primitives (no content)
//   content.js  every word in the deck (no layout)
//   build.js    one function per slide, consuming both
//
// Slides are numbered automatically by createDeck(), so reordering or adding a
// slide never means renumbering anything by hand.
// ============================================================================
const pptxgen = require('pptxgenjs');
const path = require('path');
const { T, P, createDeck } = require('./theme');
const C = require('./content');

const col = T.color;
const F = T.font;

// ------------------------------------------------------------------- slide 1
function cover(slide) {
  const s = slide({ dark: true, chrome: false });
  P.meshDots(s, [
    [10.4, 1.0, 11.7, 1.85], [11.7, 1.85, 12.6, 1.15], [10.4, 1.0, 9.8, 2.2],
    [9.8, 2.2, 11.7, 1.85], [9.8, 2.2, 10.1, 3.9], [10.1, 3.9, 11.5, 4.5],
  ]);

  const d = C.cover;
  s.addText(d.eyebrow, { x: T.margin, y: 0.6, w: 6, h: 0.35, fontFace: F.body, fontSize: 11, bold: true, color: col.accent, charSpacing: 3 });
  s.addText(d.title, { x: 0.58, y: 2.1, w: 11, h: 1.5, fontFace: F.head, fontSize: T.size.hero, bold: true, color: col.white });
  s.addText(d.subtitle, { x: 0.62, y: 3.35, w: 10, h: 0.55, fontFace: F.body, fontSize: 19, italic: true, color: col.inkOnDark });

  s.addText(d.horizonLabel, { x: 0.62, y: 4.35, w: 6, h: 0.35, fontFace: F.body, fontSize: 12, bold: true, color: col.white, charSpacing: 1.5 });
  s.addShape('line', { x: 0.62, y: 4.85, w: 1.4, h: 0, line: { color: col.accent, width: 2 } });

  s.addText(d.blurb, { x: 0.62, y: 5.15, w: 8.0, h: 0.75, fontFace: F.body, fontSize: 13, color: col.inkOnDarkSoft, lineSpacingMultiple: 1.3 });
  s.addText(d.credit, { x: 0.62, y: 6.9, w: 8, h: 0.3, fontFace: F.body, fontSize: 9.5, color: col.inkSoft });
}

// ------------------------------------------------------------------- slide 2
function timeline(slide) {
  const s = slide({ section: 'Roadmap' });
  P.eyebrow(s, C.timeline.eyebrow);
  P.title(s, C.timeline.title, { size: 28 });

  const gridX = T.margin, gridW = T.contentW, rowY = 2.3, rowH = 0.82;
  const colW = gridW / C.months.length;
  const shades = ['E8541A', 'EC703F', 'F08C64', 'F3A888', 'F6C4AC'];

  C.months.forEach((m, i) => {
    s.addText(m, { x: gridX + i * colW, y: 1.75, w: colW, h: 0.3, fontFace: F.body, fontSize: 9, bold: true, color: col.muted, charSpacing: 1 });
    if (i > 0) s.addShape('line', { x: gridX + i * colW, y: 1.95, w: 0, h: rowH * 5 + 0.3, line: { color: col.line, width: 1 } });
  });
  P.rule(s, 1.95);

  C.phases.forEach((p, i) => {
    const y = rowY + i * rowH;
    s.addText(p.n, { x: gridX, y: y - 0.34, w: 1, h: 0.28, fontFace: F.body, fontSize: 10, bold: true, color: col.accent, charSpacing: 1 });
    s.addText(p.short, { x: gridX + 0.45, y: y - 0.36, w: 5, h: 0.32, fontFace: F.body, fontSize: 10.5, bold: true, color: col.ink });
    s.addShape('roundRect', {
      x: gridX + p.start * colW, y, w: p.span * colW - 0.08, h: 0.36,
      rectRadius: 0.06, fill: { color: shades[i] }, line: { type: 'none' },
    });
  });

  s.addText(C.timeline.note, { x: T.margin, y: 6.85, w: 11, h: 0.3, fontFace: F.body, fontSize: 10.5, italic: true, color: col.muted });
}

// ------------------------------------------------------------------- slide 3
function overview(slide) {
  const s = slide({ section: 'Roadmap' });
  P.eyebrow(s, C.overview.eyebrow);
  P.title(s, C.overview.title);

  const rowH = 0.95;
  C.phases.forEach((p, i) => {
    const y = 1.95 + i * rowH;
    s.addText(p.n, { x: T.margin, y: y - 0.05, w: 0.9, h: 0.6, fontFace: F.head, fontSize: 26, bold: true, color: col.tint2 });
    s.addText(p.title, { x: T.margin + 0.85, y: y - 0.05, w: 6.3, h: 0.4, valign: 'middle', fontFace: F.head, fontSize: 16.5, bold: true, color: col.ink });
    s.addText(p.summary, { x: T.margin + 0.85, y: y + 0.32, w: 6.3, h: 0.5, fontFace: F.body, fontSize: 10.3, color: col.inkSoft, lineSpacingMultiple: 1.2 });
    s.addText(p.window, { x: 9.6, y: y + 0.05, w: 2.4, h: 0.4, align: 'right', valign: 'middle', fontFace: F.body, fontSize: 10, bold: true, color: col.muted, charSpacing: 0.5 });
    if (i < C.phases.length - 1) P.rule(s, y + rowH - 0.14);
  });
}

/** Shared chrome for a phase detail slide: eyebrow, ghost numeral, title, lead. */
function phaseHeader(slide, idx, lead) {
  const p = C.phases[idx];
  const s = slide({ section: `Phase ${idx + 1}` });
  P.ghostNumber(s, p.n);
  P.eyebrow(s, `PHASE ${idx + 1} · ${p.windowUpper}`);
  P.title(s, p.title);
  P.lead(s, lead);
  return s;
}

// ------------------------------------------------------------- slide 4 (P1)
function phase1(slide) {
  const d = C.phase1;
  const s = phaseHeader(slide, 0, d.lead);
  P.rule(s, 2.55);

  d.columns.forEach((c, ci) => {
    const x = T.margin + ci * 5.85;
    P.sectionLabel(s, c.label, x, 2.8);
    c.items.forEach((t, i) => {
      s.addText(t, { x, y: 3.2 + i * 0.72, w: 5.5, h: 0.62, fontFace: F.body, fontSize: 10.8, color: col.inkSoft, lineSpacingMultiple: 1.2 });
    });
  });

  P.tintPanel(s, T.margin, 6.15, T.contentW, 0.85);
  s.addText(d.doneLabel, { x: 0.9, y: 6.15, w: 2.6, h: 0.85, valign: 'middle', fontFace: F.body, fontSize: 11, bold: true, color: col.accent });
  s.addText(d.done, { x: 3.35, y: 6.15, w: 8.6, h: 0.85, valign: 'middle', fontFace: F.body, fontSize: 11, italic: true, color: col.ink, lineSpacingMultiple: 1.2 });
}

// ------------------------------------------------------------- slide 5 (P2)
function phase2(slide) {
  const d = C.phase2;
  const s = phaseHeader(slide, 1, d.lead);

  const cardW = 5.65, cardH = 3.75, pad = 0.15, imgH = 2.25, top = 2.3;
  d.products.forEach((p, i) => {
    const x = T.margin + i * (cardW + 0.3);
    s.addShape('roundRect', { x, y: top, w: cardW, h: cardH, rectRadius: 0.1, fill: { color: col.bgDark }, line: { type: 'none' } });
    s.addImage({
      path: path.join(__dirname, p.image),
      x: x + pad, y: top + pad, w: cardW - pad * 2, h: imgH,
      sizing: { type: 'cover', w: cardW - pad * 2, h: imgH },
    });
    const textTop = top + pad + imgH;
    s.addText(p.tag, { x: x + 0.3, y: textTop + 0.14, w: 4.8, h: 0.26, fontFace: F.body, fontSize: 9, bold: true, color: col.accent, charSpacing: 1 });
    s.addText(p.name, { x: x + 0.3, y: textTop + 0.38, w: 4.8, h: 0.4, fontFace: F.head, fontSize: 17, bold: true, color: col.white });
    s.addText(p.body, { x: x + 0.3, y: textTop + 0.76, w: cardW - 0.6, h: 0.55, fontFace: F.body, fontSize: 9.3, color: col.inkOnDark, lineSpacingMultiple: 1.2 });
  });

  s.addText(d.caveat, { x: T.margin, y: 6.2, w: 11.4, h: 0.4, fontFace: F.body, fontSize: 10, color: col.inkSoft, lineSpacingMultiple: 1.25 });
  P.note(s, d.done, 6.6);
}

// ------------------------------------------------------------------- slide 6
function goToMarket(slide) {
  const d = C.gtm;
  const s = slide({ section: 'Go-to-market' });
  P.eyebrow(s, d.eyebrow);
  P.title(s, d.title, { size: T.size.titleSm });
  P.lead(s, d.lead, { y: 1.58, w: 11.3 });

  const sw = 2.85, top = 2.35;
  d.stages.forEach((st, i) => {
    const x = T.margin + i * sw;
    if (i === d.highlightIndex) {
      s.addShape('roundRect', { x: x - 0.12, y: top - 0.12, w: sw - 0.02, h: 3.55, rectRadius: 0.08, fill: { color: col.tint }, line: { type: 'none' } });
    }
    s.addText(st.months, { x, y: top, w: sw - 0.3, h: 0.28, fontFace: F.body, fontSize: 9.5, bold: true, color: col.muted, charSpacing: 1 });
    s.addText(st.tag, { x, y: top + 0.28, w: sw - 0.3, h: 0.32, fontFace: F.body, fontSize: 11.5, bold: true, color: col.accent, charSpacing: 0.5 });
    s.addText(`⌁ ${st.hook}`, { x, y: top + 0.66, w: sw - 0.3, h: 0.28, fontFace: F.body, fontSize: 9.5, italic: true, color: col.ink });
    s.addText(st.body, { x, y: top + 1.0, w: sw - 0.35, h: 1.5, fontFace: F.body, fontSize: 10, color: col.inkSoft, lineSpacingMultiple: 1.22 });
    s.addText(st.metric, { x, y: top + 2.62, w: sw - 0.35, h: 0.6, fontFace: F.body, fontSize: 9, bold: true, color: col.ink, lineSpacingMultiple: 1.15 });
    // divider only between non-highlighted neighbours
    const touchesHighlight = i === d.highlightIndex || i + 1 === d.highlightIndex;
    if (i < d.stages.length - 1 && !touchesHighlight) {
      s.addShape('line', { x: x + sw - 0.18, y: top, w: 0, h: 3.2, line: { color: col.line, width: 1 } });
    }
  });

  P.rule(s, 6.1);
  s.addText(d.audience, { x: T.margin, y: 6.25, w: 11.4, h: 0.3, fontFace: F.body, fontSize: 10.3, color: col.inkSoft });
  P.note(s, d.note, 6.6, { size: 10, h: 0.45 });
}

// ------------------------------------------------------------------- slide 7
function missions(slide) {
  const d = C.missions;
  const s = slide({ section: 'Go-to-market' });
  P.eyebrow(s, d.eyebrow);
  P.title(s, d.title, { size: T.size.titleSm });
  P.lead(s, d.lead, { y: 1.6, w: 11 });

  d.tracks.forEach((track, ci) => {
    const x = T.margin + ci * 5.85;
    P.sectionLabel(s, track.label, x, 2.35);
    s.addText(track.sub, { x, y: 2.63, w: 5.5, h: 0.3, fontFace: F.body, fontSize: 9.5, italic: true, color: col.muted });
    track.items.forEach((t, i) => {
      const y = 2.98 + i * 0.68;
      s.addText(String(track.offset + i), { x, y, w: 0.4, h: 0.4, fontFace: F.head, fontSize: 15, bold: true, color: col.tint2 });
      s.addText(t, { x: x + 0.42, y: y - 0.02, w: 5.1, h: 0.6, fontFace: F.body, fontSize: 10.5, color: col.inkSoft, lineSpacingMultiple: 1.18 });
    });
  });

  P.tintPanel(s, T.margin, 5.95, T.contentW, 1.05);
  s.addText(d.rewardLabel, { x: 0.9, y: 6.08, w: 2, h: 0.28, fontFace: F.body, fontSize: 9.5, bold: true, color: col.accent, charSpacing: 1.5 });
  s.addText(d.reward, { x: 0.9, y: 6.35, w: 11.0, h: 0.6, fontFace: F.body, fontSize: 10, color: col.ink, lineSpacingMultiple: 1.2 });
}

// ------------------------------------------------------------- slide 8 (P3)
function phase3(slide) {
  const d = C.phase3;
  const s = phaseHeader(slide, 2, d.lead);

  const stepW = 2.05, gap = 0.22, top = 2.85;
  d.flow.forEach((t, i) => {
    const x = T.margin + i * (stepW + gap);
    const last = i === d.flow.length - 1;
    s.addShape('roundRect', { x, y: top, w: stepW, h: 1.1, rectRadius: 0.08, fill: { color: last ? col.accent : col.tint }, line: { type: 'none' } });
    s.addText(t, { x: x + 0.1, y: top, w: stepW - 0.2, h: 1.1, align: 'center', valign: 'middle', fontFace: F.body, fontSize: 11, bold: true, color: last ? col.white : col.ink, lineSpacingMultiple: 1.15 });
    if (!last) P.icon(s, 'arrow_right', 'muted', x + stepW + 0.02, top + 0.4, 0.3);
  });
  s.addText(d.flowNote, { x: T.margin, y: 4.15, w: 10, h: 0.3, fontFace: F.body, fontSize: 10, italic: true, color: col.muted });

  P.rule(s, 4.7);
  P.sectionLabel(s, d.shipsLabel, T.margin, 4.88);
  d.ships.forEach((t, i) => {
    s.addText(`—  ${t}`, { x: T.margin, y: 5.24 + i * 0.46, w: 11, h: 0.4, fontFace: F.body, fontSize: 10.8, color: col.inkSoft, lineSpacingMultiple: 1.15 });
  });

  P.rule(s, 6.75);
  P.note(s, d.done, 6.85, { h: 0.3 });
}

// ------------------------------------------------------------- slide 9 (P4)
function phase4(slide) {
  const d = C.phase4;
  const s = phaseHeader(slide, 3, d.lead);

  [{ ...d.before, tint: false }, { ...d.after, tint: true }].forEach((c, ci) => {
    const x = T.margin + ci * 5.85;
    if (c.tint) s.addShape('roundRect', { x: x - 0.25, y: 2.4, w: 5.85, h: 3.1, rectRadius: 0.1, fill: { color: col.tint }, line: { type: 'none' } });
    P.sectionLabel(s, c.label, x, 2.6, c.tint ? col.accent : col.muted);
    c.items.forEach((t, i) => {
      s.addText(`${c.tint ? '✓' : '—'}  ${t}`, {
        x, y: 3.0 + i * 0.82, w: 5.15, h: 0.75,
        fontFace: F.body, fontSize: 11, color: c.tint ? col.ink : col.inkSoft, lineSpacingMultiple: 1.25,
      });
    });
  });

  s.addShape('roundRect', { x: T.margin, y: 5.9, w: T.contentW, h: 0.8, rectRadius: 0.08, fill: { color: col.ghost }, line: { type: 'none' } });
  s.addText(d.caveat, { x: 0.9, y: 5.9, w: 10.9, h: 0.8, valign: 'middle', fontFace: F.body, fontSize: 10.3, italic: true, color: col.inkSoft, lineSpacingMultiple: 1.2 });
}

// ------------------------------------------------------------ slide 10 (P5)
function phase5(slide) {
  const d = C.phase5;
  const s = slide({ section: 'Phase 5' });
  P.eyebrow(s, d.eyebrow);

  s.addText(d.headline, { x: 1.0, y: 2.15, w: 11.3, h: 0.7, align: 'center', fontFace: F.head, fontSize: T.size.title, bold: true, color: col.ink });
  s.addText(d.headlineAccent, { x: 1.0, y: 2.85, w: 11.3, h: 0.7, align: 'center', fontFace: F.head, fontSize: T.size.title, bold: true, color: col.accent });
  s.addText(d.sub, { x: 2.0, y: 3.65, w: 9.3, h: 0.4, align: 'center', fontFace: F.body, fontSize: 12.5, italic: true, color: col.muted });

  d.items.forEach((t, i) => {
    const y = 4.6 + i * 0.62;
    s.addShape('ellipse', { x: 3.15, y: y + 0.08, w: 0.09, h: 0.09, fill: { color: col.accent }, line: { type: 'none' } });
    s.addText(t, { x: 3.45, y: y - 0.08, w: 6.4, h: 0.5, fontFace: F.body, fontSize: 12, color: col.inkSoft, lineSpacingMultiple: 1.3 });
  });
}

// ------------------------------------------------------------------ slide 11
function closing(slide) {
  const d = C.closing;
  const s = slide({ dark: true, chrome: false });
  P.meshDots(s, [
    [1.0, 1.1, 2.2, 1.7], [2.2, 1.7, 1.6, 2.6], [1.0, 1.1, 0.6, 2.3],
    [10.6, 5.0, 11.8, 5.6], [11.8, 5.6, 11.2, 6.6], [10.6, 5.0, 12.2, 4.3],
  ]);

  s.addText(d.title, { x: 1.0, y: 2.2, w: 11.3, h: 0.8, align: 'center', fontFace: F.head, fontSize: 32, bold: true, color: col.white });
  s.addText(d.sub, { x: 2.15, y: 3.05, w: 9, h: 0.7, align: 'center', fontFace: F.body, fontSize: 13, italic: true, color: col.inkOnDarkSoft, lineSpacingMultiple: 1.3 });

  const lw = 3.15;
  d.links.forEach(([label, sub], i) => {
    const x = 1.9 + i * (lw + 0.25);
    s.addShape('line', { x, y: 4.4, w: lw - 0.5, h: 0, line: { color: col.lineDark, width: 1 } });
    s.addText(label, { x, y: 4.55, w: lw, h: 0.32, fontFace: F.body, fontSize: 11.5, bold: true, color: col.white });
    s.addText(sub, { x, y: 4.87, w: lw, h: 0.4, fontFace: F.body, fontSize: 10, color: col.accent });
  });

  s.addText(d.footer, { x: T.margin, y: 7.05, w: 12.1, h: 0.35, align: 'center', fontFace: F.body, fontSize: 9.5, color: col.inkSoft, charSpacing: 2 });
}

// --------------------------------------------------------------------- main
async function main() {
  const pres = new pptxgen();
  pres.layout = 'LAYOUT_WIDE';
  pres.author = 'CabalMesh';
  pres.company = 'CabalMesh';
  pres.title = 'CabalMesh — Development Plan';

  const slide = createDeck(pres);

  // Deck order. Add, remove or reorder freely — numbering follows.
  [cover, timeline, overview, phase1, phase2, goToMarket, missions, phase3, phase4, phase5, closing]
    .forEach((render) => render(slide));

  const out = path.join(__dirname, 'CabalMesh-Project-Plan.pptx');
  await pres.writeFile({ fileName: out });
  console.log('WROTE', out);
}

main().catch((e) => { console.error(e); process.exit(1); });
