/** Why NEAR Intents, reason 1: of three peers on the mesh, the one that is online submits the pair. */
export function WhySceneSubmit() {
  return (
    <svg className="sc why-scene" viewBox="0 0 360 196" role="img" aria-label="Three peers linked over Bluetooth. Two are offline and cannot cross to the internet; the one that is online submits the pair to execute_intents on NEAR Intents">
      <rect className="box" x="100" y="2" width="160" height="46" />
      <text className="h" x="180" y="21" textAnchor="middle">NEAR INTENTS</text>
      <text className="t w" x="180" y="39" textAnchor="middle">execute_intents</text>
      <path className="hair" strokeDasharray="4 4" d="M0 66H360" />
      <text className="l m" x="0" y="59">INTERNET</text>
      <path className="ln c" d="M180 92V56" />
      <path className="hd c" d="M180 48l-4.5 8h9z" />
      <text className="l c" x="190" y="84">SUBMIT</text>
      <path className="ln" d="M40 92V73M33 73H47" />
      <path className="ln" d="M320 92V73M313 73H327" />
      <rect className="ph" x="4" y="92" width="72" height="100" rx="11" />
      <rect className="ph-slot" x="30" y="99" width="20" height="3" />
      <rect className="scr" x="9" y="108" width="62" height="72" />
      <text className="l m" x="40" y="141" textAnchor="middle">PEER</text>
      <text className="l m" x="40" y="159" textAnchor="middle">OFFLINE</text>
      <rect className="ph" x="144" y="92" width="72" height="100" rx="11" />
      <rect className="ph-slot" x="170" y="99" width="20" height="3" />
      <rect className="scr" x="149" y="108" width="62" height="72" />
      <text className="l w" x="180" y="141" textAnchor="middle">PEER</text>
      <text className="l g" x="180" y="159" textAnchor="middle">ONLINE</text>
      <rect className="ph" x="284" y="92" width="72" height="100" rx="11" />
      <rect className="ph-slot" x="310" y="99" width="20" height="3" />
      <rect className="scr" x="289" y="108" width="62" height="72" />
      <text className="l m" x="320" y="141" textAnchor="middle">PEER</text>
      <text className="l m" x="320" y="159" textAnchor="middle">OFFLINE</text>
      <path className="ln" strokeDasharray="4 4" d="M80 144H132" />
      <path className="hd" d="M140 144l-8 -4.5v9z" />
      <text className="l m" x="108" y="135" textAnchor="middle">BLE</text>
      <path className="ln" strokeDasharray="4 4" d="M280 144H228" />
      <path className="hd" d="M220 144l8 -4.5v9z" />
      <text className="l m" x="252" y="135" textAnchor="middle">BLE</text>
    </svg>
  );
}

/** Why NEAR Intents, reason 2: the two legs of the trade apply together, or neither does. */
export function WhySceneSettle() {
  return (
    <svg className="sc why-scene" viewBox="0 0 360 196" role="img" aria-label="The seller gives 0.1 wNEAR and the buyer gives 0.37 USDC. NEAR Intents applies the two legs as one pair that nets to zero: both legs or none">
      <rect className="box" x="100" y="2" width="160" height="46" />
      <text className="h" x="180" y="21" textAnchor="middle">NEAR INTENTS</text>
      <text className="t g" x="180" y="39" textAnchor="middle">both legs or none</text>
      <path className="ln g" d="M180 96V56" />
      <path className="hd g" d="M180 48l-4.5 8h9z" />
      <rect className="ln g" strokeDasharray="4 4" x="118" y="96" width="124" height="92" />
      <rect className="ph" x="4" y="92" width="72" height="100" rx="11" />
      <rect className="ph-slot" x="30" y="99" width="20" height="3" />
      <rect className="scr" x="9" y="108" width="62" height="72" />
      <text className="l w" x="40" y="150" textAnchor="middle">SELLER</text>
      <rect className="ph" x="284" y="92" width="72" height="100" rx="11" />
      <rect className="ph-slot" x="310" y="99" width="20" height="3" />
      <rect className="scr" x="289" y="108" width="62" height="72" />
      <text className="l w" x="320" y="150" textAnchor="middle">BUYER</text>
      <text className="t w" x="180" y="116" textAnchor="middle">0.1 wNEAR</text>
      <path className="ln c" d="M80 125H268" />
      <path className="hd c" d="M276 125l-8 -4.5v9z" />
      <text className="h g" x="180" y="147" textAnchor="middle">NET 0</text>
      <path className="ln c" d="M280 160H92" />
      <path className="hd c" d="M84 160l8 -4.5v9z" />
      <text className="t w" x="180" y="179" textAnchor="middle">0.37 USDC</text>
    </svg>
  );
}
