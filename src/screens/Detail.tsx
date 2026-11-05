import { useCallback, useEffect, useState } from "react";
import { invoke } from "@tauri-apps/api/core";
import { Badge, Button, Panel, StatusDot, Terminal } from "../ds";
import { useLogStream } from "../state/useLogStream";
import { errorCopy } from "../state/errorCopy";
import type { IntentId } from "../shell/screen";
import type { IntentDetailView, LogLine } from "../types/bindings";

/** Visible terminal lines. The rest are retained but scrolled. */
const VISIBLE = 6;
const RETAINED = 200;

/**
 * One intent, live.
 *
 * **Navigating away from this screen does not stop a settlement.** The log
 * subscription is torn down on unmount, and that cancels *delivery* only — the
 * settlement runs in a task that holds the ledger and nothing from here, so it
 * cannot be reached by a UI navigation. The rule is enforced in Rust
 * (`src/intents.rs`, `src/commands.rs`) rather than by this screen being
 * careful, because being careful is not a guarantee. Coming back replays every
 * line that was recorded while away.
 *
 * Cancelling the **intent** is the opposite: a deliberate action that ends it.
 * Once this side has signed it is refused until the signature expires, because
 * a signature cannot be recalled. The two are different buttons and different commands, and
 * conflating them is the correctness bug this ticket names.
 *
 * The seven-row breakdown, the elapsed timer and whether settling is possible
 * all come from Rust. In particular, settling is refused with a reason rather
 * than offered and then rejected — a signature commits to an agreed price, and
 * with no deal there is nothing to sign.
 */
export function Detail({ id, onSettled }: { id: IntentId; onSettled: (id: IntentId) => void }) {
  const [detail, setDetail] = useState<IntentDetailView | null>(null);
  const [lines, setLines] = useState<LogLine[]>([]);
  const [error, setError] = useState<string | null>(null);
  const [busy, setBusy] = useState(false);

  const refresh = useCallback(() => {
    invoke<IntentDetailView>("intent_detail", { id })
      .then(setDetail)
      .catch(() => undefined);
  }, [id]);

  useEffect(() => {
    refresh();
    // Polled for the elapsed timer, which has to tick whether or not anything
    // happened. Status changes arrive on the log stream, so this is a clock
    // rather than the way state is learned.
    const timer = window.setInterval(refresh, 1_000);
    return () => window.clearInterval(timer);
  }, [refresh]);

  useLogStream("subscribe_settlement_log", { id }, (line) => {
    setLines((previous) => {
      const next = [...previous, line];
      return next.length > RETAINED ? next.slice(-RETAINED) : next;
    });
    // A new line means something moved. Cheaper and more responsive than
    // shortening the poll.
    refresh();
  });

  const status = detail?.status.status;
  const settled = status === "SETTLED";

  useEffect(() => {
    if (settled) onSettled(id);
  }, [settled, id, onSettled]);

  const act = async (command: "settle_intent" | "cancel_intent") => {
    if (busy) return;
    setBusy(true);
    setError(null);
    try {
      await invoke(command, { id });
      refresh();
    } catch (failure) {
      setError(errorCopy(failure));
    } finally {
      setBusy(false);
    }
  };

  return (
    <div style={{ display: "flex", flexDirection: "column", gap: "var(--space-6)", padding: "var(--space-6)" }}>
      <Panel label="INTENT">
        <div style={{ padding: "var(--space-6)", display: "flex", flexDirection: "column", gap: "var(--space-5)" }}>
          <div style={{ display: "flex", alignItems: "center", justifyContent: "space-between", gap: "var(--space-5)" }}>
            <span
              style={{
                fontFamily: "var(--type-heading-family)",
                fontSize: "var(--text-sm)",
                letterSpacing: "var(--type-heading-tracking)",
                color: "var(--text-primary)",
              }}
            >
              {detail?.title ?? "—"}
            </span>
            <span style={{ display: "flex", alignItems: "center", gap: "var(--space-3)" }}>
              <StatusDot tone={toneFor(status)} pulse={isLive(status)} />
              {/* The status word exists as text. Colour alone reaches no
                  screen reader. */}
              <span
                style={{
                  fontFamily: "var(--type-label-family)",
                  fontSize: "var(--text-2xs)",
                  letterSpacing: "var(--tracking-widest)",
                  color: "var(--text-secondary)",
                }}
              >
                {(status ?? "—").replace(/_/g, " ")}
              </span>
            </span>
          </div>

          <div style={{ display: "flex", alignItems: "center", gap: "var(--space-4)" }}>
            <Badge tone="quiet" size="sm">
              {detail?.elapsed ?? "—"}
            </Badge>
            {detail?.status.status === "NEGOTIATING" ? (
              <span style={{ fontSize: "var(--text-2xs)", letterSpacing: "var(--tracking-widest)", color: "var(--text-muted)" }}>
                {detail.status.bids} {detail.status.bids === 1 ? "BID" : "BIDS"}
                {detail.status.best ? ` · BEST ${detail.status.best}` : ""}
              </span>
            ) : null}
          </div>
        </div>
      </Panel>

      <Panel label="BREAKDOWN">
        <div style={{ padding: "var(--space-6)", display: "flex", flexDirection: "column", gap: "var(--space-5)" }}>
          {(detail?.rows ?? []).map((row) => (
            <div key={row.key} className="cm-row" style={{ display: "flex", justifyContent: "space-between", gap: "var(--space-5)" }}>
              <span
                style={{
                  fontFamily: "var(--type-label-family)",
                  fontSize: "var(--text-2xs)",
                  letterSpacing: "var(--tracking-widest)",
                  color: "var(--text-muted)",
                }}
              >
                {row.key}
              </span>
              <span
                style={{
                  fontFamily: "var(--type-data-family)",
                  fontSize: "var(--text-sm)",
                  color: "var(--text-primary)",
                  textAlign: "right",
                }}
              >
                {row.value}
              </span>
            </div>
          ))}
        </div>
      </Panel>

      <Terminal
        label="VERIFICATION LOG"
        role="log"
        aria-live="polite"
        lines={lines.slice(-VISIBLE).map((line) => ({ text: line.text, tone: line.tone }))}
      />

      {error ? (
        <p role="alert" style={{ fontSize: "var(--text-base)", color: "var(--accent-blood-red)" }}>
          {error}
        </p>
      ) : null}

      {/* Blocked with a reason rather than hidden. A missing button explains
          nothing; a disabled one with the reason beside it does. */}
      {detail?.settleBlocked ? (
        <p style={{ fontSize: "var(--text-base)", color: "var(--text-muted)", margin: 0 }}>{detail.settleBlocked}</p>
      ) : null}

      <Button
        tone="primary"
        size="lg"
        block
        className="cm-touch"
        disabled={busy || !detail?.canSettle}
        onClick={() => act("settle_intent")}
      >
        CONFIRM TO SETTLE
      </Button>

      {detail?.canCancel ? (
        <Button tone="danger" size="lg" block className="cm-touch" disabled={busy} onClick={() => act("cancel_intent")}>
          CANCEL INTENT
        </Button>
      ) : null}
    </div>
  );
}

/** Whether the intent is still moving, which is what the pulse means. */
function isLive(status: string | undefined): boolean {
  return status === "BROADCAST" || status === "NEGOTIATING" || status === "FINDING_ROUTE" || status === "WAITING";
}

/** Indicator tone per lifecycle state. */
function toneFor(status: string | undefined): "online" | "alert" | "idle" | "info" | "offline" {
  switch (status) {
    case "SETTLED":
      return "online";
    case "FAILED":
      return "alert";
    case "CANCELLED":
      return "offline";
    case "WAITING":
    case "DRAFT":
      return "idle";
    default:
      return "info";
  }
}
