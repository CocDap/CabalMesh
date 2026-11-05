# Device art — generators

Every hardware image in the pitch is drawn here, in Python that emits SVG. No image model, no
binary export in the middle: the millimetre figures in
[`../hardware-device-prompts.md`](../hardware-device-prompts.md) are the same numbers these
scripts project, so a spec change is a one-line edit and the diff shows what moved.

```bash
cd docs/pitch/render
python3 dimensions.py      # → ../assets/hardware-dimensions.svg
python3 compute_model.py   # → ../assets/compute-model.svg
python3 isometric.py       # → ../assets/product-family.svg
python3 deck_art.py        # → ../src/deck-{shadowbox,nobody,two-boxes}.svg
```

| File | What it holds |
| --- | --- |
| `dimensions.py` | The 2D drawing helpers (dimension lines, ticks, panels, the palette) **and** the dimension sheet. The other three import from it. |
| `compute_model.py` | The three-column workload diagram and the six-step deal path. |
| `isometric.py` | The isometric engine — `Iso` projects millimetres to screen and `frame()` returns a transform that maps face-local millimetres onto any face, including the swung-back lid — plus the two devices and the product-family scene. |
| `deck_art.py` | Re-poses the same two devices into the three deck images. |

## Two rules the drawings follow

**No filters, no masks, no clip paths.** Blur and bloom are stacked low-opacity passes. This is
what lets one SVG render correctly in GitHub, in a PowerPoint import and in a PDF export.

**Face frames must have a positive determinant.** A frame built with the v-axis pointing *up*
mirrors everything drawn in it — text reads backwards and the eye-slash flips. Every vertical
face therefore takes its origin at the top and points v *down*: `frame((0, d, z_top), (1,0,0),
(0,0,-1))`. If a label ever renders mirrored, that is the bug.

## PNGs for the decks

pptxgenjs cannot embed SVG, so the three deck images are rasterised with headless Chrome at 2×:

```bash
cd docs/pitch/src
CH="/Applications/Google Chrome.app/Contents/MacOS/Google Chrome"
for f in deck-shadowbox:1480:400 deck-nobody:1480:400 deck-two-boxes:1400:825; do
  n=${f%%:*}; r=${f#*:}
  "$CH" --headless=new --disable-gpu --hide-scrollbars --force-device-scale-factor=2 \
        --window-size=${r%%:*},${r##*:} --screenshot=$n.png "file://$PWD/$n.svg"
done
```

Then rebuild the decks — `node build-pitch.js`, `node build-roadmap.js` in `../decks`, and
`NODE_PATH=../decks/node_modules node build.js` in `../src` — and re-export the PDFs with
LibreOffice as [`../decks/README.md`](../decks/README.md) describes.
