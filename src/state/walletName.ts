export function walletNameError(name: string): string | null {
  if (!name) return "Enter a name for your wallet.";
  if (name.length > 56) return "Use up to 56 characters before .testnet.";
  if (!/^[a-z0-9]+(?:[-_][a-z0-9]+)*$/.test(name)) {
    return "Use lowercase letters, numbers, and single hyphens or underscores.";
  }
  return null;
}
