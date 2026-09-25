import { useLayoutEffect, useRef, type ReactNode } from "react";

/** Native modal semantics provide focus containment, Escape and focus restoration.
 * Nested source review stays in the browser's modal top layer above its opener.
 */
export function Dialog({
  label,
  wide,
  preventClose,
  restoreFocus,
  onClose,
  children,
}: {
  label: string;
  wide?: boolean;
  preventClose?: boolean;
  restoreFocus?: () => void;
  onClose: () => void;
  children: ReactNode;
}) {
  const ref = useRef<HTMLDialogElement>(null);
  useLayoutEffect(() => {
    const dialog = ref.current!;
    const opener =
      document.activeElement instanceof HTMLElement
        ? document.activeElement
        : null;
    dialog.showModal();
    return () => {
      dialog.close();
      // Commit may also re-enable the opener or mount a replacement modal.
      // Restore after those DOM changes; an active modal keeps its background inert.
      queueMicrotask(() => {
        if (restoreFocus) restoreFocus();
        else if (opener?.isConnected) opener.focus({ preventScroll: true });
      });
    };
  }, []);
  return (
    <dialog
      ref={ref}
      className={`modal${wide ? " wide" : ""}`}
      aria-label={label}
      onClose={onClose}
      onCancel={(event) => {
        if (preventClose) event.preventDefault();
      }}
      onKeyDown={(event) => {
        if (event.key !== "Tab" || event.defaultPrevented) return;
        const items = [
          ...event.currentTarget.querySelectorAll<HTMLElement>(
            "button:not([disabled]),input:not([disabled]),select:not([disabled]),textarea:not([disabled]),summary,a[href],[tabindex]",
          ),
        ].filter(
          (item) => item.tabIndex >= 0 && item.getClientRects().length > 0,
        );
        const first = items[0],
          last = items.at(-1);
        if (event.shiftKey && document.activeElement === first) {
          event.preventDefault();
          last?.focus();
        } else if (!event.shiftKey && document.activeElement === last) {
          event.preventDefault();
          first?.focus();
        }
      }}
    >
      {children}
    </dialog>
  );
}
