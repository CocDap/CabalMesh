/**
 * The hero scene in isometric 3D: two offline iPhones in front, the gateway router behind them and the
 * NEAR Intents block at the back. World axes: x runs down-right, y down-left, z up; P() projects a world
 * point onto the 480x620 canvas, and the matrix helpers lay flat content (a screen, a label) on a face.
 */
const C = 0.866;
const S = 0.5;
const OX = 240;
const OY = 518;

const r = (n: number) => Math.round(n * 10) / 10;
const P = (x: number, y: number, z = 0): [number, number] => [r(OX + (x - y) * C), r(OY + (x + y) * S - z)];
const pts = (...p: [number, number][]) => p.map((q) => q.join(',')).join(' ');
/** A face whose normal is +x (it rises to the right); origin = its top-left corner. */
const faceRise = (o: [number, number]) => `matrix(${C} ${-S} 0 1 ${o[0]} ${o[1]})`;
/** A face whose normal is +y (it falls to the right); origin = its top-left corner. */
const faceFall = (o: [number, number]) => `matrix(${C} ${S} 0 1 ${o[0]} ${o[1]})`;
/** A top face; origin = its back corner, u runs down-right and v down-left. */
const faceTop = (o: [number, number]) => `matrix(${C} ${S} ${-C} ${S} ${o[0]} ${o[1]})`;

/** An axis-aligned box drawn as its three visible faces. */
function Box({ x, y, w, d, h }: { x: number; y: number; w: number; d: number; h: number }) {
  const [x1, y1] = [x + w, y + d];
  return (
    <>
      <polygon className="f-top" points={pts(P(x, y1, h), P(x, y, h), P(x1, y, h), P(x1, y1, h))} />
      <polygon className="f-left" points={pts(P(x, y1, h), P(x1, y1, h), P(x1, y1), P(x, y1))} />
      <polygon className="f-right" points={pts(P(x1, y1, h), P(x1, y, h), P(x1, y), P(x1, y1))} />
    </>
  );
}

const PW = 104;
const PH = 212;
const DEPTH = [9, 8, 7, 6, 5, 4, 3, 2, 1];

/** One iPhone standing upright: a stack of rounded slabs for the body, then the screen on its front face. */
function Phone({ side, role, rows }: { side: 'a' | 'b'; role: string; rows: [string, string, string][] }) {
  const face = side === 'a' ? faceRise : faceFall;
  const at = (k: number) => (side === 'a' ? P(-k, 56 + PW, PH) : P(56, -k, PH));
  return (
    <g>
      {DEPTH.map((k) => (
        <rect key={k} className="ph-side" width={PW} height={PH} rx="15" transform={face(at(k))} />
      ))}
      <g transform={face(at(0))}>
        <rect className="ph" width={PW} height={PH} rx="15" />
        <rect className="scr" x="5" y="5" width={PW - 10} height={PH - 10} rx="11" />
        <rect className="ph-slot" x="38" y="11" width="28" height="8" rx="4" />
        <text className="t p m" x="14" y="40">OFFLINE</text>
        <text className="t p c" x="90" y="40" textAnchor="end">BLE</text>
        <path className="hair" d="M5 48H99" />
        <text className="h" x="14" y="73">{role}</text>
        {rows.map(([k, v, tone], i) => (
          <g key={k}>
            <text className="t p m" x="14" y={99 + i * 20}>{k}</text>
            <text className={`t p ${tone}`} x="90" y={99 + i * 20} textAnchor="end">{v}</text>
          </g>
        ))}
        <path className="hair" d="M14 153H90" />
        <text className="t p m" x="14" y="171">NEP-413</text>
        <rect className="ln g" x="14" y="180" width="76" height="20" />
        <text className="t p g" x="52" y="194" textAnchor="middle">✓ SIGNED</text>
      </g>
    </g>
  );
}

export default function HeroScene() {
  return (
    <svg className="sc hero-scene" viewBox="0 0 480 620" preserveAspectRatio="xMidYMid meet" role="img" aria-label="Two offline iPhones, a seller and a buyer, agree 0.1 wNEAR at $3.78 over Bluetooth and each signs its half locally as a NEP-413 diff. The gateway, a router with internet behind them, receives both signed diffs over the mesh, submits the pair to NEAR Intents, pays the gas, earns 0.02 USDC and sends the proof back to both phones">
      <defs>
        <marker id="hs-w" viewBox="0 0 8 8" refX="7" refY="4" markerWidth="8" markerHeight="8" markerUnits="userSpaceOnUse" orient="auto">
          <path className="hd" d="M0 .5 8 4 0 7.5z" />
        </marker>
        <marker id="hs-g" viewBox="0 0 8 8" refX="7" refY="4" markerWidth="8" markerHeight="8" markerUnits="userSpaceOnUse" orient="auto">
          <path className="hd g" d="M0 .5 8 4 0 7.5z" />
        </marker>
        <marker id="hs-c" viewBox="0 0 8 8" refX="7" refY="4" markerWidth="8" markerHeight="8" markerUnits="userSpaceOnUse" orient="auto">
          <path className="hd c" d="M0 .5 8 4 0 7.5z" />
        </marker>
      </defs>

      {/* NEAR Intents: the block at the back, on the internet side of the line */}
      <Box x={-476} y={-476} w={108} d={108} h={30} />
      <text className="h xl" x="54" y="62" textAnchor="middle" transform={faceTop(P(-476, -476, 30))}>NEAR</text>

      <path className="hair" strokeDasharray="4 4" d="M12 222H468" />
      <text className="l m" x="12" y="214">INTERNET</text>
      <text className="l m" x="12" y="240">NO INTERNET</text>

      {/* gateway to NEAR and back */}
      <path className="ln c flow" d="M232 254V154" markerEnd="url(#hs-c)" />
      <path className="ln g flow" d="M248 152V255" markerEnd="url(#hs-g)" />
      <text className="l c" x="222" y="196" textAnchor="end">SUBMIT PAIR</text>
      <text className="l g" x="258" y="196">TX HASH</text>

      {/* the gateway: a router with two antennas */}
      <path className="ant" d={`M${P(-240, -180, 18).join(' ')}V244M${P(-180, -240, 18).join(' ')}V244`} />
      <Box x={-248} y={-248} w={76} d={76} h={18} />
      <text className="h xs" x="38" y="42" textAnchor="middle" transform={faceTop(P(-248, -248, 18))}>GATEWAY</text>
      <g transform={faceFall(P(-248, -172, 18))}>
        <rect className="led" x="10" y="7" width="5" height="4" />
        <rect className="led led--2" x="20" y="7" width="5" height="4" />
        <rect className="led led--3" x="30" y="7" width="5" height="4" />
      </g>
      <text className="t p g" x="38" y="13" textAnchor="middle" transform={faceRise(P(-172, -172, 18))}>ONLINE</text>
      <text className="t xs m" x="322" y="270">pays gas</text>
      <text className="t xs g" x="322" y="286">+0.02 USDC</text>

      {/* each phone hands its signed half to the gateway and gets the proof back */}
      <path className="ln flow" d="M143 356L175 302" markerEnd="url(#hs-w)" />
      <path className="ln g flow" d="M186 309L156 352" markerEnd="url(#hs-g)" />
      <text className="t xs w" x="132" y="322" textAnchor="end">SignedDiff A</text>
      <text className="l g" x="132" y="338" textAnchor="end">PROOF</text>
      <path className="ln flow" d="M337 356L305 302" markerEnd="url(#hs-w)" />
      <path className="ln g flow" d="M294 309L324 352" markerEnd="url(#hs-g)" />
      <text className="t xs w" x="348" y="322">SignedDiff B</text>
      <text className="l g" x="348" y="338">PROOF</text>

      <Phone side="a" role="SELLER" rows={[['wNEAR', '−0.1', 'w'], ['USDC', '+0.378', 'w'], ['fee', '0.01', 'm']]} />
      <Phone side="b" role="BUYER" rows={[['wNEAR', '+0.1', 'w'], ['USDC', '−0.378', 'w'], ['fee', '0.01', 'm']]} />

      {/* between the phones: the Bluetooth link and the price they agreed */}
      <rect className="plate" x="197" y="384" width="86" height="150" />
      <text className="h xs" x="240" y="404" textAnchor="middle">BLUETOOTH</text>
      <path className="ln flow" d="M204 418H276" markerEnd="url(#hs-w)" />
      <path className="ln flow" d="M276 430H204" markerEnd="url(#hs-w)" />
      <text className="h lg g" x="240" y="466" textAnchor="middle">$3.78</text>
      <text className="l g" x="240" y="484" textAnchor="middle">AGREED</text>
      <text className="t xs m" x="240" y="508" textAnchor="middle">each signs</text>
      <text className="t xs m" x="240" y="522" textAnchor="middle">its own half</text>
    </svg>
  );
}
