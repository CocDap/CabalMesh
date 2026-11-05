# -*- coding: utf-8 -*-
"""docs/pitch/assets/product-family.svg — the two-product line, isometric.

Filter-free and mask-free on purpose: this has to survive GitHub's SVG sanitiser,
a PowerPoint import and a PDF export. Depth is face shading, edge highlights,
stacked shadow ellipses and a faded floor reflection.
"""
import os, sys, pathlib
sys.path.insert(0, str(pathlib.Path(__file__).resolve().parent))
PITCH = pathlib.Path(__file__).resolve().parents[1]

import math

C30, S30 = math.cos(math.radians(30)), 0.5
MONO = "ui-monospace, 'IBM Plex Mono', 'SFMono-Regular', 'Courier New', monospace"
TEXT, MUTED, MINT = "#E6EDF3", "#7D8A9C", "#00D9A3"
W, H = 1600, 1000
HORIZON = 430

def esc(s):
    return s.replace("&", "&amp;").replace("<", "&lt;").replace(">", "&gt;")

def t(x, y, s, size=11, fill=TEXT, anchor="start", track=0.0, weight=400, op=1.0):
    ls = f' letter-spacing="{track}em"' if track else ""
    return (f'<text x="{round(x,2)}" y="{round(y,2)}" font-family="{MONO}" font-size="{size}" fill="{fill}" '
            f'text-anchor="{anchor}" font-weight="{weight}" opacity="{op}"{ls}>{esc(s)}</text>')

def ln(p1, p2, stroke, sw=1, op=1.0):
    return (f'<line x1="{round(p1[0],2)}" y1="{round(p1[1],2)}" x2="{round(p2[0],2)}" '
            f'y2="{round(p2[1],2)}" stroke="{stroke}" stroke-width="{sw}" opacity="{op}"/>')


class Iso:
    def __init__(self, S):
        self.S, self.ox, self.oy = S, 0.0, 0.0

    def anchor(self, m, screen):
        self.ox, self.oy = 0.0, 0.0
        px, py = self.p(*m)
        self.ox, self.oy = screen[0] - px, screen[1] - py
        return self

    def p(self, x, y, z):
        return (self.ox + (x - y) * C30 * self.S, self.oy + ((x + y) * S30 - z) * self.S)

    def d(self, dx, dy, dz):
        return ((dx - dy) * C30 * self.S, ((dx + dy) * S30 - dz) * self.S)

    def quad(self, pts3, fill, stroke="none", sw=1, op=1.0):
        pts = " ".join(f"{round(a,2)},{round(b,2)}" for a, b in (self.p(*q) for q in pts3))
        return f'<polygon points="{pts}" fill="{fill}" stroke="{stroke}" stroke-width="{sw}" opacity="{op}"/>'

    def frame(self, o3, eu3, ev3):
        """A transform that maps face-local (u, v) millimetres onto the screen."""
        ox, oy = self.p(*o3)
        ax, ay = self.d(*eu3)
        bx, by = self.d(*ev3)
        return f"matrix({ax:.4f},{ay:.4f},{bx:.4f},{by:.4f},{ox:.3f},{oy:.3f})"

    def box(self, x, y, z, w, d, h, gid, rim=True):
        P = [(x, y, z), (x + w, y, z), (x + w, y + d, z), (x, y + d, z),
             (x, y, z + h), (x + w, y, z + h), (x + w, y + d, z + h), (x, y + d, z + h)]
        o = [self.quad([P[1], P[2], P[6], P[5]], f"url(#{gid}R)"),      # x = x+w
             self.quad([P[3], P[2], P[6], P[7]], f"url(#{gid}L)"),      # y = y+d
             self.quad([P[4], P[5], P[6], P[7]], f"url(#{gid}T)")]      # z = z+h
        e = [(P[4], P[5], "#4A596B", 1.4, 0.85), (P[4], P[7], "#3B485A", 1.4, 0.6),
             (P[7], P[6], "#5A6A7E", 1.2, 0.5), (P[5], P[6], "#2E3A48", 1.2, 0.75),
             (P[3], P[7], "#3B485A", 1.1, 0.5), (P[2], P[6], "#46546A", 1.4, 0.65)]
        for a, b, c, sw, op in e:
            o.append(ln(self.p(*a), self.p(*b), c, sw, op))
        if rim:
            o.append(ln(self.p(*P[5]), self.p(*P[1]), MINT, 1.2, 0.22))
            o.append(ln(self.p(*P[5]), self.p(*P[6]), MINT, 1.0, 0.14))
        return "".join(o)

    def shadow(self, x, y, w, d, spread=1.0):
        cx = (self.p(x, y + d, 0)[0] + self.p(x + w, y, 0)[0]) / 2
        cy = self.p(x + w, y + d, 0)[1] - 3
        rx = (w + d) * C30 * self.S * 0.62 * spread
        o = []
        for sx, op in ((1.5, 0.15), (1.15, 0.22), (0.85, 0.30), (0.55, 0.36)):
            o.append(f'<ellipse cx="{round(cx,2)}" cy="{round(cy,2)}" rx="{round(rx*sx,2)}" '
                     f'ry="{round(rx*sx*0.21,2)}" fill="#04060A" opacity="{op}"/>')
        return "".join(o)


def grads(gid, top=("#2C3644", "#1A222D"), left=("#171E28", "#0D131B"), right=("#11161E", "#080C12")):
    return (f'<linearGradient id="{gid}T" x1="0" y1="0" x2="0.6" y2="1">'
            f'<stop offset="0" stop-color="{top[0]}"/><stop offset="1" stop-color="{top[1]}"/></linearGradient>'
            f'<linearGradient id="{gid}L" x1="0" y1="0" x2="0.2" y2="1">'
            f'<stop offset="0" stop-color="{left[0]}"/><stop offset="1" stop-color="{left[1]}"/></linearGradient>'
            f'<linearGradient id="{gid}R" x1="1" y1="0" x2="0" y2="1">'
            f'<stop offset="0" stop-color="{right[0]}"/><stop offset="1" stop-color="{right[1]}"/></linearGradient>')

def glow_rect(x, y, w, h, sw=0.8, fill="none"):
    return "".join([
        f'<rect x="{x}" y="{y}" width="{w}" height="{h}" fill="none" stroke="{MINT}" stroke-width="{sw*3.2}" opacity="0.10"/>',
        f'<rect x="{x}" y="{y}" width="{w}" height="{h}" fill="none" stroke="{MINT}" stroke-width="{sw*1.9}" opacity="0.18"/>',
        f'<rect x="{x}" y="{y}" width="{w}" height="{h}" fill="{fill}" stroke="{MINT}" stroke-width="{sw}" opacity="0.95"/>'])

def glow_line(x1, y1, x2, y2, sw=0.8):
    return "".join([
        f'<line x1="{x1}" y1="{y1}" x2="{x2}" y2="{y2}" stroke="{MINT}" stroke-width="{sw*4}" opacity="0.09"/>',
        f'<line x1="{x1}" y1="{y1}" x2="{x2}" y2="{y2}" stroke="{MINT}" stroke-width="{sw*2}" opacity="0.20"/>',
        f'<line x1="{x1}" y1="{y1}" x2="{x2}" y2="{y2}" stroke="{MINT}" stroke-width="{sw}" opacity="0.95"/>'])

def eye(cx, cy, r, op=0.85, sw=None):
    sw = sw or r * 0.16
    return (f'<g opacity="{op}"><path d="M {cx-r} {cy} q {r} {-r*0.85} {2*r} 0 q {-r} {r*0.85} {-2*r} 0 z" '
            f'fill="none" stroke="{MINT}" stroke-width="{sw}"/>'
            f'<circle cx="{cx}" cy="{cy}" r="{r*0.34}" fill="none" stroke="{MINT}" stroke-width="{sw}"/>'
            f'<line x1="{cx-r*0.95}" y1="{cy+r*0.7}" x2="{cx+r*0.95}" y2="{cy-r*0.7}" '
            f'stroke="{MINT}" stroke-width="{sw*1.1}"/></g>')


# ── 1 · THE NOBODY BOX — 520 × 460 × 720, lid open ──────────────────────────
def nobody_box(anchor, S=0.40):
    """Body 0..520 x 0..460, legs to z=165, plinth 165..210, body 210..640, lid 640..720."""
    iso = Iso(S).anchor((520, 460, 0), anchor)
    o = []
    for lx, ly in ((20, 20), (430, 20), (20, 370), (430, 370)):        # legs
        o.append(iso.box(lx, ly, 0, 70, 70, 168, "nbL", rim=False))
    o.append(iso.box(8, 8, 165, 504, 444, 48, "nb"))                    # plinth
    o.append(iso.box(0, 0, 205, 520, 460, 435, "nb"))                   # body

    # front face detail: recessed panel, plank grooves, engraved eye
    fr = iso.frame((0, 460, 640), (1, 0, 0), (0, 0, -1))                # u along x, v down from the rim
    f = [f'<g transform="{fr}">',
         f'<rect x="58" y="60" width="404" height="330" fill="#090D14" stroke="#2E3A48" stroke-width="2.5"/>']
    for k in range(1, 8):
        f.append(f'<line x1="{58 + k*50.5}" y1="70" x2="{58 + k*50.5}" y2="380" stroke="#212A38" stroke-width="2.5"/>')
    f.append(eye(260, 220, 62, 0.8, sw=8))
    f.append("</g>")
    o.append("".join(f))

    # the opening: rim lip, dark cavity, a parcel most of the way up, mint rim glow
    o.append(iso.box(0, 0, 640, 520, 460, 18, "nb"))                    # rim lip
    o.append(iso.quad([(16, 16, 658), (504, 16, 658), (504, 444, 658), (16, 444, 658)], "#04060A"))
    # only the parcel's top face is ever visible from outside — drawing the whole
    # box painted its unclipped side faces straight over the body's front panel
    o.append(iso.quad([(70, 70, 600), (450, 70, 600), (450, 390, 600), (70, 390, 600)], "url(#pcT)"))
    o.append(ln(iso.p(70, 70, 600), iso.p(450, 70, 600), "#39434F", 1, 0.5))
    o.append(ln(iso.p(70, 70, 600), iso.p(70, 390, 600), "#2A3441", 1, 0.4))
    for inset, op, sw in ((12, 0.08, 3.0), (6, 0.18, 3.0), (0, 0.85, 1.4)):
        pts = [(16 - inset, 16 - inset, 658), (504 + inset, 16 - inset, 658),
               (504 + inset, 444 + inset, 658), (16 - inset, 444 + inset, 658)]
        o.append(iso.quad(pts, "none", MINT, sw, op))

    # ── the lid, hinged along y = 0 at z = 658, swung back 104°
    th = math.radians(104)
    ct, st = math.cos(th), math.sin(th)
    def L(x, y, z):
        """Lid-local (x, y from hinge, z above hinge) → model."""
        return (x, y * ct - z * st, 658 + y * st + z * ct)
    lw, ld, lt = 536, 468, 78
    C = {(a, b, c): L(-8 + a * lw, b * ld, c * lt) for a in (0, 1) for b in (0, 1) for c in (0, 1)}
    inner = [C[(0, 0, 0)], C[(1, 0, 0)], C[(1, 1, 0)], C[(0, 1, 0)]]
    o.append(iso.quad([C[(0, 0, 1)], C[(1, 0, 1)], C[(1, 1, 1)], C[(0, 1, 1)]], "#0B1017"))   # outer shell
    o.append(iso.quad([C[(0, 1, 0)], C[(1, 1, 0)], C[(1, 1, 1)], C[(0, 1, 1)]], "#141B24"))   # top edge
    o.append(iso.quad([C[(1, 0, 0)], C[(1, 1, 0)], C[(1, 1, 1)], C[(1, 0, 1)]], "#0E141C"))   # right edge
    o.append(iso.quad(inner, "url(#lidT)"))                                                   # inner face
    fr2 = iso.frame(L(-8, ld, 0), (1, 0, 0), (0, -ct, -st))
    li = [f'<g transform="{fr2}">']
    li.append(f'<rect x="26" y="26" width="484" height="416" fill="none" stroke="#26303D" stroke-width="3"/>')
    for row in range(4):                                                # moulded stiffening ribs
        for col in range(3):
            li.append(f'<rect x="{58 + col*150}" y="{376 - row*96}" width="118" height="30" rx="15" '
                      f'fill="#161D27" stroke="#2A3441" stroke-width="1.5"/>')
    li.append(f'<circle cx="268" cy="218" r="46" fill="none" stroke="#26303D" stroke-width="2"/>')
    li.append(f'<rect x="228" y="42" width="80" height="30" rx="4" fill="#0A0F16" stroke="#39434F" stroke-width="2"/>')
    li.append(eye(268, 218, 26, 0.5, sw=3))
    li.append("</g>")
    o.append("".join(li))

    # latch block on the body's front rim, released — mint ring lit
    fr3 = iso.frame((0, 460, 640), (1, 0, 0), (0, 0, -1))
    lt_ = [f'<g transform="{fr3}">',
           f'<rect x="228" y="-1" width="64" height="44" fill="#0F141C" stroke="#39434F" stroke-width="2"/>',
           glow_rect(238, 9, 44, 24, 2), "</g>"]
    o.append("".join(lt_))
    return iso, "".join(o), (0, 0, 520, 460)


# ── 2 · SHADOWBOX — 220 × 160 × 70, the one node ────────────────────────────
def shadowbox(anchor, S=1.15):
    iso = Iso(S).anchor((220, 160, 0), anchor)
    o = [iso.box(0, 0, 0, 220, 160, 70, "sb")]
    ant = (iso.p(206, 12, 70), iso.p(206, 12, 120))                     # stub antenna
    o.append(ln(ant[0], ant[1], "#2A3442", 5.4))
    o.append(ln(ant[0], ant[1], "#54637A", 2.4))
    o.append(f'<circle cx="{round(ant[1][0],2)}" cy="{round(ant[1][1],2)}" r="3" fill="#3B485A"/>')

    top = [f'<g transform="{iso.frame((0,0,70), (1,0,0), (0,1,0))}">']
    for k in range(3):                                                  # RADIO / CRYPTO / POWER bays
        x = 16 + k * 62
        top.append(f'<rect x="{x}" y="10" width="46" height="16" fill="#080C12" stroke="#232C38" stroke-width="1"/>')
        top.append(glow_line(x + 2, 26, x + 44, 26, 1))
    top.append(f'<rect x="78" y="10" width="46" height="16" fill="#121821" stroke="#2E3A48" stroke-width="1"/>')
    top.append(glow_rect(78, 10, 46, 16, 0.9))                          # one module half-inserted
    top.append(f'<rect x="52" y="46" width="116" height="44" fill="#05080C" stroke="#232C38" stroke-width="1"/>')
    top.append(glow_rect(52, 46, 116, 44, 0.9))
    top.append(f'<text x="110" y="70" font-family="{MONO}" font-size="15" fill="{MINT}" '
               f'text-anchor="middle" letter-spacing="0.18em">PROVEN</text>')
    for k in range(8):                                                  # proof step lamps
        x = 54 + k * 14.4
        if k < 5:
            top.append(f'<rect x="{x}" y="104" width="9" height="9" fill="{MINT}" opacity="0.9"/>')
            top.append(f'<rect x="{x-1.5}" y="102.5" width="12" height="12" fill="{MINT}" opacity="0.12"/>')
        else:
            top.append(f'<rect x="{x}" y="104" width="9" height="9" fill="none" stroke="#2B3542" stroke-width="1"/>')
    top.append(eye(196, 132, 11, 0.7))
    top.append("</g>")
    o.append("".join(top))

    front = [f'<g transform="{iso.frame((0,160,70), (1,0,0), (0,0,-1))}">']
    for k in range(6):                                                  # the model's thinking bar
        x = 16 + k * 11
        h = 9 - (k % 3) * 2
        front.append(f'<rect x="{x}" y="40" width="7" height="{h}" fill="{MINT}" opacity="{0.95-k*0.1}"/>')
        front.append(f'<rect x="{x-1}" y="39" width="9" height="{h+2}" fill="{MINT}" opacity="0.10"/>')
    front.append(f'<rect x="14" y="38" width="76" height="18" fill="none" stroke="#222B36" stroke-width="1"/>')
    front.append(f'<rect x="120" y="42" width="20" height="12" fill="#0A0F16" stroke="#39424F" stroke-width="1"/>')
    front.append(f'<rect x="121" y="43" width="18" height="5" fill="#2B3542"/>')      # NET, flicked off
    front.append(f'<text x="146" y="52" font-family="{MONO}" font-size="9" fill="{MUTED}" letter-spacing="0.2em">NET</text>')
    front.append(f'<rect x="176" y="42" width="30" height="12" fill="#080C12" stroke="#232C38" stroke-width="1"/>')
    front.append(glow_line(178, 48, 204, 48, 1))                        # shuttered input slot
    front.append("</g>")
    o.append("".join(front))

    side = [f'<g transform="{iso.frame((220,0,0), (0,1,0), (0,0,1))}">']
    for k in range(11):                                                 # heat-sink fins
        x = 8 + k * 13.6
        side.append(f'<line x1="{x}" y1="8" x2="{x}" y2="62" stroke="#04070B" stroke-width="4" opacity="0.92"/>')
        side.append(f'<line x1="{x+2.4}" y1="8" x2="{x+2.4}" y2="62" stroke="#3D4754" stroke-width="1.3" opacity="0.6"/>')
    side.append("</g>")
    o.append("".join(side))
    return iso, "".join(o), (0, 0, 220, 160)


def main():
    nb_iso, nb_svg, nb_fp = nobody_box((590, 812))
    sb_iso, sb_svg, sb_fp = shadowbox((1330, 742))

    out = [f'<svg xmlns="http://www.w3.org/2000/svg" viewBox="0 0 {W} {H}" width="{W}" height="{H}" '
           f'role="img" aria-label="CabalMesh product line — the node and the escrow locker, isometric concept render">',
           "<defs>", grads("nb", top=("#2A3340", "#171E28")), grads("nbL", top=("#222A35", "#141A22")),
           grads("sb"), grads("pc", top=("#252B34", "#191E26"), left=("#1B2029", "#12161D"), right=("#141920", "#0C1016")),
           '<linearGradient id="lidT" x1="0.2" y1="0" x2="0.8" y2="1">'
           '<stop offset="0" stop-color="#222A35"/><stop offset="1" stop-color="#10161E"/></linearGradient>',
           '<radialGradient id="wall" cx="0.5" cy="0.42" r="0.78">'
           '<stop offset="0" stop-color="#161D28"/><stop offset="0.55" stop-color="#0C111A"/>'
           '<stop offset="1" stop-color="#04060A"/></radialGradient>',
           '<linearGradient id="floor" x1="0" y1="0" x2="0" y2="1">'
           '<stop offset="0" stop-color="#0B1017"/><stop offset="0.5" stop-color="#070A10"/>'
           '<stop offset="1" stop-color="#04060A"/></linearGradient>',
           '<linearGradient id="fade" x1="0" y1="0" x2="0" y2="1">'
           '<stop offset="0" stop-color="#070A10" stop-opacity="0.2"/>'
           '<stop offset="0.5" stop-color="#070A10" stop-opacity="0.85"/>'
           '<stop offset="1" stop-color="#04060A" stop-opacity="1"/></linearGradient>',
           '<radialGradient id="haze" cx="0.5" cy="0.5" r="0.5">'
           f'<stop offset="0" stop-color="{MINT}" stop-opacity="0.10"/>'
           f'<stop offset="1" stop-color="{MINT}" stop-opacity="0"/></radialGradient>',
           "</defs>",
           f'<rect x="0" y="0" width="{W}" height="{H}" fill="url(#wall)"/>',
           f'<rect x="0" y="{HORIZON}" width="{W}" height="{H-HORIZON}" fill="url(#floor)"/>',
           f'<ellipse cx="760" cy="{HORIZON+40}" rx="640" ry="170" fill="url(#haze)"/>',
           f'<line x1="0" y1="{HORIZON}" x2="{W}" y2="{HORIZON}" stroke="#141A24" stroke-width="1" opacity="0.8"/>']

    for base, svg in ((812, nb_svg), (742, sb_svg)):
        out.append(f'<g opacity="0.16" transform="translate(0,{round(base*1.86,2)}) scale(1,-0.86)">{svg}</g>')
    out.append(f'<rect x="0" y="700" width="{W}" height="300" fill="url(#fade)"/>')
    out.append(nb_iso.shadow(*nb_fp, spread=1.15))
    out.append(sb_iso.shadow(*sb_fp))
    out.append(nb_svg)
    out.append(sb_svg)

    out.append(t(70, 74, "CABALMESH · THE TWO BOXES", 20, TEXT, "start", 0.20, 500))
    out.append(t(70, 98, "One node on the desk. One locker at the door. Nothing else to buy.", 11.5, MUTED, "start", 0.04))
    out.append(t(W - 70, 74, "CONCEPT RENDER · NOT A PHOTOGRAPH", 10, MINT, "end", 0.20))
    out.append(t(W - 70, 96, "Shown at different scales — hardware-dimensions.svg has the true size relationship", 10, MUTED, "end", 0.04))
    out.append(f'<line x1="70" y1="120" x2="{W-70}" y2="120" stroke="#141A24" stroke-width="1"/>')

    caps = [(590, 812, "THE NOBODY BOX", "520 × 460 × 720 mm · ≈ 88 L", "PHYSICAL ESCROW LOCKER", "0.40 px/mm"),
            (1330, 742, "SHADOWBOX", "220 × 160 × 70 mm · 1.9 kg", "MESH · MODEL · PROVER", "1.15 px/mm")]
    for x, base, name, dims, role, scale in caps:
        out.append(ln((x, base + 16), (x, 886), "#1C2431", 1, 0.9))
        out.append(f'<circle cx="{x}" cy="886" r="2" fill="{MINT}" opacity="0.8"/>')
        out.append(t(x, 916, name, 14, TEXT, "middle", 0.16, 500))
        out.append(t(x, 936, dims, 10.5, MUTED, "middle", 0.04))
        out.append(t(x, 956, role, 9, MINT, "middle", 0.22))

    out.append(f'<line x1="70" y1="{H-40}" x2="{W-70}" y2="{H-40}" stroke="#141A24" stroke-width="1"/>')
    out.append(t(70, H - 18, "PHASE 2 CONCEPT — NO SILICON SELECTED, NO BOARD, NO TOOLING.", 9.5, MUTED, "start", 0.14))
    out.append(t(W - 70, H - 18, "#0B0E14 · #00D9A3", 9.5, MUTED, "end", 0.14))
    out.append("</svg>")

    open(PITCH / "assets" / "product-family.svg", "w").write("\n".join(out))
    print("wrote product-family.svg")


if __name__ == "__main__":
    main()
