#!/usr/bin/env python3
"""Resample design-system rasters to exact device sizes.

WHY THIS EXISTS

The brand mandates ``image-rendering: pixelated`` and "scale by whole
multiples". The delivered glyphs are 264x264 board crops, and the nominal
render size is 20px. On a 3x device that is a 4.4x *downscale* — and
nearest-neighbour is correct for upscaling but destructive downscaling: thin
1px strokes drop out entirely and the survivors shimmer as a list scrolls.

So the resampling happens once, here, with a proper box filter, producing exact
@1x/@2x/@3x variants. The browser then renders them at their native size with
``pixelated`` doing what it is good at — never resizing at all.

Run from the repo root:

    python3 scripts/resample-ds-assets.py

Idempotent: existing variants are overwritten from the original each time, so
this never resamples an already-resampled file.
"""

from __future__ import annotations

import sys
from pathlib import Path

try:
    from PIL import Image
except ImportError:  # pragma: no cover - developer environment
    sys.exit("Pillow is required: pip install Pillow")

ROOT = Path(__file__).resolve().parent.parent
ASSETS = ROOT / "src" / "ds" / "assets"

# Nominal CSS sizes from the brand book. Glyphs render at 20px; the marks and
# the character portrait are hero-scale and keep more detail.
TARGETS: dict[str, int] = {
    "icons": 20,
    "logo": 96,
    "characters": 160,
}


def variants(source: Path, base_size: int) -> list[tuple[Path, int]]:
    """(destination, pixel size) for each device density."""
    return [
        (source.with_name(f"{source.stem}@{density}x.png"), base_size * density)
        for density in (1, 2, 3)
    ]


def resample(source: Path, base_size: int) -> int:
    with Image.open(source) as original:
        image = original.convert("RGBA")
        written = 0
        for destination, size in variants(source, base_size):
            # Preserve aspect ratio: these are not all square, and stretching a
            # mark to fit a box would distort the brand.
            ratio = size / max(image.width, image.height)
            target = (
                max(1, round(image.width * ratio)),
                max(1, round(image.height * ratio)),
            )
            # LANCZOS, not NEAREST. Nearest here is what drops the strokes.
            image.resize(target, Image.LANCZOS).save(destination, optimize=True)
            written += 1
    return written


def main() -> int:
    if not ASSETS.is_dir():
        sys.exit(f"asset directory not found: {ASSETS}")

    total = 0
    for group, base_size in TARGETS.items():
        directory = ASSETS / group
        if not directory.is_dir():
            continue
        # Skip already-generated variants so re-running resamples originals.
        for source in sorted(directory.glob("*.png")):
            if "@" in source.stem:
                continue
            total += resample(source, base_size)
        print(f"{group}: {base_size}px base")

    print(f"wrote {total} variants")
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
