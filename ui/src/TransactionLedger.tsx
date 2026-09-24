import { useEffect, useRef, useState } from "react";
import type { ReviewState, Transaction } from "./types";
import { TransactionFacetSelect } from "./TransactionFacetSelect";
import {
  PageControls,
  firstPosition,
  useTransactionPage,
  type PagePosition,
} from "./TransactionPageControls";
import { BalanceLabel, useTransactionBalances } from "./TransactionBalances";
import { emptyFilter, type LedgerScope } from "./transaction-ledger-types";
import { readTransactionExport } from "./transaction-export";
import "./transaction-ledger.css";

export function TransactionLedger({
  revision,
  currency,
  review,
  pivot,
  selectedId,
  onInspect,
  onScope,
  onRefresh,
  download,
}: {
  revision: number;
  currency: string | null;
  review: ReviewState | null;
  pivot: number;
  selectedId?: string;
  onInspect: (row: Transaction, revision: number) => void;
  onScope: (scope: LedgerScope, visibleIds: string[]) => void;
  onRefresh: () => Promise<boolean>;
  download: (content: string, name: string, type: string) => Promise<void>;
}) {
  const initial = (): LedgerScope => ({
    query: "",
    filter: { ...emptyFilter(), currency, review },
    order: "date_ascending",
    page_size: 100,
  });
  const [draft, setDraft] = useState<LedgerScope>(initial),
    [scope, setScope] = useState<LedgerScope>(initial);
  const [position, setPosition] = useState(firstPosition),
    [history, setHistory] = useState<PagePosition[]>([]);
  const [attempt, setAttempt] = useState(0),
    [balanceAttempt, setBalanceAttempt] = useState(0);
  const [exporting, setExporting] = useState(false),
    [exportError, setExportError] = useState(""),
    [notice, setNotice] = useState("");
  const [refreshError, setRefreshError] = useState("");
  const lifetime = useRef(true),
    activeExport = useRef(false);
  const latest = useRef({ revision, scope });
  latest.current = { revision, scope };
  const previousPivot = useRef(pivot),
    previousRevision = useRef(revision);
  useEffect(() => {
    lifetime.current = true;
    return () => {
      lifetime.current = false;
    };
  }, []);
  useEffect(() => {
    if (previousPivot.current === pivot) return;
    previousPivot.current = pivot;
    setScope((old) => {
      const next = { ...old, filter: { ...old.filter, currency } };
      setDraft(next);
      return next;
    });
    setPosition(firstPosition());
    setHistory([]);
  }, [pivot, currency]);
  // A known new workspace revision re-reads the first page of the applied scope.
  const effectivePosition =
    previousRevision.current === revision ? position : firstPosition();
  useEffect(() => {
    previousRevision.current = revision;
    setPosition(firstPosition());
    setHistory([]);
    setExportError("");
    setNotice("");
  }, [revision]);
  const read = useTransactionPage(
    {
      action: "search_transactions",
      request: {
        query: scope.query,
        page: {
          filter: scope.filter,
          order: scope.order,
          page_size: scope.page_size,
          cursor: effectivePosition.cursor,
        },
      },
      expected_revision: revision,
    },
    revision,
    scope.page_size,
    attempt,
    effectivePosition.offset,
  );
  const balances = useTransactionBalances(
    read.value?.page.rows,
    revision,
    balanceAttempt,
  );
  const onScopeRef = useRef(onScope);
  onScopeRef.current = onScope;
  useEffect(() => {
    if (read.value)
      onScopeRef.current(
        scope,
        read.value.page.rows.map((row) => row.id),
      );
  }, [read.value, scope]);
  const invalid = new TextEncoder().encode(draft.query).length > 1024;
  const dirty = JSON.stringify(draft) !== JSON.stringify(scope);
  const loading = !read.value && !read.error;
  const changeFilter = (
    key: keyof LedgerScope["filter"],
    value: string | null,
  ) => setDraft((old) => ({ ...old, filter: { ...old.filter, [key]: value } }));
  const refresh = async () => {
    setRefreshError("");
    if (!(await onRefresh()) && lifetime.current)
      setRefreshError("Workspace refresh failed. Page revision is unchanged.");
  };
  const exportAll = async () => {
    if (!read.value || activeExport.current) return;
    const snapshot = {
      scope,
      revision,
      count: read.value.page.selected_count,
      matching: read.value.matching,
    };
    activeExport.current = true;
    setExporting(true);
    setExportError("");
    setNotice("");
    try {
      const value = await readTransactionExport(
        snapshot.scope,
        snapshot.revision,
        snapshot.count,
        snapshot.matching,
      );
      if (
        !lifetime.current ||
        latest.current.revision !== snapshot.revision ||
        latest.current.scope !== snapshot.scope
      )
        throw new Error(
          "Ledger changed while preparing export. Apply and export the current scope again.",
        );
      await download(value.json, "transactions.json", "application/json");
      if (lifetime.current)
        setNotice(
          `Prepared ${value.row_count} matching rows at revision ${value.workspace_revision} for export.`,
        );
    } catch (cause) {
      if (lifetime.current) setExportError(String(cause));
    } finally {
      activeExport.current = false;
      if (lifetime.current) setExporting(false);
    }
  };
  return (
    <section
      className="panel transaction-ledger-panel"
      aria-label="Paged transaction ledger"
    >
      <div className="panel-heading">
        <h2 id="transaction-ledger-heading" tabIndex={-1}>
          Transaction ledger
        </h2>
        <span className="pill">REV {revision}</span>
      </div>
      <form
        onSubmit={(event) => {
          event.preventDefault();
          if (invalid) return;
          setScope(draft);
          setPosition(firstPosition());
          setHistory([]);
          setAttempt((n) => n + 1);
          setNotice("");
        }}
      >
        <div className="ledger-controls">
          <label>
            Literal text
            <input
              aria-label="Filter transactions"
              placeholder="Description, account or date…"
              value={draft.query}
              onChange={(event) =>
                setDraft((old) => ({ ...old, query: event.target.value }))
              }
            />
          </label>
          <TransactionFacetSelect
            kind="account"
            label="Account filter"
            revision={revision}
            value={draft.filter.account}
            onChange={(value) => changeFilter("account", value)}
          />
          <TransactionFacetSelect
            kind="currency"
            label="Currency filter"
            revision={revision}
            value={draft.filter.currency}
            onChange={(value) => changeFilter("currency", value)}
          />
          <label>
            Review filter
            <select
              aria-label="Review filter"
              value={draft.filter.review ?? "all"}
              onChange={(event) =>
                changeFilter(
                  "review",
                  event.target.value === "all" ? null : event.target.value,
                )
              }
            >
              {["all", "pending", "accepted", "rejected", "deferred"].map(
                (value) => (
                  <option key={value}>{value}</option>
                ),
              )}
            </select>
          </label>
          <label>
            From date
            <input
              aria-label="Ledger from date"
              type="date"
              value={draft.filter.date_from ?? ""}
              onChange={(event) =>
                changeFilter("date_from", event.target.value || null)
              }
            />
          </label>
          <label>
            Through date
            <input
              aria-label="Ledger through date"
              type="date"
              value={draft.filter.date_to ?? ""}
              onChange={(event) =>
                changeFilter("date_to", event.target.value || null)
              }
            />
          </label>
          <label>
            Date order
            <select
              aria-label="Ledger date order"
              value={draft.order}
              onChange={(event) =>
                setDraft((old) => ({
                  ...old,
                  order: event.target.value as LedgerScope["order"],
                }))
              }
            >
              <option value="date_ascending">Date ascending</option>
              <option value="date_descending">Date descending</option>
            </select>
          </label>
          <label>
            Rows per request
            <select
              aria-label="Ledger rows per request"
              value={draft.page_size}
              onChange={(event) =>
                setDraft((old) => ({
                  ...old,
                  page_size: Number(event.target.value),
                }))
              }
            >
              {[25, 50, 100, 200].map((size) => (
                <option key={size}>{size}</option>
              ))}
            </select>
          </label>
        </div>
        <div className="actions">
          <button className="button primary" disabled={invalid} type="submit">
            Apply ledger filters
          </button>
          <button
            className="button"
            type="button"
            onClick={() => setDraft({ ...initial(), filter: emptyFilter() })}
          >
            Clear draft filters
          </button>
          <button
            className="button subtle"
            type="button"
            disabled={!read.value || exporting || dirty}
            onClick={() => void exportAll()}
          >
            {exporting ? "Preparing complete export…" : "Export JSON"}
          </button>
        </div>
      </form>
      {dirty && (
        <p className="alert" role="status">
          Draft filters are not applied. Existing page and selection retain
          their applied scope.
        </p>
      )}
      {invalid && (
        <p className="alert error" role="alert">
          Transaction query exceeds 1,024 UTF-8 bytes; it has not been
          submitted.
        </p>
      )}
      <p className="context-note">
        Requested scope: {scope.filter.date_from ?? "earliest"} →{" "}
        {scope.filter.date_to ?? "latest"} inclusive · account{" "}
        {scope.filter.account ?? "all"} · currency{" "}
        {scope.filter.currency ?? "all"} · review {scope.filter.review ?? "all"}{" "}
        ·{" "}
        {scope.order === "date_ascending"
          ? "date ascending"
          : "date descending"}
        ; equal dates retain source insertion order. Literal query:{" "}
        <code>{scope.query || "(empty)"}</code>.
      </p>
      <PageControls
        label="Ledger"
        value={read.value}
        error={read.error}
        position={effectivePosition}
        history={previousRevision.current === revision ? history : []}
        pending={loading}
        move={(next, trail) => {
          setPosition(next);
          setHistory(trail);
        }}
        retry={() => setAttempt((n) => n + 1)}
        refresh={() => void refresh()}
      />
      {(refreshError || exportError) && (
        <p className="alert error" role="alert">
          {refreshError || exportError}
        </p>
      )}
      {notice && <p role="status">{notice}</p>}
      <p className="muted">
        Export includes every row matching the successful applied scope, not
        just this page. Unsaved filter changes must be applied first. Originals
        are verified by the core; no currency conversion or automatic duplicate
        removal occurs.
      </p>
      {balances.error && (
        <div className="alert" role="status">
          Source-order balance checks unavailable: {balances.error}{" "}
          <button
            className="button"
            onClick={() => setBalanceAttempt((n) => n + 1)}
          >
            Retry balance checks
          </button>
        </div>
      )}
      <div
        className="table-scroll"
        role="region"
        aria-label="Transaction ledger"
        tabIndex={0}
      >
        <table>
          <caption>
            {read.value
              ? `${read.value.page.selected_count} transactions in the applied scope · ${read.value.page.rows.length} returned on this page`
              : "Transaction page unavailable while loading or after an error"}
          </caption>
          <thead>
            <tr>
              <th>Date</th>
              <th>Original description</th>
              <th>Account</th>
              <th className="numeric">Amount</th>
              <th>Checks</th>
              <th>Review</th>
            </tr>
          </thead>
          <tbody>
            {read.value?.page.rows.map((row) => (
              <tr
                key={row.id}
                className={selectedId === row.id ? "selected-row" : undefined}
              >
                <td>{row.date}</td>
                <td>
                  <button
                    className="cell-button"
                    id={`transaction-${row.id}`}
                    onClick={() => onInspect(row, revision)}
                  >
                    {row.description}
                  </button>
                  <small>
                    {row.posting_date ? `Posted ${row.posting_date}` : ""}
                  </small>
                </td>
                <td>
                  <code>{row.account}</code>
                </td>
                <td className="numeric">
                  <strong>{row.amount}</strong>
                  <small>{row.currency}</small>
                </td>
                <td>
                  {row.duplicate_candidates.length > 0 && (
                    <span className="pill warning">Possible duplicate</span>
                  )}
                  <BalanceLabel
                    balance={
                      balances.rows?.find((b) => b.id === row.id)?.balance
                    }
                    unavailable={!!balances.error}
                  />
                  {row.transfer_peer && (
                    <span className="pill">Matched transfer</span>
                  )}
                </td>
                <td>
                  <span className={`pill ${row.review}`}>{row.review}</span>
                </td>
              </tr>
            ))}
          </tbody>
        </table>
      </div>
      {read.value?.page.selected_count === 0 && (
        <p>
          No transactions match this applied selection. Review counts above
          describe the full matching scope.
        </p>
      )}
      <p className="context-note">
        Repeated purchases remain visible. Balances use canonical source row
        order across every review state, independently of date sorting and
        pagination.
      </p>
    </section>
  );
}
