import type { CSSProperties } from 'react';
import BleRibbon from './BleRibbon';

/** Roadmap rows, copied from the README checklist. Update both together. */
export default function Roadmap() {
  return (
    <section className="section" id="roadmap" style={{ "--hm": "2" } as CSSProperties} aria-labelledby="roadmap-h">
      <div className="wrap">
        <div className="section-head">
          <h2 id="roadmap-h">The CabalMesh roadmap</h2>
          <p>Everything below runs on NEAR. Two physical phones over Bluetooth are the next proof.</p>
        </div>
        <div className="grid grid--top fill">
          <ul className="rows span-6">
            <li>
              <span className="rows__name">
                Settlement on NEAR Intents
                <small>Two Android emulators, bids and signatures over BLE</small>
              </span>
              <span className="rows__value">Done</span>
            </li>
            <li>
              <span className="rows__name">
                Gateway rewards
                <small>The rewards contract pays both fees per pair</small>
              </span>
              <span className="rows__value">Done</span>
            </li>
            <li>
              <span className="rows__name">
                Bring your own model
                <small>Ollama, NEAR AI Cloud or OpenRouter</small>
              </span>
              <span className="rows__value">Done</span>
            </li>
            <li>
              <span className="rows__name">
                Two physical phones
                <small>The same trade on real hardware over BLE</small>
              </span>
              <span className="rows__value">Next</span>
            </li>
            <li>
              <span className="rows__name">
                ZK Box
                <small>Prove an offer stays inside your limit</small>
              </span>
              <span className="rows__value">Planned</span>
            </li>
            <li>
              <span className="rows__name">
                On-device AI
                <small>A 4-bit model on phones with 8 GB of RAM</small>
              </span>
              <span className="rows__value">Planned</span>
            </li>
          </ul>
          <BleRibbon className="roadmap__wave span-6 start-7" />
        </div>
      </div>
    </section>
  );
}
