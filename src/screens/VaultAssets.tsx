import { useEffect, useRef, useState } from "react";
import { invoke } from "@tauri-apps/api/core";
import { Button, Field, Input, Panel } from "../ds";
import { errorCopy } from "../state/errorCopy";
import { transferAmountError } from "../state/vaultTransfer";
import type { NearAccountView, NearTokenView } from "../types/bindings";
import "./vault.css";

type TransferMode = "deposit" | "withdraw";

/** Account identity, holdings and token movement have separate task surfaces. */
export function VaultAssets() {
  const [account, setAccount] = useState<NearAccountView | null>(null);
  const [loading, setLoading] = useState(true);
  const [refreshing, setRefreshing] = useState(false);
  const [error, setError] = useState<string | null>(null);
  const [mode, setMode] = useState<TransferMode | null>(null);
  const actionRef = useRef<HTMLDivElement>(null);

  useEffect(() => {
    let cancelled = false;
    let timer: ReturnType<typeof setTimeout>;
    async function load(attempt = 0) {
      try {
        const next = await invoke<NearAccountView>("near_account");
        if (!cancelled) { setAccount(next); setLoading(false); }
      } catch (failure) {
        if (cancelled) return;
        if (isStarting(failure) && attempt < 30) {
          timer = setTimeout(() => void load(attempt + 1), 1000);
          return;
        }
        setError(errorCopy(failure));
        setLoading(false);
      }
    }
    void load();
    return () => { cancelled = true; clearTimeout(timer); };
  }, []);

  async function refresh() {
    if (refreshing) return;
    setRefreshing(true);
    setError(null);
    try {
      if (account) await invoke("vault_refresh_balance");
      setAccount(await invoke<NearAccountView>("near_account"));
    } catch (failure) {
      setError(errorCopy(failure));
    } finally {
      setRefreshing(false);
    }
  }

  function closeTransfer() {
    const previous = mode;
    setMode(null);
    requestAnimationFrame(() => actionRef.current?.querySelector<HTMLButtonElement>(`[data-action="${previous}"]`)?.focus());
  }

  return (
    <div className="vault-assets">
      {!mode && <Panel label="ACCOUNT" pad={false}>
        <div className="vault-account">
          <div className="vault-account-network">
            <span className="vault-caption">NETWORK</span>
            <span>{account?.network ?? (loading ? "Loading…" : "Unavailable")}</span>
          </div>
          {account ? <PublicValue value={account.accountId} label="account ID" /> : (
            <p className="vault-note" role="status">{loading ? "Loading account…" : error}</p>
          )}
        </div>
      </Panel>}

      {mode && account ? (
        <TransferPanel
          key={mode}
          mode={mode}
          account={account}
          onBack={closeTransfer}
          onAccount={setAccount}
        />
      ) : (
        <>
          <div className="vault-actions" ref={actionRef}>
            <Button data-action="deposit" disabled={!account || !account.tokens.length} onClick={() => setMode("deposit")}>
              <TransferIcon direction="in" /> DEPOSIT
            </Button>
            <Button data-action="withdraw" disabled={!account || !account.tokens.length} onClick={() => setMode("withdraw")}>
              <TransferIcon direction="out" /> WITHDRAW
            </Button>
          </div>
          <Panel label="ASSETS" pad={false} action={
            <Button size="sm" tone="ghost" disabled={loading || refreshing} onClick={() => void refresh()} aria-label="Refresh balances">
              {refreshing ? "UPDATING…" : "REFRESH"}
            </Button>
          }>
            <div className="vault-holdings" aria-busy={loading || refreshing}>
              <p className="vault-note vault-assets-intro">Wallet holds your tokens. In Intents is ready to trade.</p>
              <table className="vault-token-table">
                <caption className="vault-sr-only">Token balances in Wallet and In Intents</caption>
                <thead><tr><th scope="col">TOKEN</th><th scope="col">WALLET</th><th scope="col">IN INTENTS</th></tr></thead>
                <tbody>
                  <tr>
                    <th scope="row"><span className="vault-token-symbol">NEAR</span><span className="vault-token-note">NETWORK GAS</span></th>
                    <td><TokenAmount value={account?.nativeBalance} loading={loading} /></td>
                    <td><span className="vault-not-applicable" aria-label="Not tradable in Intents">N/A</span></td>
                  </tr>
                  {account?.tokens.map((token) => (
                    <tr key={token.symbol}>
                      <th scope="row"><span className="vault-token-symbol">{token.symbol}</span><span className="vault-token-note">{tokenName(token.symbol)}</span></th>
                      <td><TokenAmount value={token.wallet} /></td>
                      <td><TokenAmount value={token.inIntents} /></td>
                    </tr>
                  ))}
                </tbody>
              </table>
              {account && <p className="vault-note vault-balance-note">Balances are from the last successful sync. — means not yet read.</p>}
              {account?.nativeBalance === "0" && <p className="vault-warning">Add NEAR to your wallet to pay network fees.</p>}
              {error && <p className="vault-warning" role="alert">{error} {account && "Last known balances are shown."}</p>}
            </div>
          </Panel>
        </>
      )}
    </div>
  );
}

function isStarting(error: unknown): boolean {
  return typeof error === "object" && error !== null && "kind" in error && error.kind === "not_ready";
}

function tokenName(symbol: string) {
  return symbol === "wNEAR" ? "WRAPPED NEAR" : symbol === "USDC" ? "USD COIN" : "TOKEN";
}

function TokenAmount({ value, loading = false }: { value?: string | null; loading?: boolean }) {
  return <span className="vault-token-amount" title={value ?? undefined} aria-label={value == null ? (loading ? "Loading balance" : "Balance not yet read") : undefined}>{value ?? "—"}</span>;
}

function TransferIcon({ direction }: { direction: "in" | "out" }) {
  return <svg width="16" height="16" viewBox="0 0 24 24" fill="none" stroke="currentColor" strokeWidth="1.5" aria-hidden="true">
    <path d={direction === "in" ? "M12 3v12m-4-4 4 4 4-4" : "M12 15V3m-4 4 4-4 4 4"} />
    <path d="M4 15v5h16v-5" />
  </svg>;
}

function TransferPanel({ mode, account, onBack, onAccount }: {
  mode: TransferMode;
  account: NearAccountView;
  onBack: () => void;
  onAccount: (account: NearAccountView) => void;
}) {
  const [symbol, setSymbol] = useState(account.tokens[0]?.symbol ?? "");
  const [amount, setAmount] = useState("");
  const [busy, setBusy] = useState(false);
  const [error, setError] = useState<string | null>(null);
  const [success, setSuccess] = useState<string | null>(null);
  const [syncError, setSyncError] = useState(false);
  const headingRef = useRef<HTMLHeadingElement>(null);
  const selected: NearTokenView | undefined = account.tokens.find((token) => token.symbol === symbol);
  const available = mode === "deposit" ? selected?.wallet : selected?.inIntents;
  const validation = selected && amount ? transferAmountError(amount, selected.decimals, available) : null;
  const source = mode === "deposit" ? "Wallet" : "In Intents";
  const destination = mode === "deposit" ? "In Intents" : "Wallet";
  const title = mode === "deposit" ? "Deposit" : "Withdraw";
  const noGas = account.nativeBalance === "0";

  useEffect(() => { headingRef.current?.focus(); }, []);

  async function submit() {
    if (busy || !selected || !amount || validation || noGas) return;
    setBusy(true);
    setError(null);
    setSuccess(null);
    setSyncError(false);
    try {
      const hash = await invoke<string>(mode === "deposit" ? "near_deposit" : "near_withdraw", { token: symbol, amount });
      setSuccess(hash);
      setAmount("");
      try { onAccount(await invoke<NearAccountView>("near_account")); }
      catch { setSyncError(true); }
    } catch (failure) {
      setError(errorCopy(failure));
    } finally {
      setBusy(false);
    }
  }

  return (
    <section className="vault-transfer" aria-labelledby="vault-transfer-title">
      <Button tone="ghost" size="sm" disabled={busy} onClick={onBack} className="vault-back">BACK TO ASSETS</Button>
      <Panel pad={false}>
        <div className="vault-transfer-content">
          <div className="vault-transfer-heading">
            <h2 ref={headingRef} tabIndex={-1} id="vault-transfer-title">{title}</h2>
            <span className="vault-caption">{account.network}</span>
          </div>
          <p className="vault-note">{mode === "deposit" ? "Move tokens into Intents to make them available for trading." : "Move tokens from Intents back to this account’s wallet."}</p>
          <div className="vault-transfer-route" aria-label={`From ${source} to ${destination}`}>
            <div><span className="vault-caption">FROM</span><span>{source}</span></div>
            <svg width="20" height="20" viewBox="0 0 24 24" fill="none" stroke="currentColor" strokeWidth="1.5" aria-hidden="true"><path d="M4 12h16m-5-5 5 5-5 5" /></svg>
            <div><span className="vault-caption">TO</span><span>{destination}</span></div>
          </div>
          <form onSubmit={(event) => { event.preventDefault(); void submit(); }} className="vault-transfer-form" aria-busy={busy}>
            <Field label="TOKEN" htmlFor="vault-transfer-token">
              <select id="vault-transfer-token" className="vault-token-select" value={symbol} disabled={busy} onChange={(event) => { setSymbol(event.target.value); setAmount(""); setError(null); }}>
                {account.tokens.map((token) => <option key={token.symbol} value={token.symbol}>{token.symbol} · {tokenName(token.symbol)}</option>)}
              </select>
            </Field>
            <div className="vault-available"><span>Available in {source}</span><span>{available ?? "Not yet read"}{available != null ? ` ${symbol}` : ""}</span></div>
            <Field label={`AMOUNT (${symbol})`} htmlFor="vault-transfer-amount">
              <Input id="vault-transfer-amount" type="text" inputMode="decimal" autoComplete="off" placeholder="0.00" value={amount} disabled={busy} invalid={!!validation} aria-describedby={validation || error ? "vault-transfer-error" : undefined} onChange={(event) => { setAmount((event.target as HTMLInputElement).value); setError(null); }} />
            </Field>
            {(validation || error) && <p id="vault-transfer-error" className="vault-warning" role="alert">{validation || error}</p>}
            <p className={noGas ? "vault-warning" : "vault-note"}>{noGas ? "Add NEAR to your wallet before continuing." : "Network fees are paid in NEAR from your wallet."}</p>
            <Button type="submit" tone="primary" block disabled={busy || !amount || !selected || !!validation || noGas}>
              {busy ? "SENDING…" : `${title.toUpperCase()} ${symbol}`}
            </Button>
          </form>
          {success && <div className="vault-transfer-success" role="status"><p>{title} submitted.</p><PublicValue value={success} label="transaction hash" />{syncError && <p className="vault-note">Balance refresh is unavailable. Refresh Assets to check the latest balance.</p>}</div>}
        </div>
      </Panel>
    </section>
  );
}

function PublicValue({ value, label }: { value: string; label: string }) {
  const [copied, setCopied] = useState(false);
  const [failed, setFailed] = useState(false);
  const timer = useRef<ReturnType<typeof setTimeout> | undefined>(undefined);
  useEffect(() => () => clearTimeout(timer.current), []);
  async function copy() {
    const field = document.createElement("textarea");
    field.value = value;
    field.readOnly = true;
    field.style.position = "fixed";
    field.style.opacity = "0";
    const previous = document.activeElement;
    document.body.appendChild(field);
    field.select();
    field.setSelectionRange(0, value.length);
    try {
      if (!document.execCommand("copy")) await navigator.clipboard.writeText(value);
      setCopied(true);
      setFailed(false);
    } catch { setFailed(true); }
    finally { field.remove(); if (previous instanceof HTMLElement) previous.focus(); }
    clearTimeout(timer.current);
    timer.current = setTimeout(() => { setCopied(false); setFailed(false); }, 2000);
  }
  return <div className="vault-public-value"><span className="vault-address">{value}</span><Button tone="ghost" size="sm" aria-label={`Copy ${label}`} onClick={() => void copy()}><span role="status">{failed ? "RETRY COPY" : copied ? "COPIED" : "COPY"}</span></Button></div>;
}
