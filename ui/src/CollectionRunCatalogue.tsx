import { useEffect, useRef, useState } from "react";
import { command } from "./api";
import { DurableCollectionSession } from "./durable-collection-session";
import {
  availabilityLabels,
  runLabels,
  validateRunPage,
  type CollectionAvailability,
  type CollectionRunPage,
} from "./durable-collection-types";
import { collectionDate as date } from "./durable-collection-types";
const first = { cursor: null as string | null, offset: 0 };
type Position = typeof first;
export function RunCatalogue({
  revision,
  session,
  onAvailability,
  onSelect,
}: {
  revision: number;
  session: DurableCollectionSession;
  onAvailability: (
    value: {
      revision: number;
      value: CollectionAvailability;
    } | null,
  ) => void;
  onSelect: (id: string) => void;
}) {
  const [position, setPosition] = useState(first),
    [history, setHistory] = useState<Position[]>([]),
    [page, setPage] = useState<CollectionRunPage | null>(null),
    [error, setError] = useState(""),
    [retry, setRetry] = useState(0);
  const range = useRef<HTMLParagraphElement>(null),
    focusAfter = useRef(false);
  useEffect(() => {
    let active = true,
      moved = false;
    onAvailability(null);
    const initial = document.activeElement;
    const focus = (e: FocusEvent) => {
      if (
        e.target !== initial &&
        e.target !== document.body &&
        e.target !== document.documentElement
      )
        moved = true;
    };
    document.addEventListener("focusin", focus);
    setPage(null);
    setError("");
    void session.catalogue
      .read(() =>
        command<CollectionRunPage>({
          action: "page_collection_runs",
          request: { page_size: 25, cursor: position.cursor },
          expected_revision: revision,
        }),
      )
      .then((value) => {
        validateRunPage(value, revision, position.offset);
        if (active) {
          setPage(value);
          onAvailability({ revision, value: value.availability });
          if (
            focusAfter.current &&
            !moved &&
            (document.activeElement === document.body ||
              document.activeElement === document.documentElement ||
              document.activeElement === initial)
          )
            requestAnimationFrame(() => {
              if (active && !moved) range.current?.focus();
            });
        }
      })
      .catch((e) => {
        if (active) setError(String(e));
      })
      .finally(() => {
        focusAfter.current = false;
      });
    return () => {
      active = false;
      document.removeEventListener("focusin", focus);
    };
  }, [position, revision, retry, session, onAvailability]);
  function go(p: Position, h: Position[]) {
    focusAfter.current = true;
    setHistory(h);
    setPosition(p);
  }
  return (
    <>
      {error ? (
        <div role="alert" className="alert error">
          Collection catalogue unavailable: {error}
          <button className="button" onClick={() => setRetry((n) => n + 1)}>
            Retry collection catalogue
          </button>
        </div>
      ) : !page ? (
        <p role="status">Loading collection catalogue…</p>
      ) : (
        <>
          <p role="status">{availabilityLabels[page.availability]}</p>
          <p className="muted">
            Oldest publication first · 25 collections per page · catalogue
            revision {page.workspace_revision}
          </p>
          <div className="durable-pager">
            <p tabIndex={-1} ref={range}>
              {page.scope_count === 0
                ? "No durable collections recorded."
                : `${position.offset + 1}–${position.offset + page.rows.length} of ${page.scope_count} collections`}
            </p>
            <nav aria-label="Collection catalogue pages">
              <button
                className="button"
                disabled={!history.length}
                onClick={() => go(first, [])}
              >
                First
              </button>
              <button
                className="button"
                aria-label="Back to previous collection page"
                disabled={!history.length}
                onClick={() => go(history.at(-1)!, history.slice(0, -1))}
              >
                Back
              </button>
              <button
                className="button"
                disabled={!page.next_cursor || history.length >= 99}
                onClick={() =>
                  go(
                    {
                      cursor: page.next_cursor,
                      offset: position.offset + page.rows.length,
                    },
                    [...history, position],
                  )
                }
              >
                Next
              </button>
            </nav>
          </div>
          {history.length >= 99 && page.next_cursor && (
            <p>Page history limit reached. Return to First to restart.</p>
          )}
          {page.rows.map((run) => (
            <article className="list-card durable-run" key={run.id}>
              <header>
                <span className="pill">{runLabels[run.state]}</span>
                <strong>
                  {run.mode === "synthetic"
                    ? "Synthetic collection"
                    : "Public-web collection"}
                </strong>
              </header>
              <h3>{run.input.urls.join(", ")}</h3>
              <p>
                <code>{run.id}</code>
              </p>
              <p>
                {run.requests_used} / {run.input.max_requests} charged requests
                · {run.pages_retained} pages retained · {run.frontier_remaining}{" "}
                remaining
              </p>
              <p className="muted">Recorded {date(run.updated_at_ms)}</p>
              <button
                className="button"
                onClick={() => onSelect(run.id)}
                aria-label={`Review collection run ${run.id}`}
              >
                Review collection
              </button>
            </article>
          ))}
        </>
      )}
    </>
  );
}
