import {
  useEffect,
  useLayoutEffect,
  useRef,
  useState,
  type ReactNode,
} from "react";
import { Dialog } from "./Dialog";

/** Keep the ledger available beside review on desktop; use a contained dialog
 * where two readable columns cannot fit. The nested source stays modal. */
export function ReviewSurface({
  onClose,
  restoreFocus,
  children,
}: {
  onClose: () => void;
  restoreFocus: () => void;
  children: ReactNode;
}) {
  const [wide, setWide] = useState(
    () => window.matchMedia("(min-width: 1280px)").matches,
  );
  useEffect(() => {
    const media = window.matchMedia("(min-width: 1280px)");
    const update = () => setWide(media.matches);
    media.addEventListener("change", update);
    return () => media.removeEventListener("change", update);
  }, []);
  return wide ? (
    <DesktopReview onClose={onClose} restoreFocus={restoreFocus}>
      {children}
    </DesktopReview>
  ) : (
    <Dialog
      label="Transaction review"
      onClose={onClose}
      restoreFocus={restoreFocus}
    >
      {children}
    </Dialog>
  );
}

function DesktopReview({
  onClose,
  restoreFocus,
  children,
}: {
  onClose: () => void;
  restoreFocus: () => void;
  children: ReactNode;
}) {
  const ref = useRef<HTMLElement>(null);
  const returnFocus = useRef(restoreFocus);
  returnFocus.current = restoreFocus;
  useLayoutEffect(() => {
    ref.current?.querySelector<HTMLButtonElement>("button")?.focus();
    return () => returnFocus.current();
  }, []);
  return (
    <aside
      ref={ref}
      className="transaction-review"
      aria-label="Transaction review"
      onKeyDown={(event) => {
        if (event.key === "Escape") {
          event.stopPropagation();
          onClose();
        }
      }}
    >
      {children}
    </aside>
  );
}
