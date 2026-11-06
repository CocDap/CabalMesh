import type { SecurityStatus } from "../types/bindings";
import type { WalletSetupStatus } from "../types/wallet";

/** Fail closed while bootstrap is pending; a fresh wallet never bypasses setup. */
export async function walletEntry(
  invokeCommand: (command: string) => Promise<unknown>,
  wait: () => Promise<void> = () => new Promise((resolve) => setTimeout(resolve, 1000)),
): Promise<"unlock" | "wallet_setup" | "home"> {
  for (let attempt = 0; ; attempt += 1) {
    try {
      const security = await invokeCommand("security_status") as SecurityStatus;
      if (security.locked) return "unlock";
      const wallet = await invokeCommand("wallet_setup_status") as WalletSetupStatus;
      return wallet.required ? "wallet_setup" : "home";
    } catch (error) {
      const starting = error === "not_ready" || (typeof error === "object" && error !== null && "kind" in error && error.kind === "not_ready");
      if (!starting || attempt >= 30) throw error;
      await wait();
    }
  }
}
