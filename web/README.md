# CabalMesh landing page

React 19 + Vite + TypeScript. The build prerenders the page to static HTML, then React hydrates it.

```bash
cd web
npm install
npm run dev       # http://localhost:8765
npm run build     # type-check, bundle, prerender into dist/
npm run preview   # serve dist/ on http://localhost:8765
```

## Layout

- `src/App.tsx`: the page, section by section.
- `src/sections/`: one component per section: `Nav`, `Hero`, `AppFrame`, `HowItWorks`, `Sequence`, `WhyNear`,
  `Roadmap`, `Closing`; and `BleRibbon`, the animated BLE wave ribbon behind the hero scene and beside the roadmap.
- `src/scenes/`: the drawn SVG pictures:
  - `HeroScene`: two offline phones sign locally; a gateway with internet submits the pair to NEAR Intents.
    It is a 3D scene drawn as SVG: an orthographic camera sways and follows the mouse, and one clock replays
    the trade every 12 seconds. Without motion it rests on the settled frame, which is also what is prerendered.
  - `StepSceneBid`, `StepSceneSign`, `StepSceneSettle`: the three steps in "A peer-to-peer trade in 3 steps".
  - `SequenceDiagram`: the "How CabalMesh Trades" sequence (the $3.70 run and its demo transaction).

  The scenes follow the $3.78 run in `docs/verification/ble-simulator-settlement`: the seller sells 0.1 wNEAR to
  the buyer, with a fee of 0.01 USDC per side and a 0.02 USDC gateway reward. Edit the SVG in these files directly.
- `src/styles/page.css`: brand fonts, palette and the page's own rules. It loads after the kit's stylesheets.
- `src/main.tsx`: hydrates the prerendered page, or renders it in dev. Once React has committed, it starts the kit.
- `src/entry-server.tsx` and `scripts/prerender.mjs`: render the page to HTML into `dist/index.html` at build time.
- `public/assets/`: the parts of the taste-skill kit the page uses (layout, motion, the window, chat, code and
  terminal fragments, the pinned-chapters moment). It is plain JS and CSS that scans the DOM on start-up, linked
  from `index.html`. Do not edit it; copy a fresh kit instead.
- `public/brand/`: the app's own fonts (Silkscreen, IBM Plex Mono), symbol mark and oracle from `src/ds/assets`,
  and one screenshot (`A-agreed.jpg`, in the nav menu) from `docs/verification/ble-simulator-settlement`, resized
  to 720px JPEG. `oracle@3x.png` is the oracle scaled 3x with nearest-neighbour; no section uses it now.
  `ble-ribbon.webp` is the BLE wave loop (ble-wave-xin-chao-540x540.gif: 80 frames, 20fps, 4s) with its teal
  ground keyed out, so the ribbon stands on the page with no box around it; `ble-ribbon-still.webp` is its
  first frame, shown under reduced motion. To remake both from a new GIF:

  ```bash
  ffmpeg -i wave.gif -vf "format=rgba,lumakey=threshold=0:tolerance=0.36:softness=0.06" fr/f%03d.png
  img2webp -loop 0 -d 50 -lossy -q 58 -m 6 -min_size fr/f*.png -o public/brand/ble-ribbon.webp
  cwebp -q 82 -alpha_q 90 fr/f001.png -o public/brand/ble-ribbon-still.webp
  ```
- `public/share.jpg`: the hero at 1440x900, for link previews.

The roadmap rows are copied by hand from the README checklist; update both together.

Markup is static: the components hold no state (except `HeroScene`, which runs its own frame clock), because
the kit adds the behaviour (menus, motion, the pinned
chapters, the terminal and code replays) to the DOM after React renders it. Keep the class names and `data-*`
attributes the kit reads.

## Checks

The taste-skill checks run on the build (`<skill>` is the taste-skill plugin's skill folder):

```bash
npm run build
node <skill>/scripts/gate.mjs dist/index.html
node <skill>/scripts/shoot.mjs --url dist/index.html --root dist --w 1440 --out shots/1440.png --profile .taste-skill/chrome-check
```
