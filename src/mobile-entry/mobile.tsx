/**
 * Mobile entry point.
 *
 * Imports nothing from the frozen desktop tree: Tailwind and framer-motion live
 * there, and both are hostile to the design system — Tailwind compiles to the
 * raw px and hex values the adherence lint bans, and the brand forbids spring
 * physics outright.
 */
import React, { useState } from "react";
import ReactDOM from "react-dom/client";
import { invoke } from "@tauri-apps/api/core";
import "../ds/index";
import { AppShell } from "../shell/AppShell";
import { Splash } from "../screens/Splash";
import { Unlock } from "../screens/Unlock";
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
import type { SecurityStatus } from "../types/bindings";

function App() {
  // Starts at splash: the app has no session until the user asks for one.
  // The mesh itself is already joined by the time this renders — bootstrap
  // does that at process startup, not on a user action — so there is nothing
  // to wait for between splash and home.
  const [screen, setScreen] = useState<Screen>({ name: "splash" });

  // Every install defaults to passphrase protection being off, so this check
  // is a no-op for almost everyone — see docs/identity-design.md, decision 1.
  // It only routes to Unlock when SECURITY has actually been turned on; any
  // failure (bootstrap still running, command unavailable) falls through to
  // Home rather than blocking entry, matching the zero-friction default.
  async function enter() {
    try {
      const status = await invoke<SecurityStatus>("security_status");
      if (status.locked) {
        setScreen({ name: "unlock" });
        return;
      }
    } catch {
      // Fall through to Home.
    }
    setScreen({ name: "home" });
  }

  // Mounted regardless of screen: a guardian approval is driven by someone
  // else's action, not by where this device's own owner happens to be
  // navigating.
  if (screen.name === "splash") {
    return (
      <>
        <Splash onEnter={() => void enter()} />
        <GuardianApproval />
      </>
    );
  }

  if (screen.name === "unlock") {
    return (
      <>
        <Unlock onUnlocked={() => setScreen({ name: "home" })} />
        <GuardianApproval />
      </>
    );
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
