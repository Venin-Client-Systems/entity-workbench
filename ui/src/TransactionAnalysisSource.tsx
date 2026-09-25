import type { PatternRow } from "./transaction-analysis-types";
import type { Transaction } from "./types";
import { useEffect, useState } from "react";
import { command } from "./api";

/** Fetch only the visible source page, pinned to its originating analysis. */
export function AnalysisSourceRows({
  ids,
  versions,
  annotations,
  contexts,
  revision,
  onInspect,
}: {
  ids: string[];
  versions: ReadonlyMap<string, number>;
  annotations: ReadonlyMap<string, PatternRow>;
  contexts?: ReadonlyMap<string, string>;
  revision: number;
  onInspect: (transaction: Transaction) => void;
}) {
  const keys = ids.map((id) => ({ id, expected_version: versions.get(id) }));
  const valid =
    keys.length > 0 &&
    keys.length <= 25 &&
    keys.every((key) => key.expected_version !== undefined);
  // A stable primitive binds both asynchronous replies and visible state to the
  // exact page, versions and revision, even before the next effect has run.
  const selection = JSON.stringify({ rows: keys, revision });
  return (
    <SelectedSourcePage
      key={selection}
      selection={selection}
      valid={valid}
      versions={versions}
      annotations={annotations}
      contexts={contexts}
      onInspect={onInspect}
    />
  );
}

/** Remount on every selection change, including a return to an earlier page. */
function SelectedSourcePage({
  selection,
  valid,
  versions,
  annotations,
  contexts,
  onInspect,
}: {
  selection: string;
  valid: boolean;
  versions: ReadonlyMap<string, number>;
  annotations: ReadonlyMap<string, PatternRow>;
  contexts?: ReadonlyMap<string, string>;
  onInspect: (transaction: Transaction) => void;
}) {
  const [attempt, setAttempt] = useState(0);
  const [state, setState] = useState<{
    selection: string;
    attempt: number;
    rows?: Transaction[];
    error?: string;
  } | null>(null);
  useEffect(() => {
    let active = true;
    if (!valid) return;
    const selected = JSON.parse(selection) as {
      rows: { id: string; expected_version: number }[];
      revision: number;
    };
    void command<{
      schema_version: number;
      workspace_revision: number;
      rows: Transaction[];
    }>({
      action: "read_transaction_sources",
      request: { rows: selected.rows },
      expected_revision: selected.revision,
    })
      .then((value) => {
        if (!active) return;
        if (
          value.schema_version !== 1 ||
          value.workspace_revision !== selected.revision ||
          !Array.isArray(value.rows) ||
          value.rows.length !== selected.rows.length ||
          value.rows.some(
            (row, index) =>
              row.id !== selected.rows[index].id ||
              row.version !== selected.rows[index].expected_version,
          )
        )
          throw new Error(
            "Source response does not match the selected analysis rows.",
          );
        setState({ selection, attempt, rows: value.rows });
      })
      .catch((cause: unknown) => {
        if (active) setState({ selection, attempt, error: String(cause) });
      });
    return () => {
      active = false;
    };
  }, [selection, valid, attempt]);
  const current =
    state?.selection === selection && state.attempt === attempt ? state : null;
  if (!valid)
    return (
      <p className="error" role="alert">
        Source versions are unavailable. Close and recalculate.
      </p>
    );
  if (current?.error)
    return (
      <div className="patterns-warning" role="alert">
        <p>
          Source rows could not be verified. Refresh the workspace and
          recalculate if it changed.
        </p>
        <p>{current.error}</p>
        <button
          className="button"
          onClick={() => setAttempt((value) => value + 1)}
        >
          Retry source rows
        </button>
      </div>
    );
  if (!current?.rows)
    return <p role="status">Verifying selected source rows…</p>;
  return (
    <>
      {current.rows.map((row) => (
        <AnalysisSourceRow
          key={row.id}
          id={row.id}
          transaction={row}
          expectedVersion={versions.get(row.id)}
          annotation={annotations.get(row.id)}
          context={contexts?.get(row.id)}
          onInspect={onInspect}
        />
      ))}
    </>
  );
}

/** Shared presentation only: Rust supplies the revision-bound rules and source versions. */
export function AnalysisSourceRow({
  id,
  transaction: t,
  expectedVersion,
  annotation: a,
  context,
  onInspect,
}: {
  id: string;
  transaction: Transaction | undefined;
  expectedVersion: number | undefined;
  annotation?: PatternRow;
  context?: string;
  onInspect: (transaction: Transaction) => void;
}) {
  return t && expectedVersion !== undefined && t.version === expectedVersion ? (
    <article className="patterns-source">
      <strong>
        {t.date} / {t.account} / {t.amount} {t.currency}
      </strong>
      {context && <p className="flow-source-context">{context}</p>}
      <p>{t.description}</p>
      <p>
        {a && !context ? `${a.disposition.replaceAll("_", " ")} · ` : ""}
        {t.review} · version {expectedVersion} · source row{" "}
        {t.anchor.row ?? "see anchor"}
      </p>
      <code>{t.id}</code>
      {a?.has_duplicate_candidates && (
        <p>Possible duplicate: retained in its review-state partition.</p>
      )}
      {a?.verified_transfer_peer && (
        <p>
          Verified reviewed transfer peer:{" "}
          <code>{a.verified_transfer_peer}</code>
        </p>
      )}
      {a?.unverified_transfer_match && (
        <p className="patterns-warning">
          Recorded transfer link is not a verified reviewed pair. It was not
          excluded.
        </p>
      )}
      {(a?.cash_rule || a?.refund_rule) && (
        <p>
          Heuristic: {(a.cash_rule ?? a.refund_rule)!.replaceAll("_", " ")}.
        </p>
      )}
      <button className="button" onClick={() => onInspect(t)}>
        Inspect source and review row {t.anchor.row ?? t.id}
      </button>
    </article>
  ) : (
    <p className="error">
      Source row {id} is unavailable at the recorded version. Refresh and
      recalculate.
    </p>
  );
}

export function Pager({
  page,
  total,
  setPage,
  label,
}: {
  page: number;
  total: number;
  setPage: (page: number) => void;
  label: string;
}) {
  const pageSize = 25;
  return total > pageSize ? (
    <div className="patterns-pager">
      <span>
        {page * pageSize + 1}–{Math.min((page + 1) * pageSize, total)} of{" "}
        {total} {label}
      </span>
      <button
        className="button"
        disabled={page === 0}
        onClick={() => setPage(page - 1)}
      >
        Previous {label}
      </button>
      <button
        className="button"
        disabled={(page + 1) * pageSize >= total}
        onClick={() => setPage(page + 1)}
      >
        Next {label}
      </button>
    </div>
  ) : null;
}
