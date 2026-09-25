import { useEffect, useRef, useState } from "react";
import { command } from "./api";
import type { ReviewDecisionPage } from "./review-history-types";
import "./review-history.css";

const pageSize = 50;
const historyLimit = 100;
type Position = { cursor: string | null; offset: number };
type Attempt = { position: Position; previous: Position[] };
const first: Attempt = { position: { cursor: null, offset: 0 }, previous: [] };
const labels = {
  accepted: "Reviewed",
  pending: "Edited · review required",
  rejected: "Rejected",
  deferred: "Deferred",
};

/** Only FindingReview mounts this reader. Whole-source citations are not decision targets. */
export function FindingReviewHistory({
  findingId,
  revision,
  busy,
  onRefresh,
}: {
  findingId: string;
  revision: number;
  busy: boolean;
  onRefresh: () => Promise<boolean>;
}) {
  const [refreshing, setRefreshing] = useState(false);
  const [refreshError, setRefreshError] = useState("");
  const [reload, setReload] = useState(0);
  const mounted = useRef(true);
  useEffect(() => {
    mounted.current = true;
    return () => {
      mounted.current = false;
    };
  }, []);
  async function refresh() {
    setRefreshing(true);
    setRefreshError("");
    try {
      const ok = await onRefresh();
      if (!mounted.current) return;
      if (ok) setReload((value) => value + 1);
      else
        setRefreshError(
          "Workspace refresh failed. History has not been revalidated.",
        );
    } catch (cause) {
      if (mounted.current) setRefreshError(String(cause));
    } finally {
      if (mounted.current) setRefreshing(false);
    }
  }
  return (
    <section
      className="finding-review-history"
      aria-label="Finding review history"
    >
      <div className="inline-actions">
        <h4>Review history</h4>
        <button
          className="button"
          disabled={busy || refreshing}
          onClick={() => void refresh()}
        >
          {refreshing
            ? "Refreshing workspace…"
            : "Refresh workspace and history"}
        </button>
      </div>
      {refreshError && (
        <p className="alert error" role="alert">
          {refreshError}
        </p>
      )}
      <HistoryPage
        key={`${findingId}:${revision}:${reload}`}
        findingId={findingId}
        revision={revision}
        disabled={busy || refreshing}
      />
    </section>
  );
}

/** A revision/target change remounts this state, including A → B → A selection. */
function HistoryPage({
  findingId,
  revision,
  disabled,
}: {
  findingId: string;
  revision: number;
  disabled: boolean;
}) {
  const [page, setPage] = useState<ReviewDecisionPage | null>(null);
  const [attempt, setAttempt] = useState<Attempt>(first);
  const [loading, setLoading] = useState(true);
  const [error, setError] = useState("");
  const generation = useRef(0);
  const mounted = useRef(true);
  const query = useRef<string | null>(null);
  const status = useRef<HTMLParagraphElement>(null);

  async function read(next: Attempt, restoreFocus = false) {
    const opener = restoreFocus ? document.activeElement : null;
    const ticket = ++generation.current;
    setAttempt(next);
    setLoading(true);
    setError("");
    setPage(null);
    try {
      const result = await command<ReviewDecisionPage>({
        action: "page_review_decisions",
        request: {
          target_id: findingId,
          page_size: pageSize,
          cursor: next.position.cursor,
        },
        expected_revision: revision,
      });
      if (!mounted.current || ticket !== generation.current) return;
      // These are response/selection bindings, not duplicated canonical review rules.
      if (
        result.schema_version !== 1 ||
        result.workspace_revision !== revision ||
        result.target_id !== findingId ||
        result.resolved_target_kind !== "finding" ||
        result.rows.length > pageSize ||
        result.rows.some((row) => row.target_id !== findingId) ||
        next.position.offset + result.rows.length > result.scope_count ||
        (query.current !== null && result.query_sha256 !== query.current)
      ) {
        throw new Error(
          "History response does not match this finding and revision. Refresh the workspace.",
        );
      }
      query.current = result.query_sha256;
      setPage(result);
    } catch (cause) {
      if (mounted.current && ticket === generation.current)
        setError(String(cause));
    } finally {
      if (mounted.current && ticket === generation.current) {
        setLoading(false);
        if (opener)
          requestAnimationFrame(() => {
            if (
              mounted.current &&
              ticket === generation.current &&
              (document.activeElement === opener ||
                document.activeElement === document.body)
            ) {
              status.current?.focus();
            }
          });
      }
    }
  }
  useEffect(() => {
    mounted.current = true;
    void read(first);
    return () => {
      mounted.current = false;
      generation.current++;
    };
  }, []);

  const locked = disabled || loading;
  const offset = attempt.position.offset;
  const earlierHistoryUnavailable = attempt.previous[0]?.offset > 0;
  return (
    <div aria-busy={loading}>
      <p className="muted">
        Canonical record order · up to 50 decisions per page · revision{" "}
        {revision}
      </p>
      <p ref={status} tabIndex={-1} role="status">
        {loading
          ? "Loading finding review history…"
          : page
            ? page.scope_count === 0
              ? "No decision recorded."
              : `${offset + 1}–${offset + page.rows.length} of ${page.scope_count} recorded decisions`
            : "Finding review history could not be loaded."}
      </p>
      {error && (
        <div>
          <p className="alert error" role="alert">
            History unavailable: {error}
          </p>
          <p className="muted">
            No empty-history conclusion can be drawn. A changed revision
            requires a workspace refresh.
          </p>
          <button
            className="button"
            disabled={locked}
            onClick={() => void read(attempt, true)}
          >
            Retry history read
          </button>
        </div>
      )}
      {page && (
        <>
          <ol className="review-history-rows" start={offset + 1}>
            {page.rows.map((decision) => (
              <li className="list-card" key={decision.id}>
                <span className={`pill ${decision.state}`}>
                  {labels[decision.state]}
                </span>
                <p className="preserve-lines">{decision.reason}</p>
                <p className="muted">
                  <time dateTime={decision.at}>{decision.at}</time>
                </p>
                <code>{decision.id}</code>
              </li>
            ))}
          </ol>
        </>
      )}
      <div className="inline-actions" aria-label="Review history pages">
        <button
          className="button"
          disabled={locked || !page || offset === 0}
          onClick={() => void read(first, true)}
        >
          First history page
        </button>
        <button
          className="button"
          disabled={locked || !page || !attempt.previous.length}
          onClick={() => {
            const previous = attempt.previous.at(-1);
            if (previous)
              void read(
                {
                  position: previous,
                  previous: attempt.previous.slice(0, -1),
                },
                true,
              );
          }}
        >
          Previous history page
        </button>
        <button
          className="button"
          disabled={locked || !page?.next_cursor}
          onClick={() => {
            if (page?.next_cursor)
              void read(
                {
                  position: {
                    cursor: page.next_cursor,
                    offset: offset + page.rows.length,
                  },
                  previous: [...attempt.previous, attempt.position].slice(
                    -historyLimit,
                  ),
                },
                true,
              );
          }}
        >
          Next history page
        </button>
      </div>
      {earlierHistoryUnavailable && (
        <p className="muted">
          Back navigation retains the last 100 page positions. First history
          page remains available.
        </p>
      )}
    </div>
  );
}
