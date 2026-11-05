import { useEffect, useState } from "react";
import { invoke } from "@tauri-apps/api/core";
import { Badge, Button, Icon, Panel } from "../ds";
import { errorCopy } from "../state/errorCopy";
import type { IntentId } from "../shell/screen";
import type { ProofView } from "../types/bindings";

/**
 * The proof.
 *
 * Every figure here is evidence, so every figure is real or absent. The hash is
 * the settling transaction's, taken from the receipt the settlement held rather
 * than read back afterwards. The timing is measured across the actual call. The
 * route is the hops the intent travelled, and an empty one says so instead of
 * inventing a path. The fill price is the condition's own price — an intent
 * with no condition has no price it filled at, and renders none rather than
 * `$0.00`, which would be a figure and a wrong one.
 *
 * A screen whose whole purpose is proving things cannot have a placeholder on
 * it.
 */
export function Settled({ id, onDone }: { id: IntentId; onDone: () => void }) {
  const [proof, setProof] = useState<ProofView | null>(null);
  const [error, setError] = useState<string | null>(null);
  const [copied, setCopied] = useState(false);

  useEffect(() => {
    let cancelled = false;
    invoke<ProofView>("intent_proof", { id })
      .then((next) => {
        if (!cancelled) setProof(next);
      })
      .catch((failure) => {
        if (!cancelled) setError(errorCopy(failure));
      });
    return () => {
      cancelled = true;
    };
  }, [id]);

  const copyHash = async () => {
    if (!proof) return;
    try {
      await navigator.clipboard.writeText(proof.hash);
      setCopied(true);
      window.setTimeout(() => setCopied(false), 2_000);
    } catch {
      // Clipboard access can be refused. Silent rather than an error banner:
      // the hash is on screen and selectable either way.
    }
  };

  if (error) {
    return (
      <div style={{ padding: "var(--space-6)" }}>
        <p role="alert" style={{ fontSize: "var(--text-base)", color: "var(--accent-blood-red)" }}>
          {error}
        </p>
      </div>
    );
  }

  return (
    <div style={{ display: "flex", flexDirection: "column", gap: "var(--space-6)", padding: "var(--space-6)" }}>
      <Panel label="SETTLED">
        <div
          style={{
            padding: "var(--space-9) var(--space-6)",
            display: "flex",
            flexDirection: "column",
            alignItems: "center",
            gap: "var(--space-5)",
            textAlign: "center",
          }}
        >
          <Icon name="proof" size={40} basePath="/ds-assets/icons" />
          <span
            style={{
              fontFamily: "var(--type-heading-family)",
              fontSize: "var(--text-md)",
              letterSpacing: "var(--type-heading-tracking)",
              color: "var(--text-primary)",
            }}
          >
            EXECUTION PROVEN
          </span>
          <span style={{ fontSize: "var(--text-base)", color: "var(--text-muted)" }}>
            Settled on-chain. No identity is attached.
          </span>
          <Badge tone="quiet" size="sm">
            {proof?.timing ?? "—"}
          </Badge>
        </div>
      </Panel>

      <Panel label="PROOF">
        <div style={{ padding: "var(--space-6)", display: "flex", flexDirection: "column", gap: "var(--space-5)" }}>
          <div style={{ display: "flex", flexDirection: "column", gap: "var(--space-3)" }}>
            <span
              style={{
                fontFamily: "var(--type-label-family)",
                fontSize: "var(--text-2xs)",
                letterSpacing: "var(--tracking-widest)",
                color: "var(--text-muted)",
              }}
            >
              TRANSACTION HASH
            </span>
            <button
              type="button"
              className="cm-touch"
              onClick={copyHash}
              aria-label="Copy transaction hash"
              style={{
                background: "none",
                border: "none",
                padding: 0,
                cursor: "pointer",
                textAlign: "left",
                fontFamily: "var(--type-data-family)",
                fontSize: "var(--text-sm)",
                color: "var(--text-primary)",
                // The hash is long and must not push the page sideways.
                wordBreak: "break-all",
              }}
            >
              {proof?.hash ?? "—"}
            </button>
            {/* aria-live so the confirmation is announced, not only seen. */}
            <span
              aria-live="polite"
              style={{ fontSize: "var(--text-2xs)", letterSpacing: "var(--tracking-widest)", color: "var(--text-muted)" }}
            >
              {copied ? "COPIED." : ""}
            </span>
          </div>

          <Row label="SETTLEMENT TIME" value={proof?.timing ?? "—"} />
          {/* Absent rather than zero: an intent with no condition has no price
              it filled at, and $0.00 would be a claim. */}
          {proof?.filledAt ? <Row label="FILLED AT" value={proof.filledAt} /> : null}
          <Row
            label="ROUTE"
            value={proof && proof.route.length > 0 ? proof.route.join(" · ") : "DIRECT — NO RELAY HOPS"}
          />
        </div>
      </Panel>

      <Button tone="secondary" size="lg" block className="cm-touch" onClick={onDone}>
        DONE
      </Button>
    </div>
  );
}

function Row({ label, value }: { label: string; value: string }) {
  return (
    <div className="cm-row" style={{ display: "flex", justifyContent: "space-between", gap: "var(--space-5)" }}>
      <span
        style={{
          fontFamily: "var(--type-label-family)",
          fontSize: "var(--text-2xs)",
          letterSpacing: "var(--tracking-widest)",
          color: "var(--text-muted)",
        }}
      >
        {label}
      </span>
      <span
        style={{
          fontFamily: "var(--type-data-family)",
          fontSize: "var(--text-sm)",
          color: "var(--text-primary)",
          textAlign: "right",
        }}
      >
        {value}
      </span>
    </div>
  );
}
