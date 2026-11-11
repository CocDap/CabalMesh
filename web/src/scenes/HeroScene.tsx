/** Two offline phones sign locally; a gateway with internet submits the pair to NEAR Intents. */
export default function HeroScene() {
  return (
    <svg className="sc hero-scene" viewBox="0 0 480 620" preserveAspectRatio="xMidYMid meet" role="img" aria-label="Two offline phones, a seller and a buyer, agree 0.1 wNEAR at $3.78 over Bluetooth and each signs its half locally as a NEP-413 diff, with the key kept on the phone. A third peer with internet, the gateway, receives both signed diffs over the mesh, submits the pair to execute_intents on NEAR Intents, pays the gas, earns 0.02 USDC and floods the proof back to both phones">
      <path className="hair" strokeDasharray="4 4" d="M12 98H468" />
      <text className="l m" x="12" y="90">INTERNET</text>
      <text className="l m" x="12" y="116">NO INTERNET</text>
      <rect className="box" x="150" y="10" width="180" height="72" />
      <path className="tick" d="M147 12V7H152M328 7H333V12M147 80V85H152M328 85H333V80" />
      <text className="h" x="240" y="34" textAnchor="middle">NEAR INTENTS</text>
      <text className="t w" x="240" y="56" textAnchor="middle">execute_intents</text>
      <text className="t s m" x="240" y="74" textAnchor="middle">both legs or none</text>
      <path className="ln c" d="M216 128V88" />
      <path className="hd c" d="M216 82l-4.5 7h9z" />
      <text className="l c" x="208" y="120" textAnchor="end">SUBMIT PAIR</text>
      <path className="ln g" d="M264 82V122" />
      <path className="hd g" d="M264 128l-4.5 -7h9z" />
      <text className="l g" x="272" y="120">TX HASH</text>
      <rect className="ph" x="170" y="128" width="140" height="150" rx="16" />
      <rect className="ph-slot" x="226.0" y="136" width="28" height="4" />
      <rect className="scr" x="177" y="148" width="126" height="120" />
      <text className="h xs" x="184" y="168">GATEWAY</text>
      <rect className="sq g" x="288" y="160" width="8" height="8" />
      <text className="l g" x="184" y="186">ROUTER · ONLINE</text>
      <path className="hair" d="M177 196H303" />
      <text className="t xs w" x="184" y="214">SignedDiff A</text>
      <text className="t xs g" x="296" y="214" textAnchor="end">✓</text>
      <text className="t xs w" x="184" y="232">SignedDiff B</text>
      <text className="t xs g" x="296" y="232" textAnchor="end">✓</text>
      <path className="hair" d="M177 242H303" />
      <text className="t xs m" x="184" y="260">pays gas</text>
      <text className="t xs g" x="296" y="260" textAnchor="end">+0.02</text>
      <path className="ln" d="M56 300V188H164" />
      <path className="hd" d="M170 188l-7 -4.5v9z" />
      <path className="ln g" d="M170 224H110V294" />
      <path className="hd g" d="M110 300l-4.5 -7h9z" />
      <text className="t xs w" x="64" y="180">SignedDiff A</text>
      <text className="l g" x="118" y="246">PROOF</text>
      <path className="ln" d="M424 300V188H316" />
      <path className="hd" d="M310 188l7 -4.5v9z" />
      <path className="ln g" d="M310 224H370V294" />
      <path className="hd g" d="M370 300l-4.5 -7h9z" />
      <text className="t xs w" x="416" y="180" textAnchor="end">SignedDiff B</text>
      <text className="l g" x="362" y="246" textAnchor="end">PROOF</text>
      <rect className="ph" x="12" y="300" width="168" height="312" rx="20" />
      <rect className="ph-slot" x="78.0" y="309" width="36" height="4" />
      <rect className="scr" x="19" y="322" width="154" height="276" />
      <rect className="sq m" x="27" y="333" width="8" height="8" />
      <text className="l m" x="41" y="341">NO INTERNET</text>
      <text className="l c" x="165" y="341" textAnchor="end">BLE</text>
      <path className="hair" d="M19 352H173" />
      <text className="h" x="27" y="376">SIGN OFFLINE</text>
      <text className="l m" x="27" y="394">SELLER</text>
      <rect className="pn" x="27" y="408" width="138" height="92" />
      <path className="tick" d="M24 410V405H29M163 405H168V410M24 498V503H29M163 503H168V498" />
      <text className="t s m" x="39" y="432">wNEAR</text>
      <text className="t s w" x="153" y="432" textAnchor="end">−0.1</text>
      <text className="t s m" x="39" y="456">USDC</text>
      <text className="t s w" x="153" y="456" textAnchor="end">+0.378</text>
      <text className="t s m" x="39" y="480">fee</text>
      <text className="t s m" x="153" y="480" textAnchor="end">0.01</text>
      <text className="t xs m" x="27" y="524">NEP-413 · ed25519</text>
      <rect className="ln g" x="27" y="536" width="138" height="30" />
      <text className="l g" x="96.0" y="556" textAnchor="middle">✓ SIGNED LOCALLY</text>
      <text className="t xs m" x="27" y="588">key stays on phone</text>
      <rect className="ph" x="300" y="300" width="168" height="312" rx="20" />
      <rect className="ph-slot" x="366.0" y="309" width="36" height="4" />
      <rect className="scr" x="307" y="322" width="154" height="276" />
      <rect className="sq m" x="315" y="333" width="8" height="8" />
      <text className="l m" x="329" y="341">NO INTERNET</text>
      <text className="l c" x="453" y="341" textAnchor="end">BLE</text>
      <path className="hair" d="M307 352H461" />
      <text className="h" x="315" y="376">SIGN OFFLINE</text>
      <text className="l m" x="315" y="394">BUYER</text>
      <rect className="pn" x="315" y="408" width="138" height="92" />
      <path className="tick" d="M312 410V405H317M451 405H456V410M312 498V503H317M451 503H456V498" />
      <text className="t s m" x="327" y="432">wNEAR</text>
      <text className="t s w" x="441" y="432" textAnchor="end">+0.1</text>
      <text className="t s m" x="327" y="456">USDC</text>
      <text className="t s w" x="441" y="456" textAnchor="end">−0.378</text>
      <text className="t s m" x="327" y="480">fee</text>
      <text className="t s m" x="441" y="480" textAnchor="end">0.01</text>
      <text className="t xs m" x="315" y="524">NEP-413 · ed25519</text>
      <rect className="ln g" x="315" y="536" width="138" height="30" />
      <text className="l g" x="384.0" y="556" textAnchor="middle">✓ SIGNED LOCALLY</text>
      <text className="t xs m" x="315" y="588">key stays on phone</text>
      <rect className="plate" x="190" y="316" width="100" height="246" />
      <text className="h s" x="240" y="336" textAnchor="middle">BLUETOOTH</text>
      <text className="t s m" x="240" y="354" textAnchor="middle">phone to</text>
      <text className="t s m" x="240" y="370" textAnchor="middle">phone</text>
      <path className="ln " d="M186 398H288" />
      <path className="hd " d="M294 398l-7 -4.5v9z" />
      <path className="ln " d="M294 412H192" />
      <path className="hd " d="M186 412l7 -4.5v9z" />
      <text className="h xl g" x="240" y="462" textAnchor="middle">$3.78</text>
      <text className="l g" x="240" y="482" textAnchor="middle">AGREED</text>
      <text className="t xs m" x="240" y="534" textAnchor="middle">each signs</text>
      <text className="t xs m" x="240" y="550" textAnchor="middle">its own half</text>
    </svg>
  );
}
