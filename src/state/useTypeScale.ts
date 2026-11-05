import { useEffect } from "react";

/**
 * Applies the OS font-scale setting to `--type-scale`.
 *
 * **Unclamped.** WCAG 1.4.4 requires text to reach 200% without losing content
 * or function; an earlier revision capping this at 130% was a decision to fail
 * that criterion, not a resolution of it. The criterion does not forbid a 9px
 * base — it forbids a layout that cannot grow, and `ds/mobile.css` is what
 * makes this one grow.
 *
 * Re-read on resume, never only at boot: a user can change system font size
 * while the app is backgrounded, and reading once would leave the app at the
 * wrong size for the rest of the session.
 *
 * The native plugin that reads `preferredContentSizeCategory` (iOS) and
 * `Resources.getConfiguration().fontScale` (Android) is not built yet, so this
 * falls back to the ratio the webview itself reports. That is a real signal on
 * Android and a partial one on iOS, and it is never worse than 1.
 */
export function useTypeScale(): void {
  useEffect(() => {
    const apply = () => {
      document.documentElement.style.setProperty("--type-scale", String(readScale()));
    };

    apply();

    // Tauri emits these on mobile from the platform's own lifecycle callbacks.
    const onResume = () => apply();
    window.addEventListener("focus", onResume);
    document.addEventListener("visibilitychange", onResume);

    return () => {
      window.removeEventListener("focus", onResume);
      document.removeEventListener("visibilitychange", onResume);
    };
  }, []);
}

/**
 * Best available scale signal until the native plugin lands.
 *
 * Compares the browser's default font size against the 16px baseline. Returns
 * 1 on anything unexpected — a wrong-but-brand-exact rendering beats a
 * miscalculated one.
 */
function readScale(): number {
  try {
    const probe = document.createElement("div");
    probe.style.cssText = "position:absolute;visibility:hidden;font-size:1rem";
    document.body.appendChild(probe);
    const size = parseFloat(getComputedStyle(probe).fontSize);
    probe.remove();

    if (!Number.isFinite(size) || size <= 0) return 1;
    // Floor at 1: the design is already at its minimum legible size, so
    // shrinking it further is never the right answer.
    return Math.max(1, size / 16);
  } catch {
    return 1;
  }
}
