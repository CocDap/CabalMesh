/**
 * Mobile entry point.
 *
 * Imports nothing from the frozen desktop tree: Tailwind and framer-motion live
 * there, and both are hostile to the design system — Tailwind compiles to the
 * raw px and hex values the adherence lint bans, and the brand forbids spring
 * physics outright.
 */
import React, { useRef, useState } from "react";
import ReactDOM from "react-dom/client";
import { invoke } from "@tauri-apps/api/core";
import "../ds/index";
import { AppShell } from "../shell/AppShell";
import { Splash } from "../screens/Splash";
import { Unlock } from "../screens/Unlock";
import { WalletSetup } from "../screens/WalletSetup";
import { walletEntry } from "../state/walletEntry";
import { walletSetupError } from "../types/wallet";
import { Home } from "../screens/Home";
import { Nodes } from "../screens/Nodes";
import { Intents } from "../screens/Intents";
import { New } from "../screens/New";
import { Detail } from "../screens/Detail";
import { Settled } from "../screens/Settled";
import { Vault } from "../screens/Vault";
import { Profile } from "../screens/Profile";
import { GuardianApproval } from "../screens/GuardianApproval";
import type { Screen } from "../shell/screen";

function App() {
  // A fresh wallet goes through named-account setup before the main app.
  const [screen, setScreen] = useState<Screen>({ name: "splash" });
  const [entering, setEntering] = useState(false);
  const [entryError, setEntryError] = useState<string | null>(null);
  const entryPending = useRef(false);

  async function enter() {
    if (entryPending.current) return;
    entryPending.current = true;
    setEntering(true);
    setEntryError(null);
    try {
      const next = await walletEntry(invoke);
      setScreen(next === "unlock" ? { name: "unlock" } : next === "wallet_setup" ? { name: "wallet_setup" } : { name: "home" });
    } catch (failure) {
      setEntryError(walletSetupError(failure));
    } finally {
      entryPending.current = false;
      setEntering(false);
    }
  }

  // Mounted regardless of screen: a guardian approval is driven by someone
  // else's action, not by where this device's own owner happens to be
  // navigating.
  if (screen.name === "splash") {
    return (
      <>
        <Splash onEnter={() => void enter()} entering={entering} error={entryError} />
        <GuardianApproval />
      </>
    );
  }

  if (screen.name === "unlock") {
    return (
      <>
        <Unlock onUnlocked={() => void enter()} />
        <GuardianApproval />
      </>
    );
  }

  if (screen.name === "wallet_setup") {
    return <WalletSetup onComplete={() => setScreen({ name: "vault", tab: "ASSETS" })} onCancel={() => setScreen({ name: "splash" })} />;
  }

  return (
    <AppShell screen={screen} onNavigate={setScreen}>
      {screen.name === "home" ? (
        <Home />
      ) : screen.name === "nodes" ? (
        <Nodes />
      ) : screen.name === "intents" ? (
        <Intents
          tab={screen.tab}
          onTabChange={(tab) => setScreen({ name: "intents", tab })}
          onCompose={() => setScreen({ name: "new" })}
          onOpen={(id) => setScreen({ name: "detail", id })}
        />
      ) : screen.name === "new" ? (
        <New onBroadcast={(id) => setScreen({ name: "detail", id })} />
      ) : screen.name === "detail" ? (
        // Settling navigates on, but only forward: the detail screen reports
        // that it settled and this decides, so a stale render cannot bounce a
        // user off a screen they navigated back to.
        <Detail id={screen.id} onSettled={(id) => setScreen({ name: "settled", id })} />
      ) : screen.name === "settled" ? (
        <Settled id={screen.id} onDone={() => setScreen({ name: "intents", tab: "HISTORY" })} />
      ) : screen.name === "vault" ? (
        <Vault
          tab={screen.tab}
          onTabChange={(tab) => setScreen({ name: "vault", tab })}
        />
      ) : screen.name === "profile" ? (
        <Profile onLeave={() => setScreen({ name: "splash" })} />
      ) : null}
      <GuardianApproval />
    </AppShell>
  );
}

ReactDOM.createRoot(document.getElementById("root") as HTMLElement).render(
  <React.StrictMode>
    <App />
  </React.StrictMode>,
);
