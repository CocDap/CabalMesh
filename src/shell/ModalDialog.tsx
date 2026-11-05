import React from "react";
import { Dialog } from "../ds";
import { popModal, pushModal } from "./modalStack";

/**
 * The design system's dialog, made usable with a keyboard and a phone.
 *
 * The vendored `Dialog` renders `role="dialog" aria-modal="true"` and stops
 * there: focus stays wherever it was, Tab walks straight out into the page
 * behind the scrim, Escape does nothing, and Android's back gesture navigates
 * the whole screen away instead of closing the dialog. `aria-modal` *promises*
 * that everything outside is inert, so shipping it without a trap is worse than
 * shipping no modal semantics — it tells a screen reader something untrue.
 *
 * The design-system tree is generated and replaced wholesale, so the fix lives
 * here rather than in `ds/`, where it would be lost on the next regeneration.
 *
 * Four behaviours, all of them required by ticket 33:
 *
 * **Focus enters and is trapped.** Focus moves to the first control on open and
 * Tab cycles within the dialog.
 *
 * **Focus is returned.** Whatever was focused before is refocused on close, so
 * dismissing a dialog does not dump a keyboard user back at the top of the
 * document.
 *
 * **Back and Escape close it.** Android's hardware back is a `popstate`, and the
 * app shell binds that to navigation. A history entry pushed on open means the
 * gesture closes the dialog instead of leaving the screen.
 *
 * **It sits above the keyboard.** A dialog centred in the layout viewport is
 * centred behind the on-screen keyboard. `visualViewport` reports what is
 * actually visible, and the dialog is positioned against that instead.
 */
export function ModalDialog({
  open,
  title,
  onClose,
  footer,
  children,
}: {
  open: boolean;
  title: string;
  onClose: () => void;
  footer?: React.ReactNode;
  children: React.ReactNode;
}) {
  const shell = React.useRef<HTMLDivElement | null>(null);
  const restoreTo = React.useRef<HTMLElement | null>(null);
  const offset = useKeyboardOffset(open);

  // Focus in on open, out on close. Stored before the dialog paints, because
  // once it has, `document.activeElement` is already inside it.
  React.useEffect(() => {
    if (!open) return;
    restoreTo.current = document.activeElement as HTMLElement | null;

    const first = focusable(shell.current)[0] ?? shell.current;
    first?.focus();

    return () => {
      restoreTo.current?.focus();
      restoreTo.current = null;
    };
  }, [open]);

  React.useEffect(() => {
    if (!open) return;

    const onKeyDown = (event: KeyboardEvent) => {
      if (event.key === "Escape") {
        event.preventDefault();
        onClose();
        return;
      }
      if (event.key !== "Tab") return;

      const stops = focusable(shell.current);
      if (stops.length === 0) return;

      const first = stops[0];
      const last = stops[stops.length - 1];
      const active = document.activeElement;

      // Wrapping is done here rather than left to the browser: the elements
      // behind the scrim are still in the tab order, and `aria-modal` claims
      // they are not.
      if (event.shiftKey && (active === first || !shell.current?.contains(active))) {
        event.preventDefault();
        last.focus();
      } else if (!event.shiftKey && active === last) {
        event.preventDefault();
        first.focus();
      }
    };

    document.addEventListener("keydown", onKeyDown, true);
    return () => document.removeEventListener("keydown", onKeyDown, true);
  }, [open, onClose]);

  // The back gesture. A pushed entry gives it something to pop that is not the
  // screen itself, and the modal flag stops the app shell from also acting on
  // the same press — see shell/modalStack.ts.
  React.useEffect(() => {
    if (!open) return;

    pushModal();
    window.history.pushState({ dialog: true }, "");
    const onPop = () => onClose();
    window.addEventListener("popstate", onPop);

    return () => {
      window.removeEventListener("popstate", onPop);
      popModal();
      // The pushed entry is deliberately left in place. Unwinding it with
      // `history.back()` would fire a `popstate` the shell then treats as a
      // navigation, so closing a dialog with the Cancel button would also
      // leave the screen. A stale entry is harmless: the shell re-pushes after
      // every pop and treats each one as exactly one step back.
    };
  }, [open, onClose]);

  if (!open) return null;

  return (
    <Dialog
      open
      title={title}
      onClose={onClose}
      footer={footer}
      style={{
        // Lifted clear of the keyboard rather than centred behind it.
        marginBottom: offset,
        maxHeight: "80dvh",
        overflowY: "auto",
      }}
    >
      <div ref={shell}>{children}</div>
    </Dialog>
  );
}

/** Tab stops inside the dialog, in document order. */
function focusable(root: HTMLElement | null): HTMLElement[] {
  if (!root) return [];
  // Climbs to the dialog element so the header's close button is included —
  // the ref is on the body, and the close button is a sibling of it.
  const dialog = root.closest('[role="dialog"]') ?? root;
  const selector =
    'button:not([disabled]), [href], input:not([disabled]), select:not([disabled]), textarea:not([disabled]), [tabindex]:not([tabindex="-1"])';
  return Array.from(dialog.querySelectorAll<HTMLElement>(selector)).filter(
    (element) => element.offsetParent !== null || element === document.activeElement,
  );
}

/**
 * How far the on-screen keyboard has eaten into the viewport.
 *
 * `visualViewport` is the only thing that reports this. Neither WebView fires a
 * resize on the layout viewport when the keyboard opens, so a dialog laid out
 * against `100dvh` sits behind it with no way to know.
 */
function useKeyboardOffset(active: boolean): number {
  const [offset, setOffset] = React.useState(0);

  React.useEffect(() => {
    const viewport = window.visualViewport;
    if (!active || !viewport) return;

    const measure = () => {
      const hidden = window.innerHeight - viewport.height - viewport.offsetTop;
      // Small differences are browser chrome, not a keyboard. Reacting to them
      // makes the dialog twitch while scrolling.
      setOffset(hidden > 120 ? hidden : 0);
    };

    measure();
    viewport.addEventListener("resize", measure);
    viewport.addEventListener("scroll", measure);
    return () => {
      viewport.removeEventListener("resize", measure);
      viewport.removeEventListener("scroll", measure);
    };
  }, [active]);

  return offset;
}
