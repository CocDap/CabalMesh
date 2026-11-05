# CabalMesh Design System

> We leave no identity, only traces.
> Knowledge is stitched, not stored.
> Every pixel is a memory.
> Every symbol is a protocol.
> Silence is our interface.

**CabalMesh** — "the nobody network" — is a zero-identity mesh layer for private
intents and verifiable execution. Users express an *intent*; the mesh executes it
offline across a network of staked nodes and settles only a cryptographic proof
on-chain. No account, no identity, no stored history.

| | |
| --- | --- |
| **Archetype** | The Silent Oracle |
| **Essence** | Invisible. Autonomous. Verifiable. |
| **Mission** | Build the zero-identity mesh layer for private intents and verifiable execution. |
| **Values** | Privacy · Autonomy · Truth · Open Protocol · Cryptographic Trust |
| **Taglines** | "Zero identity. Private intents." · "The nobody network." |

The design language is a **technical spec sheet from a monastery**: pure black
grounds, hairline rules, corner registration ticks, dense pixel type, and hooded
pixel-art archetypes. It reads as surveillance equipment documentation and as
liturgy at the same time. Nothing is decorative; every frame, tick and label
behaves like an instrument marking.

---

## Sources

| Source | Access | What was taken |
| --- | --- | --- |
| `uploads/photo_6294156173384552233_y.jpg` — CabalMesh Brand Identity v1.0 board | Read in full | **Everything in this system.** All colours, type specs, the manifesto, brand DNA, all 5 logo lockups, all 14 protocol glyphs, all 8 character-family figures, the 3 texture plates, the application preview and the application-examples plate |
| `https://github.com/kurodenjiro/Cabal-Mesh` | **Not accessed** | Nothing — see caveat below |

### ⚠️ The GitHub repo was not read

`github.com/kurodenjiro/Cabal-Mesh` was supplied but GitHub was not connected, and
the user chose to proceed without it. **Every component and screen in this system is
derived from the brand identity board, not from product code.** Colours, type,
spacing, marks, glyphs and the console's Dashboard hero are faithful to the board;
the remaining console views are extensions of the board's own visual language, not
recreations of real repo screens.

Connect GitHub and re-run to replace the inferred parts with the real component
inventory and screens. Reading that repository directly will produce noticeably
better results for anyone building on this system — the board defines the brand, but
only the code defines the product's actual component contracts and screen structure.

### ⚠️ Font substitution

The board specifies **Pixel Operator** (Jayvee Enaguas, free) as the primary display
face. It is not served by Google Fonts and no binary was supplied, so
**Silkscreen** is loaded as the nearest available bitmap match. IBM Plex Mono, the
specified secondary face, is the real font.

**Please send `PixelOperator.ttf` and `PixelOperator-Bold.ttf`.** Dropping them into
`assets/fonts/` and uncommenting the block in `tokens/fonts.css` restores the true
face — nothing else needs to change.

---

## Content fundamentals

**Voice: the oracle, not the vendor.** Copy is declarative, present tense, and
states protocol facts as though they were physical laws. It never persuades, never
apologises, never says "we're excited to". There is no marketing warmth here.

**Person: neither.** The brand almost never says "you" and only says "we" in the
manifesto, where "we" means the network rather than a company. Product copy is
impersonal and agentless — *"Intent 0x4a91 settled in 11.4s"*, not *"Your intent
settled"*. Absence of a second person is the point: there is no you, because there
is no identity.

**Casing carries the hierarchy.**

| Level | Casing | Example |
| --- | --- | --- |
| Wordmark, headlines, buttons, nav | UPPERCASE, wide tracking | `ZERO IDENTITY` · `LAUNCH APP` |
| Micro-labels, badges, captions | UPPERCASE, widest tracking, 9–10px | `BRAND MANIFESTO` · `ACCENT — USED < 5%` |
| Body, hints, table cells | Sentence case | "Cabal Mesh is the autonomous mesh layer…" |
| Log and terminal lines | UPPERCASE, terse, ellipsis while in flight | `CONNECTING TO MESH...` → `SUCCESS.` |
| Hex, ids, addresses | lowercase, unabbreviated | `0xa4f2c9e1b70d5533` |

**Sentences are short and end in full stops — including fragments.** "Invisible.
Autonomous. Verifiable." Three words, three periods. The full stop is a structural
device: it makes a fragment land like a specification line. Taglines are punctuated
the same way: *"Zero identity. Private intents. Verifiable execution."*

**Vocabulary is the protocol's own.** *intent, mesh, node, agent, proof,
vault, reputation, signal, encrypt, identity, bridge, relayer, log, settle, offline,
on-chain, zero-identity, slashed, liveness, witness, circuit.* Use these words
literally. Never soften them into product-speak ("transactions", "your wallet",
"seamless").

**Numbers are always exact and always separated.** `1,284` · `9,731` · `23,118` ·
`99.98%` · `11.4s`. Never "over 1,000" or "~23k". Precision is a trust signal for a
protocol that proves things.

**No emoji. Ever.** The board contains none and the voice cannot support them. In
place of an emoji, use a protocol glyph or the rotated-square ornament `◇`.

**Unicode is used sparingly and structurally:** `◇` as the wordmark ornament, `·`
as an inline separator, `|` between status readouts, `>` and `$` as terminal
prompts, `×` as a close affordance. Never as decoration.

**Copy examples that are on-voice**

- "Zero identity. Private intents. Verifiable execution."
- "Cabal Mesh is the autonomous mesh layer that enables private intents to be executed offline and settled on-chain with zero-identity." — *flagged, not changed. Ticket 04's sweep for sibling copy found this makes the same "executed offline" claim the dialog string was retired for. It ships nowhere in the app today, and rewriting the product's one-line positioning is a wider call than a dialog string, so it is left for the design owner rather than corrected here.*
- "This intent broadcasts to the mesh and settles on-chain. No identity is attached."
- "Queued locally. Broadcast and settlement follow reconnection. No identity is attached."
- "We leave no identity, only traces."
- Empty state: "Nothing to export. Nothing is stored."
- Error: "Node 0x2f11 failed liveness." (not "Something went wrong")

**Off-voice — do not write these**

- "Welcome back! 👋 Ready to send your first intent?"
- "Blazing-fast, seamless private transactions."
- "Oops! We couldn't find that page."
- ~~"This intent executes offline and settles on-chain. No identity is attached."~~ — **retired 2026-08-03, ticket 04.** On voice but not true. The architecture is queue-then-drain: offline, an intent is created and queued *locally*, and broadcast and settlement both happen after reconnection. Nothing executes offline. The two replacements above are the approved wording, and the confirm dialog picks between them by connection state.

---

## Visual foundations

### Colour

Six greys, and nothing else, carry the brand: `#FFFFFF` `#BEBEBE` `#7A7A7A`
`#3A3A3A` `#0F0F0F` `#000000` (`--ink-white` … `--ink-void`). The page is pure
black; panels are `#0F0F0F`. Raised, hover and active surfaces are derived from the
ramp with `color-mix`, never invented.

Three accents exist — **neon blue `#00E5FF`**, **blood red `#FF3B3B`**, **acid green
`#9BFF00`** — and the board states the rule explicitly: **used < 5%**. In practice
that means accents appear only as status pips, focus rings, delta figures, toast
edge-bars, and log lines. There are no accent fills, no accent backgrounds, no
accent headings. A screen with a coloured button is off-brand. The only exempt
surface is a terminal log, where `ok`/`err`/`info` lines can be dense.

**No gradients as colour.** The only gradients in the system are a barely-there
top-light panel sheen (4% white, fading by 45%) and a black radial vignette. There
are no hue-shifting gradients anywhere, and specifically no blue-purple ones.

### Type

Two faces. **Display** — a bitmap pixel face (Pixel Operator, currently Silkscreen),
used uppercase only, for the wordmark, headlines, buttons, nav, and numeric figures.
**Body** — IBM Plex Mono, for all prose, labels, table cells and terminal output.
There is no proportional sans anywhere; even body copy is monospaced.

**Tracking does the work that weight does in other systems.** The scale runs
`0em → 0.02 → 0.08 → 0.16 → 0.32 → 0.42em`. The wordmark sits at `0.42em` with a
matching `text-indent` so it stays optically centred. Micro-labels sit at `0.32em`.
Weight barely varies: regular and medium cover almost everything.

Sizes are small and dense — the scale starts at **9px** and body text is **13px**.
Headlines top out around 32px in product UI. This is instrumentation type, not
marketing type.

### Layout

Everything is built from **bordered rectangular panels on a graph grid**. The board
itself is a modular grid of such panels, and the product follows it: an 8px fine
grid (32px coarse) with a 2px base spacing unit. Panels carry a top-left uppercase
label followed by a hairline rule that runs to the panel edge, and **four corner
registration ticks** — 5px L-brackets inset 3px. Fixed elements: a 52px sticky top
nav, a two-row sticky status bar at the bottom, and a 22px right-hand measurement
rail of tick marks. Body copy is capped around 64ch, display copy around 22ch.

### Corners, borders, shadows

**Radius is zero everywhere.** Buttons, inputs, badges, panels, dialogs, status
pips, switch knobs — all sharp rectangles. Even the status indicator is a square,
not a dot. The single exception is `--radius-appicon: 22%`, the OS mask on the
app icon.

Borders are the primary structural device, in four tiers: `hairline` (55% iron,
default), `default`, `strong`, `loud` (white). Emphasis climbs by border tier, not
by fill.

**There are no drop shadows** — dark-on-dark makes them invisible. Elevation is
expressed by **glow** instead: `--glow-blue/green/red/white`, a 10–14px coloured
halo. A modal gets a faint white glow plus a full-viewport black scrim. Cards are
therefore *flat panels with a hairline border and corner ticks*, never shadowed,
never rounded, never with a coloured left edge.

### Texture and imagery

Three plates from the board: **grid** (fine graph paper), **dither** (an ordered
noise gradient), **glitch** (horizontal scan tears). Plus CSS **scanlines** and a
black **vignette**. Standard page treatment is coarse grid + vignette at low
opacity. Dither and glitch are hero-only; never behind body copy. Terminals always
carry scanlines — that is what makes them read as CRT rather than as a code block.

All imagery is **1-bit-feeling pixel art in pure white on black** — cold, high
contrast, no warmth, no colour grade, no photography. Every raster asset renders
with `image-rendering: pixelated` and should be scaled by whole multiples. The
character family is greyscale line-and-dither work; tinting a figure an accent
colour is forbidden.

### Transparency and blur

Used almost never. Transparency appears in the hairline border (`color-mix` with
transparent), the panel sheen, and the modal scrim (86% black). Blur appears in
exactly one place: a **2px backdrop blur on the modal scrim**. There is no frosted
glass, no translucent nav, no blurred cards.

### Motion

**Stepped and mechanical.** The signature easings are `steps(6, end)` and
`steps(3, end)`: meters advance in visible jumps, the switch knob teleports rather
than slides, the terminal caret strobes on a 3-step blink, status pips pulse on a
900ms stepped cycle. `--ease-out: cubic-bezier(.16,.84,.44,1)` covers ordinary
colour transitions. Durations: 60 / 120 / 200 / 420 / 900ms.

**No bounce, no overshoot, no spring, no soft ease-in-out — ever.** Also available:
`cm-blink`, `cm-pulse`, `cm-scan`, `cm-glitch-x`, `cm-flicker`, `cm-rotate`
keyframes. Everything respects `prefers-reduced-motion`.

### Interaction states

| State | Treatment |
| --- | --- |
| **Hover (button)** | **Inverts** — fills solid white, text goes black. Never lightens. |
| **Hover (row, tab, icon button)** | Surface steps one notch up the ramp; text goes from muted to primary |
| **Press** | `opacity: 0.72`. No scale, no translate, no shrink. |
| **Focus** | 1px neon-blue ring plus `--glow-blue`. The one accent every screen is allowed. |
| **Active/selected** | 2px white underline flush with the container rule (nav, tabs), or a white fill with black text (segmented control) |
| **Disabled** | Text drops to `--text-disabled`, border to `hairline`, `cursor: not-allowed`. No opacity fade on the whole control. |
| **Invalid** | Border goes blood red; an uppercase error line replaces the hint |

### Anti-patterns

No rounded corners. No drop shadows. No gradient backgrounds. No emoji. No
proportional sans. No accent fills. No blue-purple gradients. No cards with a
coloured left border. No bouncy or springy motion. No photography. No warm colour.
No sentence-case buttons. No hand-drawn SVG substitutes for the real glyphs.

---

## Iconography

**There is exactly one icon set: the 14 protocol glyphs** from the board's ICON
SYSTEM plate — `node` `agent` `intent` `mesh` `proof` `escrow` `vault` `reputation`
`signal` `encrypt` `identity` `bridge` `relayer` `log`. They live in
`assets/icons/*.png` as white-on-transparent pixel art extracted directly from the
board, and are rendered through the `Icon` component.

They are **not an icon font and not SVGs** — the board supplies them as raster pixel
art, so they are shipped as PNGs and rendered `pixelated`. No icon font, sprite
sheet or vector source exists in the supplied material.

**No CDN icon library is used or linked.** Lucide, Heroicons, Feather and friends
are all wrong here: their 1.5–2px rounded strokes are the opposite of a 1px bitmap
grid glyph. If a concept has no glyph, use the nearest protocol glyph or fall back
to a tracked uppercase text label — do **not** substitute a library icon and do
**not** draw a new one.

Glyphs are white by default (`opacity` makes them read as muted, which is how the
board greys them); `tint` recolours one via a CSS mask for the rare accented case.
Nominal size is 20px, and 12–46px all appear in the board. Emoji are never used.
Unicode `◇ · | > $ ×` do structural duty as described under Content fundamentals.

The **8 character-family archetypes** (`assets/characters/*.png`) are the brand's
illustration system rather than iconography — full-figure hooded portraits used to
personify network roles. The **5 logo lockups** and **3 texture plates** are
likewise extracted rasters. Every visual asset in this system was copied out of the
supplied board; none was drawn or generated.

---

## Index

### Root
| File | Purpose |
| --- | --- |
| `readme.md` | This design guide |
| `SKILL.md` | Agent Skills front-matter, for use in Claude Code |
| `styles.css` | **The single entry point consumers link.** `@import` list only |
| `thumbnail.html` | Homepage tile for this design system |
| `github.md` | Source-repo association for upstream sync |

### `tokens/` — 162 custom properties
`fonts.css` (webfaces + the Pixel Operator swap block) · `colors.css` (mono ramp,
accents, surfaces, text, borders, status) · `typography.css` (families, sizes,
weights, tracking, leading, semantic roles) · `spacing.css` (2px scale + semantic
roles) · `effects.css` (radii, borders, glows, textures, motion, z-index) ·
`base.css` (reset, document defaults, link colours, keyframes)

### `assets/`
`logo/` — `primary-logo` `wordmark-stacked` `symbol-mark` `icon-mark` `minimal-mark`
`hero-lockup` `oracle-emblem` · `icons/` — the 14 protocol glyphs ·
`characters/` — the 8 archetypes · `textures/` — `grid` `dither` `glitch`

### `components/` — 22 components in 5 groups

**`core/`** — `Panel` (+ `CornerTicks`), `Button`, `IconButton`, `Badge`,
`StatusDot`, `Divider`, `Logo` (+ `LogoType`), `Icon` (+ `MESH_ICONS`)

**`forms/`** — `Field`, `Input`, `Select`, `Checkbox`, `Radio` (+ `RadioGroup`),
`Switch`

**`data/`** — `StatBlock` (+ `StatInline`), `Meter`, `DataTable`, `Terminal`

**`navigation/`** — `NavBar`, `Tabs`

**`feedback/`** — `Dialog`, `Toast` (+ `ToastStack`), `Tooltip`

**`brand/`** — `TextureField`, `CharacterPortrait` (+ `CHARACTERS`)

Each has a sibling `.d.ts` props contract and a `.prompt.md` usage note; each
directory has one `@dsCard` HTML showing its variants and states.

#### Intentional additions

No codebase or Figma file defined a component inventory (see the GitHub caveat), so
this is an authored standard set sized to the brand. Three entries go beyond a
generic set and exist because the board explicitly defines them:

- **`Icon` / `MESH_ICONS`** — a wrapper for the board's 14-glyph plate, since the glyphs are rasters with no font or sprite.
- **`TextureField`** — the PATTERN & TEXTURE plate as a composable background layer.
- **`CharacterPortrait` / `CHARACTERS`** — the CHARACTER FAMILY plate; the archetypes are a named, closed set in the board.

`Panel`, `Terminal`, `StatBlock`/`StatInline` and `NavBar` are also board-driven
rather than generic: each recreates a specific element of the APPLICATION PREVIEW or
the panel chrome.

### `ui_kits/`
| Kit | Contents |
| --- | --- |
| `mesh-console/` | The protocol console — Dashboard (faithful APPLICATION PREVIEW rebuild), Nodes, Intents, Proofs, Reputation, Settings. Click-through with real dialogs and toasts. |
| `brand-surfaces/` | The APPLICATION EXAMPLES plate — app icon, sticker, tee, terminal, business card, poster — plus browsable Marks, Glyphs and Archetypes tabs. |
| `ds-runtime.js` | Resolves primitives from `_ds_bundle.js`, falling back to the component sources so a kit opens standalone. |

### `guidelines/` — 19 specimen cards
**Colors** — Mono Ramp · Accents · Surfaces · Text · Status & Borders
**Type** — Display Face · Wordmark Tracking · Mono Face · Type Scale · Tracking Scale · Labels & Data
**Spacing** — Space Scale · Semantic Spacing · Grid In Use
**Brand** — Logo System · Icon System · Character Family · Pattern & Texture · Manifesto · Brand DNA · Corners & Ticks · States & Glows · Motion

### Generated — never edit
`_ds_bundle.js` · `_ds_manifest.json` · `_adherence.oxlintrc.json`
