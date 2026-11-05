import { useEffect, useState } from "react";
import { invoke } from "@tauri-apps/api/core";
import { Badge, Button, Icon, Panel, Switch } from "../ds";
import type { GlyphName } from "../shell/screen";
import type { ProfileView } from "../types/bindings";
import { AiSettings } from "./AiSettings";

const ROWS: ReadonlyArray<{ label: string; icon: GlyphName }> = [
  { label: "ACHIEVEMENTS", icon: "reputation" },
  { label: "ACTIVITY LOG", icon: "log" },
  { label: "SECURITY", icon: "encrypt" },
  { label: "ABOUT CABAL MESH", icon: "mesh" },
];

/**
 * Identity, settings and leaving the mesh.
 *
 * **The settled row is a real count**, read from the intent ledger — the same
 * place the home tile and the intents list read from, so no two screens can
 * disagree. It replaced a mocked "reputation score" in ticket 39; the label
 * changed along with the value, because a count called a score is a figure
 * whose name promises more than its definition delivers.
 *
 * It needs no mesh: this is local history, not network state.
 *
 * The network is shown plainly, with testnet marked, so nobody mistakes a test
 * balance for a real one. Switchable via the MAINNET switch below it, but only
 * takes effect after a restart — see `toggleNetwork`.
 */
export function Profile({ onLeave }: { onLeave: () => void }) {
  const [profile, setProfile] = useState<ProfileView | null>(null);
  const [busy, setBusy] = useState(false);
  const [networkBusy, setNetworkBusy] = useState(false);
  // Switching only writes network.json, which the app reads once at startup,
  // so the change has nothing to apply to until the app restarts.
  const [restartNeeded, setRestartNeeded] = useState(false);

  useEffect(() => {
    let cancelled = false;

    // Polled, not fetched once. A single fetch at mount races bootstrap: the
    // screen can mount before services are published, and a one-shot call then
    // leaves every field showing an em dash for the rest of the session with
    // no way to recover.
    const refresh = () =>
      invoke<ProfileView>("profile_summary")
        .then((next) => {
          if (!cancelled) setProfile(next);
        })
        .catch(() => undefined);

    refresh();
    const timer = window.setInterval(refresh, 5_000);
    return () => {
      cancelled = true;
      window.clearInterval(timer);
    };
  }, []);

  const toggleNetwork = async () => {
    if (!profile || networkBusy) return;
    const goingMainnet = profile.isTestnet;
    setNetworkBusy(true);
    // Optimistic, like toggleOffline above — reverted on failure since
    // nothing was actually persisted.
    setProfile({ ...profile, isTestnet: !profile.isTestnet });
    try {
      await invoke("network_set", { mainnet: goingMainnet });
      setRestartNeeded(true);
    } catch {
      setProfile((previous) => (previous ? { ...previous, isTestnet: !previous.isTestnet } : previous));
    } finally {
      setNetworkBusy(false);
    }
  };

  const toggleOffline = async () => {
    if (!profile || busy) return;
    const next = !profile.offline;
    setBusy(true);
    // Optimistic, then reconciled: the switch must not feel laggy, but it also
    // must not claim a state the mesh refused.
    setProfile({ ...profile, offline: next });
    try {
      await invoke("set_offline_mode", { offline: next });
    } catch {
      setProfile({ ...profile, offline: !next });
    } finally {
      setBusy(false);
    }
  };

  return (
    <div style={{ display: "flex", flexDirection: "column", gap: "var(--space-6)", padding: "var(--space-6)" }}>
      <Panel label="IDENTITY">
        <div style={{ padding: "var(--space-6)", display: "flex", flexDirection: "column", gap: "var(--space-5)" }}>
          <Field label="NODE ID" value={profile?.nodeId ?? "—"} />
          <Field label="INTENTS SETTLED" value={profile?.settled ?? "—"} />
          <Field label="MEMBER SINCE" value={profile?.memberSince ?? "—"} />
          <div style={{ display: "flex", alignItems: "center", gap: "var(--space-4)" }}>
            <Badge tone={profile?.isTestnet ? "alert" : "quiet"} size="sm">
              {profile?.network ?? "—"}
            </Badge>
            {profile?.isTestnet ? (
              <span style={{ fontSize: "var(--text-2xs)", letterSpacing: "var(--tracking-widest)", color: "var(--text-muted)" }}>
                TEST FUNDS ONLY.
              </span>
            ) : null}
          </div>

          {profile && (
            <div
              className="cm-row"
              style={{ display: "flex", alignItems: "center", justifyContent: "space-between", gap: "var(--space-5)" }}
            >
              <span
                id="mainnet-label"
                style={{
                  fontFamily: "var(--type-label-family)",
                  fontSize: "var(--text-2xs)",
                  letterSpacing: "var(--tracking-widest)",
                  color: "var(--text-secondary)",
                }}
              >
                MAINNET — REAL FUNDS
              </span>
              <Switch
                checked={!profile.isTestnet}
                role="switch"
                aria-checked={!profile.isTestnet}
                aria-labelledby="mainnet-label"
                className="cm-touch"
                onClick={() => void toggleNetwork()}
              />
            </div>
          )}

          {restartNeeded && (
            <span
              style={{
                fontFamily: "var(--type-label-family)",
                fontSize: "var(--text-2xs)",
                letterSpacing: "var(--tracking-widest)",
                color: "var(--text-muted)",
                textTransform: "uppercase",
              }}
            >
              Restart CABAL MESH to apply.
            </span>
          )}
        </div>
      </Panel>

      <AiSettings />

      <Panel>
        {ROWS.map((row) => (
          <button
            key={row.label}
            type="button"
            className="cm-touch cm-row"
            style={{
              width: "100%",
              display: "flex",
              alignItems: "center",
              gap: "var(--space-5)",
              padding: "var(--space-5) var(--space-6)",
              borderTop: "var(--border-hairline-style)",
              background: "none",
              border: "none",
              cursor: "pointer",
              textAlign: "left",
            }}
          >
            <Icon name={row.icon} size={20} basePath="/ds-assets/icons" />
            <span
              style={{
                flex: 1,
                fontFamily: "var(--type-label-family)",
                fontSize: "var(--text-2xs)",
                letterSpacing: "var(--tracking-widest)",
                color: "var(--text-secondary)",
              }}
            >
              {row.label}
            </span>
            <span aria-hidden="true" style={{ color: "var(--text-muted)" }}>
              ›
            </span>
          </button>
        ))}

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
            id="offline-label"
            style={{
              flex: 1,
              fontFamily: "var(--type-label-family)",
              fontSize: "var(--text-2xs)",
              letterSpacing: "var(--tracking-widest)",
              color: "var(--text-secondary)",
            }}
          >
            OFFLINE MODE
          </span>
          {/* role=switch with aria-checked announces the resulting state
              rather than the action, which is what a screen reader user needs. */}
          <Switch
            checked={profile?.offline ?? false}
            role="switch"
            aria-checked={profile?.offline ?? false}
            aria-labelledby="offline-label"
            className="cm-touch"
            onClick={toggleOffline}
          />
        </div>
      </Panel>

      <Button tone="danger" size="lg" block className="cm-touch" onClick={onLeave}>
        LEAVE THE MESH
      </Button>

      <p
        style={{
          textAlign: "center",
          fontFamily: "var(--type-label-family)",
          fontSize: "var(--text-2xs)",
          letterSpacing: "var(--tracking-widest)",
          color: "var(--text-disabled)",
          textTransform: "uppercase",
        }}
      >
        We leave no identity, only traces.
      </p>
    </div>
  );
}

function Field({ label, value }: { label: string; value: string }) {
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
          letterSpacing: "var(--type-data-tracking)",
          color: "var(--text-primary)",
        }}
      >
        {value}
      </span>
    </div>
  );
}
