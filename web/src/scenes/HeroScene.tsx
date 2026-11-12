import { useEffect, useRef, useState } from 'react';

/**
 * The hero scene in real 3D, drawn as SVG: two offline iPhones in front, the gateway router behind them and the
 * NEAR block floating at the back. Everything is placed in world space (x runs down-right, y down-left, z up)
 * and projected by an orthographic camera that sways and follows the mouse; flat content (a screen, a label on
 * a face) is laid on its plane with an affine matrix. One clock replays the trade every 12 seconds: the agents
 * bid over Bluetooth, both phones sign, the signed halves fly to the gateway, the pair goes to NEAR, and the
 * proof comes back. Without motion (the prerender, reduced motion) the scene rests on the settled frame.
 */
type V = [number, number, number];
type Pt = [number, number];
type Tone = 'w' | 'g' | 'c';

const K = 1.2247; // isometric scale: a world unit along z is one pixel at the resting pitch
const RAD = Math.PI / 180;
const PITCH0 = Math.asin(1 / Math.sqrt(3)) / RAD;
/** The camera turns about the phones' feet, which stay at this point of the 480x620 canvas. */
const PIVOT: V = [28, 28, 0];
const PIVOT_AT: Pt = [240, 546];
const LOOP = 12;

const r1 = (n: number) => Math.round(n * 10) / 10;
const r2 = (n: number) => Math.round(n * 100) / 100;
const r4 = (n: number) => Math.round(n * 1e4) / 1e4;
const clamp = (n: number) => Math.max(0, Math.min(1, n));
/** Progress of t through [a, b], clamped to 0..1. */
const seg = (t: number, a: number, b: number) => clamp((t - a) / (b - a));
const ease = (s: number) => s * s * (3 - 2 * s);
const mix = (a: V, b: V, s: number): V => [a[0] + (b[0] - a[0]) * s, a[1] + (b[1] - a[1]) * s, a[2] + (b[2] - a[2]) * s];
const pts = (...p: Pt[]) => p.map((q) => q.join(',')).join(' ');

function camera(yaw: number, pitch: number) {
  const th = (45 + yaw) * RAD;
  const ph = (PITCH0 + pitch) * RAD;
  const ct = K * Math.cos(th);
  const st = K * Math.sin(th);
  const sp = Math.sin(ph);
  const kz = K * Math.cos(ph);
  const P = ([x, y, z]: V): Pt => {
    const dx = x - PIVOT[0];
    const dy = y - PIVOT[1];
    return [r1(PIVOT_AT[0] + dx * ct - dy * st), r1(PIVOT_AT[1] + (dx * st + dy * ct) * sp - z * kz)];
  };
  const m = (a: number, b: number, c: number, d: number, o: Pt) => `matrix(${r4(a)} ${r4(b)} ${r4(c)} ${r4(d)} ${o[0]} ${o[1]})`;
  return {
    P,
    /** A wall whose normal is +x: u runs toward -y, v runs down; origin = its top-left corner. */
    faceX: (o: V) => m(st, -ct * sp, 0, kz, P(o)),
    /** A wall whose normal is +y: u runs toward +x, v runs down; origin = its top-left corner. */
    faceY: (o: V) => m(ct, st * sp, 0, kz, P(o)),
    /** A horizontal plane: u runs toward +x, v toward +y. */
    flat: (o: V) => m(ct, st * sp, -st, ct * sp, P(o)),
    line: (a: V, b: V) => `M${P(a).join(' ')}L${P(b).join(' ')}`,
  };
}
type Cam = ReturnType<typeof camera>;

/** An axis-aligned box drawn as its three visible faces; glow tints the top face. */
function Box({ c, at, w, d, h, glow }: { c: Cam; at: V; w: number; d: number; h: number; glow?: [Tone, number] }) {
  const [x, y, z] = at;
  const [x1, y1, z1] = [x + w, y + d, z + h];
  const top = pts(c.P([x, y1, z1]), c.P([x, y, z1]), c.P([x1, y, z1]), c.P([x1, y1, z1]));
  return (
    <>
      <polygon className="f-top" points={top} />
      <polygon className="f-left" points={pts(c.P([x, y1, z1]), c.P([x1, y1, z1]), c.P([x1, y1, z]), c.P([x, y1, z]))} />
      <polygon className="f-right" points={pts(c.P([x1, y1, z1]), c.P([x1, y, z1]), c.P([x1, y, z]), c.P([x1, y1, z]))} />
      {glow && glow[1] > 0 && <polygon className={`fx ${glow[0]}`} points={top} opacity={r2(glow[1])} />}
    </>
  );
}

/** A message in flight: a small solid cube with a fading tail, s = 0..1 along the link. */
function Packet({ c, from, to, s, tone, size = 9 }: { c: Cam; from: V; to: V; s: number; tone: Tone; size?: number }) {
  if (s <= 0 || s >= 1) return null;
  const cubes = [];
  for (let i = 3; i >= 0; i--) {
    const si = s - i * 0.045;
    if (si <= 0) continue;
    const k = size * Math.min(1, si / 0.1, (1 - si) / 0.1) * (1 - i * 0.2);
    const p = mix(from, to, ease(si));
    cubes.push(
      <g key={i} opacity={r2(1 - i * 0.26)}>
        <Box c={c} at={[p[0] - k / 2, p[1] - k / 2, p[2] - k / 2]} w={k} d={k} h={k} />
      </g>,
    );
  }
  return <g className={`pk ${tone}`}>{cubes}</g>;
}

const PW = 104;
const PH = 212;
const DEPTH = [9, 8, 7, 6, 5, 4, 3, 2, 1];
const STATE = ['BIDDING', '✓ SIGNED', '✓ SETTLED'];

/** One iPhone standing upright: a stack of rounded slabs for the body, then the screen on its front face. */
function Phone({ c, side, role, rows, state, flash }: { c: Cam; side: 'a' | 'b'; role: string; rows: [string, string, string][]; state: 0 | 1 | 2; flash: number }) {
  const face = (k: number) => (side === 'a' ? c.faceX([-k, 56 + PW, PH]) : c.faceY([56, -k, PH]));
  return (
    <g>
      {DEPTH.map((k) => (
        <rect key={k} className="ph-side" width={PW} height={PH} rx="15" transform={face(k)} />
      ))}
      <g transform={face(0)}>
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
        {state === 2 && <rect className="fx g" x="14" y="180" width="76" height="20" />}
        <rect className={state === 0 ? 'idle' : 'ln g'} x="14" y="180" width="76" height="20" />
        <text className={`t p ${['m', 'g', 'k'][state]}`} x="52" y="194" textAnchor="middle">{STATE[state]}</text>
        {flash > 0 && <rect className="fx g" x="5" y="5" width={PW - 10} height={PH - 10} rx="11" opacity={r2(flash * 0.34)} />}
      </g>
    </g>
  );
}

/** The agents' bids, in the order the app's chat tells them: [starts at, price, whose turn, which phone speaks]. */
const BIDS: [number, string, string, 'a' | 'b'][] = [
  [0, '$4.15', 'ASK', 'a'],
  [0.8, '$3.70', 'BID', 'b'],
  [1.6, '$3.88', 'ASK', 'a'],
  [2.4, '$3.78', 'BID', 'b'],
];
const AGREE = 3.2;
const SIGN_A = 3.5;
const SIGN_B = 3.75;
const DIFF: Pt = [4.2, 5.4];
const PAIR: Pt = [5.8, 7];
const SETTLE: Pt = [7, 7.8];
const HASH: Pt = [7.8, 8.8];
const PROOF: Pt = [8.8, 9.8];
const DONE = 9.8;

/* the links, in world space: phone tops to the router's sides, the router's back corner to NEAR's pad */
const A_UP: [V, V] = [[-4.5, 108, 226], [-240, -172, 12]];
const A_DOWN: [V, V] = [[-228, -172, 12], [7.5, 108, 226]];
const B_UP: [V, V] = [[108, -4.5, 226], [-172, -240, 12]];
const B_DOWN: [V, V] = [[-172, -228, 12], [108, 7.5, 226]];
const N_UP: [V, V] = [[-250.6, -241.4, 18], [-362.6, -353.4, 0]];
const N_DOWN: [V, V] = [[-353.4, -362.6, 0], [-241.4, -250.6, 18]];
/* the middle of each phone's top edge: its two labels hang off it, clear of the links at any angle */
const TOP_A: V = [0, 108, 212];
const TOP_B: V = [108, 0, 212];
const NEAR = -466;

type Frame = { t: number; yaw: number; pitch: number; live: boolean };
const REST: Frame = { t: 11, yaw: 0, pitch: 0, live: false };

export default function HeroScene() {
  const ref = useRef<SVGSVGElement>(null);
  const [frame, setFrame] = useState(REST);

  useEffect(() => {
    const svg = ref.current;
    if (!svg) return;
    const host = svg.parentElement;
    const motion = window.matchMedia('(prefers-reduced-motion: no-preference)');
    let raf = 0;
    let start = 0;
    let seen = true;
    let [px, py, ex, ey] = [0, 0, 0, 0];
    const tick = (now: number) => {
      if (!start) start = now;
      const s = (now - start) / 1000;
      ex += (px - ex) * 0.05;
      ey += (py - ey) * 0.05;
      const yaw = 5 * Math.sin((s * 2 * Math.PI) / 20) + 4 * ex;
      setFrame({ t: s % LOOP, yaw, pitch: 1.5 * ey, live: true });
      host?.style.setProperty('--hs-yaw', `${yaw.toFixed(2)}deg`);
      raf = requestAnimationFrame(tick);
    };
    const run = () => {
      cancelAnimationFrame(raf);
      if (motion.matches && seen) {
        raf = requestAnimationFrame(tick);
      } else if (!motion.matches) {
        setFrame(REST);
        host?.style.removeProperty('--hs-yaw');
      }
    };
    const onMove = (e: PointerEvent) => {
      if (e.pointerType !== 'mouse') return;
      px = (e.clientX / window.innerWidth) * 2 - 1;
      py = (e.clientY / window.innerHeight) * 2 - 1;
    };
    const io = new IntersectionObserver(([entry]) => {
      seen = entry.isIntersecting;
      run();
    });
    io.observe(svg);
    motion.addEventListener('change', run);
    window.addEventListener('pointermove', onMove, { passive: true });
    run();
    return () => {
      cancelAnimationFrame(raf);
      io.disconnect();
      motion.removeEventListener('change', run);
      window.removeEventListener('pointermove', onMove);
      host?.style.removeProperty('--hs-yaw');
    };
  }, []);

  const { t, yaw, pitch, live } = frame;
  const c = camera(yaw, pitch);
  const at = (p: V, dx = 0, dy = 0) => {
    const q = c.P(p);
    return { x: r1(q[0] + dx), y: r1(q[1] + dy) };
  };

  /* which step is on: its labels are lit, the rest are dimmed; in the last stretch everything is lit */
  const hold = live ? Math.min(seg(t, DONE, DONE + 0.5), 1 - seg(t, LOOP - 0.5, LOOP)) : 1;
  const lit = (a: number, b: number) => r2(0.5 + 0.5 * Math.max(hold, Math.min(seg(t, a - 0.2, a), 1 - seg(t, b, b + 0.4))));

  const agreed = t >= AGREE;
  const bid = BIDS.reduce((cur, b) => (t >= b[0] ? b : cur), BIDS[0]);
  const popAt = agreed ? AGREE : bid[0];
  const pop = live ? r2(1 + 0.24 * (1 - seg(t, popAt, popAt + 0.25))) : 1;
  const wave = live && !agreed ? seg(t, bid[0], bid[0] + 0.8) : -1;
  const waveAt = bid[3] === 'a' ? [0, 108] : [108, 0];

  const state = (signed: number): 0 | 1 | 2 => (t < signed ? 0 : t < DONE ? 1 : 2);
  const flash = (signed: number) => (!live || t < signed ? 0 : t < DONE ? 1 - seg(t, signed, signed + 0.5) : 1 - seg(t, DONE, DONE + 0.6));

  const settle = live && t >= SETTLE[0] ? seg(t, SETTLE[0], SETTLE[1]) : -1;
  const nearZ = live ? 6 + 3 * Math.sin((t * 2 * Math.PI) / 4) + (settle >= 0 ? 9 * Math.sin(Math.PI * settle) : 0) : 6;
  const nearTop: V = [NEAR, NEAR, nearZ + 30];
  const routerGlow: [Tone, number] = !live
    ? ['c', 0]
    : t >= PROOF[0] - 0.1
      ? ['g', 0.5 * (1 - seg(t, PROOF[0] - 0.1, PROOF[0] + 0.5))]
      : t >= DIFF[1] - 0.1 && t < HASH[0]
        ? ['c', 0.5 * (1 - seg(t, DIFF[1] - 0.1, PAIR[0] + 0.3))]
        : ['c', 0];
  const reward = live ? r2(1 + 0.3 * Math.sin(Math.PI * seg(t, PROOF[0], PROOF[0] + 0.5))) : 1;

  const plate = c.P([28, 28, 162]);
  const gas = c.P([-172, -248, 18]);
  const netY = c.P([-290, -290, 6])[1];

  return (
    <svg ref={ref} className="sc hero-scene" viewBox="0 0 480 620" preserveAspectRatio="xMidYMid meet" role="img" aria-label="Two offline iPhones, a seller and a buyer, agree 0.1 wNEAR at $3.78 over Bluetooth and each signs its half locally as a NEP-413 diff. The gateway, a router with internet behind them, receives both signed diffs over the mesh, submits the pair to NEAR Intents, pays the gas, earns 0.02 USDC and sends the proof back to both phones">
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

      {/* NEAR: a block floating over its pad, on the internet side of the line */}
      <polygon className="pad" points={pts(c.P([NEAR, NEAR + 108, 0]), c.P([NEAR, NEAR, 0]), c.P([NEAR + 108, NEAR, 0]), c.P([NEAR + 108, NEAR + 108, 0]))} />
      <Box c={c} at={[NEAR, NEAR, nearZ]} w={108} d={108} h={30} glow={['g', settle >= 0 ? 0.6 * (1 - settle) : 0]} />
      <g transform={c.flat(nearTop)}>
        {settle >= 0 && settle < 1 && <circle className="ring g" cx="54" cy="54" r={r1(8 + 44 * settle)} opacity={r2(1 - settle)} />}
        <text className="h xl" x="54" y="62" textAnchor="middle">NEAR</text>
      </g>

      <path className="hair" strokeDasharray="4 4" d={`M12 ${netY}H468`} />
      <text className="l m" x="12" y={r1(netY - 8)}>INTERNET</text>
      <text className="l m" x="12" y={r1(netY + 18)}>NO INTERNET</text>

      {/* gateway to NEAR and back */}
      <path className="ln c flow" d={c.line(...N_UP)} markerEnd="url(#hs-c)" />
      <path className="ln g flow" d={c.line(...N_DOWN)} markerEnd="url(#hs-g)" />
      <text className="l c" {...at(mix(N_UP[0], N_UP[1], 0.62), -10, -8)} textAnchor="end" opacity={lit(...PAIR)}>SUBMIT PAIR</text>
      <text className="l g" {...at(mix(N_DOWN[0], N_DOWN[1], 0.38), 10, -8)} opacity={lit(...HASH)}>TX HASH</text>

      {/* the gateway: a router with two antennas */}
      <path className="ant" d={`${c.line([-240, -180, 18], [-240, -180, 64])}${c.line([-180, -240, 18], [-180, -240, 64])}`} />
      <Box c={c} at={[-248, -248, 0]} w={76} d={76} h={18} glow={routerGlow} />
      <text className="h xs" x="38" y="42" textAnchor="middle" transform={c.flat([-248, -248, 18])}>GATEWAY</text>
      <g transform={c.faceY([-248, -172, 18])}>
        <rect className="led" x="10" y="7" width="5" height="4" />
        <rect className="led led--2" x="20" y="7" width="5" height="4" />
        <rect className="led led--3" x="30" y="7" width="5" height="4" />
      </g>
      <text className="t p g" x="38" y="13" textAnchor="middle" transform={c.faceX([-172, -172, 18])}>ONLINE</text>
      <g transform={`translate(${r1(gas[0] + 16)} ${r1(gas[1] - 4)})`} opacity={lit(...PROOF)}>
        <text className="t xs m" y="-16">pays gas</text>
        <text className="t xs g" transform={`scale(${reward})`}>+0.02 USDC</text>
      </g>

      {/* each phone hands its signed half to the gateway and gets the proof back */}
      <path className="ln flow" d={c.line(...A_UP)} markerEnd="url(#hs-w)" />
      <path className="ln g flow" d={c.line(...A_DOWN)} markerEnd="url(#hs-g)" />
      <text className="t xs w" {...at(TOP_A, -20, -28)} textAnchor="end" opacity={lit(...DIFF)}>SignedDiff A</text>
      <text className="l g" {...at(TOP_A, -20, -12)} textAnchor="end" opacity={lit(...PROOF)}>PROOF</text>
      <path className="ln flow" d={c.line(...B_UP)} markerEnd="url(#hs-w)" />
      <path className="ln g flow" d={c.line(...B_DOWN)} markerEnd="url(#hs-g)" />
      <text className="t xs w" {...at(TOP_B, 20, -28)} opacity={lit(...DIFF)}>SignedDiff B</text>
      <text className="l g" {...at(TOP_B, 20, -12)} opacity={lit(...PROOF)}>PROOF</text>

      {/* the bidding phone's Bluetooth wave, spreading over the floor */}
      {wave >= 0 && (
        <g transform={c.flat([0, 0, 0])}>
          <circle className="ring" cx={waveAt[0]} cy={waveAt[1]} r={r1(16 + 64 * wave)} opacity={r2(0.8 * (1 - wave))} />
          {wave > 0.3 && <circle className="ring" cx={waveAt[0]} cy={waveAt[1]} r={r1(16 + 64 * (wave - 0.3))} opacity={r2(0.8 * (1.3 - wave))} />}
        </g>
      )}

      <Phone c={c} side="a" role="SELLER" state={state(SIGN_A)} flash={flash(SIGN_A)} rows={[['wNEAR', '−0.1', 'w'], ['USDC', '+0.378', 'w'], ['fee', '0.01', 'm']]} />
      <Phone c={c} side="b" role="BUYER" state={state(SIGN_B)} flash={flash(SIGN_B)} rows={[['wNEAR', '+0.1', 'w'], ['USDC', '−0.378', 'w'], ['fee', '0.01', 'm']]} />

      {/* between the phones: the Bluetooth link and the price the agents close in on */}
      <g transform={`translate(${plate[0]} ${plate[1]})`}>
        <rect className="plate" x="-43" width="86" height="150" />
        <text className="h xs" y="20" textAnchor="middle">BLUETOOTH</text>
        <path className="ln flow" d="M-36 34H36" markerEnd="url(#hs-w)" opacity={live && !agreed && bid[3] === 'b' ? 0.3 : 1} />
        <path className="ln flow" d="M36 46H-36" markerEnd="url(#hs-w)" opacity={live && !agreed && bid[3] === 'a' ? 0.3 : 1} />
        <text className={`h lg ${agreed ? 'g' : 'w'}`} textAnchor="middle" transform={`translate(0 82) scale(${pop})`}>{agreed ? '$3.78' : bid[1]}</text>
        <text className={`l ${agreed ? 'g' : 'm'}`} y="100" textAnchor="middle">{agreed ? 'AGREED' : bid[2]}</text>
        <g opacity={lit(AGREE, DIFF[0])}>
          <text className="t xs m" y="124" textAnchor="middle">each signs</text>
          <text className="t xs m" y="138" textAnchor="middle">its own half</text>
        </g>
      </g>

      {/* the messages in flight */}
      {live && (
        <>
          <Packet c={c} from={A_UP[0]} to={A_UP[1]} s={seg(t, ...DIFF)} tone="w" />
          <Packet c={c} from={B_UP[0]} to={B_UP[1]} s={seg(t, ...DIFF)} tone="w" />
          <Packet c={c} from={[N_UP[0][0] - 6, N_UP[0][1] + 6, 18]} to={[N_UP[1][0] - 6, N_UP[1][1] + 6, 6]} s={seg(t, ...PAIR)} tone="c" />
          <Packet c={c} from={[N_UP[0][0] + 6, N_UP[0][1] - 6, 18]} to={[N_UP[1][0] + 6, N_UP[1][1] - 6, 6]} s={seg(t, ...PAIR)} tone="c" />
          <Packet c={c} from={N_DOWN[0]} to={N_DOWN[1]} s={seg(t, ...HASH)} tone="g" size={11} />
          <Packet c={c} from={A_DOWN[0]} to={A_DOWN[1]} s={seg(t, ...PROOF)} tone="g" />
          <Packet c={c} from={B_DOWN[0]} to={B_DOWN[1]} s={seg(t, ...PROOF)} tone="g" />
        </>
      )}
    </svg>
  );
}
