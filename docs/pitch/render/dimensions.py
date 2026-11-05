# -*- coding: utf-8 -*-
"""Generates the two hardware SVGs for docs/pitch/assets/."""
import os, sys, pathlib
sys.path.insert(0, str(pathlib.Path(__file__).resolve().parent))
PITCH = pathlib.Path(__file__).resolve().parents[1]

import math

BG      = "#0B0E14"
PANEL   = "#0F141C"
LINE    = "#2A3442"
FAINT   = "#1A222E"
TEXT    = "#E6EDF3"
MUTED   = "#7D8A9C"
MINT    = "#00D9A3"
MONO    = "ui-monospace, 'IBM Plex Mono', 'SFMono-Regular', 'Courier New', monospace"

def esc(s):
    return (s.replace("&", "&amp;").replace("<", "&lt;").replace(">", "&gt;"))

def t(x, y, s, size=11, fill=TEXT, anchor="start", track=0.0, weight=400, op=1.0, rotate=None):
    tr = f' transform="rotate({rotate},{x},{y})"' if rotate is not None else ""
    ls = f' letter-spacing="{track}em"' if track else ""
    return (f'<text x="{x}" y="{y}" font-family="{MONO}" font-size="{size}" fill="{fill}" '
            f'text-anchor="{anchor}" font-weight="{weight}" opacity="{op}"{ls}{tr}>{esc(s)}</text>')

def rect(x, y, w, h, fill="none", stroke=LINE, sw=1, dash=None, op=1.0):
    d = f' stroke-dasharray="{dash}"' if dash else ""
    return (f'<rect x="{x}" y="{y}" width="{w}" height="{h}" fill="{fill}" stroke="{stroke}" '
            f'stroke-width="{sw}" opacity="{op}"{d}/>')

def line(x1, y1, x2, y2, stroke=LINE, sw=1, dash=None, op=1.0):
    d = f' stroke-dasharray="{dash}"' if dash else ""
    return (f'<line x1="{x1}" y1="{y1}" x2="{x2}" y2="{y2}" stroke="{stroke}" stroke-width="{sw}" '
            f'opacity="{op}"{d}/>')

def arrow(x, y, dx, dy, size=5, fill=MINT):
    """Solid triangle at (x,y) pointing along unit-ish (dx,dy)."""
    ln = math.hypot(dx, dy) or 1
    ux, uy = dx / ln, dy / ln
    px, py = -uy, ux
    p1 = (x, y)
    p2 = (x - ux * size * 2 + px * size * 0.7, y - uy * size * 2 + py * size * 0.7)
    p3 = (x - ux * size * 2 - px * size * 0.7, y - uy * size * 2 - py * size * 0.7)
    pts = " ".join(f"{round(a,2)},{round(b,2)}" for a, b in (p1, p2, p3))
    return f'<polygon points="{pts}" fill="{fill}"/>'

def ticks(x, y, w, h, size=7, stroke=MINT, op=0.9):
    """Corner registration ticks."""
    o = []
    for cx, cy, sx, sy in ((x, y, 1, 1), (x + w, y, -1, 1), (x, y + h, 1, -1), (x + w, y + h, -1, -1)):
        o.append(line(cx, cy, cx + sx * size, cy, stroke, 1, op=op))
        o.append(line(cx, cy, cx, cy + sy * size, stroke, 1, op=op))
    return "".join(o)

def dim_h(x1, x2, y, label, sub=None):
    """Horizontal dimension line with arrowheads and a label above it."""
    o = [line(x1, y - 6, x1, y + 6, LINE), line(x2, y - 6, x2, y + 6, LINE),
         line(x1, y, x2, y, MINT, 1, op=0.75),
         arrow(x1, y, -1, 0, 4.5, MINT), arrow(x2, y, 1, 0, 4.5, MINT),
         t((x1 + x2) / 2, y - 9, label, 10, MINT, "middle", 0.08)]
    if sub:
        o.append(t((x1 + x2) / 2, y + 17, sub, 9, MUTED, "middle", 0.16))
    return "".join(o)

def dim_v(y1, y2, x, label):
    """Vertical dimension line with arrowheads and a rotated label to its right."""
    return "".join([
        line(x - 6, y1, x + 6, y1, LINE), line(x - 6, y2, x + 6, y2, LINE),
        line(x, y1, x, y2, MINT, 1, op=0.75),
        arrow(x, y1, 0, -1, 4.5, MINT), arrow(x, y2, 0, 1, 4.5, MINT),
        t(x + 12, (y1 + y2) / 2, label, 10, MINT, "middle", 0.08, rotate=-90),
    ])

def header(w, title, subtitle, right):
    o = [rect(0, 0, w, 0, fill=BG)]
    o.append(t(40, 46, title, 20, TEXT, "start", 0.20, 500))
    o.append(t(40, 68, subtitle, 11, MUTED, "start", 0.06))
    o.append(t(w - 40, 46, right, 10, MINT, "end", 0.20))
    o.append(line(40, 88, w - 40, 88, LINE))
    return "".join(o)

def chip(x, y, label, tone=MINT):
    wch = 8.2 * len(label) + 16
    return "".join([rect(x, y - 11, wch, 16, "none", tone, 1, op=0.55),
                    t(x + 8, y, label, 9, tone, "start", 0.20)])



# ─────────────────────────────────────────────────────────────────────────────
# hardware-dimensions.svg — two devices, each elevation at its own stated
# scale, plus one strip that puts both (and a phone) at a single scale.
# ─────────────────────────────────────────────────────────────────────────────
W, H = 1280, 1140
BASE = 600                    # the sheet's datum line

out = [f'<svg xmlns="http://www.w3.org/2000/svg" viewBox="0 0 {W} {H}" width="{W}" height="{H}" '
       f'role="img" aria-label="CabalMesh concept hardware — dimensioned orthographic views">',
       rect(0, 0, W, H, BG, "none"),
       header(W, "HARDWARE · DIMENSIONS",
              "Concept enclosures, Phase 2 — mechanical intent only. All dimensions in millimetres.",
              "CONCEPT · NOT BUILT")]

def poly_pts(pts, fill=PANEL, stroke=LINE, sw=1):
    p = " ".join(f"{round(a,2)},{round(b,2)}" for a, b in pts)
    return f'<polygon points="{p}" fill="{fill}" stroke="{stroke}" stroke-width="{sw}"/>'

def eye_flat(cx, cy, r, op=0.8):
    sw = max(r * 0.14, 0.8)
    return (f'<g opacity="{op}"><path d="M {cx-r} {cy} q {r} {-r*0.85} {2*r} 0 q {-r} {r*0.85} {-2*r} 0 z" '
            f'fill="none" stroke="{MINT}" stroke-width="{sw}"/>'
            f'<circle cx="{cx}" cy="{cy}" r="{r*0.34}" fill="none" stroke="{MINT}" stroke-width="{sw}"/>'
            f'<line x1="{cx-r*0.95}" y1="{cy+r*0.7}" x2="{cx+r*0.95}" y2="{cy-r*0.7}" '
            f'stroke="{MINT}" stroke-width="{sw}"/></g>')

def dropbox_elevation(x0, base, s, side=False, detail=True):
    """The Nobody Box in elevation: 520 wide (front) or 460 (side), 720 tall."""
    wd = 460 if side else 520
    def E(x, z):
        return (x0 + x * s, base - z * s)
    def R(x, z, w, h, fill=PANEL, stroke=LINE, sw=1):
        p1 = E(x, z + h)
        return rect(round(p1[0], 2), round(p1[1], 2), round(w * s, 2), round(h * s, 2), fill, stroke, sw)
    o = []
    for lx in (25, wd - 95):                                        # two splayed legs
        a, b = E(lx, 0), E(lx + 70, 0)
        c, d = E(lx + 70, 165), E(lx, 165)
        o.append(poly_pts([(a[0] - 6 * s, a[1]), b, c, (d[0] + 4 * s, d[1])]))
    o.append(R(8, 160, wd - 16, 48))                                # plinth rail
    o.append(R(0, 205, wd, 435))                                    # body
    o.append(R(-8, 640, wd + 16, 80))                               # lid, overhanging
    if detail and not side:
        o.append(R(58, 250, wd - 116, 340, "none", "#232C38"))       # recessed front panel
        for k in range(1, 8):
            gx = 58 + k * (wd - 116) / 8
            p1, p2 = E(gx, 258), E(gx, 582)
            o.append(line(round(p1[0], 2), round(p1[1], 2), round(p2[0], 2), round(p2[1], 2), "#1B2330"))
        cx, cy = E(wd / 2, 420)
        o.append(eye_flat(round(cx, 2), round(cy, 2), 62 * s, 0.75))
        o.append(R(wd / 2 - 32, 606, 64, 34, "#0F141C", MINT, 1))    # latch block
        c2 = E(wd / 2, 623)
        o.append(f'<circle cx="{round(c2[0],2)}" cy="{round(c2[1],2)}" r="{round(11*s,2)}" '
                 f'fill="none" stroke="{MINT}" stroke-width="1"/>')
    elif detail and side:
        p1, p2 = E(18, 646), E(80, 646)                              # hinge line, rear top
        o.append(line(round(p1[0], 2), round(p1[1], 2), round(p2[0], 2), round(p2[1], 2), MINT, 1, op=0.8))
    return "".join(o)

devices = [
    dict(name="1 · SHADOWBOX", role="MESH · MODEL · PROVER — ONE NODE",
         spec=["220 × 160 × 70 mm", "1.9 kg · matte anodised Al, finned flanks",
               "3 module bays: RADIO / CRYPTO / POWER", "Readout + 8 proof lamps · NET cut-off switch",
               "Fanless · 12 W idle, 25 W while proving", "BLE 5.3 · Wi-Fi 6 · LoRa in the RADIO bay"]),
    dict(name="2 · THE NOBODY BOX", role="PHYSICAL ESCROW LOCKER",
         spec=["520 × 460 × 720 mm (legs 165)", "14 kg empty · ≈ 88 L usable",
               "Roto-moulded LLDPE over a steel latch frame", "Motorised bolt · no keyhole on the outside",
               "Load cell in the floor · status ring at the latch", "BLE 5.3 · 4 × D cells ≈ 12 months"]),
]

def panel(i):
    return 40 + i * 640

def scale_note(px, s):
    out.append(t(px, BASE + 62, f"SCALE {s} px/mm", 8.5, MUTED, "start", 0.20))

# ── 1 · ShadowBox: top view over front elevation, side to the right ─────────
S1 = 1.4
x0 = panel(0) + 30
fw, fh = 220 * S1, 70 * S1
fy = BASE - fh
tw, th = 220 * S1, 160 * S1
ty = fy - 18 - th
out.append(rect(x0, ty, tw, th, PANEL, LINE))                        # TOP
for k in range(3):
    bw = tw / 5.2
    bx = x0 + tw * 0.07 + k * (bw + 14)
    out.append(rect(bx, ty + 10, bw, 14, PANEL if k != 1 else "#121821", MINT, 1, op=0.85))
out.append(rect(x0 + tw * 0.24, ty + th * 0.34, tw * 0.52, th * 0.26, "none", MINT, 1, op=0.8))
out.append(t(x0 + tw / 2, ty + th * 0.5, "PROVEN", 11, MINT, "middle", 0.18))
for k in range(8):
    lx = x0 + tw * 0.25 + k * (tw * 0.5 / 8) + 3
    out.append(rect(lx, ty + th * 0.68, 9, 9, MINT if k < 5 else "none", MINT, 1, op=0.85))
out.append(eye_flat(x0 + tw - 24, ty + th - 20, 8, 0.75))
out.append(rect(x0, fy, fw, fh, PANEL, LINE))                        # FRONT
out.append(rect(x0 + 16, fy + fh - 34, 44, 22, "none", MINT, 1, op=0.8))
out.append(rect(x0 + 96, fy + fh / 2 - 8, 20, 14, "none", MINT, 1, op=0.7))
out.append(t(x0 + 122, fy + fh / 2 + 4, "NET", 8, MUTED, "start", 0.18))
out.append(rect(x0 + 176, fy + fh / 2 - 6, 44, 11, "none", MINT, 1, op=0.85))
sx = x0 + fw + 34
sw_ = 160 * S1
out.append(rect(sx, fy, sw_, fh, PANEL, LINE))                       # SIDE, finned
for k in range(11):
    lxx = sx + 8 + k * (sw_ - 16) / 10
    out.append(line(lxx, fy + 8, lxx, BASE - 8, LINE, 1, op=0.85))
out.append(t(x0, ty - 10, "TOP", 9, MUTED, "start", 0.20))
out.append(t(x0, fy - 5, "FRONT", 9, MUTED, "start", 0.20))
out.append(t(sx, fy - 5, "SIDE", 9, MUTED, "start", 0.20))
out.append(dim_h(x0, x0 + fw, BASE + 34, "220"))
out.append(dim_h(sx, sx + sw_, BASE + 34, "160"))
out.append(dim_v(ty, ty + th, x0 + tw + 26, "160"))
out.append(dim_v(fy, BASE, sx + sw_ + 26, "70"))
scale_note(panel(0), S1)

# ── 2 · The Nobody Box: front + side ────────────────────────────────────────
S2 = 0.45
x0 = panel(1) + 40
out.append(dropbox_elevation(x0, BASE, S2))
sx = x0 + 520 * S2 + 34
out.append(dropbox_elevation(sx, BASE, S2, side=True))
out.append(t(x0, BASE - 720 * S2 - 10, "FRONT", 9, MUTED, "start", 0.20))
out.append(t(sx, BASE - 720 * S2 - 10, "SIDE", 9, MUTED, "start", 0.20))
out.append(dim_h(x0, x0 + 520 * S2, BASE + 34, "520"))
out.append(dim_h(sx, sx + 460 * S2, BASE + 34, "460"))
out.append(dim_v(BASE - 720 * S2, BASE, sx + 460 * S2 + 26, "720"))
out.append(dim_v(BASE - 165 * S2, BASE, x0 - 24, "165"))
scale_note(panel(1), S2)

# ── datum, titles, spec blocks ──────────────────────────────────────────────
out.append(line(40, BASE, W - 40, BASE, LINE, 1, dash="2 5", op=0.8))
for i, d in enumerate(devices):
    px = panel(i)
    out.append(t(px, 122, d["name"], 14, TEXT, "start", 0.16, 500))
    out.append(t(px, 140, d["role"], 9, MINT, "start", 0.22))
    out.append(line(px, 154, px + 600, 154, FAINT))
    out.append(line(px, 690, px + 600, 690, FAINT))
    for j, sp in enumerate(d["spec"]):
        out.append(t(px, 716 + j * 20, sp, 10.5, TEXT if j == 0 else MUTED, "start", 0.02))

# ── one strip, one scale ────────────────────────────────────────────────────
SB_Y, S3 = 1040, 0.24
out.append(line(40, 830, W - 40, 830, LINE))
out.append(t(40, 856, "BOTH AT ONE SCALE", 12, TEXT, "start", 0.22, 500))
out.append(t(240, 856, f"— {S3} px/mm, with a phone for reference. One of these is furniture; one fits on a desk.", 10, MUTED, "start", 0.02))
out.append(dropbox_elevation(90, SB_Y, S3, detail=False))
out.append(t(90, SB_Y + 20, "THE NOBODY BOX", 9.5, TEXT, "start", 0.16))
out.append(t(90, SB_Y + 34, "520 × 720 mm", 8.5, MUTED, "start", 0.06))
sx = 90 + 520 * S3 + 130
out.append(rect(sx, SB_Y - 70 * S3, 220 * S3, 70 * S3, PANEL, LINE))
out.append(line(sx, SB_Y - 70 * S3, sx + 220 * S3, SB_Y - 70 * S3, MINT, 1, op=0.5))
out.append(t(sx, SB_Y + 20, "SHADOWBOX", 9.5, TEXT, "start", 0.16))
out.append(t(sx, SB_Y + 34, "220 × 70 mm", 8.5, MUTED, "start", 0.06))
sx += 220 * S3 + 130
out.append(rect(sx, SB_Y - 147 * S3, 71 * S3, 147 * S3, "none", LINE, 1, dash="3 3"))
out.append(t(sx, SB_Y + 20, "PHONE", 9.5, MUTED, "start", 0.16))
out.append(t(sx, SB_Y + 34, "71 × 147 mm · reference", 8.5, MUTED, "start", 0.06))
out.append(line(40, SB_Y, W - 40, SB_Y, LINE, 1, dash="2 5", op=0.8))

out.append(line(40, H - 58, W - 40, H - 58, LINE))
out.append(t(40, H - 36, "EACH PANEL STATES ITS OWN SCALE — THE LOCKER IS DRAWN SMALLER SO IT FITS. THE STRIP ABOVE IS THE HONEST COMPARISON.", 9.5, MUTED, "start", 0.14))
out.append(t(W - 40, H - 36, "docs/pitch/hardware-device-prompts.md", 9.5, MUTED, "end", 0.06))
out.append(ticks(40, 20, W - 80, H - 78, 8, MINT, 0.6))
out.append("</svg>")

open(PITCH / "assets" / "hardware-dimensions.svg", "w").write("\n".join(out))
print("wrote hardware-dimensions.svg")
