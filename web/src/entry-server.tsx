import { renderToString } from 'react-dom/server';
import App from './App';

/** The page as HTML, for scripts/prerender.mjs. */
export function render() {
  return renderToString(<App />);
}
