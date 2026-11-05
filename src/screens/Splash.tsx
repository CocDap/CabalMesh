import { Button, Logo } from "../ds";

/**
 * First screen. Two offers, no chrome.
 *
 * Copy is the board's, verbatim: impersonal, present tense, fragments ending in
 * full stops. "ZERO IDENTITY. PRIVATE INTENTS." is three statements, not a
 * tagline to be softened.
 */
export function Splash({ onEnter }: { onEnter: () => void }) {
  return (
    <section
      style={{
        minHeight: "100dvh",
        display: "flex",
        flexDirection: "column",
        alignItems: "center",
        justifyContent: "center",
        gap: "var(--space-8)",
        padding: "var(--space-9) var(--space-8)",
        paddingTop: "calc(var(--safe-top) + var(--space-9))",
        paddingBottom: "calc(var(--safe-bottom) + var(--space-9))",
        textAlign: "center",
      }}
    >
      <Logo variant="minimal" size={72} basePath="/ds-assets/logo" />

      <h1
        style={{
          margin: 0,
          fontFamily: "var(--type-wordmark-family)",
          fontSize: "var(--text-xl)",
          letterSpacing: "var(--type-wordmark-tracking)",
          // The wordmark's tracking pushes it off-centre without a matching
          // indent — the board specifies both together.
          textIndent: "var(--type-wordmark-tracking)",
          color: "var(--text-primary)",
        }}
      >
        CABAL MESH
      </h1>

      <p
        style={{
          fontFamily: "var(--type-label-family)",
          fontSize: "var(--text-2xs)",
          letterSpacing: "var(--tracking-widest)",
          color: "var(--text-muted)",
          textTransform: "uppercase",
        }}
      >
        Zero identity. Private intents.
      </p>

      <div
        style={{
          display: "flex",
          flexDirection: "column",
          gap: "var(--space-6)",
          width: "100%",
          maxWidth: 320,
          // 16px apart: expanded hit areas that meet would steal each other's
          // taps (ds/mobile.css, C4).
          marginTop: "var(--space-8)",
        }}
      >
        <Button tone="primary" size="lg" block className="cm-touch" onClick={onEnter}>
          ENTER THE MESH
        </Button>
        <Button tone="ghost" size="lg" block className="cm-touch" onClick={onEnter}>
          CREATE ANONYMOUS NODE
        </Button>
      </div>

      <p
        style={{
          marginTop: "var(--space-8)",
          fontSize: "var(--text-2xs)",
          letterSpacing: "var(--tracking-widest)",
          color: "var(--text-disabled)",
          textTransform: "uppercase",
        }}
      >
        The nobody network.
      </p>
    </section>
  );
}
