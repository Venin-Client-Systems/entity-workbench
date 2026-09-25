import type { CsvPolicy } from "./transaction-csv";
import type { ReviewCounts } from "./types";
export type TransactionOutputFormat = "json" | "csv";
export function TransactionExportOptions({
  format,
  policy,
  onFormat,
  onPolicy,
  revision,
  count,
  counts,
  disabled,
  exporting,
  onExport,
}: {
  format: TransactionOutputFormat;
  policy: CsvPolicy;
  onFormat: (value: TransactionOutputFormat) => void;
  onPolicy: (value: CsvPolicy) => void;
  revision: number;
  count: number | null;
  counts: ReviewCounts | null;
  disabled: boolean;
  exporting: boolean;
  onExport: () => void;
}) {
  return (
    <section
      className="transaction-export-options"
      aria-label="Complete transaction export"
    >
      <h3>Complete transaction export</h3>
      <p>
        {count === null
          ? "Applied-scope count unavailable."
          : `${count} selected ${count === 1 ? "row" : "rows"} across all pages · captured workspace revision ${revision}`}
      </p>
      {counts && (
        <p className="muted">
          Before the review filter: {counts.accepted} accepted ·{" "}
          {counts.pending} pending · {counts.rejected} rejected ·{" "}
          {counts.deferred} deferred.
        </p>
      )}
      <div className="ledger-export-selectors">
        <label>
          Output format
          <select
            aria-label="Transaction output format"
            value={format}
            disabled={exporting}
            onChange={(e) =>
              onFormat(e.target.value as TransactionOutputFormat)
            }
          >
            <option value="json">JSON — exact canonical values</option>
            <option value="csv">CSV — visible typed literals</option>
          </select>
        </label>
        {format === "csv" && (
          <label>
            Non-accepted selected rows
            <select
              aria-label="Non-accepted selected rows — CSV review policy"
              value={policy}
              disabled={exporting}
              onChange={(e) => onPolicy(e.target.value as CsvPolicy)}
            >
              <option value="reject">
                Refuse the whole export if any are selected
              </option>
              <option value="allow_selected">
                Include all selected review states explicitly
              </option>
            </select>
          </label>
        )}
      </div>
      {format === "csv" ? (
        <>
          <div className="alert" role="note">
            <strong>CSV preserves values as visibly tagged text.</strong>
            <p>
              Amounts and dates are not spreadsheet numeric/date cells. Leading
              zeros and exact decimal digits remain text; prefixes are part of
              the file.
            </p>
            <p>
              <code>text:000042</code> · <code>decimal:-0.10000001</code> ·{" "}
              <code>date:2024-02-29</code> · <code>text:=SUM(A1)</code>
            </p>
            <p>
              Missing values use <code>null</code>; present empty text uses{" "}
              <code>text:</code>. Keep the prefixes when opening untrusted text.
              Removing them can change interpretation; no universal
              spreadsheet-safety guarantee is made.
            </p>
          </div>
          <p>
            {policy === "reject"
              ? "The whole CSV fails if the selected scope contains pending, deferred or rejected rows. No rows are silently dropped; narrow the review filter or explicitly choose to include them."
              : "All review states in the applied scope will be included. This choice does not accept or change any transaction."}
          </p>
          <p className="muted">
            Typed-literal v1 · UTF-8 with BOM · quoted comma-separated fields ·
            CRLF records. Source anchors, review states and exact values are
            retained. JSON remains available for canonical machine-readable
            values.
          </p>
        </>
      ) : (
        <p className="muted">
          JSON retains the complete selected canonical rows, exact decimal
          strings, source anchors and review states.
        </p>
      )}
      <button
        className="button subtle"
        type="button"
        disabled={disabled}
        onClick={onExport}
      >
        {exporting
          ? "Preparing complete export…"
          : format === "csv"
            ? "Export CSV"
            : "Export JSON"}
      </button>
    </section>
  );
}
