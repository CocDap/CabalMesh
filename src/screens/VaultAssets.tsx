import { useEffect, useRef, useState } from "react";
import { invoke } from "@tauri-apps/api/core";
import { Button, Field, Input, Panel } from "../ds";
import { startBalanceSync } from "../state/balanceSync";
import { errorCopy } from "../state/errorCopy";
import { hasTokenBalance, transferAmountError, transferTokens } from "../state/vaultTransfer";
import type { NearAccountView, NearTokenView } from "../types/bindings";
import "./vault.css";
import { WalletSetup } from "./WalletSetup";

type TransferMode = "deposit" | "withdraw";

/** Account identity, holdings and token movement have separate task surfaces. */
export function VaultAssets() {
  const [account, setAccount] = useState<NearAccountView | null>(null);
  const [loading, setLoading] = useState(true);
  const [refreshing, setRefreshing] = useState(false);
  const [error, setError] = useState<string | null>(null);
  const [mode, setMode] = useState<TransferMode | null>(null);
  const [settingUp, setSettingUp] = useState(false);
  const actionRef = useRef<HTMLDivElement>(null);
  const syncing = useRef<Promise<void> | null>(null);

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

  // near_account only returns the cache, so without this a new account shows no
  // balances until someone presses REFRESH.
  const accountId = account?.accountId;
  const onOverview = !mode && !settingUp;
  useEffect(() => {
    if (!accountId || !onOverview) return;
    return startBalanceSync(() => void syncBalances(true), document);
  }, [accountId, onOverview]);

  /** One read at a time: REFRESH during a background read waits for that read. */
  function syncBalances(readChain: boolean) {
    syncing.current ??= (async () => {
      try {
        if (readChain) await invoke("vault_refresh_balance");
        setAccount(await invoke<NearAccountView>("near_account"));
        setError(null);
      } catch (failure) {
        setError(errorCopy(failure));
      } finally {
        syncing.current = null;
      }
    })();
    return syncing.current;
  }

  async function refresh() {
    if (refreshing) return;
    setRefreshing(true);
    setError(null);
    await syncBalances(account != null);
    setRefreshing(false);
  }

  function closeTransfer() {
    const previous = mode;
    setMode(null);
    requestAnimationFrame(() => actionRef.current?.querySelector<HTMLButtonElement>(`[data-action="${previous}"]`)?.focus());
  }

  if (settingUp) return <div className="vault-assets"><WalletSetup onCancel={() => setSettingUp(false)} onComplete={() => {
    setSettingUp(false);
    setError(null);
    void invoke<NearAccountView>("near_account").then(setAccount).catch((failure) => setError(errorCopy(failure)));
  }} /></div>;

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
          {account && /^[a-f0-9]{64}$/.test(account.accountId) && account.network.toLowerCase().includes("testnet") && (
            <Button className="vault-setup-action" tone="ghost" size="sm" onClick={() => setSettingUp(true)}>SET UP .TESTNET ACCOUNT</Button>
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
                    <th scope="row"><span className="vault-token-symbol">NEAR</span><span className="vault-token-note">NATIVE · GAS</span></th>
                    <td><TokenAmount value={account?.nativeBalance} loading={loading} /></td>
                    <td><TokenAmount value={account?.nativeInIntents} loading={loading} /></td>
                  </tr>
                  {account?.tokens.map((token) => (
                    <tr key={token.symbol}>
                      <th scope="row"><span className="vault-token-symbol">{token.symbol}</span><span className="vault-token-note">{tokenName(token.symbol, account.nativeSharesWnear)}</span></th>
                      <td><TokenAmount value={token.wallet} /></td>
                      <td><TokenAmount value={token.inIntents} /></td>
                    </tr>
                  ))}
                </tbody>
              </table>
              {account?.nativeSharesWnear && <p className="vault-note">NEAR and wNEAR share the same In Intents balance.</p>}
              {account && !account.nativeSharesWnear && hasTokenBalance(account.nativeWrapped) && <p className="vault-note">{account.nativeWrapped} NEAR-backed wNEAR is in your wallet. Choose Withdraw → NEAR → Wrapped NEAR in Wallet to unwrap it.</p>}
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

function tokenName(symbol: string, sharesWnear = true) {
  return symbol === "NEAR" ? "NATIVE NEAR" : symbol === "wNEAR" ? (sharesWnear ? "WRAPPED NEAR" : "TEST TOKEN") : symbol === "USDC" ? "USD COIN" : "TOKEN";
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
  const choices = transferTokens(account);
  const [symbol, setSymbol] = useState(choices[0]?.symbol ?? "");
  const [nativeSource, setNativeSource] = useState("intents");
  const [amount, setAmount] = useState("");
  const [busy, setBusy] = useState(false);
  const [error, setError] = useState<string | null>(null);
  const [success, setSuccess] = useState<string | null>(null);
  const [syncError, setSyncError] = useState(false);
  const [refreshing, setRefreshing] = useState(false);
  const [needsRefresh, setNeedsRefresh] = useState(false);
  const headingRef = useRef<HTMLHeadingElement>(null);
  const native = symbol === "NEAR";
  const unwrapping = native && mode === "withdraw" && nativeSource === "wallet";
  const selected: NearTokenView | undefined = choices.find((token) => token.symbol === symbol);
  const available = unwrapping ? account.nativeWrapped : mode === "deposit" ? selected?.wallet : selected?.inIntents;
  const validation = selected && amount ? native && available == null ? "Refresh balances before transferring NEAR." : transferAmountError(amount, selected.decimals, available) : null;
  const source = unwrapping ? "Wallet (wrapped NEAR)" : mode === "deposit" ? "Wallet" : "In Intents";
  const destination = mode === "deposit" ? "In Intents" : native ? "Wallet (NEAR)" : "Wallet";
  const title = mode === "deposit" ? "Deposit" : "Withdraw";
  const noGas = account.nativeBalance === "0";

  useEffect(() => { headingRef.current?.focus(); }, []);
  useEffect(() => {
    if (!hasTokenBalance(account.nativeWrapped)) setNativeSource("intents");
  }, [account.nativeWrapped]);

  async function refreshBalances() {
    if (busy || refreshing) return;
    setRefreshing(true);
    setSyncError(false);
    try {
      await invoke("vault_refresh_balance");
      onAccount(await invoke<NearAccountView>("near_account"));
      setNeedsRefresh(false);
      setError(null);
    } catch { setSyncError(true); }
    finally { setRefreshing(false); }
  }

  async function submit() {
    if (busy || refreshing || needsRefresh || !selected || !amount || validation || noGas) return;
    setBusy(true);
    setError(null);
    setSuccess(null);
    setSyncError(false);
    try {
      const hash = await invoke<string>(unwrapping ? "near_unwrap" : mode === "deposit" ? "near_deposit" : "near_withdraw", unwrapping ? { amount } : { token: symbol, amount });
      setSuccess(hash);
      setAmount("");
      try { onAccount(await invoke<NearAccountView>("near_account")); }
      catch { setSyncError(true); }
    } catch (failure) {
      setError(errorCopy(failure));
      if (native) {
        setNeedsRefresh(true);
        try { onAccount(await invoke<NearAccountView>("near_account")); }
        catch { setSyncError(true); }
      }
    } finally {
      setBusy(false);
    }
  }

  return (
    <section className="vault-transfer" aria-labelledby="vault-transfer-title">
      <Button tone="ghost" size="sm" disabled={busy || refreshing} onClick={onBack} className="vault-back">BACK TO ASSETS</Button>
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
              <select id="vault-transfer-token" className="vault-token-select" value={symbol} disabled={busy || refreshing} onChange={(event) => {
                setSymbol(event.target.value);
                setNativeSource("intents");
                setAmount("");
                setError(null);
                setSuccess(null);
                setSyncError(false);
              }}>
                {choices.map((token) => <option key={token.symbol} value={token.symbol}>{token.symbol} · {tokenName(token.symbol, account.nativeSharesWnear)}</option>)}
              </select>
            </Field>
            {native && mode === "withdraw" && hasTokenBalance(account.nativeWrapped) && <Field label="BALANCE" htmlFor="vault-native-source">
              <select id="vault-native-source" className="vault-token-select" value={nativeSource} disabled={busy || refreshing} onChange={(event) => { setNativeSource(event.target.value); setAmount(""); setSuccess(null); }}>
                <option value="intents">In Intents</option>
                <option value="wallet">Wrapped NEAR in Wallet</option>
              </select>
            </Field>}
            {native && <p className="vault-note">{mode === "deposit" ? "NEAR is wrapped and deposited automatically. The available amount keeps NEAR for storage and network fees." : unwrapping ? "Unwrap NEAR already held in your wallet. This does not withdraw from Intents again." : "Withdraw your NEAR-backed balance from Intents and receive native NEAR in this wallet."}</p>}
            {native && !account.nativeSharesWnear && <p className="vault-note">Test wNEAR is a separate token. It cannot be converted to native NEAR.</p>}
            <div className="vault-available" aria-live="polite"><span>{native && mode === "deposit" ? "Available to deposit" : `${source} balance`}</span><span>{available ?? "Not yet read"}{available != null ? ` ${symbol}` : ""}</span></div>
            <Button type="button" tone="ghost" size="sm" disabled={busy || refreshing} onClick={() => void refreshBalances()}>{refreshing ? "REFRESHING…" : "REFRESH BALANCES"}</Button>
            {syncError && <p className="vault-warning" role="alert">Balances could not be refreshed. Try refreshing again before sending.</p>}
            {needsRefresh && <p className="vault-note" role="status">Refresh balances before sending again.</p>}
            <Field label={`AMOUNT (${symbol})`} htmlFor="vault-transfer-amount">
              <Input id="vault-transfer-amount" type="text" inputMode="decimal" autoComplete="off" placeholder="0.00" value={amount} disabled={busy} invalid={!!validation} aria-describedby={validation || error ? "vault-transfer-error" : undefined} onChange={(event) => { setAmount((event.target as HTMLInputElement).value); setError(null); }} />
            </Field>
            {(validation || error) && <p id="vault-transfer-error" className="vault-warning" role="alert">{validation || error}</p>}
            <p className={noGas ? "vault-warning" : "vault-note"}>{noGas ? "Add NEAR to your wallet before continuing." : "Network fees are paid in NEAR from your wallet."}</p>
            <Button type="submit" tone="primary" block disabled={busy || refreshing || needsRefresh || !amount || !selected || !!validation || noGas || (native && available == null)}>
              {busy ? "SENDING…" : unwrapping ? "UNWRAP TO NEAR" : `${title.toUpperCase()} ${symbol}`}
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
