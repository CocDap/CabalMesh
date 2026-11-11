/** Step 3: a third phone with internet, the gateway, settles the pair; both phones stay offline. */
export default function StepSceneSettle() {
  return (
    <svg className="sc" viewBox="0 0 480 360" role="img" aria-label="The seller and the buyer stay offline. A third phone with internet, the gateway, takes both signed diffs over Bluetooth, submits the pair to execute_intents on NEAR and passes the proof back over Bluetooth; the seller ends USDC plus 0.368, the buyer USDC minus 0.388, and the gateway earns a 0.02 USDC reward">
      <rect className="bg" width="480" height="360" />
      <path className="hair" strokeDasharray="4 4" d="M12 92H468" />
      <text className="l m" x="12" y="84">INTERNET</text>
      <text className="l m" x="468" y="84" textAnchor="end">BLUETOOTH BELOW</text>
      <rect className="box" x="160" y="10" width="160" height="68" />
      <text className="h s" x="240" y="32" textAnchor="middle">NEAR</text>
      <text className="t w" x="240" y="52" textAnchor="middle">execute_intents</text>
      <text className="t g" x="240" y="70" textAnchor="middle">both legs settled ✓</text>
      <path className="ln c" d="M226 108V84" />
      <path className="hd c" d="M226 78l-4.5 7h9z" />
      <path className="ln g" d="M254 78V102" />
      <path className="hd g" d="M254 108l-4.5 -7h9z" />
      <text className="l c" x="218" y="104" textAnchor="end">SUBMIT</text>
      <text className="l g" x="262" y="104">PROOF</text>
      <rect className="ph" x="196" y="108" width="88" height="128" rx="14" />
      <rect className="ph-slot" x="228.0" y="116" width="24" height="4" />
      <rect className="scr" x="202" y="128" width="76" height="96" />
      <text className="h xs" x="240" y="148" textAnchor="middle">GATEWAY</text>
      <path className="hair" d="M202 158H278" />
      <rect className="sq g" x="210" y="170" width="8" height="8" />
      <text className="l g" x="224" y="178">ONLINE</text>
      <text className="t xs m" x="240" y="200" textAnchor="middle">reward</text>
      <text className="t xs g" x="240" y="216" textAnchor="middle">+0.02 USDC</text>
      <rect className="ph" x="12" y="120" width="140" height="228" rx="18" />
      <rect className="ph-slot" x="66.0" y="129" width="32" height="4" />
      <rect className="scr" x="19" y="142" width="126" height="184" />
      <text className="l m" x="28" y="166">SELLER</text>
      <text className="h g" x="28" y="190">SETTLED</text>
      <path className="hair" d="M27 206H137" />
      <text className="t m" x="28" y="230">wNEAR</text>
      <text className="t w" x="136" y="230" textAnchor="end">−0.1</text>
      <text className="t m" x="28" y="254">USDC</text>
      <text className="t w" x="136" y="254" textAnchor="end">+0.368</text>
      <text className="t m" x="28" y="274">paid no gas</text>
      <path className="hair" d="M19 304H145" />
      <text className="l m" x="82.0" y="319" textAnchor="middle">OFFLINE</text>
      <rect className="ph" x="328" y="120" width="140" height="228" rx="18" />
      <rect className="ph-slot" x="382.0" y="129" width="32" height="4" />
      <rect className="scr" x="335" y="142" width="126" height="184" />
      <text className="l m" x="344" y="166">BUYER</text>
      <text className="h g" x="344" y="190">SETTLED</text>
      <path className="hair" d="M343 206H453" />
      <text className="t m" x="344" y="230">wNEAR</text>
      <text className="t w" x="452" y="230" textAnchor="end">+0.1</text>
      <text className="t m" x="344" y="254">USDC</text>
      <text className="t w" x="452" y="254" textAnchor="end">−0.388</text>
      <text className="t m" x="344" y="274">paid no gas</text>
      <path className="hair" d="M335 304H461" />
      <text className="l m" x="398.0" y="319" textAnchor="middle">OFFLINE</text>
      <path className="ln c" d="M156 162H186" />
      <path className="hd c" d="M192 162l-7 -4.5v9z" />
      <path className="ln g" d="M192 198H162" />
      <path className="hd g" d="M156 198l7 -4.5v9z" />
      <path className="ln c" d="M324 162H294" />
      <path className="hd c" d="M288 162l7 -4.5v9z" />
      <path className="ln g" d="M288 198H318" />
      <path className="hd g" d="M324 198l-7 -4.5v9z" />
      <text className="l c" x="174" y="154" textAnchor="middle">DIFF</text>
      <text className="l c" x="306" y="154" textAnchor="middle">DIFF</text>
      <text className="l g" x="174" y="216" textAnchor="middle">PROOF</text>
      <text className="l g" x="306" y="216" textAnchor="middle">PROOF</text>
      <text className="t g" x="240" y="270" textAnchor="middle">SettlementComplete</text>
      <text className="t m" x="240" y="288" textAnchor="middle">over Bluetooth</text>
      <text className="t m" x="240" y="318" textAnchor="middle">both phones</text>
      <text className="t m" x="240" y="334" textAnchor="middle">stay offline</text>
    </svg>
  );
}
