import { useEffect, useRef, useState } from "react";
import { invoke } from "@tauri-apps/api/core";
import { Button, Field, Input } from "../ds";
import { walletNameError } from "../state/walletName";
import { walletSetupError, type WalletNameStatus, type WalletSetupStatus } from "../types/wallet";
import "./wallet-setup.css";

export function WalletSetup({ onComplete, onCancel }: { onComplete: () => void; onCancel: () => void }) {
  const [status, setStatus] = useState<WalletSetupStatus | null>(null);
  const [name, setName] = useState("");
  const [availability, setAvailability] = useState<WalletNameStatus | "checking" | null>(null);
  const [error, setError] = useState<string | null>(null);
  const [checkingError, setCheckingError] = useState<string | null>(null);
  const [busy, setBusy] = useState(false);
  const [created, setCreated] = useState<string | null>(null);
  const [reload, setReload] = useState(0);
  const titleRef = useRef<HTMLHeadingElement>(null);
  const submitted = useRef(false);
  const invalid = name ? walletNameError(name) : null;
  const pending = status?.pendingAccountId;

  useEffect(() => { titleRef.current?.focus(); }, []);

  useEffect(() => {
    let cancelled = false;
    let timer: ReturnType<typeof setTimeout>;
    async function load(attempt = 0) {
      try {
        const next = await invoke<WalletSetupStatus>("wallet_setup_status");
        if (cancelled) return;
        setStatus(next);
        if (!status) setError(null);
        if (next.pendingAccountId) setName(next.pendingAccountId.replace(/\.testnet$/, ""));
      } catch (failure) {
        if (cancelled) return;
        if (failure === "not_ready" && attempt < 30) {
          timer = setTimeout(() => void load(attempt + 1), 1000);
        } else { setError(walletSetupError(failure)); }
      }
    }
    void load();
    return () => { cancelled = true; clearTimeout(timer); };
  }, [reload]);

  useEffect(() => {
    if (!status?.testnet || !name || invalid || busy || created) return;
    let cancelled = false;
    const timer = setTimeout(() => {
      setAvailability("checking");
      invoke<WalletNameStatus>("wallet_check_name", { name })
        .then((result) => { if (!cancelled) { setAvailability(result); setCheckingError(null); } })
        .catch((failure) => { if (!cancelled) { setAvailability(null); setCheckingError(walletSetupError(failure)); } });
    }, 600);
    return () => { cancelled = true; clearTimeout(timer); };
  }, [name, invalid, status, busy, created, reload]);

  async function create() {
    if (submitted.current || !status?.testnet || walletNameError(name) || availability === "taken") return;
    submitted.current = true;
    setBusy(true);
    setError(null);
    try {
      setCreated(await invoke<string>("wallet_create_testnet_account", { name }));
    } catch (failure) {
      setError(walletSetupError(failure));
      setReload((value) => value + 1);
    } finally {
      submitted.current = false;
      setBusy(false);
    }
  }

  return (
    <section className="wallet-setup" aria-labelledby="wallet-setup-title">
      <div className="wallet-setup-body">
        <h1 id="wallet-setup-title" ref={titleRef} tabIndex={-1}>{created ? "Your wallet is ready." : "Name your wallet."}</h1>
        {created ? (
          <>
            <p className="wallet-setup-copy">Your account is on NEAR testnet and ready for Intents.</p>
            <div className="wallet-setup-created" role="status">{created}</div>
            <p className="wallet-setup-copy">Back up your wallet key in Vault → Keys. Your account name is public.</p>
            <Button tone="primary" size="lg" block onClick={onComplete}>OPEN VAULT</Button>
          </>
        ) : (
          <>
            <p className="wallet-setup-copy">Choose the account name you’ll use to receive tokens and trade. Your key stays on this device.</p>
            {pending && <p className="wallet-setup-notice">Setup for {pending} is unfinished. Retry to continue with the same saved key.</p>}
            {status && !status.required && !pending && <p className="wallet-setup-copy">This creates a named account. It does not rename or move balances from your existing address.</p>}
            <form className="wallet-setup-form" onSubmit={(event) => { event.preventDefault(); void create(); }} aria-busy={busy}>
              <Field label="WALLET NAME" htmlFor="wallet-name">
                <div className="wallet-name-field">
                  <Input id="wallet-name" value={name} placeholder="your-name" maxLength={56} autoCapitalize="none" autoCorrect="off" spellCheck={false} autoComplete="off" disabled={!status?.testnet || busy || !!pending} invalid={!!invalid || availability === "taken"} aria-describedby="wallet-name-help wallet-name-state" onChange={(event) => {
                    setName((event.target as HTMLInputElement).value.toLowerCase());
                    setAvailability(null);
                    setCheckingError(null);
                    setError(null);
                  }} />
                  <span>.testnet</span>
                </div>
              </Field>
              <p id="wallet-name-help" className="wallet-setup-hint">Lowercase letters, numbers, hyphens or underscores. Up to 56 characters.</p>
              <div id="wallet-name-state" className={`wallet-name-state${invalid || availability === "taken" ? " is-error" : ""}`} aria-live="polite">
                {invalid ?? (availability === "checking" ? "Checking name…" : availability === "available" ? `${name}.testnet is available.` : availability === "owned" ? "This account has your key. Continue to finish setup." : availability === "taken" ? "This name is taken. Choose another." : checkingError ?? (status ? "Your full address will end in .testnet." : "Loading your wallet…"))}
              </div>
              {error && <p className="wallet-setup-error" role="alert">{error}</p>}
              {status && !status.testnet && <p className="wallet-setup-error" role="alert">Named account creation is available on testnet in this build.</p>}
              {busy && <p className="wallet-setup-notice" role="status">Creating your account, checking its key and preparing Intents. Keep this screen open.</p>}
              <Button type="submit" tone="primary" size="lg" block disabled={!status?.testnet || busy || !name || !!invalid || availability === "taken" || availability === "checking"}>
                {busy ? "SETTING UP WALLET…" : pending || availability === "owned" ? "FINISH WALLET SETUP" : "CREATE TESTNET WALLET"}
              </Button>
            </form>
            {!status && error && <Button onClick={() => setReload((value) => value + 1)}>RETRY</Button>}
            <Button tone="ghost" disabled={busy} onClick={onCancel}>BACK</Button>
          </>
        )}
      </div>
    </section>
  );
}
