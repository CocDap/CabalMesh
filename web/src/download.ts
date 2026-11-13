/**
 * The builds in the latest GitHub release, one per platform. The hero's download button and the footer read
 * them from here; update this list (file names and sizes) with each release.
 */
export const RELEASE = {
  version: '0.1.0',
  page: 'https://github.com/CocDap/CabalMesh/releases/tag/0.1.0',
};

export type Platform = 'android' | 'ios' | 'mac-arm' | 'mac-intel';
export type Build = {
  id: Platform;
  /** The name in the platform list. */
  name: string;
  /** The end of the button's label: "Download free for …". */
  short: string;
  ext: string;
  size: string;
  kind: 'phone' | 'laptop';
  href: string;
};

const file = (name: string) => `https://github.com/CocDap/CabalMesh/releases/download/${RELEASE.version}/${name}`;

export const BUILDS: Build[] = [
  { id: 'android', name: 'Android', short: 'Android', ext: '.apk', size: '37.9 MB', kind: 'phone', href: file('cabalmesh-android-release-unsigned.apk') },
  { id: 'ios', name: 'iOS', short: 'iOS', ext: '.app.zip', size: '9.2 MB', kind: 'phone', href: file('cabalmesh-ios.app.zip') },
  { id: 'mac-arm', name: 'Mac with Apple silicon', short: 'Mac', ext: '.dmg', size: '11 MB', kind: 'laptop', href: file('cabalmesh-apple-silicon.dmg') },
  { id: 'mac-intel', name: 'Mac with Intel', short: 'Intel Mac', ext: '.dmg', size: '11.5 MB', kind: 'laptop', href: file('cabalmesh-intel.dmg') },
];

type UAData = { getHighEntropyValues?: (hints: string[]) => Promise<{ architecture?: string }> };

/** The visitor's platform, or null when there is no build for it (Windows, Linux). Runs in the browser only. */
export async function detectPlatform(): Promise<Platform | null> {
  const ua = navigator.userAgent;
  if (/Android/i.test(ua)) return 'android';
  // an iPad asks for the desktop site and says it is a Mac; it is the Mac with a touch screen
  if (/iPhone|iPad|iPod/.test(ua) || (/Macintosh/.test(ua) && navigator.maxTouchPoints > 1)) return 'ios';
  if (/Macintosh|Mac OS X/.test(ua)) {
    // Chromium browsers can tell an Intel Mac apart; Safari and Firefox report Intel on every Mac, so they get Apple silicon
    try {
      const data = (navigator as Navigator & { userAgentData?: UAData }).userAgentData;
      const hints = await data?.getHighEntropyValues?.(['architecture']);
      if (hints?.architecture === 'x86') return 'mac-intel';
    } catch {
      /* no hint: keep the default */
    }
    return 'mac-arm';
  }
  return null;
}
