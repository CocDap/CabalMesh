#!/usr/bin/env python3
"""Stage design-system rasters for the mobile build.

The vendored `Icon` component requests `{basePath}/{name}.png` and has no
srcset support, so whatever sits at that filename is what every density gets.
The delivered originals are 264x264 board crops — rendering those at a 20px CSS
size is a 4.4x downscale, and `pixelated` is destructive downscaling: thin
strokes drop out and shimmer as a list scrolls.

So the staged copy puts the **@3x variant** at the plain filename. At 20 CSS px
on a 3x device that is 60 device pixels against a 60px source — exactly 1:1, no
resampling at all, which is what `pixelated` is good at. On a 2x device it is a
1.5x downscale: slightly soft, but far better than 4.4x.

The originals stay in src/ds/assets so resampling can be redone; only the
staged copy is substituted.

Run from the repo root:

    python3 scripts/stage-ds-assets.py
"""

from __future__ import annotations

import shutil
from pathlib import Path

ROOT = Path(__file__).resolve().parent.parent
SOURCE = ROOT / "src" / "ds" / "assets"
STAGED = ROOT / "src" / "mobile-entry" / "public" / "ds-assets"


def main() -> None:
    if STAGED.exists():
        shutil.rmtree(STAGED)
    shutil.copytree(SOURCE, STAGED)

    substituted = 0
    for variant in STAGED.rglob("*@3x.png"):
        plain = variant.with_name(variant.name.replace("@3x", ""))
        shutil.copyfile(variant, plain)
        substituted += 1

    # Density variants are dead weight in the bundle once the plain name holds
    # the right pixels — Icon never asks for them.
    removed = 0
    for variant in STAGED.rglob("*@*x.png"):
        variant.unlink()
        removed += 1

    print(f"staged {substituted} rasters at render size, dropped {removed} unused variants")


if __name__ == "__main__":
    main()
