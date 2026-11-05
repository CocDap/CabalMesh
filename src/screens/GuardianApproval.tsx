import { useEffect, useState } from "react";
import { invoke } from "@tauri-apps/api/core";
import { listen } from "@tauri-apps/api/event";
import { Button } from "../ds";
import { ModalDialog } from "../shell/ModalDialog";
import type { GuardianUnlockPrompt } from "../types/bindings";

/**
 * Mounted once, for the whole app's lifetime — a guardian approval can
 * arrive on any screen, since it is driven by someone else's action, not
 * this device's navigation.
 *
 * This is the human gate `docs/identity-design.md` requires and the backend
 * enforces structurally: `guardian_actor::respond_to_guardian_traffic` never
 * sends a share back on its own. It only ever computes one and holds it,
 * waiting on `guardian_approve_unlock` / `guardian_deny_unlock` — which this
 * is the only caller of.
 */
export function GuardianApproval() {
  const [prompt, setPrompt] = useState<GuardianUnlockPrompt | null>(null);
  const [busy, setBusy] = useState(false);

  useEffect(() => {
    // `listen` reaches into `window.__TAURI_INTERNALS__`, which only exists
    // inside a real Tauri webview — guarded so the browser-only dev preview
    // (no Tauri runtime) can render every other screen instead of crashing
    // on mount.
    if (!("__TAURI_INTERNALS__" in window)) return;
    const unlisten = listen<GuardianUnlockPrompt>("guardian-unlock-request", (event) => {
      setPrompt(event.payload);
    });
    return () => {
      void unlisten.then((stop) => stop());
    };
  }, []);

  async function resolve(command: "guardian_approve_unlock" | "guardian_deny_unlock") {
    if (!prompt || busy) return;
    setBusy(true);
    try {
      await invoke(command, { id: prompt.id });
    } catch {
      // Nothing actionable — the id may already have expired or been
      // resolved elsewhere. Either way the prompt closes.
    } finally {
      setBusy(false);
      setPrompt(null);
    }
  }

  return (
    <ModalDialog open={prompt !== null} title="UNLOCK REQUEST" onClose={() => void resolve("guardian_deny_unlock")}>
      <div style={{ display: "flex", flexDirection: "column", gap: "var(--space-6)", padding: "var(--space-2) 0" }}>
        <div style={{ display: "flex", flexDirection: "column", gap: "var(--space-2)" }}>
          <span
            style={{
              fontFamily: "var(--type-data-family)",
              fontSize: "var(--text-base)",
              letterSpacing: "var(--type-data-tracking)",
              color: "var(--text-primary)",
            }}
          >
            NODE-{prompt?.from}
          </span>
          <span
            style={{
              fontFamily: "var(--type-label-family)",
              fontSize: "var(--text-2xs)",
              letterSpacing: "var(--tracking-widest)",
              color: "var(--text-muted)",
            }}
          >
            ASKS TO OPEN THEIR VAULT
          </span>
        </div>

        <span
          style={{
            fontFamily: "var(--type-label-family)",
            fontSize: "var(--text-2xs)",
            letterSpacing: "var(--tracking-wide)",
            color: "var(--accent-blood-red)",
          }}
        >
          ⚠ APPROVE ONLY IF YOU CAN SEE THEM.
        </span>

        <div style={{ display: "flex", gap: "var(--space-4)" }}>
          <Button
            tone="ghost"
            size="md"
            block
            className="cm-touch"
            disabled={busy}
            onClick={() => void resolve("guardian_deny_unlock")}
          >
            DENY
          </Button>
          <Button
            tone="primary"
            size="md"
            block
            className="cm-touch"
            disabled={busy}
            onClick={() => void resolve("guardian_approve_unlock")}
          >
            APPROVE
          </Button>
        </div>
      </div>
    </ModalDialog>
  );
}
