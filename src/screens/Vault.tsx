import { useEffect, useState } from "react";
import { invoke } from "@tauri-apps/api/core";
import { Button, Field, Input, Panel } from "../ds";
import { GuardianPanel, RestoreFromGuardians } from "./guardians";
import { errorCopy } from "../state/errorCopy";
import { VaultAssets } from "./VaultAssets";
import type { VaultTab } from "../shell/screen";
import type {
  SecurityStatus,
  VaultRow,
} from "../types/bindings";

const MIN_PASSPHRASE_LEN = 8;

const TABS: VaultTab[] = ["ASSETS", "IDENTITIES", "KEYS"];

const COMMAND: Record<VaultTab, string> = {
  ASSETS: "vault_assets",
  IDENTITIES: "vault_identities",
  KEYS: "vault_keys",
};

/**
 * Assets, identities and key metadata.
 *
 * The KEYS tab never renders key material. It describes what is held and where;
 * the values stay in the encrypted vault. That is the promise the screen's own
 * copy makes, and the command keeps it too.
 */
export function Vault({
  tab,
  onTabChange,
}: {
  tab: VaultTab;
  onTabChange: (tab: VaultTab) => void;
}) {
  const [rows, setRows] = useState<VaultRow[] | null>(null);
  const [security, setSecurity] = useState<SecurityStatus | null>(null);

  useEffect(() => {
    if (tab === "ASSETS") return;
    let cancelled = false;
    setRows(null);
    invoke<VaultRow[]>(COMMAND[tab])
      .then((next) => {
        if (!cancelled) setRows(next);
      })
      .catch(() => {
        if (!cancelled) setRows([]);
      });
    return () => {
      cancelled = true;
    };
  }, [tab]);

  // Only needed on KEYS, where SECURITY lives — cheap enough not to bother
  // gating the fetch on the tab, and it keeps the panel's state correct if
  // the user switches to KEYS after the screen already mounted.
  const refreshSecurity = () => {
    invoke<SecurityStatus>("security_status")
      .then(setSecurity)
      .catch(() => setSecurity(null));
  };
  useEffect(refreshSecurity, []);

  return (
    <div style={{ display: "flex", flexDirection: "column", gap: "var(--space-6)", padding: "var(--space-6)" }}>
      <div role="tablist" aria-label="Vault section" style={{ display: "flex", flexWrap: "wrap", gap: "var(--space-7)" }}>
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

      {tab === "ASSETS" ? (
        <VaultAssets />
      ) : (
        <Panel label={tab}>
          {rows === null ? null : rows.length === 0 ? (
            <Empty tab={tab} />
          ) : (
            rows.map((row) => <Row key={`${row.tag}-${row.name}-${row.detail ?? ""}`} row={row} />)
          )}
        </Panel>
      )}

      {tab === "KEYS" && (
        <SecurityPanel
          status={security}
          onChanged={() => {
            refreshSecurity();
            invoke<VaultRow[]>("vault_keys").then(setRows).catch(() => {});
          }}
        />
      )}

      {tab === "KEYS" && <GuardianPanel />}

      {tab === "KEYS" && <AdvancedPanel />}
    </div>
  );
}

/**
 * Turns passphrase protection on or off. Lives on KEYS because that is where
 * the `VAULT KEY` row already lives — see `docs/identity-design.md`'s
 * `SECURITY` mock-up, which this is a first, unstyled pass at rather than a
 * literal implementation of every row it draws (mesh unlock, guardians).
 */
function SecurityPanel({ status, onChanged }: { status: SecurityStatus | null; onChanged: () => void }) {
  const [mode, setMode] = useState<"idle" | "enabling" | "confirming-disable">("idle");
  const [passphrase, setPassphrase] = useState("");
  const [confirm, setConfirm] = useState("");
  const [busy, setBusy] = useState(false);
  const [error, setError] = useState<string | null>(null);

  if (status === null) return null;

  function reset() {
    setMode("idle");
    setPassphrase("");
    setConfirm("");
    setError(null);
  }

  async function enable() {
    setBusy(true);
    setError(null);
    try {
      await invoke("security_enable_passphrase", { passphrase });
      reset();
      onChanged();
    } catch (failure) {
      setError(errorCopy(failure));
    } finally {
      setBusy(false);
    }
  }

  async function disable() {
    setBusy(true);
    setError(null);
    try {
      await invoke("security_disable_passphrase");
      reset();
      onChanged();
    } catch (failure) {
      setError(errorCopy(failure));
      setBusy(false);
    }
  }

  return (
    <Panel label="SECURITY">
      <div style={{ padding: "var(--space-6)", display: "flex", flexDirection: "column", gap: "var(--space-6)" }}>
        <div
          className="cm-row"
          style={{ display: "flex", alignItems: "center", justifyContent: "space-between", gap: "var(--space-5)" }}
        >
          <div style={{ display: "flex", flexDirection: "column", gap: "var(--space-2)" }}>
            <span
              style={{
                fontFamily: "var(--type-heading-family)",
                fontSize: "var(--text-sm)",
                letterSpacing: "var(--type-heading-tracking)",
                color: "var(--text-primary)",
              }}
            >
              PASSPHRASE UNLOCK
            </span>
            <span
              style={{
                fontFamily: "var(--type-label-family)",
                fontSize: "var(--text-2xs)",
                letterSpacing: "var(--tracking-widest)",
                color: "var(--text-muted)",
              }}
            >
              {status.passphraseEnabled ? "ENABLED — REQUIRED EVERY LAUNCH" : "OFF — DEFAULT"}
            </span>
          </div>

          {status.passphraseEnabled ? (
            mode === "confirming-disable" ? (
              <div style={{ display: "flex", gap: "var(--space-4)" }}>
                <Button tone="ghost" size="sm" className="cm-touch" disabled={busy} onClick={reset}>
                  CANCEL
                </Button>
                <Button tone="danger" size="sm" className="cm-touch" disabled={busy} onClick={() => void disable()}>
                  {busy ? "REMOVING…" : "CONFIRM"}
                </Button>
              </div>
            ) : (
              <Button
                tone="ghost"
                size="sm"
                className="cm-touch"
                onClick={() => setMode("confirming-disable")}
              >
                REMOVE
              </Button>
            )
          ) : mode === "enabling" ? null : (
            <Button tone="secondary" size="sm" className="cm-touch" onClick={() => setMode("enabling")}>
              SET UP
            </Button>
          )}
        </div>

        {!status.passphraseEnabled && mode === "enabling" && (
          <form
            onSubmit={(event) => {
              event.preventDefault();
              void enable();
            }}
            style={{ display: "flex", flexDirection: "column", gap: "var(--space-5)" }}
          >
            <Field label="NEW PASSPHRASE" htmlFor="security-passphrase" hint={`AT LEAST ${MIN_PASSPHRASE_LEN} CHARACTERS`}>
              <Input
                id="security-passphrase"
                type="password"
                autoComplete="new-password"
                autoFocus
                value={passphrase}
                onChange={(event) => setPassphrase((event.target as HTMLInputElement).value)}
              />
            </Field>
            <Field label="CONFIRM PASSPHRASE" htmlFor="security-passphrase-confirm" error={error ?? undefined}>
              <Input
                id="security-passphrase-confirm"
                type="password"
                autoComplete="new-password"
                invalid={!!error}
                value={confirm}
                onChange={(event) => setConfirm((event.target as HTMLInputElement).value)}
              />
            </Field>
            <div style={{ display: "flex", gap: "var(--space-4)" }}>
              <Button tone="ghost" size="md" className="cm-touch" disabled={busy} onClick={reset}>
                CANCEL
              </Button>
              <Button
                tone="primary"
                size="md"
                className="cm-touch"
                type="submit"
                disabled={busy || passphrase.length < MIN_PASSPHRASE_LEN || passphrase !== confirm}
              >
                {busy ? "SETTING UP…" : "CONFIRM"}
              </Button>
            </div>
          </form>
        )}

        {error && mode !== "enabling" && (
          <span
            style={{
              fontFamily: "var(--type-label-family)",
              fontSize: "var(--text-2xs)",
              letterSpacing: "var(--tracking-wide)",
              color: "var(--accent-blood-red)",
              textTransform: "uppercase",
            }}
          >
            {error}
          </span>
        )}
      </div>
    </Panel>
  );
}

function Row({ row }: { row: VaultRow }) {
  return (
    <div
      className="cm-row"
      style={{
        display: "flex",
        alignItems: "center",
        gap: "var(--space-5)",
        padding: "var(--space-5) var(--space-6)",
        borderTop: "var(--border-hairline-style)",
      }}
    >
      <span
        aria-hidden="true"
        style={{
          fontFamily: "var(--type-label-family)",
          fontSize: "var(--text-2xs)",
          letterSpacing: "var(--tracking-wider)",
          color: "var(--text-muted)",
          border: "var(--border-hairline-style)",
          padding: "var(--space-2) var(--space-3)",
        }}
      >
        {row.tag}
      </span>

      <div style={{ flex: 1, display: "flex", flexDirection: "column", gap: "var(--space-2)" }}>
        <span
          style={{
            fontFamily: "var(--type-heading-family)",
            fontSize: "var(--text-sm)",
            letterSpacing: "var(--type-heading-tracking)",
            color: "var(--text-primary)",
          }}
        >
          {row.name}
        </span>
        {row.detail ? (
          <span
            style={{
              fontFamily: "var(--type-label-family)",
              fontSize: "var(--text-2xs)",
              letterSpacing: "var(--tracking-widest)",
              color: "var(--text-muted)",
            }}
          >
            {row.detail}
          </span>
        ) : null}
      </div>

      <span
        style={{
          fontFamily: "var(--type-data-family)",
          fontSize: "var(--text-sm)",
          letterSpacing: "var(--type-data-tracking)",
          color: "var(--text-secondary)",
        }}
      >
        {row.amount}
      </span>
    </div>
  );
}

function Empty({ tab }: { tab: VaultTab }) {
  const body =
    tab === "ASSETS"
      ? "Nothing is held. Nothing is stored."
      : tab === "IDENTITIES"
        ? "No identity exists yet."
        : "No key material is held.";

  return (
    <div style={{ padding: "var(--space-9) var(--space-6)", textAlign: "center" }}>
      <span style={{ fontSize: "var(--text-base)", color: "var(--text-muted)" }}>{body}</span>
    </div>
  );
}

/**
 * Export the current wallet's raw key, or replace the wallet with one
 * derived from a supplied key. See `docs/identity-design.md`, "Where things
 * stand": until this existed, losing the device meant losing the wallet with
 * no recourse — `get_primary_private_key` / `import_identity` already lived
 * in the bridge but nothing on the command surface reached them.
 */
function AdvancedPanel() {
  const [open, setOpen] = useState(false);

  return (
    <Panel label="ADVANCED">
      <div style={{ padding: "var(--space-6)", display: "flex", flexDirection: "column", gap: "var(--space-6)" }}>
        {!open ? (
          <Button tone="ghost" size="sm" className="cm-touch" onClick={() => setOpen(true)}>
            EXPORT · IMPORT · RESTORE
          </Button>
        ) : (
          <>
            <ExportKey />
            <ImportKey />
            <RestoreFromGuardians />
          </>
        )}
      </div>
    </Panel>
  );
}

function ExportKey() {
  const [key, setKey] = useState<string | null>(null);
  const [error, setError] = useState<string | null>(null);
  const [busy, setBusy] = useState(false);

  async function reveal() {
    setBusy(true);
    setError(null);
    try {
      setKey(await invoke<string>("vault_export_key"));
    } catch (failure) {
      setError(errorCopy(failure));
    } finally {
      setBusy(false);
    }
  }

  return (
    <div style={{ display: "flex", flexDirection: "column", gap: "var(--space-4)" }}>
      <span
        style={{
          fontFamily: "var(--type-heading-family)",
          fontSize: "var(--text-sm)",
          letterSpacing: "var(--type-heading-tracking)",
          color: "var(--text-primary)",
        }}
      >
        EXPORT KEY
      </span>

      {key === null ? (
        <>
          <span
            style={{
              fontFamily: "var(--type-label-family)",
              fontSize: "var(--text-2xs)",
              letterSpacing: "var(--tracking-wide)",
              color: "var(--text-muted)",
            }}
          >
            Anyone holding this key spends everything it controls.
          </span>
          <Button tone="secondary" size="sm" className="cm-touch" disabled={busy} onClick={() => void reveal()}>
            {busy ? "REVEALING…" : "REVEAL"}
          </Button>
        </>
      ) : (
        <div
          className="cm-row"
          style={{
            padding: "var(--space-4)",
            fontFamily: "var(--type-data-family)",
            fontSize: "var(--text-2xs)",
            letterSpacing: "var(--type-data-tracking)",
            color: "var(--text-primary)",
            wordBreak: "break-all",
          }}
        >
          {key}
        </div>
      )}

      {error && (
        <span
          style={{
            fontFamily: "var(--type-label-family)",
            fontSize: "var(--text-2xs)",
            letterSpacing: "var(--tracking-wide)",
            color: "var(--accent-blood-red)",
            textTransform: "uppercase",
          }}
        >
          {error}
        </span>
      )}
    </div>
  );
}

function ImportKey() {
  const [confirming, setConfirming] = useState(false);
  const [privateKey, setPrivateKey] = useState("");
  const [alias, setAlias] = useState("");
  const [accountId, setAccountId] = useState("");
  const [busy, setBusy] = useState(false);
  const [error, setError] = useState<string | null>(null);
  const [done, setDone] = useState(false);

  async function submit() {
    setBusy(true);
    setError(null);
    try {
      await invoke("vault_import_key", { privateKeyHex: privateKey, alias: alias || "Imported Fox", emoji: "🦊", accountId: accountId || null });
      setPrivateKey("");
      setAlias("");
      setAccountId("");
      setConfirming(false);
      setDone(true);
    } catch (failure) {
      setError(errorCopy(failure));
    } finally {
      setBusy(false);
    }
  }

  return (
    <div style={{ display: "flex", flexDirection: "column", gap: "var(--space-4)" }}>
      <span
        style={{
          fontFamily: "var(--type-heading-family)",
          fontSize: "var(--text-sm)",
          letterSpacing: "var(--type-heading-tracking)",
          color: "var(--text-primary)",
        }}
      >
        IMPORT KEY
      </span>
      <span
        style={{
          fontFamily: "var(--type-label-family)",
          fontSize: "var(--text-2xs)",
          letterSpacing: "var(--tracking-wide)",
          color: "var(--text-muted)",
        }}
      >
        This replaces the current NEAR account. Export its key first if it holds anything.
      </span>

      <form
        onSubmit={(event) => {
          event.preventDefault();
          if (confirming) void submit();
          else setConfirming(true);
        }}
        style={{ display: "flex", flexDirection: "column", gap: "var(--space-4)" }}
      >
        <Field label="NEAR SECRET KEY (64 HEX)" htmlFor="import-private-key">
          <Input
            id="import-private-key"
            type="password"
            autoComplete="off"
            value={privateKey}
            onChange={(event) => {
              setPrivateKey((event.target as HTMLInputElement).value);
              setConfirming(false);
            }}
          />
        </Field>
        <Field label="TESTNET ACCOUNT (IF NAMED)" htmlFor="import-account">
          <Input id="import-account" placeholder="your-name.testnet" autoCapitalize="none" autoComplete="off" spellCheck={false} value={accountId} onChange={(event) => {
            setAccountId((event.target as HTMLInputElement).value.toLowerCase());
            setConfirming(false);
          }} />
        </Field>
        <Field label="ALIAS (OPTIONAL)" htmlFor="import-alias" error={error ?? undefined}>
          <Input
            id="import-alias"
            autoComplete="off"
            value={alias}
            onChange={(event) => {
              setAlias((event.target as HTMLInputElement).value);
              setConfirming(false);
            }}
          />
        </Field>

        <Button
          tone={confirming ? "danger" : "primary"}
          size="md"
          className="cm-touch"
          type="submit"
          disabled={busy || !privateKey}
        >
          {busy ? "IMPORTING…" : confirming ? "CONFIRM — REPLACE ACCOUNT" : "IMPORT"}
        </Button>
      </form>

      {done && (
        <span
          style={{
            fontFamily: "var(--type-label-family)",
            fontSize: "var(--text-2xs)",
            letterSpacing: "var(--tracking-widest)",
            color: "var(--text-muted)",
            textTransform: "uppercase",
          }}
        >
          Imported. Reopen VAULT to see it reflected everywhere.
        </span>
      )}
    </div>
  );
}
