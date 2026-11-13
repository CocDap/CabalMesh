import { useEffect, useId, useLayoutEffect, useRef, useState } from 'react';
import { BUILDS, RELEASE, detectPlatform, type Build, type Platform } from '../download';

/** Pixel glyphs on a 12-unit grid, in the Silkscreen manner: a phone, a laptop and the list's chevron. */
const GLYPHS: Record<Build['kind'] | 'chevron', string> = {
  phone: 'M3 0h6v12H3zM4 2v7h4V2z',
  laptop: 'M2 1h8v7H2zM3 2v5h6V2zM0 9h12v2H0z',
  chevron: 'M1 3h2v2H1zM3 5h2v2H3zM5 7h2v2H5zM7 5h2v2H7zM9 3h2v2H9z',
};

export function Glyph({ kind }: { kind: keyof typeof GLYPHS }) {
  return (
    <svg className="dl__glyph" viewBox="0 0 12 12" aria-hidden="true">
      <path d={GLYPHS[kind]} fillRule="evenodd" />
    </svg>
  );
}

/**
 * "Download free for Mac" with the other platforms one click away. The page is prerendered with the Mac build;
 * in the browser the label follows the visitor's platform, and where there is no build (Windows, Linux) the
 * main button opens the list instead. The hero clips what runs past its bottom, so when the list does not fit
 * under the button it opens upward.
 */
export default function DownloadButton() {
  const [platform, setPlatform] = useState<Platform | null>('mac-arm');
  const [open, setOpen] = useState(false);
  const [up, setUp] = useState(false);
  const root = useRef<HTMLDivElement>(null);
  const menu = useRef<HTMLDivElement>(null);
  const toggle = useRef<HTMLButtonElement>(null);
  const listId = useId();

  useEffect(() => {
    let live = true;
    detectPlatform().then((p) => live && setPlatform(p));
    return () => {
      live = false;
    };
  }, []);

  useLayoutEffect(() => {
    const box = root.current?.closest('section')?.getBoundingClientRect();
    const at = root.current?.getBoundingClientRect();
    const h = menu.current?.offsetHeight ?? 0;
    if (!open || !box || !at) return;
    const below = box.bottom - at.bottom;
    setUp(h + 16 > below && at.top - box.top > below);
  }, [open]);

  useEffect(() => {
    if (!open) return;
    const away = (e: PointerEvent) => {
      if (!root.current?.contains(e.target as Node)) setOpen(false);
    };
    const esc = (e: KeyboardEvent) => {
      if (e.key !== 'Escape') return;
      setOpen(false);
      toggle.current?.focus();
    };
    document.addEventListener('pointerdown', away);
    document.addEventListener('keydown', esc);
    return () => {
      document.removeEventListener('pointerdown', away);
      document.removeEventListener('keydown', esc);
    };
  }, [open]);

  const build = BUILDS.find((b) => b.id === platform);
  const flip = () => setOpen((o) => !o);

  return (
    <div className="dl" ref={root}>
      <div className="dl__split">
        {build ? (
          <a className="btn btn--primary dl__main" href={build.href}>
            <Glyph kind={build.kind} />
            <span>
              {'Download '}
              <span className="dl__free">{'free '}</span>
              {`for ${build.short}`}
            </span>
          </a>
        ) : (
          <button type="button" className="btn btn--primary dl__main" aria-expanded={open} aria-controls={listId} onClick={flip}>
            Download free
          </button>
        )}
        <button ref={toggle} type="button" className="btn btn--primary dl__more" aria-label="All platforms" aria-expanded={open} aria-controls={listId} onClick={flip}>
          <Glyph kind="chevron" />
        </button>
      </div>
      <div ref={menu} className={up ? 'dl__menu dl__menu--up' : 'dl__menu'} id={listId} hidden={!open}>
        <ul>
          {BUILDS.map((b) => (
            <li key={b.id}>
              <a href={b.href} onClick={() => setOpen(false)}>
                <Glyph kind={b.kind} />
                <span className="dl__name">{b.name}</span>
                <span className="dl__ext">{b.ext}</span>
                <span className="dl__size">{b.size}</span>
              </a>
            </li>
          ))}
        </ul>
        <a className="dl__all" href={RELEASE.page}>{`Release ${RELEASE.version} on GitHub`}</a>
      </div>
    </div>
  );
}
