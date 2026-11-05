import { useEffect } from "react";
import { Channel, invoke } from "@tauri-apps/api/core";
import type { LogLine } from "../types/bindings";

type StreamCommand = "enter_mesh" | "subscribe_mesh_log" | "subscribe_settlement_log";

/**
 * Opens a Rust log stream and guarantees teardown.
 *
 * **A Tauri channel does not clean itself up.** It releases its callback only
 * when Rust sends an end message; there is no unsubscribe in the JS API, and
 * releasing a JS callback would not stop the Rust producer anyway. Without
 * explicit teardown every visit to a streaming screen leaves a live producer,
 * and tab-switching accumulates them.
 *
 * Two details that are easy to omit and expensive to debug:
 *
 * - **Unmount can beat subscribe.** Fast tab switching runs cleanup before the
 *   registering invoke has returned, so a handle arriving after cancellation is
 *   torn down immediately. Handled once here rather than per screen.
 * - **Both promises are caught.** A rejected teardown would otherwise surface
 *   as an unhandled rejection with no stack pointing at the screen that caused
 *   it.
 *
 * Cancelling stops *delivery*, never the operation: leaving `connecting` does
 * not disconnect the mesh, and leaving a settling intent does not abort the
 * settlement. That is guaranteed on the Rust side — the settlement task holds
 * no token from the subscription registry — not by anything this hook does.
 */
export function useLogStream(
  command: StreamCommand,
  args: Record<string, unknown>,
  onLine: (line: LogLine) => void,
  enabled = true,
): void {
  const argsKey = JSON.stringify(args);

  useEffect(() => {
    if (!enabled) return;
    // `new Channel()` reaches into `window.__TAURI_INTERNALS__` synchronously
    // in its constructor — outside `invoke`'s own async/await boundary, so a
    // throw here isn't a promise rejection `.catch` can reach. Guarded so the
    // browser-only dev preview (no Tauri runtime) can render every other
    // screen instead of crashing on mount — see the same guard in
    // `GuardianApproval`.
    if (!("__TAURI_INTERNALS__" in window)) return;

    const channel = new Channel<LogLine>();
    channel.onmessage = onLine;

    let id: string | undefined;
    let cancelled = false;

    const teardown = (subscription: string) =>
      invoke("unsubscribe", { id: subscription }).catch(reportStreamError);

    invoke<string>(command, { ...JSON.parse(argsKey), onLine: channel })
      .then((subscription) => {
        if (cancelled) void teardown(subscription);
        else id = subscription;
      })
      .catch(reportStreamError);

    return () => {
      cancelled = true;
      if (id) void teardown(id);
    };
    // onLine is intentionally not a dependency: screens pass an inline
    // callback, and depending on it would resubscribe on every render.
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, [command, argsKey, enabled]);
}

function reportStreamError(error: unknown): void {
  // Not fatal: a failed subscribe leaves the pane empty, which the screen
  // already renders as a state.
  console.error("[stream]", error);
}
