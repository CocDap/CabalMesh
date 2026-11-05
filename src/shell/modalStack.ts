/**
 * How many modals are open, so the back gesture reaches exactly one of them.
 *
 * # The problem this exists to solve
 *
 * Android's hardware back arrives as a `popstate`, and both the app shell and
 * an open dialog need it: the shell to navigate, the dialog to close. Both
 * listen on `window`, so both handlers run for the same press — back would
 * close the dialog *and* leave the screen behind it, which is one press doing
 * two things.
 *
 * Listener order cannot fix it. `stopImmediatePropagation` only stops handlers
 * registered *after* the one that calls it, and the shell registers on mount,
 * long before any dialog opens.
 *
 * So the shell asks instead. While a modal is open the shell declines the
 * event and the dialog takes it.
 *
 * # Why a counter and not a boolean
 *
 * Two dialogs can briefly overlap while one is unmounting and the next is
 * mounting. A boolean cleared by the first would leave the shell live under the
 * second for exactly as long as that takes, which is precisely the kind of
 * timing bug that only reproduces on a slow device.
 */
let depth = 0;

/** Registers a modal as open. */
export function pushModal(): void {
  depth += 1;
}

/** Registers a modal as closed. Never goes below zero. */
export function popModal(): void {
  depth = Math.max(0, depth - 1);
}

/** Whether any modal currently owns the back gesture. */
export function modalOpen(): boolean {
  return depth > 0;
}
