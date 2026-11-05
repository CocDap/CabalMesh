# -*- coding: utf-8 -*-
"""Deck banner art: wide strips for the two product cards, plus one two-up image.

Written to docs/pitch/src/ as SVG; convert-to-PNG happens in the shell step after.
"""
import os, sys, pathlib
sys.path.insert(0, str(pathlib.Path(__file__).resolve().parent))
PITCH = pathlib.Path(__file__).resolve().parents[1]

from isometric import nobody_box, shadowbox, grads, MINT

DEFS = ("<defs>"
        + grads("nb", top=("#2A3340", "#171E28")) + grads("nbL", top=("#222A35", "#141A22"))
        + grads("sb")
        + grads("pc", top=("#252B34", "#191E26"), left=("#1B2029", "#12161D"), right=("#141920", "#0C1016"))
        + '<linearGradient id="lidT" x1="0.2" y1="0" x2="0.8" y2="1">'
          '<stop offset="0" stop-color="#222A35"/><stop offset="1" stop-color="#10161E"/></linearGradient>'
        + '<radialGradient id="wall" cx="0.5" cy="0.42" r="0.85">'
          '<stop offset="0" stop-color="#161D28"/><stop offset="0.55" stop-color="#0C111A"/>'
          '<stop offset="1" stop-color="#04060A"/></radialGradient>'
        + '<linearGradient id="floor" x1="0" y1="0" x2="0" y2="1">'
          '<stop offset="0" stop-color="#0B1017"/><stop offset="0.55" stop-color="#070A10"/>'
          '<stop offset="1" stop-color="#04060A"/></linearGradient>'
        + '<linearGradient id="fade" x1="0" y1="0" x2="0" y2="1">'
          '<stop offset="0" stop-color="#070A10" stop-opacity="0.2"/>'
          '<stop offset="0.5" stop-color="#070A10" stop-opacity="0.85"/>'
          '<stop offset="1" stop-color="#04060A" stop-opacity="1"/></linearGradient>'
        + '<radialGradient id="haze" cx="0.5" cy="0.5" r="0.5">'
          f'<stop offset="0" stop-color="{MINT}" stop-opacity="0.13"/>'
          f'<stop offset="1" stop-color="{MINT}" stop-opacity="0"/></radialGradient>'
        + "</defs>")

def scene(path, w, h, horizon, haze, body, view=None):
    vb = view or f"0 0 {w} {h}"
    out = [f'<svg xmlns="http://www.w3.org/2000/svg" viewBox="{vb}" width="{w}" height="{h}">', DEFS,
           f'<rect x="{-w}" y="{-h}" width="{w*3}" height="{h*3}" fill="url(#wall)"/>',
           f'<rect x="{-w}" y="{horizon}" width="{w*3}" height="{h*3}" fill="url(#floor)"/>',
           f'<ellipse cx="{haze[0]}" cy="{haze[1]}" rx="{haze[2]}" ry="{haze[3]}" fill="url(#haze)"/>',
           f'<line x1="{-w}" y1="{horizon}" x2="{w*2}" y2="{horizon}" stroke="#141A24" stroke-width="1" opacity="0.8"/>',
           body, "</svg>"]
    open(path, "w").write("\n".join(out))
    print("wrote", path)

DST = str(PITCH / "src") + "/"

# ── 1 · the node, wide card strip ───────────────────────────────────────────
iso, svg, fp = shadowbox((980, 372), S=1.5)
body = "".join([
    f'<g opacity="0.16" transform="translate(0,{372*1.86}) scale(1,-0.86)">{svg}</g>',
    '<rect x="-100" y="330" width="1700" height="260" fill="url(#fade)"/>',
    iso.shadow(*fp), svg])
scene(DST + "deck-shadowbox.svg", 1480, 400, 214, (820, 250, 700, 190), body)

# ── 2 · the locker, wide card strip: the open rim, the parcel, the lid above
iso2, svg2, fp2 = nobody_box((880, 1000), S=0.75)
body2 = "".join([iso2.shadow(*fp2, spread=1.2), svg2])
scene(DST + "deck-nobody.svg", 1480, 400, 150, (760, 210, 660, 200), body2)

# ── 3 · both boxes, one image for the pitch deck's device card ──────────────
iso3, svg3, fp3 = nobody_box((560, 782))
iso4, svg4, fp4 = shadowbox((1230, 700))
body3 = "".join([
    f'<g opacity="0.16" transform="translate(0,{782*1.86}) scale(1,-0.86)">{svg3}</g>',
    f'<g opacity="0.16" transform="translate(0,{726*1.86}) scale(1,-0.86)">{svg4}</g>',
    '<rect x="-100" y="700" width="1900" height="360" fill="url(#fade)"/>',
    iso3.shadow(*fp3, spread=1.15), iso4.shadow(*fp4), svg3, svg4])
scene(DST + "deck-two-boxes.svg", 1400, 825, 430, (760, 470, 620, 170), body3, view="300 110 1120 660")
