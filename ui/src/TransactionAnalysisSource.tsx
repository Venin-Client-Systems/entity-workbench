import type { PatternRow } from "./transaction-analysis-types";
import type { Transaction } from "./types";

/** Shared presentation only: Rust supplies the revision-bound rules and source versions. */
export function AnalysisSourceRow({
  id,
  transaction: t,
  expectedVersion,
  annotation: a,
  onInspect,
}: {
  id: string;
  transaction: Transaction | undefined;
  expectedVersion: number | undefined;
  annotation?: PatternRow;
  onInspect: (transaction: Transaction) => void;
}) {
  return t && expectedVersion !== undefined && t.version === expectedVersion ? (
    <article className="patterns-source">
      <strong>
        {t.date} / {t.account} / {t.amount} {t.currency}
      </strong>
      <p>{t.description}</p>
      <p>
        {a ? `${a.disposition.replaceAll("_", " ")} · ` : ""}
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
