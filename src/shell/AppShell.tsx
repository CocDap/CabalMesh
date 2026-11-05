import React from "react";
import { Icon } from "../ds";
import { TABS, activeTab, back, hasChrome, title, type Screen, type TabKey } from "./screen";
import { modalOpen } from "./modalStack";
import { useTypeScale } from "../state/useTypeScale";

/**
 * The fixed chrome every screen sits inside.
 *
 * Three things here are load-bearing beyond layout:
 *
 * **Nothing has a fixed height.** Every text-bearing box is sized by content
 * with a `minHeight` floor. A fixed height clips descenders the moment the OS
 * font scale rises, which is exactly what supporting 200% requires avoiding.
 *
 * **The tab bar shows glyphs, with labels that hide when they no longer fit.**
 * Five long uppercase labels in 390px at 200% is roughly 2.5x the available
 * width. The board treats glyphs as primary rather than decorative, so the icon
 * carries the meaning and `aria-label` carries the name — no overflow menu,
 * which would hide primary destinations.
 *
 * **Roles and states are explicit.** The design system is built from generic
 * elements styled inline and signals selection through a white underline, which
 * reaches no assistive technology at all.
 */
export function AppShell({
  screen,
  onNavigate,
  children,
}: {
  screen: Screen;
  onNavigate: (next: Screen) => void;
  children: React.ReactNode;
}) {
  useTypeScale();
  useHardwareBack(screen, onNavigate);

  const chrome = hasChrome(screen);
  const current = activeTab(screen);

  return (
    <div
      style={{
        display: "flex",
        flexDirection: "column",
        minHeight: "100dvh",
        background: "var(--surface-page)",
        color: "var(--text-secondary)",
      }}
    >
      {chrome && <Header screen={screen} onNavigate={onNavigate} />}

      <main
        className="cm-scroll"
        style={{
          flex: 1,
          minHeight: 0,
          paddingLeft: "var(--safe-left)",
          paddingRight: "var(--safe-right)",
        }}
      >
        {children}
      </main>

      {chrome && <TabBar current={current} onNavigate={onNavigate} />}
    </div>
  );
}

function Header({ screen, onNavigate }: { screen: Screen; onNavigate: (next: Screen) => void }) {
  const canGoBack = back(screen) !== screen;

  return (
    <header
      className="cm-header"
      style={{
        // minHeight, not height: the 52px chrome must grow with the type scale.
        minHeight: 52,
        display: "flex",
        alignItems: "center",
        gap: "var(--space-5)",
        padding: "var(--space-4) var(--space-6)",
        paddingTop: "calc(var(--safe-top) + var(--space-4))",
        borderBottom: "var(--border-hairline-style)",
        background: "var(--surface-page)",
        position: "sticky",
        top: 0,
        zIndex: "var(--z-nav)",
      }}
    >
      {canGoBack ? (
        <button
          type="button"
          className="cm-touch"
          aria-label="Back"
          onClick={() => onNavigate(back(screen))}
          style={buttonReset}
        >
          ←
        </button>
      ) : (
        <span style={{ width: "1em" }} aria-hidden="true" />
      )}

      <h1
        style={{
          flex: 1,
          margin: 0,
          fontFamily: "var(--type-heading-family)",
          fontSize: "var(--text-sm)",
          letterSpacing: "var(--type-heading-tracking)",
          color: "var(--text-primary)",
          textTransform: "uppercase",
        }}
      >
        {title(screen)}
      </h1>
    </header>
  );
}

function TabBar({
  current,
  onNavigate,
}: {
  current: TabKey | null;
  onNavigate: (next: Screen) => void;
}) {
  return (
    <nav
      className="cm-tabbar"
      // role=tablist plus aria-selected below: the white underline is a visual
      // selected state and reaches no screen reader on its own.
      role="tablist"
      aria-label="Primary"
      style={{
        display: "flex",
        minHeight: 48,
        borderTop: "var(--border-hairline-style)",
        background: "var(--surface-page)",
        paddingBottom: "var(--safe-bottom)",
        position: "sticky",
        bottom: 0,
        zIndex: "var(--z-nav)",
      }}
    >
      {TABS.map((tab) => {
        const selected = current === tab.key;
        return (
          <button
            key={tab.key}
            type="button"
            role="tab"
            aria-selected={selected}
            aria-label={tab.label}
            className="cm-touch"
            onClick={() => onNavigate(screenForTab(tab.key))}
            style={{
              ...buttonReset,
              flex: 1,
              display: "flex",
              flexDirection: "column",
              alignItems: "center",
              justifyContent: "center",
              gap: "var(--space-2)",
              padding: "var(--space-4) var(--space-2)",
              opacity: selected ? 1 : 0.34,
              color: selected ? "var(--text-primary)" : "var(--text-muted)",
              borderTop: selected
                ? "var(--border-width-thick) solid var(--border-loud)"
                : "var(--border-width-thick) solid transparent",
            }}
          >
            <Icon name={tab.icon} size={20} basePath="/ds-assets/icons" />
            <span
              style={{
                fontFamily: "var(--type-label-family)",
                fontSize: "var(--text-2xs)",
                letterSpacing: "var(--tracking-widest)",
                // Hidden rather than wrapped once the label no longer fits.
                // The glyph still carries the meaning and aria-label still
                // carries the name.
                overflow: "hidden",
                textOverflow: "clip",
                whiteSpace: "nowrap",
                maxWidth: "100%",
              }}
            >
              {tab.label}
            </span>
          </button>
        );
      })}
    </nav>
  );
}

function screenForTab(tab: TabKey): Screen {
  switch (tab) {
    case "home":
      return { name: "home" };
    case "intents":
      return { name: "intents", tab: "ACTIVE" };
    case "nodes":
      return { name: "nodes" };
    case "vault":
      return { name: "vault", tab: "ASSETS" };
    case "profile":
      return { name: "profile" };
  }
}

/**
 * Android's hardware back, bound to the same function as the header affordance
 * so the two can never disagree.
 *
 * Declines the press while a modal is open. Both this and the dialog listen for
 * the same `popstate`, and without the check one press would close the dialog
 * *and* leave the screen behind it. See shell/modalStack.ts.
 */
function useHardwareBack(screen: Screen, onNavigate: (next: Screen) => void): void {
  React.useEffect(() => {
    const handler = (event: PopStateEvent) => {
      if (modalOpen()) return;

      event.preventDefault();
      const target = back(screen);
      if (target !== screen) onNavigate(target);
      // Re-push so the next back press is still captured rather than exiting.
      window.history.pushState(null, "");
    };
    window.history.pushState(null, "");
    window.addEventListener("popstate", handler);
    return () => window.removeEventListener("popstate", handler);
  }, [screen, onNavigate]);
}

const buttonReset: React.CSSProperties = {
  background: "none",
  border: "none",
  padding: 0,
  font: "inherit",
  color: "inherit",
  cursor: "pointer",
};
