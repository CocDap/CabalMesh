// ============================================================================
// theme.js — design tokens + drawing primitives for the CabalMesh decks.
//
// The visual language is BRAND.md's: pure black grounds, hairline rules,
// corner registration ticks, uppercase micro-labels, one accent used sparingly.
// No content lives here. No slide knows about another slide.
// ============================================================================
const fs = require('fs');
const path = require('path');

const T = {
  W: 13.333, H: 7.5,
  margin: 0.72,
  get contentW() { return this.W - this.margin * 2; },

  color: {
    void: '000000',
    panel: '0D0D0D',
    panelUp: '141414',
    line: '2A2A2A',
    lineSoft: '1C1C1C',
    white: 'FFFFFF',
    ink1: 'BEBEBE',
    ink2: '7A7A7A',
    ink3: '3A3A3A',
    accent: '00E5FF',   // neon blue — brand rule: < 5% of the surface
    live: '9BFF00',     // acid green
    warn: 'FF3B3B',     // blood red
  },

  font: { sans: 'Arial', mono: 'Courier New' },

  size: {
    hero: 50, title: 27, titleSm: 22, lead: 12.5,
    body: 10.5, bodySm: 9.8, fine: 9, micro: 8, eyebrow: 9.5,
    ghost: 92,
  },
};

const C = T.color;
const F = T.font;
const ICONS = path.join(__dirname, '..', 'src', 'icons2');

const P = {
  // ---------------------------------------------------------------- surfaces
  /** Hairline-bordered panel. Brand: never rounded, never a coloured fill. */
  panel(s, x, y, w, h, o = {}) {
    s.addShape('rect', {
      x, y, w, h,
      fill: { color: o.fill ?? C.panel },
      line: { color: o.line ?? C.line, width: 1 },
    });
  },

  fill(s, x, y, w, h, color) {
    s.addShape('rect', { x, y, w, h, fill: { color }, line: { type: 'none' } });
  },

  rule(s, y, o = {}) {
    s.addShape('line', {
      x: o.x ?? T.margin, y, w: o.w ?? T.contentW, h: 0,
      line: { color: o.color ?? C.line, width: o.width ?? 1 },
    });
  },

  vrule(s, x, y, h, o = {}) {
    s.addShape('line', { x, y, w: 0, h, line: { color: o.color ?? C.line, width: 1 } });
  },

  /** Registration ticks — the instrument-marking corner detail from the board. */
  ticks(s) {
    const a = 0.16, m = 0.3, col = C.ink3;
    const corner = (x, y, dx, dy) => {
      s.addShape('line', { x, y, w: a * dx, h: 0, line: { color: col, width: 1 } });
      s.addShape('line', { x, y, w: 0, h: a * dy, line: { color: col, width: 1 } });
    };
    corner(m, m, 1, 1);
    corner(T.W - m, m, -1, 1);
    corner(m, T.H - m, 1, -1);
    corner(T.W - m, T.H - m, -1, -1);
  },

  // -------------------------------------------------------------------- type
  eyebrow(s, text, o = {}) {
    s.addText(text.toUpperCase(), {
      x: o.x ?? T.margin, y: o.y ?? 0.86, w: o.w ?? 9, h: 0.26,
      fontFace: F.sans, fontSize: T.size.eyebrow, bold: true,
      color: o.color ?? C.accent, charSpacing: 2.4,
    });
  },

  title(s, text, o = {}) {
    s.addText(text, {
      x: o.x ?? T.margin, y: o.y ?? 1.2, w: o.w ?? 11.3, h: o.h ?? 0.9,
      fontFace: F.sans, fontSize: o.size ?? T.size.title, bold: true,
      color: o.color ?? C.white, lineSpacingMultiple: 1.12,
    });
  },

  lead(s, text, o = {}) {
    s.addText(text, {
      x: o.x ?? T.margin, y: o.y ?? 2.02, w: o.w ?? 9.4, h: o.h ?? 0.5,
      fontFace: F.sans, fontSize: o.size ?? T.size.lead,
      color: o.color ?? C.ink1, lineSpacingMultiple: 1.34,
    });
  },

  /** Uppercase, widest-tracked micro label. */
  label(s, text, x, y, o = {}) {
    s.addText(text.toUpperCase(), {
      x, y, w: o.w ?? 5, h: o.h ?? 0.24,
      fontFace: F.sans, fontSize: o.size ?? T.size.micro, bold: true,
      color: o.color ?? C.ink2, charSpacing: o.charSpacing ?? 2,
      align: o.align ?? 'left',
    });
  },

  body(s, text, x, y, w, o = {}) {
    s.addText(text, {
      x, y, w, h: o.h ?? 0.6,
      fontFace: F.sans, fontSize: o.size ?? T.size.bodySm,
      color: o.color ?? C.ink1, lineSpacingMultiple: o.ls ?? 1.3,
      valign: o.valign ?? 'top', align: o.align ?? 'left', bold: o.bold ?? false,
    });
  },

  /** Monospace evidence line — file paths, hex ids, contract names. */
  code(s, text, x, y, w, o = {}) {
    s.addText(text, {
      x, y, w, h: o.h ?? 0.24,
      fontFace: F.mono, fontSize: o.size ?? T.size.fine,
      color: o.color ?? C.ink2, align: o.align ?? 'left',
    });
  },

  /** Oversized watermark numeral, top-right. */
  ghost(s, n, o = {}) {
    s.addText(String(n), {
      x: o.x ?? 10.6, y: o.y ?? 0.62, w: 2.1, h: 1.6, wrap: false,
      fontFace: F.sans, fontSize: T.size.ghost, bold: true,
      color: C.panelUp, align: 'right', valign: 'top',
    });
  },

  // ------------------------------------------------------------------ chrome
  /** Status chip: hairline box, coloured uppercase text. No filled buttons. */
  chip(s, text, x, y, o = {}) {
    const w = o.w ?? 1.06, h = o.h ?? 0.22;
    const col = o.color ?? C.ink2;
    s.addShape('rect', { x, y, w, h, fill: { type: 'none' }, line: { color: col, width: 1 } });
    s.addText(text.toUpperCase(), {
      x, y: y - 0.005, w, h,
      fontFace: F.sans, fontSize: 7.5, bold: true, color: col,
      align: 'center', valign: 'middle', charSpacing: 1.2,
    });
  },

  icon(s, name, x, y, size, tone = 'white') {
    s.addImage({ path: path.join(ICONS, `${name}_${tone}.png`), x, y, w: size, h: size });
  },

  image(s, file, x, y, w, h, sizing) {
    s.addImage(sizing
      ? { path: file, x, y, w, h, sizing: { type: sizing, w, h } }
      : { path: file, x, y, w, h });
  },

  /** Decorative node constellation for the bookend slides. */
  mesh(s, pts, o = {}) {
    const col = o.color ?? C.ink3;
    pts.forEach(([x1, y1, x2, y2]) =>
      s.addShape('line', { x: x1, y: y1, w: x2 - x1, h: y2 - y1, line: { color: col, width: 1 } }));
    const seen = new Set();
    pts.flatMap(([x1, y1, x2, y2]) => [[x1, y1], [x2, y2]]).forEach(([x, y]) => {
      const k = `${x}:${y}`;
      if (seen.has(k)) return;
      seen.add(k);
      s.addShape('ellipse', {
        x: x - 0.035, y: y - 0.035, w: 0.07, h: 0.07,
        fill: { color: o.dot ?? C.ink2 }, line: { type: 'none' },
      });
    });
  },

  /** Small accent underline used under cover and closing titles. */
  accentBar(s, x, y, w = 1.3) {
    s.addShape('rect', { x, y, w, h: 0.035, fill: { color: C.accent }, line: { type: 'none' } });
  },
};

/**
 * Slide factory: black ground, registration ticks, running head and automatic
 * page number, so reordering slides never means renumbering anything by hand.
 */
function createDeck(pres, deckLabel) {
  let page = 0;
  return function slide({ section = '', chrome = true } = {}) {
    const s = pres.addSlide();
    page += 1;
    s.background = { color: C.void };
    P.ticks(s);
    if (chrome) {
      s.addText('CABALMESH', {
        x: T.margin, y: 0.3, w: 3, h: 0.24,
        fontFace: F.sans, fontSize: T.size.micro, bold: true, color: C.ink2, charSpacing: 2.4,
      });
      s.addText((section || deckLabel).toUpperCase(), {
        x: 7.2, y: 0.3, w: 5.4, h: 0.24, align: 'right',
        fontFace: F.sans, fontSize: T.size.micro, bold: true, color: C.ink3, charSpacing: 2.4,
      });
      P.rule(s, 0.62, { color: C.lineSoft });
      s.addText(String(page).padStart(2, '0'), {
        x: 11.9, y: 7.02, w: 0.72, h: 0.24, align: 'right',
        fontFace: F.mono, fontSize: T.size.micro, color: C.ink3,
      });
    }
    return s;
  };
}

module.exports = { T, P, C, F, createDeck };
