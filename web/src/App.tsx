import Nav from './sections/Nav';
import Hero from './sections/Hero';
import AppFrame from './sections/AppFrame';
import HowItWorks from './sections/HowItWorks';
import Sequence from './sections/Sequence';
import WhyNear from './sections/WhyNear';
import Roadmap from './sections/Roadmap';
import Closing from './sections/Closing';

/**
 * type: software (site-kind software): an open-source peer-to-peer crypto trading app for phones
 *      deal: hero collage, nav nav-mega, close close-type, footer footer-dark, rhythm space, buttons arrow, mark client logo (brand/mark.png),
 *      palette brand (start.mjs brand: #0b0b0b ground, white ink, #00e5ff accent, the app's own black and neon blue), type brand (Silkscreen display, IBM Plex Mono text, local files from src/ds), radius sharp, grounds none
 *      moments: pinned-chapters in #how (the three steps of one trade, each with the app's own phone screen)
 *      sections: hero (1) · app (1, app-frame: intents list + the agent's log) · how (2, moment) · sequence (1, diagram of the recorded trade)
 *                · why (two titles, each over a small drawing, content height) · roadmap (1, rows) · footer.closing (links only, content height)
 */
export default function App() {
  return (
    <>
      <Nav />
      <main id="top">
        <Hero />
        <AppFrame />
        <HowItWorks />
        <Sequence />
        <WhyNear />
        <Roadmap />
      </main>
      <Closing />
    </>
  );
}
