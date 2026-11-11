/** Step 2: each phone signs its half as a NEP-413 diff, still offline. */
export default function StepSceneSign() {
  return (
    <svg className="sc" viewBox="0 0 480 360" role="img" aria-label="Both phones, still offline, sign their half as a NEP-413 token diff: the seller wNEAR minus 0.1 and USDC plus 0.378, the buyer the mirror, each with a 0.01 USDC fee; the two signed diffs cross over Bluetooth">
      <rect className="bg" width="480" height="360" />
      <rect className="ph" x="12" y="24" width="140" height="312" rx="18" />
      <rect className="ph-slot" x="66.0" y="33" width="32" height="4" />
      <rect className="scr" x="19" y="46" width="126" height="268" />
      <text className="l m" x="28" y="72">SELLER</text>
      <text className="h" x="28" y="98">SELL WNEAR</text>
      <text className="l m" x="28" y="118">NEP-413 DIFF</text>
      <path className="hair" d="M27 134H137" />
      <text className="t m" x="28" y="158">wNEAR</text>
      <text className="t w" x="136" y="158" textAnchor="end">−0.1</text>
      <text className="t m" x="28" y="182">USDC</text>
      <text className="t w" x="136" y="182" textAnchor="end">+0.378</text>
      <text className="t m" x="28" y="206">fee</text>
      <text className="t m" x="136" y="206" textAnchor="end">0.01 USDC</text>
      <path className="hair" d="M27 222H137" />
      <rect className="sq g" x="28" y="242" width="8" height="8" />
      <text className="l g" x="42" y="250">SIGNED</text>
      <path className="hair" d="M19 292H145" />
      <text className="l m" x="82.0" y="307" textAnchor="middle">OFFLINE</text>
      <rect className="ph" x="328" y="24" width="140" height="312" rx="18" />
      <rect className="ph-slot" x="382.0" y="33" width="32" height="4" />
      <rect className="scr" x="335" y="46" width="126" height="268" />
      <text className="l m" x="344" y="72">BUYER</text>
      <text className="h" x="344" y="98">BUY WNEAR</text>
      <text className="l m" x="344" y="118">NEP-413 DIFF</text>
      <path className="hair" d="M343 134H453" />
      <text className="t m" x="344" y="158">wNEAR</text>
      <text className="t w" x="452" y="158" textAnchor="end">+0.1</text>
      <text className="t m" x="344" y="182">USDC</text>
      <text className="t w" x="452" y="182" textAnchor="end">−0.378</text>
      <text className="t m" x="344" y="206">fee</text>
      <text className="t m" x="452" y="206" textAnchor="end">0.01 USDC</text>
      <path className="hair" d="M343 222H453" />
      <rect className="sq g" x="344" y="242" width="8" height="8" />
      <text className="l g" x="358" y="250">SIGNED</text>
      <path className="hair" d="M335 292H461" />
      <text className="l m" x="398.0" y="307" textAnchor="middle">OFFLINE</text>
      <text className="h s" x="240.0" y="30" textAnchor="middle">BLUETOOTH</text>
      <text className="t m" x="240.0" y="46" textAnchor="middle">no internet</text>
      <path className="ln " d="M158 128H316" />
      <path className="hd " d="M322 128l-7 -4.5v9z" />
      <text className="t" x="240.0" y="120" textAnchor="middle">SignedDiff A</text>
      <path className="ln " d="M322 184H164" />
      <path className="hd " d="M158 184l7 -4.5v9z" />
      <text className="t" x="240.0" y="176" textAnchor="middle">SignedDiff B</text>
      <text className="l g" x="240.0" y="240" textAnchor="middle">COUNTERPARTY</text>
      <text className="l g" x="240.0" y="256" textAnchor="middle">SIGNED</text>
      <text className="t m" x="240.0" y="290" textAnchor="middle">no gas, no RPC,</text>
      <text className="t m" x="240.0" y="306" textAnchor="middle">no balance moved</text>
    </svg>
  );
}
