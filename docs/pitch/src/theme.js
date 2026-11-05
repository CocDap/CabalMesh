// ============================================================================
// theme.js — design tokens + low-level drawing primitives.
// No content lives here; no slide knows about another slide.
// ============================================================================
const fs = require('fs');
const path = require('path');

const T = {
  // canvas
  W: 13.3, H: 7.5,
  margin: 0.6,
  get contentW() { return this.W - this.margin * 2; },

  // palette — warm ink + ember accent
  color: {
    bg: 'FFFFFF',
    bgDark: '141210',
    ink: '19140F',
    inkSoft: '5C5346',
    inkOnDark: 'C9C0B3',
    inkOnDarkSoft: 'B7AC9C',
    muted: '9A8F80',
    mutedDark: '6B6255',
    accent: 'E8541A',
    tint: 'FCE7DB',      // accent wash
    tint2: 'F6D9C7',     // ghost numerals / de-emphasised numbers
    ghost: 'F2EEE7',     // very light fill
    line: 'E7E1D8',
    lineDark: '3A322A',
    white: 'FFFFFF',
  },

  font: { head: 'Cambria', body: 'Calibri' },

  // type scale
  size: {
    hero: 62, title: 30, titleSm: 26, lead: 13,
    body: 10.5, bodySm: 10, fine: 9.3, micro: 8.5,
    eyebrow: 10.5, label: 10, ghostNum: 108,
  },
};

const ICON_DIR = path.join(__dirname, 'icons2');
const iconData = (name, tone) =>
  `image/png;base64,${fs.readFileSync(path.join(ICON_DIR, `${name}_${tone}.png`)).toString('base64')}`;

// ---------------------------------------------------------------- primitives
const P = {
  icon(s, name, tone, x, y, size) {
    s.addImage({ data: iconData(name, tone), x, y, w: size, h: size });
  },

  eyebrow(s, text) {
    s.addText(text, {
      x: T.margin, y: 0.5, w: 9, h: 0.35,
      fontFace: T.font.body, fontSize: T.size.eyebrow, bold: true,
      color: T.color.accent, charSpacing: 2.5,
    });
  },

  title(s, text, opts = {}) {
    s.addText(text, {
      x: T.margin, y: opts.y ?? 0.9, w: opts.w ?? 11.4, h: 0.8,
      fontFace: T.font.head, fontSize: opts.size ?? T.size.title, bold: true,
      color: opts.color ?? T.color.ink,
    });
  },

  lead(s, text, opts = {}) {
    s.addText(text, {
      x: T.margin, y: opts.y ?? 1.68, w: opts.w ?? 9.0, h: opts.h ?? 0.4,
      fontFace: T.font.body, fontSize: T.size.lead,
      color: opts.color ?? T.color.inkSoft, lineSpacingMultiple: 1.3,
    });
  },

  /** Oversized watermark numeral, top-right. */
  ghostNumber(s, n) {
    s.addText(n, {
      x: 9.6, y: 0.25, w: 3.1, h: 2.0, wrap: false,
      fontFace: T.font.head, fontSize: T.size.ghostNum, bold: true,
      color: T.color.ghost, align: 'right', valign: 'top',
    });
  },

  /** Section heading above a block of content. */
  sectionLabel(s, text, x, y, color = T.color.accent) {
    s.addText(text, {
      x, y, w: 6, h: 0.3,
      fontFace: T.font.body, fontSize: T.size.label, bold: true,
      color, charSpacing: 1,
    });
  },

  rule(s, y, opts = {}) {
    s.addShape('line', {
      x: opts.x ?? T.margin, y, w: opts.w ?? T.contentW, h: 0,
      line: { color: opts.color ?? T.color.line, width: 1 },
    });
  },

  /** Italic accent note — replaces the old heavy "definition of done" panel. */
  note(s, text, y, opts = {}) {
    s.addText(text, {
      x: T.margin, y, w: opts.w ?? 11.4, h: opts.h ?? 0.4,
      fontFace: T.font.body, fontSize: opts.size ?? 10.3,
      italic: true, color: opts.color ?? T.color.accent,
      lineSpacingMultiple: 1.2,
    });
  },

  tintPanel(s, x, y, w, h) {
    s.addShape('roundRect', {
      x, y, w, h, rectRadius: 0.08,
      fill: { color: T.color.tint }, line: { type: 'none' },
    });
  },

  /** Decorative constellation for the dark bookend slides. */
  meshDots(s, pts) {
    const c = T.color.lineDark;
    pts.forEach(([x1, y1, x2, y2]) =>
      s.addShape('line', { x: x1, y: y1, w: x2 - x1, h: y2 - y1, line: { color: c, width: 1 } }));
    pts.forEach(([x1, y1]) =>
      s.addShape('ellipse', { x: x1 - 0.03, y: y1 - 0.03, w: 0.06, h: 0.06, fill: { color: c }, line: { type: 'none' } }));
  },
};

/**
 * Slide factory. Auto-numbers every slide and stamps the footer, so adding or
 * reordering slides never requires touching a page number by hand.
 */
function createDeck(pres) {
  let pageNo = 0;
  return function slide({ dark = false, section = 'CabalMesh', chrome = true } = {}) {
    const s = pres.addSlide();
    pageNo += 1;
    s.background = { color: dark ? T.color.bgDark : T.color.bg };
    if (chrome) {
      const tone = dark ? T.color.mutedDark : T.color.muted;
      s.addText(section.toUpperCase(), {
        x: 0.5, y: 7.15, w: 6, h: 0.3,
        fontFace: T.font.body, fontSize: T.size.micro, bold: true, color: tone, charSpacing: 2,
      });
      s.addText(String(pageNo).padStart(2, '0'), {
        x: 12.3, y: 7.15, w: 0.6, h: 0.3, align: 'right',
        fontFace: T.font.body, fontSize: T.size.micro, color: tone,
      });
    }
    return s;
  };
}

module.exports = { T, P, createDeck };
