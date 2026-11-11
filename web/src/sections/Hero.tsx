import HeroScene from '../scenes/HeroScene';
import BleRibbon from './BleRibbon';

/** First screen: the headline, the two calls to action and the hero scene on the BLE ribbon. */
export default function Hero() {
  return (
    <section className="hero hero--collage" data-enter="" aria-labelledby="hero-h">
      <div className="wrap">
        <div className="hero-text grid hero__grid">
          <div className="hero__text span-5">
            <h1 id="hero-h">Peer-to-peer crypto trading for offline phones</h1>
            <p className="hero__lead">
              Two phones meet over Bluetooth, their AI agents agree a price inside each owner's limit, and the first peer online settles both sides on NEAR Intents.
            </p>
            <div className="btn-row">
              <a className="btn btn--primary" href="https://github.com/CocDap/CabalMesh">
                Read the source
                <svg className="m-arrow" viewBox="0 0 12 12" aria-hidden="true">
                  <path className="m-arrow__stem" d="M1 6h8" />
                  <path className="m-arrow__head" d="M4.5 2.5 8 6l-3.5 3.5" />
                </svg>
              </a>
              <a className="btn btn--secondary" href="#how">See a trade</a>
            </div>
            <p className="hero__proof">Android and iOS app, open source. Built on NEAR.</p>
          </div>
          <div className="hero__art span-6 start-7">
            <BleRibbon className="hero__bg" />
            <HeroScene />
          </div>
        </div>
      </div>
    </section>
  );
}
