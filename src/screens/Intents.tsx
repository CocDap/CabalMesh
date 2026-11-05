import { useEffect, useState } from "react";
import { invoke } from "@tauri-apps/api/core";
import { Badge, Button, Panel, StatusDot } from "../ds";
import type { IntentTab } from "../shell/screen";
import type { IntentView } from "../types/bindings";

const TABS: IntentTab[] = ["ACTIVE", "PENDING", "HISTORY"];

/**
 * Empty copy per tab, in the board's voice: declarative, present tense,
 * fragments ending in full stops. "Nothing is queued. Nothing is stored." is
 * the prototype's own line, and the pattern generalises to the other tabs.
 */
const EMPTY: Record<IntentTab, { title: string; body: string }> = {
  ACTIVE: { title: "NO ACTIVE INTENTS", body: "Nothing is running. Nothing is stored." },
  PENDING: { title: "NO PENDING INTENTS", body: "Nothing is queued. Nothing is stored." },
  HISTORY: { title: "NO HISTORY", body: "Nothing has settled. Nothing is stored." },
};

/**
 * The intent list.
 *
 * Status drives both the text and the indicator tone from **one** enum variant.
 * The prototype passes a colour alongside the status (`dot: BLUE`), which would
 * hard-code the palette into Rust and turn a design change into a backend
 * release.
 *
 * The empty state is first-class rather than an afterthought — it is the state
 * this build is actually in, and the prototype ships a toggle for it precisely
 * because it matters.
 */
export function Intents({
  tab,
  onTabChange,
  onCompose,
}: {
  tab: IntentTab;
  onTabChange: (tab: IntentTab) => void;
  onCompose: () => void;
}) {
  const [intents, setIntents] = useState<IntentView[] | null>(null);

  useEffect(() => {
    let cancelled = false;
    invoke<IntentView[]>("list_intents", { filter: tab })
      .then((next) => {
        if (!cancelled) setIntents(next);
      })
      .catch(() => {
        if (!cancelled) setIntents([]);
      });
    return () => {
      cancelled = true;
    };
  }, [tab]);

  const empty = EMPTY[tab];

  return (
    <div style={{ display: "flex", flexDirection: "column", gap: "var(--space-6)", padding: "var(--space-6)" }}>
      {/* role=tablist plus aria-selected: the 2px underline is a visual
          selected state and reaches no assistive technology on its own. */}
      <div role="tablist" aria-label="Intent filter" style={{ display: "flex", gap: "var(--space-7)" }}>
        {TABS.map((name) => (
          <button
            key={name}
            type="button"
            role="tab"
            aria-selected={tab === name}
            className="cm-touch"
            onClick={() => onTabChange(name)}
            style={{
              background: "none",
              border: "none",
              padding: "var(--space-4) 0",
              cursor: "pointer",
              fontFamily: "var(--type-label-family)",
              fontSize: "var(--text-2xs)",
              letterSpacing: "var(--tracking-widest)",
              color: tab === name ? "var(--text-primary)" : "var(--text-muted)",
              borderBottom:
                tab === name
                  ? "var(--border-width-thick) solid var(--border-loud)"
                  : "var(--border-width-thick) solid transparent",
            }}
          >
            {name}
          </button>
        ))}
      </div>

      {intents === null ? null : intents.length === 0 ? (
        <Panel>
          <div
            style={{
              padding: "var(--space-11) var(--space-6)",
              display: "flex",
              flexDirection: "column",
              gap: "var(--space-5)",
              alignItems: "center",
              textAlign: "center",
            }}
          >
            <span
              style={{
                fontFamily: "var(--type-heading-family)",
                fontSize: "var(--text-sm)",
                letterSpacing: "var(--type-heading-tracking)",
                color: "var(--text-primary)",
              }}
            >
              {empty.title}
            </span>
            <span style={{ fontSize: "var(--text-base)", color: "var(--text-muted)" }}>{empty.body}</span>
            <Button
              tone="secondary"
              size="md"
              className="cm-touch"
              onClick={onCompose}
              style={{ marginTop: "var(--space-6)" }}
            >
              NEW INTENT
            </Button>
          </div>
        </Panel>
      ) : (
        <>
          {intents.map((intent) => (
            <IntentRow key={intent.id} intent={intent} />
          ))}
          <Button tone="secondary" size="lg" block className="cm-touch" onClick={onCompose}>
            + NEW INTENT
          </Button>
        </>
      )}
    </div>
  );
}

function IntentRow({ intent }: { intent: IntentView }) {
  const status = intent.status.status;
  const tone =
    status === "SETTLED"
      ? "online"
      : status === "FAILED"
        ? "alert"
        : status === "WAITING"
          ? "idle"
          : "info";

  return (
    <Panel>
      <div
        className="cm-row"
        style={{ padding: "var(--space-6)", display: "flex", flexDirection: "column", gap: "var(--space-5)" }}
      >
        <div style={{ display: "flex", flexDirection: "column", gap: "var(--space-3)" }}>
          <span
            style={{
              fontFamily: "var(--type-heading-family)",
              fontSize: "var(--text-sm)",
              letterSpacing: "var(--type-heading-tracking)",
              color: "var(--text-primary)",
            }}
          >
            {intent.title}
          </span>
          <span
            style={{
              fontFamily: "var(--type-label-family)",
              fontSize: "var(--text-2xs)",
              letterSpacing: "var(--tracking-widest)",
              color: "var(--text-muted)",
            }}
          >
            {intent.subtitle}
          </span>
          {intent.badge ? (
            <Badge tone="quiet" size="sm">
              {intent.badge}
            </Badge>
          ) : null}
        </div>

        <div style={{ display: "flex", justifyContent: "space-between", alignItems: "center", gap: "var(--space-5)" }}>
          <span
            style={{
              fontFamily: "var(--type-data-family)",
              fontSize: "var(--text-sm)",
              color: "var(--text-primary)",
            }}
          >
            {intent.amount}
          </span>
          <span style={{ display: "flex", alignItems: "center", gap: "var(--space-3)" }}>
            <StatusDot tone={tone} />
            {/* The status word exists as text. Colour alone reaches no screen
                reader, and the brand's accent budget forbids leaning on it. */}
            <span
              style={{
                fontFamily: "var(--type-label-family)",
                fontSize: "var(--text-2xs)",
                letterSpacing: "var(--tracking-widest)",
                color: "var(--text-secondary)",
              }}
            >
              {status.replace("_", " ")}
            </span>
          </span>
          <span
            style={{
              fontFamily: "var(--type-data-family)",
              fontSize: "var(--text-2xs)",
              color: "var(--text-muted)",
            }}
          >
            {intent.elapsed}
          </span>
        </div>
      </div>
    </Panel>
  );
}
