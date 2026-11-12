import { useEffect } from 'react';
import { createRoot, hydrateRoot } from 'react-dom/client';
import App from './App';
import './styles/page.css';

// The taste-skill kit (public/assets) scans the DOM on start-up: page behaviour, motion, layout,
// UI fragments and the pinned-chapters moment. It loads its own pieces relative to these files,
// so it starts only once React has committed the page.
let kitLoaded = false;
function loadKit() {
  if (kitLoaded) return;
  kitLoaded = true;
  for (const file of ['assets/system.js', 'assets/motion/motion.js', 'assets/layout/layout.js']) {
    const script = document.createElement('script');
    script.type = 'module';
    script.src = import.meta.env.BASE_URL + file;
    document.body.append(script);
  }
}

function Boot() {
  useEffect(loadKit, []);
  return <App />;
}

// `npm run build` prerenders the page into #root (scripts/prerender.mjs); `npm run dev` starts empty.
const container = document.getElementById('root')!;
if (container.firstElementChild) hydrateRoot(container, <Boot />);
else createRoot(container).render(<Boot />);
