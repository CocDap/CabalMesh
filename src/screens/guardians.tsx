import { useEffect, useRef, useState, type CSSProperties, type ReactNode } from "react";
import { invoke } from "@tauri-apps/api/core";
import { listen } from "@tauri-apps/api/event";
import { Button, Checkbox, Field, Input, Meter, Panel, StatusDot } from "../ds";
import { ModalDialog } from "../shell/ModalDialog";
import { errorCopy } from "../state/errorCopy";
import type {
  GuardianCandidate,
  GuardianEnrollProgress,
  GuardianEnrollResult,
  GuardianEnrollStage,
  GuardianRestorePendingNotice,
  GuardianRestoreProgress,
  GuardianStatus,
  GuardianUnlockProgress,
} from "../types/bindings";

/**
 * Guardian screens from `docs/identity-design.md`: CHOOSE GUARDIANS,
 * DISTRIBUTING SHARES, MESH UNLOCK (owner side), RESTORE FROM GUARDIANS,
 * and the original device's veto when a restore of its vault is pending.
 *
 * K/N is fixed at 3-of-5 in v1 — the doc's decision, not a setting.
 */
export const GUARDIANS_N = 5;
export const GUARDIANS_K = 3;

const inTauri = () => "__TAURI_INTERNALS__" in window;

/** Subscribes to a Tauri event for the component's lifetime. */
function useTauriEvent<T>(name: string, handler: (payload: T) => void) {
  const latest = useRef(handler);
  latest.current = handler;
  useEffect(() => {
    if (!inTauri()) return;
    const unlisten = listen<T>(name, (event) => latest.current(event.payload));
    return () => {
      void unlisten.then((stop) => stop());
    };
  }, [name]);
}

const label: CSSProperties = {
  fontFamily: "var(--type-label-family)",
  fontSize: "var(--text-2xs)",
  letterSpacing: "var(--tracking-widest)",
  color: "var(--text-muted)",
  textTransform: "uppercase",
};
const heading: CSSProperties = {
  fontFamily: "var(--type-heading-family)",
  fontSize: "var(--text-sm)",
  letterSpacing: "var(--type-heading-tracking)",
  color: "var(--text-primary)",
};
const data: CSSProperties = {
  fontFamily: "var(--type-data-family)",
  fontSize: "var(--text-sm)",
  letterSpacing: "var(--type-data-tracking)",
  color: "var(--text-primary)",
};
const alert: CSSProperties = { ...label, color: "var(--accent-blood-red)", letterSpacing: "var(--tracking-wide)" };
const column = (gap: string): CSSProperties => ({ display: "flex", flexDirection: "column", gap });
const row: CSSProperties = { display: "flex", alignItems: "center", justifyContent: "space-between", gap: "var(--space-4)" };

function ErrorLine({ error }: { error: string | null }) {
  return error ? <span style={alert}>{error}</span> : null;
}

/** "in 23h 14m", "in 9m", or "any moment now". */
export function untilCopy(iso: string, now = Date.now()): string {
  const minutes = Math.max(0, Math.round((Date.parse(iso) - now) / 60000));
  if (minutes === 0) return "ANY MOMENT NOW";
  const h = Math.floor(minutes / 60);
  const m = minutes % 60;
  return `IN ${h > 0 ? `${h}H ` : ""}${m}M`;
}

// ---------------------------------------------------------------------------
// VAULT → KEYS → GUARDIANS
// ---------------------------------------------------------------------------

type Stage = "overview" | "choose" | "distributing" | "code";

export function GuardianPanel() {
  const [status, setStatus] = useState<GuardianStatus | null>(null);
  const [stage, setStage] = useState<Stage>("overview");
  const [chosen, setChosen] = useState<GuardianCandidate[]>([]);

  const refresh = () => {
    invoke<GuardianStatus>("guardian_status").then(setStatus).catch(() => setStatus(null));
  };
  useEffect(refresh, []);

  if (status === null) return null;

  return (
    <Panel label="GUARDIANS">
      <div style={{ padding: "var(--space-6)", ...column("var(--space-6)") }}>
        {stage === "overview" && <Overview status={status} onSetUp={() => setStage("choose")} onShowCode={() => setStage("code")} />}
        {stage === "choose" && (
          <ChooseGuardians
            onCancel={() => setStage("overview")}
            onContinue={(picked) => {
              setChosen(picked);
              setStage("distributing");
            }}
          />
        )}
        {stage === "distributing" && (
          <DistributingShares
            guardians={chosen}
            onDone={(enrolled) => {
              refresh();
              setStage(enrolled ? "code" : "overview");
            }}
          />
        )}
        {stage === "code" && <RestoreCode onDone={() => setStage("overview")} />}
      </div>
    </Panel>
  );
}

function Overview({ status, onSetUp, onShowCode }: { status: GuardianStatus; onSetUp: () => void; onShowCode: () => void }) {
  return (
    <>
      <div className="cm-row" style={row}>
        <div style={column("var(--space-2)")}>
          <span style={heading}>MESH UNLOCK</span>
          <span style={label}>
            {status.enrolled ? `${status.guardianCount} ENROLLED · ANY ${status.threshold} CAN UNLOCK` : `NONE · NEED ${GUARDIANS_N}`}
          </span>
        </div>
        <Button tone="secondary" size="sm" className="cm-touch" onClick={onSetUp}>
          {status.enrolled ? "RE-ENROLL" : "SET UP"}
        </Button>
      </div>

      {status.enrolled && !status.restoreReady && (
        <span style={alert}>
          ⚠ SET UP BEFORE NEW-DEVICE RESTORE EXISTED. RE-ENROLL TO GET A RESTORE CODE — WITHOUT ONE, A LOST DEVICE CANNOT BE RESTORED.
        </span>
      )}
      {status.restoreReady && (
        <div className="cm-row" style={row}>
          <span style={label}>RESTORE CODE · NEEDED ON A NEW DEVICE</span>
          <Button tone="ghost" size="sm" className="cm-touch" onClick={onShowCode}>
            SHOW
          </Button>
        </div>
      )}

      {status.holdingFor > 0 && (
        <span style={label}>
          HOLDING A SHARE FOR {status.holdingFor} OTHER{status.holdingFor === 1 ? "" : "S"}
          {status.holdingUnlocked ? "" : " · LOCKED — UNLOCK TO ANSWER THEM"}
        </span>
      )}
    </>
  );
}

function ChooseGuardians({ onCancel, onContinue }: { onCancel: () => void; onContinue: (picked: GuardianCandidate[]) => void }) {
  const [candidates, setCandidates] = useState<GuardianCandidate[] | null>(null);
  const [selected, setSelected] = useState<Set<string>>(new Set());

  useEffect(() => {
    invoke<GuardianCandidate[]>("guardian_candidates")
      // Closest first. The doc orders by relay count, but peer ids reset
      // every session by design, so there is no durable history to count;
      // hop distance is the honest proxy for "can actually reach you".
      .then((list) => setCandidates([...list].sort((a, b) => a.hops - b.hops)))
      .catch(() => setCandidates([]));
  }, []);

  function toggle(peerId: string) {
    setSelected((previous) => {
      const next = new Set(previous);
      if (next.has(peerId)) next.delete(peerId);
      else if (next.size < GUARDIANS_N) next.add(peerId);
      return next;
    });
  }

  return (
    <div style={column("var(--space-5)")}>
      <span style={heading}>CHOOSE GUARDIANS</span>
      <span style={label}>
        PICK {GUARDIANS_N}. ANY {GUARDIANS_K} CAN UNLOCK.
      </span>

      {candidates === null ? (
        <span style={label}>SCANNING…</span>
      ) : candidates.length < GUARDIANS_N ? (
        <span style={label}>
          {candidates.length} NODE{candidates.length === 1 ? "" : "S"} IN RANGE · NEED {GUARDIANS_N}. ASK THEM TO OPEN CABALMESH NEARBY.
        </span>
      ) : null}

      {candidates && candidates.length > 0 && (
        <div style={column("var(--space-4)")}>
          {candidates.map((candidate) => (
            <div key={candidate.peerId} style={row}>
              <Checkbox
                label={`NODE-${candidate.label}`}
                description={candidate.hops <= 1 ? "IN RANGE NOW" : `${candidate.hops} HOPS AWAY`}
                checked={selected.has(candidate.peerId)}
                onChange={() => toggle(candidate.peerId)}
              />
              <StatusDot tone={candidate.hops <= 1 ? "online" : "idle"} />
            </div>
          ))}
        </div>
      )}

      <span style={label}>
        SELECTED {selected.size} / {GUARDIANS_N}
      </span>
      <div style={{ display: "flex", gap: "var(--space-4)" }}>
        <Button tone="ghost" size="md" className="cm-touch" onClick={onCancel}>
          CANCEL
        </Button>
        <Button
          tone="primary"
          size="md"
          block
          className="cm-touch"
          disabled={selected.size !== GUARDIANS_N}
          onClick={() => onContinue((candidates ?? []).filter((c) => selected.has(c.peerId)))}
        >
          CONTINUE
        </Button>
      </div>
    </div>
  );
}

const STAGE_COPY: Record<GuardianEnrollStage | "waiting", { text: string; fill: number }> = {
  waiting: { text: "WAITING", fill: 0 },
  invited: { text: "ASKING", fill: 1 },
  accepted: { text: "SENDING", fill: 2 },
  delivered: { text: "DELIVERED", fill: 3 },
  "no-reply": { text: "NO REPLY", fill: 0 },
};

function DistributingShares({ guardians, onDone }: { guardians: GuardianCandidate[]; onDone: (enrolled: boolean) => void }) {
  const [stages, setStages] = useState<Record<string, GuardianEnrollStage>>({});
  const [result, setResult] = useState<GuardianEnrollResult | null>(null);
  const [error, setError] = useState<string | null>(null);
  const started = useRef(false);

  useTauriEvent<GuardianEnrollProgress>("guardian-enroll-progress", (progress) => {
    setStages((previous) => ({ ...previous, [progress.peerId]: progress.stage }));
  });

  useEffect(() => {
    if (started.current) return;
    started.current = true;
    invoke<GuardianEnrollResult>("guardian_enroll", { peerIds: guardians.map((g) => g.peerId), threshold: GUARDIANS_K })
      .then(setResult)
      .catch((failure) => setError(errorCopy(failure)));
  }, [guardians]);

  const delivered = guardians.filter((g) => stages[g.peerId] === "delivered").length;
  const finished = result !== null || error !== null;

  return (
    <div style={column("var(--space-5)")}>
      <span style={heading}>DISTRIBUTING SHARES</span>
      {guardians.map((guardian) => {
        const stage = STAGE_COPY[stages[guardian.peerId] ?? "waiting"];
        return (
          <div key={guardian.peerId} style={row}>
            <span style={data}>NODE-{guardian.label}</span>
            <div style={{ flex: 1, maxWidth: 140 }}>
              <Meter value={stage.fill} max={3} segments={12} tone={stage.fill === 3 ? "success" : "info"} />
            </div>
            <span style={{ ...label, minWidth: 88, textAlign: "right" }}>{stage.text}</span>
          </div>
        );
      })}

      <span style={label}>
        {delivered} OF {guardians.length} DELIVERED
        {delivered >= GUARDIANS_K ? " — ENOUGH TO UNLOCK." : "."}
      </span>
      {result && result.enrolled.length < GUARDIANS_K && (
        <span style={alert}>
          ONLY {result.enrolled.length} ACCEPTED — FEWER THAN {GUARDIANS_K}, SO THIS ENROLLMENT CANNOT UNLOCK. TRY AGAIN WITH NODES THAT ARE IN RANGE.
        </span>
      )}
      <ErrorLine error={error} />
      {finished && (
        <Button
          tone="primary"
          size="md"
          className="cm-touch"
          onClick={() => onDone(result !== null && result.enrolled.length >= GUARDIANS_K)}
        >
          CONTINUE
        </Button>
      )}
    </div>
  );
}

function RestoreCode({ onDone }: { onDone: () => void }) {
  const [code, setCode] = useState<string | null>(null);
  const [error, setError] = useState<string | null>(null);

  useEffect(() => {
    invoke<string | null>("guardian_restore_code")
      .then(setCode)
      .catch((failure) => setError(errorCopy(failure)));
  }, []);

  return (
    <div style={column("var(--space-5)")}>
      <span style={heading}>RESTORE CODE</span>
      <span style={label}>
        WRITE THIS DOWN AND KEEP IT OFF THIS DEVICE. ON A NEW DEVICE, THIS CODE PLUS {GUARDIANS_K} GUARDIANS RESTORES YOUR VAULT.
      </span>
      {code && (
        <code style={{ ...data, wordBreak: "break-all", userSelect: "all", lineHeight: 1.6 }} aria-label="Restore code">
          {code}
        </code>
      )}
      <span style={alert}>⚠ ON ITS OWN THIS CODE OPENS NOTHING. RE-ENROLLING REPLACES IT, AND THE OLD ONE STOPS WORKING.</span>
      <ErrorLine error={error} />
      <Button tone="primary" size="md" className="cm-touch" onClick={onDone}>
        DONE
      </Button>
    </div>
  );
}

// ---------------------------------------------------------------------------
// VAULT → KEYS → ADVANCED → RESTORE FROM GUARDIANS (new device)
// ---------------------------------------------------------------------------

const POLL_MS = 10_000;

export function RestoreFromGuardians() {
  const [code, setCode] = useState("");
  const [confirming, setConfirming] = useState(false);
  const [progress, setProgress] = useState<GuardianRestoreProgress | null>(null);
  const [busy, setBusy] = useState(false);
  const [error, setError] = useState<string | null>(null);

  const active = progress?.state === "collecting";

  // Resume a restore started before the app was closed: guardians approved
  // that attempt, and its delay is already running.
  useEffect(() => {
    if (!inTauri()) return;
    invoke<GuardianRestoreProgress>("guardian_restore_poll")
      .then((p) => p.state !== "none" && setProgress(p))
      .catch(() => {});
  }, []);

  useEffect(() => {
    if (!active) return;
    let cancelled = false;
    const tick = async () => {
      try {
        const next = await invoke<GuardianRestoreProgress>("guardian_restore_poll");
        if (!cancelled) setProgress(next);
      } catch (failure) {
        if (!cancelled) setError(errorCopy(failure));
      }
    };
    const timer = window.setInterval(() => void tick(), POLL_MS);
    return () => {
      cancelled = true;
      window.clearInterval(timer);
    };
  }, [active]);

  async function start() {
    setBusy(true);
    setError(null);
    try {
      setProgress(await invoke<GuardianRestoreProgress>("guardian_restore_start", { code }));
      setConfirming(false);
    } catch (failure) {
      setError(errorCopy(failure));
    } finally {
      setBusy(false);
    }
  }

  async function abandon() {
    await invoke("guardian_restore_abandon").catch(() => {});
    setProgress(null);
  }

  const shell = (children: ReactNode) => (
    <div style={column("var(--space-4)")}>
      <span style={heading}>RESTORE FROM GUARDIANS</span>
      {children}
    </div>
  );

  if (progress?.state === "done") {
    return shell(<span style={label}>RESTORED. THIS DEVICE NOW HOLDS YOUR VAULT. SET UP GUARDIANS AGAIN FROM HERE — THE OLD ENROLLMENT BELONGED TO THE LOST DEVICE.</span>);
  }

  if (active && progress) {
    const need = progress.threshold || GUARDIANS_K;
    return shell(
      <>
        <span style={label}>STAND NEAR {need} OF YOUR GUARDIANS AND ASK THEM TO OPEN CABALMESH.</span>
        <div style={row}>
          <span style={data}>
            ◇ {progress.released} / {need}
          </span>
          <span style={label}>SHARES RELEASED</span>
        </div>
        {progress.waiting > 0 && progress.waitingUntil && (
          <span style={label}>
            {progress.waiting} APPROVED · SAFETY WAIT ENDS {untilCopy(progress.waitingUntil)}. YOUR OLD DEVICE CAN CANCEL UNTIL THEN.
          </span>
        )}
        {progress.cancelled > 0 && (
          <span style={alert}>⚠ {progress.cancelled} GUARDIAN{progress.cancelled === 1 ? "" : "S"} REPORT THE ORIGINAL DEVICE CANCELLED THIS RESTORE.</span>
        )}
        <span style={label}>SCANNING… KEEP THIS SCREEN OPEN.</span>
        <ErrorLine error={error} />
        <Button tone="ghost" size="md" className="cm-touch" onClick={() => void abandon()}>
          CANCEL
        </Button>
      </>,
    );
  }

  return shell(
    <>
      <span style={alert}>⚠ THIS REPLACES THE CURRENT VAULT. EXPORT ITS KEY FIRST IF IT HOLDS ANYTHING. (THE OLD VAULT FILE IS KEPT BESIDE THE NEW ONE.)</span>
      <Field label="RESTORE CODE" htmlFor="restore-code" error={error ?? undefined}>
        <Input
          id="restore-code"
          value={code}
          autoComplete="off"
          spellCheck={false}
          placeholder="XXXX-XXXX-…"
          onChange={(event) => setCode((event.target as HTMLInputElement).value)}
        />
      </Field>
      <Button
        tone={confirming ? "danger" : "secondary"}
        size="md"
        className="cm-touch"
        disabled={busy || code.trim().length < 20}
        onClick={() => (confirming ? void start() : setConfirming(true))}
      >
        {busy ? "ASKING GUARDIANS…" : confirming ? "CONFIRM — REPLACE VAULT" : "RESTORE"}
      </Button>
    </>,
  );
}

// ---------------------------------------------------------------------------
// Lock screen: MESH UNLOCK (owner side)
// ---------------------------------------------------------------------------

export function MeshUnlock({ onUnlocked }: { onUnlocked: () => void }) {
  const [enrolled, setEnrolled] = useState(false);
  const [progress, setProgress] = useState<GuardianUnlockProgress | null>(null);
  const [busy, setBusy] = useState(false);
  const [error, setError] = useState<string | null>(null);

  useEffect(() => {
    if (!inTauri()) return;
    invoke<GuardianStatus>("guardian_status")
      .then((s) => setEnrolled(s.enrolled))
      .catch(() => setEnrolled(false));
  }, []);

  useTauriEvent<GuardianUnlockProgress>("guardian-unlock-progress", setProgress);

  if (!enrolled) return null;

  async function ask() {
    setBusy(true);
    setError(null);
    setProgress(null);
    try {
      await invoke("guardian_request_unlock");
      onUnlocked();
    } catch (failure) {
      setError(errorCopy(failure));
    } finally {
      setBusy(false);
    }
  }

  return (
    <div style={column("var(--space-4)")}>
      {busy && progress && (
        <div style={{ ...column("var(--space-2)"), alignItems: "center" }}>
          <span style={data}>
            ◇ {progress.approved} / {progress.threshold}
          </span>
          <span style={label}>NODES APPROVED</span>
          <span style={label}>
            {progress.approved < progress.threshold ? `WAITING FOR ${progress.threshold - progress.approved} MORE.` : "OPENING…"}
          </span>
        </div>
      )}
      <ErrorLine error={error} />
      <Button tone="secondary" size="md" block className="cm-touch" disabled={busy} onClick={() => void ask()}>
        {busy ? "ASKING NEARBY GUARDIANS…" : "UNLOCK WITH GUARDIANS"}
      </Button>
    </div>
  );
}

// ---------------------------------------------------------------------------
// Original device: a restore of this vault is pending somewhere.
// ---------------------------------------------------------------------------

export function RestorePendingAlert() {
  const [notice, setNotice] = useState<GuardianRestorePendingNotice | null>(null);
  const [dismissedUntil, setDismissedUntil] = useState<string | null>(null);
  const [busy, setBusy] = useState(false);
  const [result, setResult] = useState<string | null>(null);

  useTauriEvent<GuardianRestorePendingNotice>("guardian-restore-pending", (next) => {
    // Several guardians announce the same restore; one prompt is enough,
    // and "it's me" silences re-announcements of that same restore.
    if (next.releaseAt === dismissedUntil) return;
    setNotice(next);
  });

  async function veto() {
    setBusy(true);
    try {
      await invoke("guardian_cancel_restore");
      setResult("CANCEL SENT TO GUARDIANS IN RANGE. KEEP THE APP OPEN NEAR THEM UNTIL THE WARNING STOPS.");
    } catch (failure) {
      setResult(errorCopy(failure));
    } finally {
      setBusy(false);
    }
  }

  const close = () => {
    if (notice) setDismissedUntil(notice.releaseAt);
    setNotice(null);
    setResult(null);
  };

  return (
    <ModalDialog open={notice !== null} title="RESTORE IN PROGRESS" onClose={close}>
      <div style={{ ...column("var(--space-6)"), padding: "var(--space-2) 0" }}>
        <span style={heading}>SOMEONE IS RESTORING YOUR VAULT ON ANOTHER DEVICE.</span>
        {notice && <span style={label}>IT COMPLETES {untilCopy(notice.releaseAt)}.</span>}
        <span style={alert}>⚠ IF THIS ISN'T YOU, CANCEL NOW — THEY HAVE YOUR RESTORE CODE AND HAVE PERSUADED YOUR GUARDIANS.</span>
        {result && <span style={label}>{result}</span>}
        <div style={{ display: "flex", gap: "var(--space-4)" }}>
          <Button tone="ghost" size="md" block className="cm-touch" disabled={busy} onClick={close}>
            IT'S ME
          </Button>
          <Button tone="danger" size="md" block className="cm-touch" disabled={busy} onClick={() => void veto()}>
            {busy ? "CANCELLING…" : "CANCEL IT"}
          </Button>
        </div>
      </div>
    </ModalDialog>
  );
}

// ---------------------------------------------------------------------------
// Home: risk-driven backup prompt
// ---------------------------------------------------------------------------

const DISMISS_KEY = "cabalmesh.backup-suggestion.dismissed";

export function BackupSuggestion({ onSetUp }: { onSetUp: () => void }) {
  const [show, setShow] = useState(false);

  useEffect(() => {
    if (!inTauri()) return;
    let dismissed = false;
    try {
      dismissed = localStorage.getItem(DISMISS_KEY) === "1";
    } catch {
      // Storage unavailable: show it; dismissing then lasts for the session.
    }
    if (dismissed) return;
    invoke<GuardianStatus>("guardian_status")
      .then((s) => setShow(s.suggestBackup))
      .catch(() => setShow(false));
  }, []);

  if (!show) return null;

  const dismiss = () => {
    try {
      localStorage.setItem(DISMISS_KEY, "1");
    } catch {
      // Session-only dismissal.
    }
    setShow(false);
  };

  return (
    <Panel label="BACKUP">
      <div style={{ padding: "var(--space-6)", ...column("var(--space-4)") }}>
        <span style={heading}>YOUR WALLET NOW HOLDS REAL VALUE BUT IS ONLY BACKED UP ON THIS DEVICE.</span>
        <span style={label}>SET UP {GUARDIANS_K}-OF-{GUARDIANS_N} GUARDIANS SO A LOST DEVICE ISN'T LOST FUNDS.</span>
        <div style={{ display: "flex", gap: "var(--space-4)" }}>
          <Button tone="ghost" size="md" className="cm-touch" onClick={dismiss}>
            LATER
          </Button>
          <Button tone="primary" size="md" className="cm-touch" onClick={onSetUp}>
            SET UP GUARDIANS
          </Button>
        </div>
      </div>
    </Panel>
  );
}
