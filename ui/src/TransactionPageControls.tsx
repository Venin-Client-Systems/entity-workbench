import { useEffect, useRef, useState } from "react";
import { command } from "./api";
import { CitationReadLane as ReadLane } from "./citation-read-lane";
import {
  validatePage,
  type SearchPage,
  type TransferPage,
} from "./transaction-ledger-types";

/** A lifetime-owned lane survives query/revision changes; obsolete replies never render. */
export function useTransactionPage(
  payload: Record<string, unknown>,
  revision: number,
  limit: number,
  retry = 0,
  offset = 0,
) {
  const key = JSON.stringify({ payload, retry, offset });
  const latest = useRef({ key, generation: 0 });
  if (latest.current.key !== key)
    latest.current = { key, generation: latest.current.generation + 1 };
  const generation = latest.current.generation;
  const binding = useRef<{ family: string; hash: string } | null>(null);
  const [lane] = useState(() => new ReadLane<SearchPage>());
  const [result, setResult] = useState<{
    generation: number;
    value: SearchPage;
  } | null>(null);
  const [failure, setFailure] = useState<{
    generation: number;
    message: string;
  } | null>(null);
  useEffect(() => {
    lane.open();
    return () => lane.close();
  }, [lane]);
  useEffect(() => {
    let active = true;
    lane.clearPending();
    const request = JSON.parse(key).payload;
    const familyRequest = structuredClone(request);
    const window = familyRequest.request.page ?? familyRequest.request;
    const requestedCursor = window.cursor;
    window.cursor = null;
    const family = JSON.stringify(familyRequest);
    void lane
      .read(() => command<SearchPage>(request))
      .then((value) => {
        if (!active || latest.current.generation !== generation) return;
        validatePage(value, revision, limit);
        const p = value.page;
        if (
          offset + p.rows.length > p.selected_count ||
          (p.rows.length === 0 && p.selected_count !== 0) ||
          (p.next_cursor !== null) !==
            offset + p.rows.length < p.selected_count ||
          (p.next_cursor !== null &&
            (p.next_cursor.length > 2048 ||
              p.next_cursor === requestedCursor)) ||
          (binding.current?.family === family &&
            binding.current.hash !== p.query_sha256)
        )
          throw new Error(
            "Transaction continuation did not match this scope and row range.",
          );
        if (request.action === "page_transfer_candidates") {
          const transfer = value as TransferPage;
          if (
            transfer.target_id !== request.request.target_id ||
            transfer.target_version !== request.request.expected_target_version
          )
            throw new Error("Transfer target identity changed.");
        }
        binding.current = { family, hash: p.query_sha256 };
        setResult({ generation, value });
        setFailure(null);
      })
      .catch((cause) => {
        if (active && latest.current.generation === generation)
          setFailure({ generation, message: String(cause) });
      });
    return () => {
      active = false;
      lane.clearPending();
    };
  }, [key, revision, limit, lane, generation, offset]);
  return {
    value: result?.generation === generation ? result.value : null,
    error: failure?.generation === generation ? failure.message : "",
  };
}
export type PagePosition = { cursor: string | null; offset: number };
export const firstPosition = (): PagePosition => ({ cursor: null, offset: 0 });
export function PageControls({
  label,
  value,
  error,
  position,
  history,
  pending,
  move,
  retry,
  refresh,
}: {
  label: string;
  value: SearchPage | null;
  error: string;
  position: PagePosition;
  history: PagePosition[];
  pending: boolean;
  move: (position: PagePosition, history: PagePosition[]) => void;
  retry: () => void;
  refresh: () => void;
}) {
  const status = useRef<HTMLParagraphElement>(null),
    pendingFocus = useRef<{ opener: Element; moved: boolean } | null>(null);
  useEffect(() => {
    const track = (event: FocusEvent) => {
      const pending = pendingFocus.current;
      if (
        pending &&
        event.target !== pending.opener &&
        event.target !== document.body &&
        event.target !== document.documentElement
      )
        pending.moved = true;
    };
    document.addEventListener("focusin", track);
    return () => {
      document.removeEventListener("focusin", track);
      pendingFocus.current = null;
    };
  }, []);
  useEffect(() => {
    const pending = pendingFocus.current;
    if ((!value && !error) || !pending) return;
    const frame = requestAnimationFrame(() => {
      if (pendingFocus.current !== pending) return;
      pendingFocus.current = null;
      if (
        !pending.moved &&
        (document.activeElement === pending.opener ||
          document.activeElement === document.body ||
          document.activeElement === document.documentElement)
      )
        status.current?.focus();
    });
    return () => cancelAnimationFrame(frame);
  }, [value, error]);
  const act = (action: () => void) => {
    pendingFocus.current = document.activeElement
      ? { opener: document.activeElement, moved: false }
      : null;
    action();
  };
  const p = value?.page;
  return (
    <>
      <p ref={status} tabIndex={-1} role="status">
        {error
          ? `${label} unavailable. No partial page is shown.`
          : !p
            ? `Loading ${label.toLowerCase()}…`
            : `${p.rows.length ? `${position.offset + 1}–${position.offset + p.rows.length}` : "0"} of ${p.selected_count} selected rows · ${p.scope_count} matching rows before review selection`}
      </p>
      {p && (
        <p className="muted">
          Scope review counts: {p.review_counts.accepted} accepted ·{" "}
          {p.review_counts.pending} pending · {p.review_counts.rejected}{" "}
          rejected · {p.review_counts.deferred} deferred. Unicode{" "}
          {value!.matching.unicode_version.join(".")} whole-string lowercase
          literal matching.
        </p>
      )}
      {error && (
        <p className="alert error" role="alert">
          {error}
        </p>
      )}
      <div className="actions">
        <button
          className="button"
          disabled={pending || position.offset === 0}
          onClick={() => act(() => move(firstPosition(), []))}
        >
          First {label.toLowerCase()} page
        </button>
        <button
          className="button"
          disabled={pending || !history.length}
          onClick={() =>
            act(() => move(history[history.length - 1], history.slice(0, -1)))
          }
        >
          Back {label.toLowerCase()} page
        </button>
        <button
          className="button"
          disabled={pending || !p?.next_cursor}
          onClick={() =>
            p &&
            act(() =>
              move(
                {
                  cursor: p.next_cursor,
                  offset: position.offset + p.rows.length,
                },
                [...history, position].slice(-100),
              ),
            )
          }
        >
          Next {label.toLowerCase()} page
        </button>
        {error && (
          <button className="button" onClick={() => act(retry)}>
            Retry {label.toLowerCase()}
          </button>
        )}
        <button className="button" onClick={refresh}>
          Refresh workspace
        </button>
      </div>
      {history[0]?.offset > 0 && (
        <p className="muted">
          Back covers the last 100 pages. First restarts the complete scope;
          Next remains available while more rows exist.
        </p>
      )}
    </>
  );
}
