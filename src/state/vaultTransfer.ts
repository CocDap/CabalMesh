/** Compare token amounts in raw units; floating point loses token precision. */
function rawAmount(value: string, decimals: number): bigint | null {
  if (!/^\d+(?:\.\d+)?$/.test(value)) return null;
  const [whole, fraction = ""] = value.split(".");
  if (fraction.length > decimals) return null;
  return BigInt(whole + fraction.padEnd(decimals, "0"));
}

export function transferAmountError(amount: string, decimals: number, available?: string | null): string | null {
  const raw = rawAmount(amount, decimals);
  if (raw === null) return `Enter a number with up to ${decimals} decimal places.`;
  if (raw === 0n) return "Enter an amount greater than zero.";
  const balance = available == null ? null : rawAmount(available, decimals);
  if (balance !== null && raw > balance) return "Amount exceeds the available balance.";
  return null;
}
