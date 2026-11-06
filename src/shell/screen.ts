/**
 * Screen state, typed so illegal navigation cannot be expressed.
 *
 * `detail` and `settled` carry an `IntentId` in the type itself, so "open the
 * detail screen with nothing loaded" — which the prototype papers over with
 * hardcoded data — is unrepresentable rather than merely avoided.
 *
 * No router library: ten screens, no URL bar, and history semantics in mobile
 * webviews are inconsistent enough that owning the stack is simpler than
 * configuring one.
 */

export type IntentId = string;

export type IntentTab = "ACTIVE" | "PENDING" | "HISTORY";
export type VaultTab = "ASSETS" | "IDENTITIES" | "KEYS";

export type Screen =
  | { name: "splash" }
  | { name: "unlock" }
  | { name: "wallet_setup" }
  | { name: "home" }
  | { name: "intents"; tab: IntentTab }
  | { name: "new" }
  | { name: "detail"; id: IntentId }
  | { name: "settled"; id: IntentId }
  | { name: "nodes" }
  | { name: "vault"; tab: VaultTab }
  | { name: "profile" };

/** The five destinations in the tab bar. */
export type TabKey = "home" | "intents" | "nodes" | "vault" | "profile";

/**
 * A glyph from the board's 14-icon plate. Typed as the literal union rather
 * than `string` so a typo is a compile error — the design system has no
 * fallback icon, and an unknown name renders nothing at all.
 */
export type GlyphName =
  | "node" | "agent" | "intent" | "mesh" | "proof" | "escrow" | "vault"
  | "reputation" | "signal" | "encrypt" | "identity" | "bridge" | "relayer" | "log";

export const TABS: ReadonlyArray<{ key: TabKey; label: string; icon: GlyphName }> = [
  { key: "home", label: "HOME", icon: "mesh" },
  { key: "intents", label: "INTENTS", icon: "intent" },
  { key: "nodes", label: "NODES", icon: "node" },
  { key: "vault", label: "VAULT", icon: "vault" },
  { key: "profile", label: "PROFILE", icon: "identity" },
];

/**
 * Where the back affordance goes, taken from the prototype's own `backMap`.
 *
 * Android's hardware back binds to the same function, so the two can never
 * disagree.
 */
export function back(screen: Screen): Screen {
  switch (screen.name) {
    case "splash":
    case "unlock":
    case "wallet_setup":
    case "home":
      return screen;
    case "intents":
    case "nodes":
    case "vault":
    case "profile":
      return { name: "home" };
    case "new":
    case "detail":
    case "settled":
      return { name: "intents", tab: "ACTIVE" };
  }
}

/** Which tab should read as selected, if any. */
export function activeTab(screen: Screen): TabKey | null {
  switch (screen.name) {
    case "home":
      return "home";
    case "intents":
    case "new":
    case "detail":
    case "settled":
      return "intents";
    case "nodes":
      return "nodes";
    case "vault":
      return "vault";
    case "profile":
      return "profile";
    case "splash":
    case "unlock":
    case "wallet_setup":
      return null;
  }
}

/** Splash, unlock and first-run wallet setup have no header or tab bar. */
export function hasChrome(screen: Screen): boolean {
  return screen.name !== "splash" && screen.name !== "unlock" && screen.name !== "wallet_setup";
}

/** Header title per screen, matching the prototype. */
export function title(screen: Screen): string {
  switch (screen.name) {
    case "home":
      return "CABAL MESH";
    case "intents":
      return "INTENTS";
    case "new":
      return "NEW INTENT";
    case "detail":
      return "INTENT DETAILS";
    case "settled":
      return "PROOF";
    case "nodes":
      return "NODES";
    case "vault":
      return "VAULT";
    case "profile":
      return "PROFILE";
    case "splash":
    case "unlock":
    case "wallet_setup":
      return "";
  }
}
