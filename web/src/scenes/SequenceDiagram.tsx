/** The trade sequence diagram: seller, mesh, buyer, gateway and NEAR, step by step. */
export default function SequenceDiagram() {
  return (
    <svg className="seq" viewBox="0 0 1080 726" role="img" aria-labelledby="seq-title" xmlns="http://www.w3.org/2000/svg">
      <title id="seq-title">
        Seller and buyer phones meet over Bluetooth, swap intents without prices, agree at $3.70, sign offline, and a gateway submits the pair to NEAR, which settles both legs and the proof floods back.
      </title>
      <defs>
        <marker id="ah-m" viewBox="0 0 10 10" refX="9" refY="5" markerWidth="6" markerHeight="6" orient="auto-start-reverse">
          <path d="M0,0 L10,5 L0,10 z" className="ah m" />
        </marker>
        <marker id="ah-a" viewBox="0 0 10 10" refX="9" refY="5" markerWidth="6" markerHeight="6" orient="auto-start-reverse">
          <path d="M0,0 L10,5 L0,10 z" className="ah a" />
        </marker>
        <marker id="ah-g" viewBox="0 0 10 10" refX="9" refY="5" markerWidth="6" markerHeight="6" orient="auto-start-reverse">
          <path d="M0,0 L10,5 L0,10 z" className="ah g" />
        </marker>
      </defs>
      <g>
        <rect className="bandbg" x="0" y="56" width="1080" height="42" />
        <line className="sep" x1="0" y1="56" x2="1080" y2="56" />
        <text className="bl" x="8" y="71">01 DISCOVER</text>
      </g>
      <g>
        <line className="sep" x1="0" y1="98" x2="1080" y2="98" />
        <text className="bl" x="8" y="113">02 INTENT</text>
      </g>
      <g>
        <line className="sep" x1="0" y1="224" x2="1080" y2="224" />
        <text className="bl" x="8" y="239">03 NEGOTIATE</text>
      </g>
      <g>
        <line className="sep" x1="0" y1="418" x2="1080" y2="418" />
        <text className="bl" x="8" y="433">04 SIGN</text>
      </g>
      <g>
        <line className="sep" x1="0" y1="492" x2="1080" y2="492" />
        <text className="bl" x="8" y="507">05 ROUTE</text>
      </g>
      <g>
        <line className="sep" x1="0" y1="604" x2="1080" y2="604" />
        <text className="bl" x="8" y="619">06 SETTLE</text>
      </g>
      <line className="life" x1="125" y1="44" x2="125" y2="722" />
      <line className="life" x1="335" y1="44" x2="335" y2="722" />
      <line className="life" x1="545" y1="44" x2="545" y2="722" />
      <line className="life" x1="755" y1="44" x2="755" y2="722" />
      <line className="life" x1="965" y1="44" x2="965" y2="722" />
      <rect className="lane" x="40" y="6" width="170" height="38" />
      <text className="lh" x="125" y="22" textAnchor="middle">SELLER PHONE</text>
      <text className="ls" x="125" y="37" textAnchor="middle">offline</text>
      <rect className="lane" x="250" y="6" width="170" height="38" />
      <text className="lh" x="335" y="22" textAnchor="middle">MESH</text>
      <text className="ls" x="335" y="37" textAnchor="middle">BLE · libp2p</text>
      <rect className="lane" x="460" y="6" width="170" height="38" />
      <text className="lh" x="545" y="22" textAnchor="middle">BUYER PHONE</text>
      <text className="ls" x="545" y="37" textAnchor="middle">offline</text>
      <rect className="lane" x="670" y="6" width="170" height="38" />
      <text className="lh" x="755" y="22" textAnchor="middle">GATEWAY</text>
      <text className="ls" x="755" y="37" textAnchor="middle">a peer online</text>
      <rect className="lane" x="880" y="6" width="170" height="38" />
      <text className="lh" x="965" y="22" textAnchor="middle">NEAR</text>
      <text className="ls" x="965" y="37" textAnchor="middle">Intents verifier</text>
      <line className="ln m" x1="130" y1="86" x2="540" y2="86" strokeDasharray="5 4" markerEnd="url(#ah-m)" markerStart="url(#ah-m)" />
      <rect className="lb" x="221" y="68" width="228" height="15" />
      <text className="t" x="335" y="80" textAnchor="middle">find each other over Bluetooth</text>
      <rect className="nb" x="14.2" y="120" width="221.6" height="42" />
      <text className="t hd" x="125" y="138" textAnchor="middle">"sell 0.1 wNEAR above $3.70"</text>
      <text className="t" x="125" y="154" textAnchor="middle">AI drafts · you confirm</text>
      <rect className="nb" x="452.2" y="120" width="185.6" height="42" />
      <text className="t hd" x="545" y="138" textAnchor="middle">"buy, under $4"</text>
      <text className="t" x="545" y="154" textAnchor="middle">AI drafts · you confirm</text>
      <line className="ln m" x1="125" y1="188" x2="540" y2="188" markerEnd="url(#ah-m)" />
      <rect className="lb" x="239" y="170" width="192" height="15" />
      <text className="t" x="335" y="182" textAnchor="middle">SELL 0.1 wNEAR · no price</text>
      <line className="ln m" x1="545" y1="212" x2="130" y2="212" markerEnd="url(#ah-m)" />
      <rect className="lb" x="242.6" y="194" width="184.8" height="15" />
      <text className="t" x="335" y="206" textAnchor="middle">BUY 0.1 wNEAR · no price</text>
      <rect className="nb p" x="39.4" y="246" width="171.2" height="26" />
      <text className="t p" x="125" y="264" textAnchor="middle">floor $3.70 · private</text>
      <rect className="nb p" x="452.2" y="246" width="185.6" height="26" />
      <text className="t p" x="545" y="264" textAnchor="middle">ceiling $4.00 · private</text>
      <line className="ln m" x1="125" y1="296" x2="540" y2="296" markerEnd="url(#ah-m)" />
      <rect className="lb" x="293" y="278" width="84" height="15" />
      <text className="t" x="335" y="290" textAnchor="middle">hold $3.70</text>
      <line className="ln m" x1="545" y1="318" x2="130" y2="318" markerEnd="url(#ah-m)" />
      <rect className="lb" x="296.6" y="300" width="76.8" height="15" />
      <text className="t" x="335" y="312" textAnchor="middle">bid $3.40</text>
      <line className="ln m" x1="125" y1="340" x2="540" y2="340" markerEnd="url(#ah-m)" />
      <rect className="lb" x="293" y="322" width="84" height="15" />
      <text className="t" x="335" y="334" textAnchor="middle">hold $3.70</text>
      <line className="ln m" x1="545" y1="362" x2="130" y2="362" markerEnd="url(#ah-m)" />
      <rect className="lb" x="296.6" y="344" width="76.8" height="15" />
      <text className="t" x="335" y="356" textAnchor="middle">bid $3.55</text>
      <line className="ln m" x1="125" y1="384" x2="540" y2="384" markerEnd="url(#ah-m)" />
      <rect className="lb" x="293" y="366" width="84" height="15" />
      <text className="t" x="335" y="378" textAnchor="middle">hold $3.70</text>
      <line className="ln g" x1="545" y1="406" x2="130" y2="406" markerEnd="url(#ah-g)" />
      <rect className="lb" x="285.8" y="388" width="98.4" height="15" />
      <text className="t g" x="335" y="400" textAnchor="middle">accept $3.70</text>
      <rect className="nb" x="32.2" y="440" width="185.6" height="42" />
      <text className="t hd" x="125" y="458" textAnchor="middle">sign offline · fee 0.01</text>
      <text className="t" x="125" y="474" textAnchor="middle">wNEAR -0.1  USDC +0.37</text>
      <rect className="nb" x="452.2" y="440" width="185.6" height="42" />
      <text className="t hd" x="545" y="458" textAnchor="middle">sign offline · fee 0.01</text>
      <text className="t" x="545" y="474" textAnchor="middle">wNEAR +0.1  USDC -0.37</text>
      <line className="ln m" x1="125" y1="524" x2="330" y2="524" markerEnd="url(#ah-m)" />
      <rect className="lb" x="188" y="506" width="84" height="15" />
      <text className="t" x="230" y="518" textAnchor="middle">SignedDiff</text>
      <line className="ln m" x1="545" y1="524" x2="340" y2="524" markerEnd="url(#ah-m)" />
      <rect className="lb" x="398" y="506" width="84" height="15" />
      <text className="t" x="440" y="518" textAnchor="middle">SignedDiff</text>
      <rect className="nb" x="647.8" y="528" width="214.4" height="42" />
      <text className="t hd" x="755" y="546" textAnchor="middle">submits for offline traders</text>
      <text className="t" x="755" y="562" textAnchor="middle">pays gas, earns the fees</text>
      <line className="ln m" x1="335" y1="552" x2="642.8" y2="552" markerEnd="url(#ah-m)" />
      <rect className="lb" x="442.2" y="534" width="98.4" height="15" />
      <text className="t" x="491.4" y="546" textAnchor="middle">matched pair</text>
      <line className="ln a" x1="755" y1="594" x2="960" y2="594" markerEnd="url(#ah-a)" />
      <rect className="lb" x="782" y="576" width="156" height="15" />
      <text className="t a" x="860" y="588" textAnchor="middle">rewards.submit(pair)</text>
      <rect className="nb g" x="854.2" y="626" width="221.6" height="42" />
      <text className="t g" x="965" y="644" textAnchor="middle">execute_intents: all or none</text>
      <text className="t g" x="965" y="660" textAnchor="middle">gateway paid 0.02 USDC</text>
      <line className="ln g" x1="854.2" y1="648" x2="760" y2="648" markerEnd="url(#ah-g)" />
      <rect className="lb" x="773.4" y="630" width="62.4" height="15" />
      <text className="t g" x="804.6" y="642" textAnchor="middle">tx hash</text>
      <line className="ln g" x1="755" y1="688" x2="340" y2="688" markerEnd="url(#ah-g)" />
      <rect className="lb" x="474.2" y="670" width="141.6" height="15" />
      <text className="t g" x="545" y="682" textAnchor="middle">SettlementComplete</text>
      <line className="ln g" x1="335" y1="712" x2="130" y2="712" markerEnd="url(#ah-g)" />
      <rect className="lb" x="206" y="694" width="48" height="15" />
      <text className="t g" x="230" y="706" textAnchor="middle">PROOF</text>
      <line className="ln g" x1="335" y1="712" x2="540" y2="712" markerEnd="url(#ah-g)" />
      <rect className="lb" x="416" y="694" width="48" height="15" />
      <text className="t g" x="440" y="706" textAnchor="middle">PROOF</text>
    </svg>
  );
}
