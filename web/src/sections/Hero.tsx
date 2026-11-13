import { RELEASE } from '../download';
import HeroScene from '../scenes/HeroScene';
import BleRibbon from './BleRibbon';
import DownloadButton from './DownloadButton';

/** First screen: the headline, the download button beside a link to the trade, and the hero scene on the BLE ribbon. */
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
              <DownloadButton />
              <a className="btn btn--secondary" href="#how">See a trade</a>
            </div>
            <p className="hero__proof">Free and open source. Version {RELEASE.version} for Android, iOS and Mac. Built on NEAR.</p>
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
