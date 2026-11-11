import type { CSSProperties } from 'react';
import StepSceneBid from '../scenes/StepSceneBid';
import StepSceneSign from '../scenes/StepSceneSign';
import StepSceneSettle from '../scenes/StepSceneSettle';

/** A peer-to-peer trade in 3 steps, as pinned chapters (bid, sign, settle). */
export default function HowItWorks() {
  return (
    <section className="section" id="how" style={{ "--h": "2" } as CSSProperties} data-moment="pinned-chapters" aria-labelledby="how-h">
      <div className="wrap mo-pc__grid">
        <div className="mo-pc__text">
          <h2 id="how-h">A peer-to-peer trade in 3 steps</h2>
          <ol className="mo-pc__steps">
            <li className="mo-pc__step">
              <h3>Agents bid over the Bluetooth mesh</h3>
              <p>
                Your agent and the peer's swap offers over the mesh. Code holds each one to its owner's floor or ceiling, and that limit never leaves the phone.
              </p>
              <figure className="mo-pc__visual mo-pc__visual--scene">
                <StepSceneBid />
              </figure>
            </li>
            <li className="mo-pc__step">
              <h3>Each phone signs over Bluetooth, offline</h3>
              <p>
                You confirm, and the phone signs its half as a NEP-413 token diff. A signature is a message, so Bluetooth carries it with no internet at all.
              </p>
              <figure className="mo-pc__visual mo-pc__visual--scene">
                <StepSceneSign />
              </figure>
            </li>
            <li className="mo-pc__step">
              <h3>The first peer online settles both</h3>
              <p>
                A gateway with internet submits the matched pair to execute_intents. Both legs apply or neither does, and the proof floods back over the mesh.
              </p>
              <figure className="mo-pc__visual mo-pc__visual--scene">
                <StepSceneSettle />
              </figure>
            </li>
          </ol>
        </div>
        <div className="mo-pc__visuals frame" />
      </div>
    </section>
  );
}
