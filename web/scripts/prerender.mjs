// Writes the rendered page into dist/index.html, so the HTML carries the content before any
// script runs (search, link previews, the taste-skill checks). The client then hydrates it.
import { readFile, rm, writeFile } from 'node:fs/promises';
import { fileURLToPath } from 'node:url';

const root = fileURLToPath(new URL('..', import.meta.url));
const { render } = await import(new URL('dist-ssr/entry-server.js', `file://${root}`).href);

const file = `${root}dist/index.html`;
const html = await readFile(file, 'utf8');
const mount = '<div id="root"></div>';
if (!html.includes(mount)) throw new Error(`prerender: ${mount} not found in dist/index.html`);
await writeFile(file, html.replace(mount, `<div id="root">${render()}</div>`));
await rm(`${root}dist-ssr`, { recursive: true, force: true });
console.log('prerendered dist/index.html');
