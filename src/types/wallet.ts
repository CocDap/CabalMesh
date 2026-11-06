/** IPC contract for src-tauri/src/wallet_setup.rs. No key material crosses it. */
export interface WalletSetupStatus {
  required: boolean;
  testnet: boolean;
  accountId: string;
  pendingAccountId: string | null;
}

export type WalletNameStatus = "available" | "owned" | "taken";

export function walletSetupError(error: unknown): string {
  switch (error) {
    case "not_ready": return "The wallet is still starting. Try again shortly.";
    case "locked": return "Unlock your vault before setting up this wallet.";
    case "invalid_name": return "Use lowercase letters, numbers, and single hyphens or underscores.";
    case "name_taken": return "This account name is already in use. Choose another name.";
    case "offline": return "Cannot reach NEAR testnet. Check your connection and retry.";
    case "helper_unavailable": return "Cannot reach the testnet account creation service. Your name and key are saved. Check your connection and retry.";
    case "rate_limited": return "The testnet faucet is busy. Wait a moment, then retry this name.";
    case "creation_failed": return "Account creation is not confirmed yet. Retry to check and continue.";
    case "intents_unavailable": return "The account exists. Intents setup is incomplete. Retry to finish with the same name.";
    case "storage": return "Wallet setup could not be saved on this device. Retry to finish.";
    case "pending_account": return "Finish the pending account setup before choosing another name.";
    case "wallet_changed": return "The active wallet changed. Open wallet setup again.";
    case "existing_account": return "This wallet already has an active account. It has been kept unchanged.";
    case "testnet_only": return "Named wallet creation is available on NEAR testnet in this build.";
    default: return "Wallet setup could not finish. Your saved key has been kept. Try again.";
  }
}
