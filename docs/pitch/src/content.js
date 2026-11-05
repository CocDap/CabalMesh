// ============================================================================
// content.js — every word in the deck. Edit copy here, never in the renderers.
//
// Register: written for engineers and investors who already know the domain.
// State facts and constraints; cite the specific mechanism. No explaining what
// a ZK proof or a keystore is, and no "you'll know it worked when" hand-holding.
// ============================================================================

const HORIZON = 'Aug 2026 → Jan 2027';

/** Master phase list. Timeline, overview and detail slides all read from this. */
const PHASES = [
  {
    n: '01',
    short: 'Recovery & intent parsing',
    title: 'Recovery guarantees & intent parsing',
    window: 'Aug – Sep 2026',
    windowUpper: 'AUG – SEP 2026',
    start: 0, span: 2,
    summary: 'Eliminate unrecoverable key loss; restore NL intent parsing over the existing validated draft pipeline.',
  },
  {
    n: '02',
    short: 'Hardware concepts & demand',
    title: 'Hardware concepts & demand signal',
    window: 'Sep – Oct 2026',
    windowUpper: 'SEP – OCT 2026',
    start: 1, span: 2,
    summary: 'Two reference designs, published as concepts. Waitlist volume decides whether tooling is worth funding.',
  },
  {
    n: '03',
    short: 'Marketplace goes live',
    title: 'Marketplace goes live',
    window: 'Oct – Nov 2026',
    windowUpper: 'OCT – NOV 2026',
    start: 2, span: 2,
    summary: 'Convert verified relay work into on-chain rewards and open a market for node modules.',
  },
  {
    n: '04',
    short: 'Verification & negotiation',
    title: 'Verification & bounded negotiation',
    window: 'Nov – Dec 2026',
    windowUpper: 'NOV – DEC 2026',
    start: 3, span: 2,
    summary: 'Close the gap between claimed and enforced: real circuit verification, guardrails in Rust.',
  },
  {
    n: '05',
    short: 'Confidential compute',
    title: 'Confidential compute & platform parity',
    window: 'Dec 2026 – Jan 2027',
    windowUpper: 'DEC 2026 – JAN 2027',
    start: 4, span: 2,
    summary: 'Scope FHE/MPC for negotiation privacy; close the desktop keystore and BLE-plane gaps.',
  },
];

const CONTENT = {
  horizon: HORIZON,
  months: ['AUG', 'SEP', 'OCT', 'NOV', 'DEC', 'JAN'],
  phases: PHASES,

  cover: {
    eyebrow: 'PROJECT DEVELOPMENT PLAN',
    title: 'CabalMesh',
    subtitle: 'Roadmap, milestones & execution plan',
    horizonLabel: 'AUG 2026  →  JAN 2027',
    blurb: 'Scoped against a code-level status audit rather than the pitch. Five phases, each with '
         + 'a stated constraint and an falsifiable exit condition.',
    credit: 'kurodenjiro  ·  Baseline: docs/product-status.md',
  },

  timeline: {
    eyebrow: 'ROADMAP',
    title: HORIZON,
    note: 'Phases overlap deliberately. Bars are execution windows, not commitments — each phase exits on its condition, not its date.',
  },

  overview: {
    eyebrow: 'ROADMAP',
    title: 'Five phases',
  },

  // ---- phase 1: two-column checklist ------------------------------------
  phase1: {
    lead: 'Today a wiped device means permanently lost funds. Guardian recovery and passphrase unlock '
        + 'already ship; this phase closes the remaining gaps and reconnects the intent parser.',
    columns: [
      {
        label: 'RECOVERY',
        items: [
          '24–48h delay on guardian recovery with a veto notification, so a compromised device can be countermanded',
          '6-digit PIN unlock, gated on the platform keystore\'s hardware-enforced retry counter (ticket 21)',
          'Guardian enrollment and share-distribution UI, matching the shipped protocol',
          'Guardian storage inherits the SECURITY passphrase setting rather than staying file-key-protected',
        ],
      },
      {
        label: 'INTENT PARSING',
        items: [
          'Editable chip UI, so a mis-parse is corrected in place instead of falling back to the form',
          'parse_intent_chat QA — the parser already emits the validated fields New.tsx consumes',
          'Conversational recovery assistant for the device-loss path, where static UI branches badly',
          'Invariant held: the model emits IntentFields only; Rust validates, previews and signs',
        ],
      },
    ],
    doneLabel: 'Exit condition',
    done: 'A wiped device reconstructs its key from 3-of-5 guardians with the veto window verified on physical '
        + 'hardware, and a natural-language intent reaches broadcast without widening what Rust will sign.',
  },

  // ---- phase 2: product spotlight ---------------------------------------
  phase2: {
    lead: 'Two reference designs that make the protocol legible — and a demand test before committing capital to tooling.',
    products: [
      {
        image: 'deck-shadowbox.png',
        tag: 'MESH · MODEL · PROVER',
        name: 'ShadowBox',
        body: 'One node instead of three: mesh relay, local model and prover in a single fanless slab, with RADIO / CRYPTO / POWER bays mirroring the in-app module system.',
      },
      {
        image: 'deck-nobody.png',
        tag: 'PHYSICAL ESCROW',
        name: 'The Nobody Box',
        body: 'A parcel locker whose bolt turns on the on-chain release. A load cell reports that something went in and something left — never what it was.',
      },
    ],
    caveat: 'Deliverable is industrial design and a demand signal, not units. No manufacturer is engaged; '
          + 'tooling spend is gated on waitlist conversion.',
    done: 'Exit condition: both designs rendered, waitlist live, and signup volume sufficient to justify — or kill — the hardware track.',
  },

  // ---- go-to-market ------------------------------------------------------
  gtm: {
    eyebrow: 'GO-TO-MARKET',
    title: 'Distribution is continuous, sequenced behind shipped capability',
    lead: 'Each push is anchored to a milestone that gives the audience something verifiable. No campaign runs ahead of the artifact it describes.',
    highlightIndex: 2,
    stages: [
      {
        months: 'AUG – SEP', tag: 'BUILD IN PUBLIC', hook: 'Recovery ships',
        body: 'Engineering updates on the offline settlement path — the one differentiated capability that already works end to end. Zero cost, compounding audience.',
        metric: 'Follower growth, update engagement',
      },
      {
        months: 'SEP – OCT', tag: 'CONCEPT REVEAL', hook: 'Renders complete',
        body: 'Publish both reference designs. Waitlist is instrumented as a demand test — no pre-orders taken, no delivery implied.',
        metric: 'Waitlist conversion rate',
      },
      {
        months: 'OCT – NOV', tag: 'PRIMARY LAUNCH', hook: 'Marketplace live',
        body: 'The substantive launch, once relay rewards and modules are transactable. Kaito mindshare campaign and mesh missions activate here.',
        metric: 'Active relaying nodes, missions completed',
      },
      {
        months: 'DEC – JAN', tag: 'TECHNICAL PROOF', hook: 'Verification real',
        body: 'Credibility content once circuit verification is genuinely running. Targets developer and press audiences, not retail.',
        metric: 'Press pickups, developer signups',
      },
    ],
    audience: 'Audience: Avalanche ecosystem builders · privacy and secure-hardware communities · off-grid and censorship-resistant trade',
    note: 'No paid acquisition until organic conversion is demonstrated at the Oct–Nov launch. Hardware remains concept-only pending a manufacturing partner.',
  },

  // ---- mesh missions -----------------------------------------------------
  missions: {
    eyebrow: 'GO-TO-MARKET · CHANNELS',
    title: 'Mesh missions & the Kaito mindshare campaign',
    lead: 'Two tracks, deliberately asymmetric: product missions settle on-chain, social missions are scored off it. '
        + 'Rewards weight toward verifiable usage.',
    tracks: [
      {
        label: 'PRODUCT MISSIONS', sub: 'Settled against on-chain state', offset: 1,
        items: [
          'Broadcast an intent over the mesh with no network path available',
          'Relay a threshold volume of gateway traffic',
          'Enroll guardians and complete a recovery dry run',
          'Acquire or equip a module NFT once the Marketplace ships',
        ],
      },
      {
        label: 'SOCIAL MISSIONS · KAITO', sub: 'Scored by Kaito\'s mindshare model', offset: 5,
        items: [
          'List CabalMesh as a Kaito Genesis/Yapper project',
          'Publish about the protocol — Kaito scores and ranks the post publicly',
          'Referral that converts to a completed product mission, not a bare signup',
        ],
      },
    ],
    rewardLabel: 'REWARD MECHANIC',
    reward: 'Threshold completion mints a soulbound Genesis Node badge via the existing CabalMeshVoucher contract — '
          + 'reusing the non-transferable primitive already backing the in-app Standing Badge. Kaito determines rank; '
          + 'on-chain state determines eligibility. Nothing is gated on a metric Kaito cannot verify.',
  },

  // ---- phase 3: flow -----------------------------------------------------
  phase3: {
    lead: 'Marketplace.sol and CabalMeshVoucher.sol are deployed to Fuji but unreachable from the app. '
        + 'This phase makes relay work economically real.',
    flow: ['Relay traffic', 'Earn MB', 'Convert to AVAX', 'Acquire module', 'Higher yield'],
    flowNote: 'A reflexive loop: utilisation raises module value, which raises the incentive to relay.',
    shipsLabel: 'SCOPE',
    ships: [
      'Redeploy CabalMeshVoucher with mint authority bound to RelayRewards — closing the current unrestricted-mint path',
      'Wire gateway relay to RelayRewards.recordGatewayRelay, paid atomically from the settlement it carries',
      'Ship MARKET and VAULT → MODULES against the redeployed contracts; compute yield from verified ownership, not local state',
    ],
    done: 'Exit condition: MB earned from attributable gateway relay converts to AVAX and buys a module whose yield effect is verifiable on-chain.',
  },

  // ---- phase 4: before / after ------------------------------------------
  phase4: {
    lead: 'The deck currently claims verified proofs and negotiating agents. Neither is enforced today. '
        + 'This phase makes the claims true or removes them.',
    before: {
      label: 'CURRENT STATE',
      items: [
        'No ZK proving code at all — the unused Noir stub was deleted rather than left to imply a capability',
        'No agent-to-agent negotiation — matching resolves against a fixed listing price',
        'No structural bound preventing an unsafe negotiation implementation',
      ],
    },
    after: {
      label: 'TARGET STATE',
      items: [
        'Proof verification runs against a real Noir circuit in CI, not a non-empty-input check',
        'Bounded offer / counter-offer / accept protocol with a hard round limit',
        'Price guardrails enforced in Rust — the model proposes, it never sets the bound',
      ],
    },
    caveat: 'Constraint: no nargo in the current CI image. Negotiation is sequenced behind single-shot matching reliability — '
          + 'llama2 JSON adherence is already marginal, and multi-turn compounds it.',
  },

  // ---- phase 5: statement ------------------------------------------------
  phase5: {
    eyebrow: 'PHASE 5 · DEC 2026 – JAN 2027 · EXPLORATORY',
    headline: 'ZK proves the outcome.',
    headlineAccent: 'It does not hide the negotiation.',
    sub: 'Genuinely open research. The deliverable is a scoped feasibility assessment, not shipped code.',
    items: [
      'Assess FHE and MPC for negotiation-content privacy — no prior art exists in this codebase',
      'Bind the desktop vault key to Secure Enclave / TPM instead of the current 0o600 file provider',
      'Resolve the BLE plane on Windows and Linux, where backend::choose returns None and the app degrades silently',
    ],
  },

  closing: {
    title: 'Revised against the code, not the narrative.',
    sub: 'Every claim in this deck is traceable to a file in the repository. When the status audit changes, this plan changes with it.',
    links: [
      ['Current phase', 'Recovery & intent parsing'],
      ['Source & docs', 'github.com — CabalMesh'],
      ['Deployed', 'Avalanche Fuji Testnet'],
    ],
    footer: 'CABALMESH  ·  PROJECT DEVELOPMENT PLAN',
  },
};

module.exports = CONTENT;
