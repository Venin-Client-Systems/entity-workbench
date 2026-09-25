import { useEffect, useState } from "react";
import { command } from "./api";
import { ReviewSurface } from "./ReviewSurface";
import { TransferCandidates } from "./TransferCandidates";
import type { Anchor, Evidence, SourceExcerpt, Transaction } from "./types";
import {
  outsideStructuredScope,
  type LedgerScope,
  type TransactionSelection,
} from "./transaction-ledger-types";

export function TransactionReview({
  selection,
  currentRevision,
  scope,
  visibleIds,
  evidence,
  busy,
  run,
  onSource,
  close,
}: {
  selection: TransactionSelection;
  currentRevision: number;
  scope: LedgerScope | null;
  visibleIds: string[];
  evidence: Evidence[];
  busy: boolean;
  run: (input: Record<string, unknown>) => Promise<boolean>;
  onSource: (evidence: Evidence, anchor?: Anchor) => void;
  close: () => void;
}) {
  const { row, revision } = selection;
  const [why, setWhy] = useState(""),
    [corrected, setCorrected] = useState(row.amount),
    [transfer, setTransfer] = useState<Transaction | null>(null);
  const [refreshError, setRefreshError] = useState("");
  const stale = revision !== currentRevision;
  const source = evidence.find((item) => item.id === row.anchor.evidence_id);
  const act = async (action: Record<string, unknown>) => {
    if (busy || stale) return;
    if (await run({ ...action, expected_revision: revision })) close();
  };
  const refresh = async () => {
    setRefreshError("");
    if (!(await run({ action: "view" })))
      setRefreshError(
        "Workspace refresh failed. Review draft and revision are unchanged.",
      );
  };
  return (
    <ReviewSurface
      onClose={close}
      restoreFocus={() =>
        (
          document.getElementById(`transaction-${row.id}`) ??
          document.getElementById("transaction-ledger-heading")
        )?.focus()
      }
    >
      <button className="close" aria-label="Close review" onClick={close}>
        ×
      </button>
      <p className="eyebrow">
        TRANSACTION REVIEW · VERSION {row.version} · REV {revision}
      </p>
      <h2>{row.description}</h2>
      <div className="amount-large">
        {row.amount} <span>{row.currency}</span>
      </div>
      <p>
        {row.date} · {row.account} · {row.review}
      </p>
      {scope && outsideStructuredScope(row, scope.filter) ? (
        <p className="alert" role="status">
          Selected transaction is outside the current filters.
        </p>
      ) : (
        !visibleIds.includes(row.id) && (
          <p className="alert" role="status">
            Selected transaction is not on this page. Text-query membership is
            unverified.
          </p>
        )
      )}
      {stale && (
        <p className="alert" role="status">
          Workspace changed. This review remains at revision {revision}; reason,
          correction and counterpart are preserved. Close and reopen a verified
          current row before deciding.
        </p>
      )}
      {refreshError && (
        <p className="alert error" role="alert">
          {refreshError}
        </p>
      )}
      <div className="disclosure">
        <strong>Source anchor</strong>
        <p>
          {row.anchor.sheet}, row {row.anchor.row}, column {row.anchor.column}
        </p>
        <button
          className="text-button"
          disabled={!source}
          onClick={() => source && onSource(source, row.anchor)}
        >
          Inspect preserved source ↗
        </button>
      </div>
      <TransactionExcerpt key={row.id + ":" + row.version} transaction={row} />
      <label>
        Decision reason
        <input
          aria-label="Transaction decision reason"
          value={why}
          onChange={(event) => setWhy(event.target.value)}
        />
      </label>
      <div className="actions">
        {(["accepted", "rejected", "deferred"] as const).map((state) => (
          <button
            className={`button ${state === "accepted" ? "primary" : ""}`}
            key={state}
            disabled={busy || stale || !why || !!row.transfer_peer}
            onClick={() =>
              void act({
                action: "review_transaction",
                id: row.id,
                state,
                reason: why,
              })
            }
          >
            {state === "accepted"
              ? "Accept"
              : state === "rejected"
                ? "Reject"
                : "Defer"}
          </button>
        ))}
      </div>
      <hr />
      <label>
        Corrected amount
        <input
          aria-label="Corrected amount"
          value={corrected}
          onChange={(event) => setCorrected(event.target.value)}
        />
      </label>
      <button
        className="button"
        disabled={busy || stale || !why || corrected === row.amount}
        onClick={() =>
          void act({
            action: "correct_transaction",
            id: row.id,
            amount: corrected,
            reason: why,
          })
        }
      >
        Save correction for review
      </button>
      <p className="muted">
        Original evidence stays intact. A correction returns the transaction to
        pending review and invalidates dependent findings.
      </p>
      {stale ? (
        <button
          className="button"
          disabled={busy}
          onClick={() => void refresh()}
        >
          Refresh workspace
        </button>
      ) : (
        <TransferCandidates
          target={row}
          revision={revision}
          selected={transfer}
          disabled={busy}
          onSelect={setTransfer}
          onRefresh={() => void refresh()}
        />
      )}
      <button
        className="button"
        disabled={busy || stale || !why || !transfer}
        onClick={() =>
          transfer &&
          void act({
            action: "match_transfer",
            first: row.id,
            second: transfer.id,
            reason: why,
          })
        }
      >
        Match internal transfer
      </button>
    </ReviewSurface>
  );
}
function TransactionExcerpt({ transaction }: { transaction: Transaction }) {
  const [excerpt, setExcerpt] = useState<SourceExcerpt | null>(null),
    [error, setError] = useState("");
  useEffect(() => {
    let active = true;
    void command<SourceExcerpt>({
      action: "inspect_source",
      anchor: transaction.anchor,
    })
      .then((value) => {
        if (active) setExcerpt(value);
      })
      .catch((cause) => {
        if (active) setError(String(cause));
      });
    return () => {
      active = false;
    };
  }, [transaction.anchor]);
  return (
    <section
      className="original-excerpt"
      aria-label="Original transaction excerpt"
    >
      <h3>Preserved source value</h3>
      {error ? (
        <p role="alert">{error}</p>
      ) : excerpt ? (
        <>
          <pre className="source-quote">{excerpt.quote}</pre>
          <p className="muted">
            {excerpt.location} · original evidence · inspected at revision{" "}
            {excerpt.workspace_revision}
          </p>
        </>
      ) : (
        <p role="status">Resolving source anchor…</p>
      )}
    </section>
  );
}
