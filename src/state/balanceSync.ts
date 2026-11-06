/**
 * NEAR RPC has no balance subscription, so a balance on screen stays current by
 * being read again. Each read is five RPC calls, so nothing is read while hidden.
 */
export const BALANCE_SYNC_MS = 10_000;

type VisibilitySource = Pick<Document, "visibilityState" | "addEventListener" | "removeEventListener">;

/** Reads now and on every interval while visible. Returns the function that stops it. */
export function startBalanceSync(read: () => void, source: VisibilitySource, intervalMs = BALANCE_SYNC_MS): () => void {
  const tick = () => {
    if (source.visibilityState === "visible") read();
  };
  tick();
  const timer = setInterval(tick, intervalMs);
  source.addEventListener("visibilitychange", tick);
  return () => {
    clearInterval(timer);
    source.removeEventListener("visibilitychange", tick);
  };
}
