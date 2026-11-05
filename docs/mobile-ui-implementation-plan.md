# CabalMesh — Mobile UI/UX Implementation Plan

Source: `~/Downloads/iPhone UIUX prototype/` (verified 2026-08-01)
Tauri specifics verified against primary sources on 2026-08-01 — see the architecture plan and its [`research note`](./research/tauri-mobile-plan-verification-2026-08-01.md).
Companion doc: [`mobile-architecture-plan.md`](./mobile-architecture-plan.md) — the Rust/`src-tauri` side. This plan consumes the command + event contract defined there.
Rust work in §7 follows the `rust-skills` rule set; rule ids are cited inline.

---

## 1. What actually shipped

```
iPhone UIUX prototype/
├── Cabal Mesh Mobile.dc.html          76 KB   the 10-screen prototype
├── support.js                         69 KB   dc-runtime (prototype only — do NOT ship)
└── _ds/cabalmesh-design-system-…/
    ├── readme.md                      18 KB   brand law: voice, colour, type, motion, anti-patterns
    ├── styles.css                             @import list, single entry point
    ├── tokens/                        162 custom properties across 6 files
    ├── _ds_bundle.js                 138 KB   30 components, precompiled, unminified
    ├── _ds_manifest.json                      name → sourcePath map
    └── _adherence.oxlintrc.json       25 KB   per-component prop + enum contracts
```

Assets (376 KB total, all present, all referenced):

| Group | Files | Status |
|---|---|---|
| `assets/icons/` | 14 protocol glyphs — `node agent intent mesh proof escrow vault reputation signal encrypt identity bridge relayer log` | ✅ complete |
| `assets/logo/` | `hero-lockup` `minimal-mark` `symbol-mark` `oracle-emblem` | ✅ all 4 the screens use (3 more exist in the full system, unused here) |
| `assets/characters/` | `oracle` | ✅ the only archetype the screens use (7 more exist upstream) |
| `assets/textures/` | `grid` `dither` | ✅ (`glitch` absent — hero-only, unused) |

**Component inventory (30, from `_ds_manifest.json`):**

| Group | Components |
|---|---|
| core | `Panel` `CornerTicks` `Button` `IconButton` `Badge` `StatusDot` `Divider` `Logo` `LogoType` `Icon` `MESH_ICONS` |
| forms | `Field` `Input` `Select` `Checkbox` `Radio` `RadioGroup` `Switch` |
| data | `StatBlock` `StatInline` `Meter` `DataTable` `Terminal` |
| navigation | `NavBar` `Tabs` |
| feedback | `Dialog` `Toast` `ToastStack` `Tooltip` |
| brand | `TextureField` `CharacterPortrait` `CHARACTERS` |

The adherence file gives every component's exact prop list and enum domain — e.g. `Button tone ∈ primary|secondary|ghost|danger|signal`, `StatusDot tone ∈ online|alert|info|idle|offline`, `TerminalLine tone ∈ out|dim|ok|err|info|loud`. This is a hard contract; §3.4 keeps it enforced in CI.

**Prototype frame:** 390 × 844 (iPhone 13/14 logical), with 52px header and 48px five-destination bar (`HOME INTENTS NODES VAULT PROFILE`). Those become minimum block sizes in product, not fixed heights at large text.

---

## 2. Conflicts between the design system and a real mobile app

These are not style opinions — each one breaks something on a device. All six are resolved. C4, C5 and C6 were revised after the 2026-08-01 review (§11) — where a "resolution" turned out to be wrong, the section says so rather than quietly restating it.

### C1 — Fonts load from a CDN. Fatal for an offline-first app.

`tokens/fonts.css` line 9:
```css
@import url("https://fonts.googleapis.com/css2?family=Silkscreen…&family=IBM+Plex+Mono…");
```

A Tauri mobile webview under a strict CSP cannot fetch this, and the whole product premise is *"operating offline"*. On first launch without network the app renders in the fallback `ui-monospace` — the brand's single most defining property gone.

**Fix:** self-host. Download Silkscreen 400/700 and IBM Plex Mono 400/500 as woff2 into `src/ds/assets/fonts/`, rewrite `fonts.css` to local `@font-face`, subset to `latin` only. Budget ~60 KB total. Drop the italic and 200/300/600 weights the screens never use.

### C2 — Pixel glyphs are 264×264 and get rendered at 20px.

`readme.md` mandates `image-rendering: pixelated` and "scale by whole multiples". The actual PNGs are **264×264** board crops. At the nominal 20px CSS size on a 3× device that is a **4.4× nearest-neighbour downscale** — `pixelated` is correct for *upscaling* and destructive for downscaling. Thin 1px strokes drop out and shimmer as the list scrolls.

**Fix:** pre-resample the 14 glyphs once, with a proper box filter, to exact device sizes — `icon@1x = 20px`, `@2x = 40px`, `@3x = 60px` — ship as a `srcset`, then render `pixelated`. Same for `minimal-mark`/`symbol-mark`/`oracle-emblem`/`oracle`. One-off script, checked into `scripts/`.

### C3 — Hover inverts. There is no hover on a phone.

The brand's signature interaction is *"Hover (button) — inverts, fills solid white, text goes black."* On touch, `:hover` sticks after a tap on iOS Safari/WKWebView, leaving buttons stuck inverted.

**Fix:** wrap every hover rule in `@media (hover: hover) and (pointer: fine)`. On touch, the brand's own press state (`opacity: 0.72`, already defined) carries the whole feedback load. Add `-webkit-tap-highlight-color: transparent` so the OS blue flash doesn't violate the accent budget.

### C4 — Touch targets are ~35px. iOS HIG wants 44pt, Android 48dp.

`--btn-pad-y: 12px` + 11px display type ≈ 35px tall. The prototype destination bar is 48px at 100%, but its items still need real targets and intrinsic growth. Buttons, `IconButton sm`, in-screen tab underlines, the privacy-level stops and the `MAX` affordance are all under.

**Default fix:** make the layout box itself at least 44pt on iOS / 48dp on Android with real padding and intrinsic sizing. That is the only approach that also enlarges the explore-by-touch/accessibility bounds reliably. For rare visual exceptions, use a positioned wrapper to expand only the pointer hit area:
```css
.cm-touch-hitbox{position:relative;}
.cm-touch-hitbox::after{content:"";position:absolute;inset:-8px;}
```
Pseudo-elements are unreliable on replaced/native form controls, so wrap those controls rather than attaching `::after` directly. The primitive owns the wrapper; screens never hand-roll it.

**Caveat — this can steal a neighbour's taps.** An 8px outward expansion on two controls sitting 8px apart makes their hit areas meet and, depending on stacking order, one swallows the other's edge. Two rules keep it honest:
- Minimum **16px gap between two 8px-expanded controls** (`--space-6`), so expanded areas do not overlap. The privacy-level stops and the `new` screen's segmented control are the tight spots — check both explicitly.
- Prefer real padding where the layout allows it; the pseudo-element is for cases where growing the box would break the composition, not the default.

Verify by hit-testing, not by eye: tap 4px inside each control's edge and confirm the right handler fires.

### C5 — Type scale starts at 9px. **REVISED: reflow to 200%, no clamp, no zoom lock**

`--text-2xs: 9px`, `--text-xs: 10px`, body `13px`. Micro-labels at 9px with `0.32em` tracking are legible on a 3× screen, but the tokens ignore Dynamic Type / Android font scale entirely. (Precision: SC 1.4.4 does **not** forbid any particular font size — it requires text to *resize to 200%* without losing content or function. A 9px base is not itself a failure; a layout that cannot grow is.) The brand explicitly wants "instrumentation type, not marketing type", so simply enlarging everything is off-brand.

**A previous revision "resolved" this by clamping OS scaling to [1.0, 1.3] and locking pinch-zoom. That is not conformance — it is a decision to fail WCAG AA, and labelling it resolved was wrong.**

[WCAG 2.2 SC 1.4.4 Resize Text](https://www.w3.org/WAI/WCAG22/Understanding/resize-text.html) (Level AA) requires text to scale to **200%** without loss of content or functionality. A 130% ceiling plus `user-scalable=no` fails it twice over: once on the ceiling, once by removing the user's own escape hatch.

**Decision — support 200%, and reflow rather than clamp.**

The app reads the OS font-scale setting and applies it **unclamped**. Pinch-zoom stays enabled. The 390px layouts have to survive 200%, which is real work the earlier clamp was avoiding rather than solving.

```css
:root{ --type-scale: 1; }        /* set from the OS setting; also re-read on resume */
:root{
  --text-2xs: calc(9px  * var(--type-scale));
  --text-xs:  calc(10px * var(--type-scale));
  --text-sm:  calc(11px * var(--type-scale));
  --text-base:calc(13px * var(--type-scale));
  /* …through --text-5xl */
}
```

Reading the setting — a `type-scale` mobile plugin following the documented shape (`@TauriPlugin` + `@Command` + `invoke.resolve` on Android, `Plugin` subclass + `@objc public func` on iOS, `run_mobile_plugin("getScale", ())` from Rust; see architecture plan §5.3.1):
- **iOS** — `-webkit-text-size-adjust` is unreliable in WKWebView for this. Compute a ratio with `UIFontMetrics(forTextStyle: .body).scaledValue(for: basePointSize) / basePointSize` under the current trait collection; do not maintain a hand-written category table or set a maximum point size.
- **Android** — `Resources.getConfiguration().fontScale`.
- **Fallback** — plugin unavailable → `--type-scale: 1`. Never worse than brand-exact.
- `type-scale:allow-get-scale` is the only native-plugin grant added to `capabilities/mobile.json`. The frontend uses it for one initial direct read and catches ACL/plugin errors to fall back to 1; `keystore`/`multicast-lock` get **no** webview grant (architecture §5.3.1).
- **Not boot-time-only.** Users change system font size while the app is backgrounded. Rust re-reads the scale through its plugin handle on the verified native `Resumed` path (Tauri 2.11 — architecture §2.7, no custom lifecycle plugin) and emits `TypeScaleChanged { scale }`; `--type-scale` updates live. The two callers are deliberate: frontend at boot, Rust on resume.

What this forces, all of it in Phase B before any screen is built:

- **`min-block-size`, never `height`, on anything containing text** — the 52px header, 48px primary nav, stat rows, list rows, buttons, badges. A fixed height at 200% clips descenders on every one of them.
- **Real padding and intrinsic sizing over fixed boxes.** Panels grow; the graph-paper grid is a background, not a constraint on box heights.
- **Layouts must reflow, not just stretch.** At 200% the `home` stat trio goes from a row to a stack; `nodes` list rows go two-line; `new`'s segmented `I WANT TO` control wraps. Decide each of these deliberately in Phase B, or discover them as bugs in Phase F.
- **The app navigation is the hardest case, and the decision is now explicit.** Five uppercase labels — `HOME INTENTS NODES VAULT PROFILE` — in 390px at 200% is roughly 2.5× the available width. Keep icons as the primary glyphs **and retain every visible label** in a horizontally scrollable `<nav>` at large text sizes; scroll the active destination fully into view and expose beginning/end affordances. At normal scale it remains the five-item fixed bar. Hiding/truncating labels was rejected because `aria-label` helps screen-reader users, not sighted low-vision users, and these custom glyphs are not universal. `MORE` remains rejected because it hides primary destinations.
- **Terminal panes cap their visible line count by available height**, not a fixed 4 — at 200% four lines do not fit.
- **Test at 100%, 130% and 200%.** Pixel-diffs against the prototype run at 100% only; 130% and 200% are checked for clipping, overlap and reachability, never for exactness.

The brand survives this better than it looks: the system is monospaced, zero-radius, border-driven and already builds everything from stacked panels. It has no fragile optical alignment to lose. Tracking (`0.32em` on micro-labels) is the one property to watch — it is in `em`, so it scales with the text and stays proportionally correct.

Cost ~2 days in Phase B. Retrofitting after 10 screens exist is 4+ days.

*(Your answer was "support for iOS and Android app" — read as: follow platform accessibility norms. This revision takes that seriously rather than half-way. If you want brand-exact 9px with scaling locked, that is a legitimate call for a demo build — but it must be recorded as "knowingly non-conformant with WCAG AA", not as resolved.)*

### C6 — The repo's current stack fights the design system. **RESOLVED: two entry points**

| Current dep | Conflict |
|---|---|
| `tailwindcss@3` | The DS is inline style objects + CSS custom properties. The adherence lint bans raw px and raw hex — which is what every Tailwind utility compiles to. Two competing systems on one page, plus Tailwind's preflight fighting `base.css`'s reset. |
| `framer-motion@12` | The brand forbids spring, bounce and overshoot outright. Every motion in the system is `steps(6,end)` / `steps(3,end)` CSS. 50 KB of spring physics for animations that must not spring. |

**Decision — desktop is frozen, so both stay in the repo but never reach the mobile bundle.**

**Correction to an earlier draft.** It proposed emitting `index.html` + `mobile.html` into one `dist/` and pointing `frontendDist` at the mobile entry. That cannot work: `frontendDist` names a **directory**, and Tauri "looks for an `index.html` and serves it as the default entry point". Both files in one folder means the phone loads the desktop UI.

**Two output directories, each with its own `index.html`:**

```
src/main.tsx      desktop RPG UI (frozen, Tailwind + framer-motion)  ->  dist-desktop/index.html
src/mobile.tsx    mobile UI (DS only)                                ->  dist-mobile/index.html
```

```ts
// vite.config.ts — two builds selected by mode.
// outDir is resolved RELATIVE TO `root`, so it must be absolute here:
// with root='src/mobile-entry', a bare 'dist-mobile' lands in
// src/mobile-entry/dist-mobile — not where the Tauri overlay points.
import { resolve } from 'node:path';
import { fileURLToPath } from 'node:url';
import { defineConfig } from 'vite';
import react from '@vitejs/plugin-react';

const projectRoot = fileURLToPath(new URL('.', import.meta.url));
// @ts-expect-error Vite config runs in Node; the app tsconfig omits Node globals.
const env = process.env;
const host = env.TAURI_DEV_HOST;
const devPort = Number.parseInt(env.PORT ?? '1420', 10);
const hmrPort = Number.parseInt(env.HMR_PORT ?? '1421', 10);

export default defineConfig(({ mode }) => {
  const mobile = mode === 'mobile';
  return {
    plugins: [react()],
    clearScreen: false, // keep Rust diagnostics visible
    root: mobile ? resolve(projectRoot, 'src/mobile-entry') : projectRoot,
    build: {
      outDir: resolve(projectRoot, mobile ? 'dist-mobile' : 'dist-desktop'),
      emptyOutDir: true,
    },
    server: {
      port: devPort,
      strictPort: true,
      host: host || false,
      hmr: host ? { protocol: 'ws', host, port: hmrPort } : undefined,
      watch: { ignored: ['**/src-tauri/**'] },
    },
  };
});
```
[`build.outDir` is relative to `root`](https://vite.dev/config/build-options.html#build-outdir) — this is the whole bug.

This repository declares `"type": "module"`, so `__dirname` is undefined in `vite.config.ts`; `fileURLToPath(import.meta.url)` is not optional ceremony. The canonical snippet also fixes the current 1420/1420 collision by reserving 1421 for HMR.

```jsonc
// package.json
"scripts": {
  "dev":          "vite",
  "build":        "tsc && vite build",
  "dev:mobile":   "vite --mode mobile",
  "build:mobile": "tsc && vite build --mode mobile"
}
```

```jsonc
// tauri.conf.json
{ "build": { "beforeDevCommand": "npm run dev",
             "beforeBuildCommand": "npm run build",
             "frontendDist": "../dist-desktop" } }

// tauri.ios.conf.json  AND  tauri.android.conf.json
{ "build": { "beforeDevCommand": "npm run dev:mobile",
             "beforeBuildCommand": "npm run build:mobile",
             "frontendDist": "../dist-mobile" } }
```

**Override `beforeDevCommand` too, not just `beforeBuildCommand`.** The current config runs `npm run dev` for dev, so `tauri ios dev` would start the desktop Vite root and serve the desktop UI on the phone — the dev-mode twin of the `frontendDist` bug, and easy to mistake for a routing problem. Overlays merge by JSON Merge Patch (RFC 7396), so the desktop config is untouched.

Entry file naming: the mobile root is `src/mobile-entry/`, containing **`index.html`** (Vite's required entry name) which loads `../mobile.tsx`. There is no `mobile.html` anywhere — an earlier draft used that name, which cannot work as a Vite root entry.

`TAURI_DEV_HOST` remains the physical-device binding. The canonical snippet above separates the Vite server (1420) from HMR (1421); architecture §5.2.1 supplies a development-only `devCsp` WebSocket allowance while production CSP stays closed. Phase A verifies a live edit reaches a physical device, so this cannot remain a copy-paste footnote.

- Tailwind's `content` glob narrows to `src/{App,components,hooks,lib}` — the frozen tree only. It never scans `src/ds` or `src/screens`, so no utility class can be generated for a DS surface, and preflight is imported by `index.html` alone.
- `framer-motion` is imported only under the frozen tree. The mobile bundle uses `base.css` keyframes (`cm-blink cm-pulse cm-scan cm-glitch-x cm-flicker cm-rotate`) plus `steps()` transitions.
- The adherence lint runs over `src/ds`, `src/screens`, `src/shell` only. Linting the frozen tree would produce hundreds of warnings about code nobody is allowed to change.
- `tauri.conf.json` sets `frontendDist` per platform overlay: mobile builds point at the mobile entry.

Cost of freezing rather than retiring: two bundles, two dependency sets, and a Tailwind config that must stay scoped. Cheap while nobody edits the desktop tree; the moment someone does, retiring it gets cheaper than maintaining it.

---

## 3. Integrating the design system into the repo

### 3.1 Un-bundle back to source

`_ds_bundle.js` is a per-file-delimited IIFE that attaches to `window.CabalMeshDS` and assumes a global `React`. Shipping it as-is means: no tree-shaking (all 30 components in every screen), no types, no source to read, a global React shim, and a file marked *"Generated — never edit"* that we would immediately need to edit for C3/C4.

It is unminified and cleanly delimited:
```js
try { (() => { …component source… })(); }
catch (e) { __ds_ns.__errors.push({ path: "components/core/Panel.jsx", … }); }
```

**Do:** run a one-off script (`scripts/unbundle-ds.mjs`) that splits on those `path:` markers into `src/ds/components/**/*.jsx`, prepends `import React from 'react'`, and appends named exports. 30 files, mechanical, reviewable in one diff. Then delete `_ds_bundle.js`.

The `.d.ts` prop contracts the readme mentions were not included in the download, but `_adherence.oxlintrc.json` carries the same information — §3.4 turns it into types.

### 3.2 Target layout

```
index.html                       desktop Vite root entry  (frozen)  -> dist-desktop/
src/
├── App.tsx  components/ hooks/ lib/   ← FROZEN desktop RPG UI. Do not touch. (C6)
├── main.tsx                     frozen desktop bootstrap
│
├── mobile-entry/
│   └── index.html               mobile Vite root entry            -> dist-mobile/
│
├── ds/                          ← the design system, vendored
│   ├── tokens/                  6 css files, verbatim except fonts.css (C1) + type scale (C5)
│   ├── assets/
│   │   ├── fonts/               self-hosted woff2          (C1)
│   │   ├── icons/               14 glyphs @1x @2x @3x      (C2)
│   │   ├── logo/  characters/  textures/
│   ├── components/              30 unbundled .jsx + .d.ts  (§3.1, §3.4)
│   ├── mobile.css               C3 hover guard, C4 hit areas, C5 scale, safe areas
│   └── index.ts                 the single public entry the lint enforces
├── ui/                          authored semantic adapters over `ds/`
│   ├── Button.tsx Tabs.tsx Dialog.tsx Field.tsx DataTable.tsx …
│   └── index.ts                 the only DS entry screens may import
├── screens/                     10 screens, one file each
├── shell/                       AppShell, Header, TabBar, Router
├── state/                       stores + Tauri event subscriptions
├── types/bindings.ts            generated from Rust by ts-rs — never hand-edited
└── mobile.tsx                   mobile bootstrap
```

Three rules keep this from rotting:

- **`src/ds/**` is vendored, not authored.** Upstream regeneration replaces `tokens/` and `components/`; mechanical visual adaptations stay in `mobile.css` and resampled assets.
- **Structural/accessibility adaptations live in `src/ui/**`.** Screens never import a raw DS component. Adapters render the necessary native element/ARIA/focus behaviour and apply the vendored visual contract; purely presentational primitives may be thin re-exports. This is how real buttons, labels, tables, dialog trapping and `inert` survive a DS re-export without pretending CSS can change semantics. CI enforces `screens|shell -> ui -> ds`, and wrapper tests are the repeatable compatibility gate after regeneration.
- **The frozen tree and the mobile tree never import each other.** Enforce with an ESLint `no-restricted-imports` boundary in both directions — one accidental `import { Panel } from '../ds'` inside a Tailwind component reintroduces the exact collision C6 exists to prevent.

### 3.3 Routing

Ten screens, three of them modal-ish, no URL bar. A router library is overkill and history semantics on mobile webviews are inconsistent.

A discriminated-union screen state mirroring the Rust `IntentStatus` shape:

```ts
type Screen =
  | { name: "splash" }
  | { name: "connecting" }
  | { name: "home" }
  | { name: "intents"; tab: "ACTIVE" | "PENDING" | "HISTORY" }
  | { name: "new" }
  | { name: "detail";  id: IntentId }
  | { name: "settled"; id: IntentId }
  | { name: "nodes" }
  | { name: "vault";   tab: "ASSETS" | "IDENTITIES" | "KEYS" }
  | { name: "profile" };
```

`detail` and `settled` cannot exist without an `IntentId` — the type forbids the "open detail with nothing loaded" bug the prototype papers over with hardcoded data. Back behaviour comes from the prototype's own `backMap` (`intents→home`, `new→intents`, `detail→intents`, `settled→intents`, `nodes|vault|profile→home`). Android hardware back binds to the same function.

### 3.4 Keeping the contract enforced

1. Generate `.d.ts` per component from `_adherence.oxlintrc.json` (the enum domains are already there as regexes) → `Button tone` becomes a union type, so a bad tone is a compile error rather than a lint warning.
2. Keep `_adherence.oxlintrc.json` wired into CI as-is. It catches what types cannot: raw hex, raw px, deep imports past `index.ts`, non-system fonts.
3. Add one project rule the design system cannot know about: **no hex colour may cross the Tauri boundary.** Rust returns semantic status (`IntentStatus::Settled`, `ToastAccent::Success`); the mapping to `--accent-acid-green` happens in `src/ui` and nowhere else. The prototype hands colours around as data (`dot: BLUE`, `color: GREEN`) — that is prototype convenience and must not survive into the product.

---

## 4. Screen build spec

Each screen: DS components used, Rust command for initial data, events for live updates. Commands/events are as defined in the architecture plan §6/§7.

| # | Screen | Composed from | Command | Live events |
|---|---|---|---|---|
| 01 | **splash** | `Logo hero` · `CharacterPortrait oracle` · `Button primary block` ×2 · `TextureField vignette` | `session_status` | — |
| 02 | **connecting** | `Terminal` (handshake log) · `Meter` (6-step progress, `steps(6,end)`) · `StatusDot pulse` | `enter_mesh(onLine)` — **Channel** | `BootstrapProgress` |
| 03 | **home** | `Panel MESH STATUS` · `StatBlock` ×3 (`NETWORK NODES` `INTENTS SETTLED` `REPUTATION SCORE`, `deltaTone up`) · `Terminal MESH LOG` · `StatusDot` | `mesh_snapshot` + `subscribe_mesh_log(onLine)` — **Channel** | `MeshStatsChanged` · `RuntimeCapsChanged` |
| 04 | **intents** | `Tabs` (ACTIVE/PENDING/HISTORY) · `Panel` rows · `Badge` (mode) · `StatusDot` (status) · empty state | `list_intents(filter)` | `IntentUpdated` |
| 05 | **new** | `Field`+`Input` · `RadioGroup` (BUY/SELL/SWAP/STAKE as segmented) · `Select` (asset, condition, mode) · `Radio` stops (privacy) · `Button` + `Dialog` (confirm) | `intent_form_options` → `preview_intent` → `broadcast_intent` | `Toast` |
| 06 | **detail** | `Panel` · `DataTable` (7 rows) · `StatusDot pulse` · `Badge` · `Button danger` | `get_intent(id)` | `IntentUpdated` |
| 07 | **settled** | `Panel PROOF` · `DataTable` (5 rows) · `Terminal` (verification log) · `Icon proof` | `settle_intent(id, onLine)` — **Channel**; call `get_proof(id)` only after terminal `IntentUpdated` | `IntentUpdated` · `Toast` (supplementary only) |
| 08 | **nodes** | custom SVG map + `StatusDot` per node · `Panel NEARBY NODES` · signal bars · failure state | `nearby_nodes(action)` — `observe` normally; `request` / `openSettings` only from the matching Android state; **no kilometres**, see below | `PeersChanged` · `RuntimeCapsChanged` |
| 09 | **vault** | `Tabs` (ASSETS/IDENTITIES/KEYS) · `Panel` total (masked until reveal) · `IconButton` reveal · `DataTable` | `vault_assets` / `vault_identities` / `vault_keys` / `vault_total_value` | — |
| 10 | **profile** | `Panel` node id + copy · `StatInline` reputation · rows w/ `Icon` · `Switch` (offline) · `Button danger` | `profile_summary` | `RuntimeCapsChanged` |
| — | **shell** | `NavBar` (52px header) · 5-destination primary nav (48px minimum) · `ToastStack` · `Dialog` | `platform_caps` + `runtime_caps` + initial `plugin:type-scale|get_scale` | `Toast` · `RuntimeCapsChanged` · `TypeScaleChanged` |

**Copy is not ours to write.** The readme fixes the voice: impersonal, present tense, full stops on fragments, exact numbers (`1,248` not `~1.2k`), lowercase unabbreviated hex, no emoji, uppercase buttons. Every string in these screens comes from the prototype or is written to that spec. Error strings arrive from Rust as `AppError` variants and map to on-voice text in one `errorCopy.ts` — *"Node 0x2f11 failed liveness."*, never *"Something went wrong"*.

**`nodes` shows no distances.** The prototype lists `1.2 km`, `2.4 km`, `3.1 km`. A libp2p peer has a peer id and a multiaddr, not coordinates, and this app deliberately requests no location permission — asking for it would contradict the entire premise. Rendering canned kilometres would be a fabricated measurement in a product whose copy rules demand exact numbers. The same slot carries what the mesh actually knows: `41ms · DIRECT`, `RELAYED · 2 HOPS`. Signal bars derive from `latency_ms`, not a canned `bars` integer. Architecture plan §6.1 defines `NodeSummary`.

**`nodes` does not fabricate permission certainty either.** On baseline iOS raw multicast, no packet can mean “Local Network denied” or simply “zero peers”; Apple exposes no general status query. Render `RuntimeCaps.localDiscovery` exactly: `disabled` can name relay-only build policy; `probing` is bounded setup work; Android `ready` means permission/socket setup succeeded but no local peers answered; `available` requires a valid local peer; and iOS `indeterminate` says local discovery is inconclusive while showing relay reachability separately. On an Android 17+ build targeting SDK 37, `permissionRequired` shows a just-in-time **ENABLE LOCAL DISCOVERY** action that calls `nearby_nodes("request")`; ordinary render/refresh uses `"observe"`. `denied` follows refusal/revocation and exposes `nearby_nodes("openSettings")`. Rust accepts those actions only in the matching state and opens app-details settings through its Rust-only native plugin—not the opener plugin. Never re-prompt in a loop or block relay access.

**Three edge states are first-class**, not afterthoughts — the prototype ships toggles for all three: empty (`NO PENDING INTENTS / Nothing is queued. Nothing is stored.`), node failure (slashed node, red, bars zeroed), offline (`MESH UNREACHABLE · OPERATING OFFLINE` banner + queueing).

---

## 5. Mobile adaptation rules

Beyond C1–C6:

- **Safe areas.** Header pads `env(safe-area-inset-top)`, tab bar pads `env(safe-area-inset-bottom)`, plus `viewport-fit=cover`. The prototype fakes a status bar and home indicator inside its 390×844 frame; the real app must yield those regions to the OS.
- **Viewport — zoom stays enabled.** `<meta name="viewport" content="width=device-width, initial-scale=1, viewport-fit=cover">`. **No `user-scalable=no`, no `maximum-scale`.** An earlier draft locked zoom to protect the fixed 390px layout; that removes the user's last resort and fails WCAG 1.4.4 alongside C5. Accidental pinch on an instrument layout is a far smaller cost than an unusable app.
- **Scroll.** One scroll container per screen between the fixed header and tab bar; `overscroll-behavior: contain` to kill iOS rubber-band on the page. `Terminal` panes scroll internally and pin to bottom.
- **Keyboard.** `new` is the only screen with text inputs (price, amount). Use `inputmode="decimal"`; on focus, scroll the field above the keyboard; `Dialog` must reposition rather than sit under it.
- **Log buffers are bounded.** The prototype's mesh ticker is a client-side `setInterval` over a canned array; the real one is a server push. Keep a fixed-size ring (4 visible, 200 retained) and drop oldest — an unbounded array behind a live gossip feed is a slow OOM on a 2 GB device.
- **`prefers-reduced-motion`** already handled by `base.css`. Keep the stepped pulses off under it.
- **Dark only.** The system is black-grounded by law; declare `color-scheme: dark` so form controls and scrollbars follow.

### 5.1 Screen-reader and keyboard acceptance criteria

Font scaling was the only accessibility axis the earlier drafts addressed. These are the rest, and they are acceptance criteria — checked per screen in Phase C/D, not audited once at the end.

**Names, roles, states.** The design system is built from `div`s with inline styles and communicates state through colour and border tier — none of which reaches an assistive technology by itself. This table covers the critical primitives; every remaining exported component receives the same name/role/state review before Phase B is done.

| Component | Required semantics |
|---|---|
| `Button` / `IconButton` | real `<button>`; `IconButton` needs an `aria-label` (its `label` prop must not be decorative-only) |
| `Tabs` inside a screen | complete [APG tabs pattern](https://www.w3.org/WAI/ARIA/apg/patterns/tabs/): `tablist` / `tab` / `tabpanel`, `aria-selected`, `aria-controls` / `aria-labelledby`, roving tab index, Left/Right and Home/End keys |
| App destination bar | **not tabs**: `<nav aria-label="Primary">` with links/buttons; current destination uses `aria-current="page"`; scrolling at large text keeps the active destination fully visible |
| `Switch` (offline mode) | `role="switch"` + `aria-checked`; announce the resulting state, not the action |
| `StatusDot` | decorative — `aria-hidden`; the status word must exist as text, never colour-only (also WCAG 1.4.1 Use of Colour) |
| `Meter` (connecting progress) | `role="progressbar"` with `aria-valuenow/min/max` |
| Finite `Terminal` (`connecting`, `settled`) | `role="log"`, `aria-live="polite"`, `aria-relevant="additions"`, `aria-atomic="false"`; **not** assertive |
| Continuous `MESH LOG` | `aria-live="off"` by default; expose a concise current-status summary and an explicit “read latest” control. A gossip feed announcing every line is worse than silence. |
| `StatBlock` | label and value associated, so `NETWORK NODES 1,248 +12.4%` reads as one unit rather than three fragments |
| `Badge` (mode) | included in the intent row's accessible name |
| `Field` / `Input` / `Select` | visible `<label>` association; help/error ids in `aria-describedby`; invalid state in `aria-invalid`; prefer native controls over partial custom patterns |
| `RadioGroup` / privacy stops | native radio inputs inside `<fieldset><legend>` or the complete APG radio pattern; selected state cannot be border/colour only |
| `DataTable` / key-value detail | real `<table>` only for two-dimensional data; use `<dl>` for label/value facts; headers or terms remain programmatically associated |
| Images / glyphs / tooltip / copy-reveal controls | decorative images hidden, meaningful images named; tooltip opens by keyboard and pointer without becoming the sole label; reveal/copy buttons expose state and a textual outcome |

**Dialog.** The confirm dialog must trap focus, return focus to the trigger on close, close on Escape/back gesture, carry `role="dialog"` + `aria-modal="true"` + a labelled title, and mark the rest of the tree inert. Test `inert` on the minimum WKWebView; keep a focus-containment + background `aria-hidden` fallback for an engine that does not enforce it. The DS `Dialog` was authored for a desktop console; verify rather than assume.

**Toasts.** `role="status"`, polite. Toast text duplicates information available elsewhere — never the sole channel for an outcome.

**Focus visible.** The brand's focus treatment (1px neon-blue ring + `--glow-blue`) is the one sanctioned accent use and must survive on every interactive element, including with a hardware keyboard on iPad or a connected Android keyboard.

**SPA route focus.** On a forward screen change, move focus to the new `<main>` heading/landmark and announce its title; on Back, restore focus to the control that opened the prior screen when it still exists. Changing pixels without moving focus leaves screen-reader and keyboard users on a detached control.

**Reachability at 200%.** Growth must not push controls off-screen or under fixed chrome — every interactive element stays reachable by scroll at every tested scale.

**Verification:** one full pass per screen with **VoiceOver** (iOS) and **TalkBack** (Android), plus hardware-keyboard tab-order. Recorded as a checklist in Phase F, spot-checked in C/D.

---

## 6. Prototype code that must not ship

- **`support.js`** (69 KB) — the `dc-runtime` prototype harness with a `DCLogic` base class and a props editor. Reference only.
- **`Cabal Mesh Mobile.dc.html`** — the behavioural spec. Read it for interaction detail; do not port it.
- **All canned data** — `MESH_LINES`, `BASE_INTENTS`, `NEARBY`, `VAULT_ASSETS`, `1,248` peers, `87.6` reputation, `3D 14H 22M` uptime, `7F3A..8C2E`. Every one comes from Rust. Keep them exactly once, in `src/screens/__fixtures__/`, as the fixtures the screen tests render against.
- **Colour-as-data** — `dot: BLUE`, `deltaColor: GREEN`, the `W/S/M/I` constants. Semantic tone props only (§3.4.3).

---

## 7. Rust-side work this UI requires

Applying `rust-skills`. This is the delta on top of the architecture plan, driven by what the screens actually need.

**Bindings, not hand-written domain duplicates.** Rust Phase 1 establishes Tauri-free `cabal-contract` and emits core types plus the complete screen DTO schema to `src/types/bindings.ts`; Phase 2 adds errors/events, and Phase 6 wires the already-generated DTOs to handlers. Presentation-only types such as `Screen` remain authored here, but import `IntentId` and domain state from the generated file. Hand-maintaining parallel Rust domain/boundary interfaces is a drift generator.

**Semantic tone enums so the UI never receives a colour** — matches `StatusDot`/`Badge`/`Toast`/`TerminalLine` domains exactly:

```rust
/// Maps 1:1 onto the design system's `StatusDot` tone domain.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, TS)]
#[serde(rename_all = "lowercase")]
pub enum StatusTone { Online, Alert, Info, Idle, Offline }

/// Maps onto `TerminalLine` tone: the handshake, mesh and proof logs.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, TS)]
#[serde(rename_all = "lowercase")]
pub enum LogTone { Out, Dim, Ok, Err, Info, Loud }
```
`type-enum-states`, `anti-stringly-typed`, `serde-rename-all`.

**Log lines are the hot path** — three screens stream them and each is short-lived:

```rust
/// One rendered terminal line. `text` is never mutated after construction,
/// so `Box<str>` drops `String`'s spare-capacity word.
#[derive(Debug, Clone, Serialize, TS)]
pub struct LogLine { pub text: Box<str>, pub tone: LogTone }
```
`mem-boxed-slice`. Build them with `write!` into a reused buffer rather than `format!` per line (`mem-write-over-format`, `anti-format-hot-path`), and pre-size the ring with `Vec::with_capacity` (`mem-with-capacity`).

**The stat tiles are pre-formatted in Rust.** `StatBlock` takes `label`, `value`, `unit`, `delta`, `deltaTone` — and the brand demands `1,248` / `+12.4%` / `99.98%` exactly. Formatting once in Rust behind a `Display` impl beats reimplementing separator and precision rules in TS (`type-display-vs-debug`):

```rust
#[derive(Debug, Clone, Serialize, TS)]
#[serde(rename_all = "camelCase")]
pub struct StatTile {
    pub label: &'static str,
    pub value: Box<str>,                       // "1,248"
    #[serde(skip_serializing_if = "Option::is_none")]
    pub delta: Option<Box<str>>,               // "+12.4%"
    pub delta_tone: DeltaTone,                 // up | down | neutral
}
```
`serde-skip-empty`, `api-non-exhaustive`.

**Log streams use Tauri `Channel`, not the event bus.** The docs are explicit that channels are the mechanism for ordered, high-rate streams and that event payloads are JSON strings unsuited to frequent messages. The three `Terminal` panes (`connecting`, `home`, `settled`) each open a channel with the command that starts the work:

**Unmount does not end a channel — you must unsubscribe.** An earlier draft claimed otherwise; it was wrong. In `@tauri-apps/api` 2.9.1, `Channel` releases its callback only when Rust sends an `end` message, there is no public JS unsubscribe, and releasing the JS callback would not stop the Rust producer regardless. Leaving `home` without teardown leaves a live broadcast receiver and a log-producing task behind — once per visit.

Every stream therefore has an explicit lifecycle (architecture plan §2.5.1):

**All three streams**, not just `subscribe_mesh_log`. `enter_mesh` and `settle_intent` also return a `SubscriptionId` immediately, so one shared hook covers every case:

```ts
/** Opens a Rust log stream and guarantees teardown. Used by connecting, home, settled. */
function useLogStream(
  command: "enter_mesh" | "subscribe_mesh_log" | "settle_intent",
  args: Record<string, unknown>,
  onLine: (line: LogLine) => void,
) {
  const onLineRef = useLatest(onLine);

  useEffect(() => {
    const channel = new Channel<LogLine>();
    channel.onmessage = (line) => onLineRef.current(line);

    let id: SubscriptionId | undefined;
    let cancelled = false;

    const teardown = (sid: SubscriptionId) =>
      invoke("unsubscribe", { id: sid }).catch(reportError);

    invoke<SubscriptionId>(command, { ...args, onLine: channel })
      // Unmount can win the race against subscribe — tear down immediately.
      .then((sid) => { if (cancelled) void teardown(sid); else id = sid; })
      .catch(reportError); // setup failure only; domain completion comes from typed state/events

    return () => { cancelled = true; if (id) void teardown(id); };
  }, [command, args]);
}
```

`useLatest` is the standard ref-backed helper: callback updates do not tear down/reopen delivery. Callers pass `args` from `useMemo`; argument identity changes are a deliberate operation/subscription change, so the lint rule rejects inline objects. `JSON.stringify(args)` is not a dependency strategy: it can throw and hides unsupported values.

Two details that are easy to omit and expensive to debug:

- **The unmount-before-subscribe-resolves race** leaks on every fast tab switch. Handled once here so no screen can forget it.
- **`.catch` on both promises.** Without it a failed `unsubscribe` during teardown surfaces as an unhandled rejection with no stack pointing at the screen that caused it.

**Unsubscribing stops delivery, not the operation.** Leaving `connecting` does not disconnect the mesh; leaving `settled` does not abort an in-flight settlement. Aborting is a separate explicit command (`cancel_intent`). Architecture §2.5.1 defines this per stream — worth reading before wiring, because getting it backwards on `settle_intent` means a UI navigation cancels a transaction.

**Effects may run twice.** This repo uses React StrictMode, and a route can legitimately unmount/remount. Rust therefore guarantees start-or-attach idempotency: repeated `enter_mesh` attaches to one join; repeated `settle_intent(id)` attaches to one settlement and can never submit another transaction. UI tests mount the hook under StrictMode and navigate away/back; Rust integration tests assert one domain invocation with independently cancellable delivery ids.

**Logs never signal success.** `enter_mesh` completion comes from terminal `BootstrapProgress` plus `session_status`; settlement completion/failure comes from `IntentUpdated`, and only the terminal settled state triggers `get_proof`. A toast is supplementary, never control flow. The channel can stop because the screen left while the operation continues.

**Suspend/resume does not create a second operation.** On a verified true-background transition Rust pauses channel forwarding and retains a bounded tail plus the existing `SubscriptionId`. The shell listens for documented `TauriEvent.WINDOW_RESUMED`; Rust replays the tail on the same channel, while the active screen refetches authoritative state (`session_status` or `get_intent`) in case terminal events were not deliverable. It never re-invokes `enter_mesh`/`settle_intent`. Explicit route unmount is what calls `unsubscribe`.

Internally each channel is fed from a bounded `broadcast`, so a subscriber that falls behind lags rather than growing the queue (`async-broadcast-pubsub`, `async-bounded-channel`). Finite delivery streams close on natural completion, delivery error and explicit unsubscribe; the separate domain task is unaffected by delivery cancellation. Low-frequency state (`MeshStatsChanged`, `IntentUpdated`, `PeersChanged`, `RuntimeCapsChanged`, `TypeScaleChanged`, `Toast`) stays on `emit` + `listen`.

**Errors carry a variant, never a sentence.** `AppError`'s tagged union lets `errorCopy.ts` hold the on-voice strings; Rust's `Display` text stays lowercase and structural (`err-lowercase-msg`), and the source chain is logged, not shipped (`obs-error-chain`, `obs-no-sensitive-data` — vault screens must never surface a key in an error).

**Assets are frontend-side.** The 14 glyphs ship in the Vite bundle, not through Tauri's asset protocol — fewer IPC round-trips, and `Icon` keeps working in Storybook/tests with no Tauri runtime.

**Tests for the boundary.** `insta` snapshots of every `AppEvent`, `AppError`, `StatTile` and `IntentView` serialization (`test-snapshot-testing`). A shape change that would silently break a screen fails CI in Rust, before the TS ever sees it.

---

## 8. Phases

Runs in parallel with the Rust phases, with one explicit dependency: UI Phase B starts only after Rust Phase 1 has emitted the complete screen DTO schema in `src/types/bindings.ts`. Phases C/D build fixtures against those generated contracts; Phase 6 later delivers command implementations/IPC wiring, not a surprise replacement type surface.

### Phase A — Vendor the system + split the build (1.5 days)
- **Two build outputs** — `dist-desktop/` and `dist-mobile/`, each with its own `index.html` (C6, §3.2); `frontendDist` set per platform overlay. Scope Tailwind `content` to the frozen tree; scope the adherence lint to `src/{ds,ui,screens,shell}`; add the import-boundary rules.
- Copy `tokens/`, assets, `readme.md` into `src/ds/`.
- `scripts/unbundle-ds.mjs` → 30 source files; delete the bundle.
- Self-host fonts (C1); resample the 14 glyphs + 4 marks (C2).
- Scaffold `src/ui/index.ts` and enforce that screens/shell cannot deep-import `src/ds`; semantic adapters themselves land in Phase B.
- **Done when:** a scratch page built into `dist-mobile` renders the vendored `Panel` + `Button` + `Terminal` + `StatusDot` visuals correctly, offline, with the real faces; the import boundary and lint are green; the desktop entry still builds and runs unchanged; zero Tailwind classes in the mobile bundle; **the mobile build actually loads the mobile UI on a simulator**; a live edit reaches a physical device over HMR on dedicated port 1421.

### Phase B — Shell + accessibility foundation (4 days)
- `AppShell`: 52px-min header, 48px-min primary nav, safe areas, single scroll container. **`min-block-size` everywhere text lives, never `height`** (C5).
- `mobile.css`: hover guard (C3), hit-area expansion with the 16px separation rule (C4), `color-scheme: dark`, `viewport-fit=cover` **with zoom left enabled** (§5).
- **Type scale (C5):** `--type-scale` through the size tokens; `type-scale` plugin reading an iOS `UIFontMetrics` ratio / Android `fontScale`, **unclamped**, re-read on the runtime's native `Resumed` event, fallback 1.
- **Reflow rules decided here, not discovered later:** stat trio row→stack, node rows one-line→two-line, segmented control wrapping, terminal visible-line count derived from available height.
- Typed `Screen` union + back stack + Android hardware back.
- `ToastStack` + `Dialog` mounted at the shell; the `useLogStream` hook (§7) so no screen hand-rolls channel teardown.
- **Primary-nav-at-200% strategy built** (C5) — icons plus retained visible labels, scrollable at large text; active destination kept in view.
- **Implement/test every exported `src/ui` semantic adapter** (§5.1): app navigation distinct from in-screen tabs; form errors/groups, tables/lists, finite vs continuous logs, `Dialog` focus trap + focus return, route-focus management and focus-visible ring. Raw generated DS components remain untouched and replaceable.
- **Done when:** all 5 destinations navigate with placeholder bodies on a real device; nothing sticks inverted after a tap; every control passes a **44pt (iOS) / 48dp (Android)** hit test with no neighbour steal; **the shell is usable at 200% system font and at full pinch zoom**; VoiceOver and TalkBack can reach and identify every shell control.

### Phase C — Static screens (2.5 days)
`splash` · `home` · `nodes` · `vault` · `profile`, built against `__fixtures__` extracted from the prototype. Pixel-diff each against the prototype at 390×844 with `--type-scale: 1`. Separate 130% and **200%** passes check reflow, clipping, overlap and reachability — never exactness (C5).

The `nodes` map takes **deterministic positions seeded by peer id** — `hash(peer_id)` mapped into the map's bounds with a minimum-separation nudge. Peers have no coordinates, and the prototype's 7 hardcoded slots do not generalise; seeding by id keeps a node in the same place across renders and app restarts, which is what makes the map readable as an instrument rather than a lava lamp. Pulse durations stay in the prototype's 900–1650 ms range, also seeded, so the field does not throb in unison.

### Phase D — Flow screens (3 days)
`connecting` · `intents` · `new` · `detail` · `settled`. The full loop: compose → confirm dialog → broadcast → detail → settle → proof. Terminal streaming with the stepped caret; `Meter` on `steps(6,end)`; all three edge states reachable.

### Phase E — Wire to Rust (1.5 days)
Swap fixtures for `invoke` + event subscriptions as architecture-plan phase 6 lands. Fetch both `platform_caps` and `runtime_caps`; static caps gate ZK/LLM affordances, runtime caps drive connectivity/discovery explanations. Wire `RuntimeCapsChanged` and `TypeScaleChanged`; verify stream completion follows typed state rather than log closure/toasts. Mount stream flows under React StrictMode and navigate away/back; one user settlement must produce one Rust domain invocation/transaction. `errorCopy.ts` maps `AppError` variants to on-voice strings.

### Phase F — Device pass (2 days)
Physical iPhone + Android: font rendering, glyph crispness, tap targets, keyboard on `new`, scroll behaviour, safe areas on a notched and a hole-punch device, offline-mode banner, reduced-motion, cold-start paint, system font scale at 100/130/200%, and a full **VoiceOver + TalkBack** pass per screen against the §5.1 checklist plus hardware-keyboard tab order. The Android permission matrix follows the pinned target: SDK ≤36 uses Android 16's `RESTRICT_LOCAL_NETWORK` blocked/granted compatibility path; SDK ≥37 runs first request → deny → `openSettings` → grant → resume → Settings revocation. Every denied state must preserve relay navigation and announce the specific recovery action.

**Total ≈ 14.5 working days** — the phase values now add to 14.5: 1.5 + 4 + 2.5 + 3 + 1.5 + 2. The increase is concentrated in accessibility foundation, per-screen semantics/reflow, and device verification that were absent rather than deferred. Overlaps Rust phases 3–7.

---

## 9. Risks

| Risk | Impact | Mitigation |
|---|---|---|
| Silkscreen ≠ Pixel Operator — the real display face was never delivered | Brand fidelity | Ask the design owner for `PixelOperator.ttf` + `PixelOperator-Bold.ttf`. `fonts.css` already has the swap block; it is a two-line change. Ship on Silkscreen meanwhile. |
| Un-bundling introduces subtle diffs from the prototype | Visual drift | Pixel-diff every screen against `Cabal Mesh Mobile.dc.html` at 390×844 in Phase C/D, before any Rust wiring. |
| Layouts break at 200% system font | Clipped text, unreachable controls, WCAG AA failure | `min-block-size` everywhere text lives and reflow rules decided in Phase B; 130% and 200% passes in every screen phase, not just at the end. |
| Channel subscriptions leak on tab switching | Battery drain, unbounded growth | Shared subscribe/unsubscribe effect helper in Phase B; the unmount-before-subscribe-resolves race is handled there once, not per screen. |
| Log closure/toast is mistaken for settlement completion | Proof fetched too early or a failed transaction shown as settled | Typed `IntentUpdated` is control flow; logs/toasts are presentation. Boundary tests cover success, failure and navigation-away. |
| Resume code re-invokes `settle_intent` | Duplicate transaction attempt | Rust retains the delivery id/tail; shell refetches authoritative state on `WINDOW_RESUMED`; test asserts one settlement invocation across suspend/resume. |
| Android local-network permission is treated as a generic offline error | Users cannot distinguish denial from relay/network failure and may get prompt loops | `permissionRequired` and `denied` have distinct `nodes` states; request just in time, preserve relay access, and test deny/grant/revoke with TalkBack in Phase F. |
| Downscaled glyphs look muddy despite resampling | Polish | Phase A ends with a device-eye check at 1×/2×/3×, not a desktop browser check. |
| Frozen desktop tree and mobile tree cross-import | C6 collision returns | Bidirectional `no-restricted-imports` boundary in CI (§3.2). A single stray import reintroduces Tailwind into the DS bundle. |
| Two bundles diverge as Rust commands change | Desktop breaks anyway | Covered on the Rust side by the `cabal-legacy` adapter + pre-refactor snapshots (architecture plan §2.10). If those are skipped, freezing fails regardless of what the frontend does. |
| Upstream design-system re-export clobbers mobile fixes | Repeat work | Vendored `tokens/` and `components/` stay replaceable; visual deltas live in `mobile.css`/assets and structural semantics in tested `src/ui` adapters. Re-export, then run adapter + pixel/a11y tests. |

---

## 10. Decisions taken

| # | Question | Resolution |
|---|---|---|
| C5 | Type scale | **Unclamped OS font scaling to 200%, zoom left enabled**, via a Tauri plugin that re-reads on resume. Layouts reflow. Brand-exact at 100%. Lands in Phase B. *(Revised from a 130% clamp — see §11.)* |
| C6 | Tailwind + framer-motion | **Kept, quarantined.** Two build outputs (`dist-desktop`, `dist-mobile`), each with its own `index.html`; Tailwind's `content` glob and framer-motion imports confined to the frozen desktop tree; import boundary enforced in CI. |
| 4 | Node map layout | **Deterministic, seeded by peer id** — stable across renders and restarts. Detail in Phase C. |
| — | `nodes` distances | **Removed.** No location source exists and none will be requested. Latency / hops / transport instead (§4, architecture §6.1). |
| — | Confirm-dialog copy | **Two strings, picked by connection state** (ticket 04, 2026-08-03). Online: *"This intent broadcasts to the mesh and settles on-chain. No identity is attached."* Offline: *"Queued locally. Broadcast and settlement follow reconnection. No identity is attached."* Recorded in `src/ds/BRAND.md`; the retired string is listed there as off-voice with its reason. |
| — | `REPUTATION SCORE` | **Replaced by `INTENTS SETTLED`** (ticket 39, 2026-08-03). The mock from ticket 03 is deleted. The tile and the profile row now show a lifetime settled count from the intent ledger, with a seven-day-over-seven-day delta, and no delta at all until there is a prior week to compare with. |

### Still open

1. **Pixel Operator binaries.** `PixelOperator.ttf` + `PixelOperator-Bold.ttf` were never delivered; Silkscreen is a substitute. Every uppercase display string in the app — wordmark, headings, buttons, nav, all numeric figures — is currently rendering in the wrong face. `fonts.css` has the swap block ready; it is a two-line change once the files arrive. Not blocking, but it is the single largest gap between the shipped app and the board.

---

## 11. Revision log — 2026-08-01 review

External review returned **approve with major revisions**. Applied here; the architecture-side findings are logged in that document's §12.

| Finding | Change |
|---|---|
| `frontendDist` names a directory and Tauri always serves `index.html` — the two-entries-in-one-`dist` design would have loaded the **desktop UI on the phone**. | §3.2, C6, Phase A — two output directories, each with its own `index.html`; Phase A now verifies the mobile build actually loads the mobile UI. |
| React unmount does **not** end a Tauri `Channel`; there is no public JS unsubscribe and the Rust producer keeps running. Prior claim was false. | §7 — explicit `SubscriptionId` + `unsubscribe`, including the unmount-before-subscribe-resolves race; shared effect helper in Phase B. |
| Clamping font scale at 130% and setting `user-scalable=no` **fails WCAG 1.4.4**; calling it "resolved" was wrong. | C5 rewritten — unclamped to 200%, zoom enabled, reflow rules decided in Phase B, 130%/200% test passes. With the later full semantics pass, Phase B is now 4 days. |
| Hit-area expansion can swallow a neighbouring control's taps. | C4 — 16px minimum separation between expanded controls; prefer real padding; verify by hit-test. |
| `type-scale` was a boot-time-only read. | C5 — re-reads on resume, pushed as an `AppEvent`. |
| `NEARBY` kilometres have no data source and no location permission is requested. | §4 — latency / hops / transport replace distance; bars derive from latency. |
| Prototype dialog copy does not match queue-then-drain. | §10, open item 3 — flagged for the design owner, not silently rewritten. |

---

## 12. Revision log — second review pass, 2026-08-01

| Finding | Change |
|---|---|
| **Vite `outDir` is resolved relative to `root`** — with `root: 'src/mobile-entry'`, a bare `dist-mobile` lands in `src/mobile-entry/dist-mobile`, not where the Tauri overlay points. | §3.2, C6 — absolute outputs for both builds. The third pass replaced ESM-invalid `__dirname` with `fileURLToPath(import.meta.url)`. |
| Only `beforeBuildCommand` was overridden, so `tauri ios dev` would have started the desktop Vite root and served the **desktop UI on the phone**. | C6 — `dev:mobile` / `build:mobile` scripts; both `beforeDevCommand` and `beforeBuildCommand` overridden in each platform overlay. |
| `mobile.html` cannot be a Vite root entry. | C6, §3.2 — the mobile root is `src/mobile-entry/index.html`. |
| "9px fails WCAG 1.4.4" was imprecise — the criterion is about resizing to 200%, not about any particular size. | C5 — restated accurately; the decision is unchanged. |
| No strategy for five long destination labels at 200%. | C5 — icons stay primary; a scrollable navigation region retains all visible labels and keeps the active item in view; `MORE` rejected. |
| No screen-reader, focus or keyboard criteria at all — the DS is `div`s with inline styles and signals state through colour. | **New §5.1** — per-component name/role/state table, `Dialog` focus trap + return, `Terminal` as a polite live region, focus-visible, VoiceOver/TalkBack passes. |
| Hit-target criterion named only iOS 44pt. | Phase B — 44pt **and** Android 48dp. |
| Channel teardown was illustrated only for `subscribe_mesh_log`; cleanup promises had no `.catch`. | §7 — one `useLogStream` hook covering all three streams, with `.catch` on both promises and the unmount race handled once. |
| Custom `lifecycle` plugin no longer needed — Tauri 2.11 propagates mobile `Suspended`/`Resumed`. | C5 — type scale re-reads on the runtime's native resume event. |
| `keystore` / `multicast-lock` permissions were granted to the webview though only Rust calls them. | C5 — only `type-scale:allow-get-scale` is granted to the frontend. |
| `bindings.ts` path disagreed with the architecture plan. | §3.2, §7 — `src/types/bindings.ts` everywhere. |

Total moved 12.5 → **14.5 days**, almost entirely the accessibility work that was previously absent rather than deferred.

---

## 13. Revision log — third consistency pass, 2026-08-01

| Finding | Change |
|---|---|
| The Vite sample used `__dirname` even though this repo is ESM, left the known HMR collision outside the canonical snippet, then omitted `defineConfig`/React plugin imports. | C6 now prints a complete ESM-safe React/Vite config with absolute roots, Fast Refresh, visible Rust diagnostics and the 1420/1421 server block; Phase A device-HMR is an exit criterion. |
| Channel logs still doubled as an implied completion signal; suspend deleted streams with no safe reacquire path. | §4/§7 — terminal `BootstrapProgress` / `IntentUpdated` drive control flow; suspend retains ids and bounded tails; resume refetches state without re-running settlement. |
| App navigation incorrectly shared ARIA tab semantics with in-screen tabs. | §5.1 — APG tabs only for tabpanels; app destinations live in a primary navigation landmark with `aria-current="page"`. |
| “Per-component semantics” covered only a small subset and omitted forms, data structures and SPA focus routing. | §5.1 — critical-primitives table expanded; every exported primitive is a Phase B gate; route focus and back-focus restoration added. |
| A polite live region for the continuous gossip feed would still flood announcements. | §5.1 — finite progress terminals are polite logs; continuous `MESH LOG` is live-off with status/read-latest affordances. |
| Hiding labels at 200% solved overflow by removing visible content for sighted low-vision users. | C5 — visible labels remain; the large-text primary nav scrolls and keeps the current destination in view. |
| `useLogStream` retained stale callbacks and serialized arbitrary args for dependencies; StrictMode could also invoke a money-moving command twice. | §7 — ref-backed latest callback, memoized args, teardown declared before use, and a Rust start-or-attach/idempotency contract tested under StrictMode and route re-entry. |
| Phase rows summed to 12 days while the stated total was 14.5. | §8 — allocations now sum exactly to 14.5, with the difference placed in accessibility implementation and device verification. |

---

## 14. Revision log — fourth primary-source pass, 2026-08-01

| Finding | Change |
|---|---|
| Android 17 blocks raw LAN/mDNS by default for apps targeting SDK 37+, while Android 16 exposes the restriction as an opt-in compatibility test. The UI had no request/deny/revoke contract. | §4, Phase F, risks — distinct `permissionRequired` / `denied` rendering, just-in-time request, no prompt loop, relay-preserving recovery, and the Android 16/17 device matrix. |

---

## 15. Revision log — final cross-plan consistency pass, 2026-08-01

| Finding | Change |
|---|---|
| A successful local-network setup with zero peers had no renderable terminal state; Settings recovery had no callable backend path. | §4, Phase F — `ready` is distinct from `available`; constrained `nearby_nodes(observe/request/openSettings)` matches the architecture without adding opener authority. |
| The plan called generated DS components replaceable while requiring structural accessibility edits inside them. | §3.2, §3.4, Phase A/B, risks — raw `src/ds` stays vendored; authored/tested `src/ui` semantic adapters own native elements, ARIA and focus behaviour. |
| UI B–D consumed screen DTOs before Rust Phase 6 generated them. | §7–8 — Tauri-free `cabal-contract` emits the complete fixture schema in Rust Phase 1; Phase 6 wires those stable DTOs to commands. |
| Android testing was unconditional across incompatible target-SDK paths. | Phase F — SDK ≤36 runs the compatibility path; SDK ≥37 runs request/deny/settings/grant/revoke. |
