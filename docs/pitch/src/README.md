# Deck source

Regenerates `../CabalMesh-Project-Plan.pptx`.

```bash
npm install pptxgenjs        # plus react-icons react react-dom sharp, for gen_icons2.js
node gen_icons2.js           # only when adding a new icon — writes icons2/
node build.js                # writes CabalMesh-Project-Plan.pptx
```

## Layout

| File | Holds | Never holds |
|---|---|---|
| `theme.js` | Colours, type scale, spacing, drawing primitives, slide factory | Any copy |
| `content.js` | Every word in the deck | Any coordinate |
| `build.js` | One function per slide, composing the two | Hard-coded strings |

**Editing copy?** Only touch `content.js`.
**Changing the look?** Only touch `theme.js`.
**Moving or adding a slide?** Add the render function in `build.js` and drop it into the
array at the bottom of `main()` — page numbers are assigned automatically by `createDeck()`,
so nothing needs renumbering.

Phase windows, titles and summaries live once in `content.js`'s `PHASES` array; the
timeline, overview and phase-detail slides all read from it, so a date change propagates
everywhere on its own.
