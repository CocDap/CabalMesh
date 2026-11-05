import { useState } from "react";
import { invoke } from "@tauri-apps/api/core";
import { Button, Field, Input, Logo } from "../ds";
import { errorCopy } from "../state/errorCopy";

/**
 * Shown instead of Home when `security_status` reports the vault locked.
 *
 * Only reachable at all if the user opted into passphrase protection from
 * `VAULT → KEYS → SECURITY` — every install still defaults to the
 * zero-friction boot straight to Home. See `docs/identity-design.md`,
 * decision 1.
 */
export function Unlock({ onUnlocked }: { onUnlocked: () => void }) {
  const [passphrase, setPassphrase] = useState("");
  const [busy, setBusy] = useState(false);
  const [error, setError] = useState<string | null>(null);

  async function submit() {
    if (!passphrase || busy) return;
    setBusy(true);
    setError(null);
    try {
      await invoke("security_unlock", { passphrase });
      onUnlocked();
    } catch (failure) {
      setError(errorCopy(failure));
      setBusy(false);
    }
  }

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
      <Logo variant="minimal" size={56} basePath="/ds-assets/logo" />

      <div>
        <h1
          style={{
            margin: 0,
            fontFamily: "var(--type-heading-family)",
            fontSize: "var(--text-lg)",
            letterSpacing: "var(--type-heading-tracking)",
            color: "var(--text-primary)",
          }}
        >
          VAULT LOCKED
        </h1>
        <p
          style={{
            marginTop: "var(--space-4)",
            fontFamily: "var(--type-label-family)",
            fontSize: "var(--text-2xs)",
            letterSpacing: "var(--tracking-widest)",
            color: "var(--text-muted)",
            textTransform: "uppercase",
          }}
        >
          Enter the passphrase to open it.
        </p>
      </div>

      <form
        onSubmit={(event) => {
          event.preventDefault();
          void submit();
        }}
        style={{ display: "flex", flexDirection: "column", gap: "var(--space-6)", width: "100%", maxWidth: 320 }}
      >
        <Field label="PASSPHRASE" htmlFor="unlock-passphrase" error={error ?? undefined}>
          <Input
            id="unlock-passphrase"
            type="password"
            autoComplete="current-password"
            autoFocus
            invalid={!!error}
            value={passphrase}
            onChange={(event) => {
              setPassphrase((event.target as HTMLInputElement).value);
              if (error) setError(null);
            }}
          />
        </Field>

        <Button
          tone="primary"
          size="lg"
          block
          className="cm-touch"
          type="submit"
          disabled={busy || !passphrase}
        >
          {busy ? "UNLOCKING…" : "UNLOCK"}
        </Button>
      </form>
    </section>
  );
}
