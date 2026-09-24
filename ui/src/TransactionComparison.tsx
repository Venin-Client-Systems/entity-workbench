import { useEffect, useMemo, useRef, useState } from "react";
import { command } from "./api";
import { Dialog } from "./Dialog";
import { AnalysisSourceRows, Pager } from "./TransactionAnalysisSource";
import type {
  ComparisonRequest,
  ComparisonResult,
  AmountChange,
  DatePeriod,
  PeriodAccountTotal,
} from "./transaction-comparison-types";
import type { Transaction, Workspace } from "./types";
import "./transaction-patterns.css";
import "./transaction-comparison.css";

type Drill = { label: string; ids: string[]; detail: string };
const periodLabel = (period: DatePeriod) =>
  `${period.from} – ${period.through}`;
function ratio(change: AmountChange) {
  const value = change.relative_change;
  if (value.state === "defined")
    return `${value.numerator} / ${value.denominator} × 100%`;
  if (value.state === "negative_baseline")
    return "No percentage: negative baseline";
  return `No percentage: zero baseline (${value.comparison_is_zero ? "both accepted totals zero" : "comparison nonzero"})`;
}
export function TransactionComparison({
  workspace,
  onInspect,
  onRefresh,
}: {
  workspace: Workspace;
  onInspect: (row: Transaction) => void;
  onRefresh: () => Promise<unknown>;
}) {
  const [draft, setDraft] = useState<ComparisonRequest>({
    baseline: { from: "", through: "" },
    comparison: { from: "", through: "" },
    account: null,
    currency: null,
    transfers: "include",
  });
  const [result, setResult] = useState<ComparisonResult | null>(null);
  const [busy, setBusy] = useState(false),
    [error, setError] = useState(""),
    [invalidated, setInvalidated] = useState(false);
  const [drill, setDrill] = useState<Drill | null>(null),
    [sourcePage, setSourcePage] = useState(0),
    [groupPage, setGroupPage] = useState(0);
  const sequence = useRef(0),
    mounted = useRef(true),
    revision = useRef(workspace.revision),
    handoff = useRef(false);
  const opener = useRef<HTMLElement | null>(null),
    calculateButton = useRef<HTMLButtonElement>(null);
  revision.current = workspace.revision;
  useEffect(() => {
    mounted.current = true;
    return () => {
      mounted.current = false;
      sequence.current++;
    };
  }, []);
  const stale =
    !!result &&
    (invalidated || result.workspace_revision !== workspace.revision);
  const changed =
    !!result &&
    (draft.account !== result.request.account ||
      draft.currency !== result.request.currency ||
      draft.transfers !== result.request.transfers ||
      draft.baseline.from !== result.request.baseline.from ||
      draft.baseline.through !== result.request.baseline.through ||
      draft.comparison.from !== result.request.comparison.from ||
      draft.comparison.through !== result.request.comparison.through);
  const annotations = useMemo(
    () =>
      new Map(
        result?.groups
          .flatMap((group) => [
            ...group.baseline.rows,
            ...group.comparison.rows,
          ])
          .map((row) => [row.transaction_id, row]),
      ),
    [result],
  );
  const versions = useMemo(
    () =>
      new Map(
        result
          ? [
              ...result.outside_period_rows,
              ...result.verified_transfer_peers,
              ...annotations.values(),
            ].map((row) => [row.transaction_id, row.version])
          : [],
      ),
    [result, annotations],
  );
  const editPeriod = (
    side: "baseline" | "comparison",
    field: keyof DatePeriod,
    value: string,
  ) => setDraft({ ...draft, [side]: { ...draft[side], [field]: value } });
  const calculate = async () => {
    const ticket = ++sequence.current,
      expected = workspace.revision;
    setBusy(true);
    setError("");
    setDrill(null);
    try {
      const response = await command<ComparisonResult>({
        action: "compare_transaction_periods",
        request: draft,
        expected_revision: expected,
      });
      if (!mounted.current || ticket !== sequence.current) return;
      if (
        revision.current !== expected ||
        response.workspace_revision !== expected
      ) {
        setInvalidated(true);
        setError(
          "Workspace changed during comparison. Refresh and compare again.",
        );
        return;
      }
      setResult(response);
      setInvalidated(false);
      setGroupPage(0);
    } catch (cause) {
      if (mounted.current && ticket === sequence.current) {
        setInvalidated(true);
        setError(String(cause));
      }
    } finally {
      if (mounted.current && ticket === sequence.current) setBusy(false);
    }
  };
  const rowsButton = (
    label: string,
    ids: string[],
    title = label,
    detail = "These source rows belong to the displayed period and review partition.",
  ) => (
    <button
      className="text-button"
      disabled={stale || busy || !ids.length}
      onClick={() => {
        opener.current =
          document.activeElement instanceof HTMLElement
            ? document.activeElement
            : null;
        handoff.current = false;
        setSourcePage(0);
        setDrill({ label: title, ids, detail });
      }}
    >
      {label} ({ids.length})
    </button>
  );
  const partition = (
    side: string,
    period: PeriodAccountTotal,
    title: string,
  ) => (
    <section
      className="comparison-partition"
      aria-label={`${side} review denominator ${title}`}
    >
      <h4>
        {side} / {period.scope_count} source rows
      </h4>
      {!period.scope_count && (
        <p>No imported rows in this period for this account/currency.</p>
      )}
      {!!period.scope_count && !period.total.transaction_ids.length && (
        <p>
          No accepted included rows. Zero totals do not establish no activity.
        </p>
      )}
      <div className="patterns-links">
        {rowsButton(
          "Included",
          period.total.transaction_ids,
          `${side} included / ${title}`,
        )}
        {rowsButton(
          "Pending",
          period.pending_ids,
          `${side} pending / ${title}`,
        )}
        {rowsButton(
          "Rejected",
          period.rejected_ids,
          `${side} rejected / ${title}`,
        )}
        {rowsButton(
          "Deferred",
          period.deferred_ids,
          `${side} deferred / ${title}`,
        )}
        {rowsButton(
          "Excluded transfers",
          period.excluded_transfer_ids,
          `${side} excluded transfers / ${title}`,
        )}
      </div>
    </section>
  );
  return (
    <section
      className="panel patterns comparison"
      aria-label="Transaction period comparison"
    >
      <div className="patterns-heading">
        <div>
          <p className="eyebrow">LOCAL ANALYSIS / TWO EXPLICIT PERIODS</p>
          <h2>Period comparison</h2>
        </div>
        <span className="patterns-revision">
          {result
            ? `REVISION ${result.workspace_revision} / ${stale ? "STALE" : "CURRENT"}`
            : "NOT CALCULATED"}
        </span>
      </div>
      <p>
        Compare reviewed transactions in their original currency. Choose both
        periods; ledger filters do not apply.
      </p>
      <form
        className="patterns-scope comparison-scope"
        onSubmit={(event) => {
          event.preventDefault();
          void calculate();
        }}
      >
        <fieldset disabled={busy}>
          <legend>Comparison scope / unapplied controls</legend>
          <div className="comparison-period-controls">
            {(["baseline", "comparison"] as const).map((side) => (
              <fieldset key={side}>
                <legend>
                  {side === "baseline" ? "A / Baseline" : "B / Comparison"}
                </legend>
                <div className="comparison-date-pair">
                  <label>
                    {side === "baseline" ? "Baseline from" : "Comparison from"}
                    <input
                      required
                      type="date"
                      value={draft[side].from}
                      onChange={(event) =>
                        editPeriod(side, "from", event.target.value)
                      }
                    />
                  </label>
                  <label>
                    {side === "baseline"
                      ? "Baseline through"
                      : "Comparison through"}
                    <input
                      required
                      type="date"
                      value={draft[side].through}
                      onChange={(event) =>
                        editPeriod(side, "through", event.target.value)
                      }
                    />
                  </label>
                </div>
              </fieldset>
            ))}
          </div>
          <div className="comparison-filters">
            <label>
              Comparison account
              <select
                aria-label="Comparison account"
                value={draft.account ?? ""}
                onChange={(event) =>
                  setDraft({ ...draft, account: event.target.value || null })
                }
              >
                <option value="">All accounts</option>
                {[...new Set(workspace.transactions.map((row) => row.account))]
                  .sort()
                  .map((account) => (
                    <option key={account}>{account}</option>
                  ))}
              </select>
            </label>
            <label>
              Comparison currency
              <select
                aria-label="Comparison currency"
                value={draft.currency ?? ""}
                onChange={(event) =>
                  setDraft({ ...draft, currency: event.target.value || null })
                }
              >
                <option value="">All currencies</option>
                {[...new Set(workspace.transactions.map((row) => row.currency))]
                  .sort()
                  .map((currency) => (
                    <option key={currency}>{currency}</option>
                  ))}
              </select>
            </label>
            <label>
              Comparison transfers
              <select
                aria-label="Comparison transfers"
                value={draft.transfers}
                onChange={(event) =>
                  setDraft({
                    ...draft,
                    transfers: event.target
                      .value as ComparisonRequest["transfers"],
                  })
                }
              >
                <option value="include">Include all rows</option>
                <option value="exclude_reviewed_pairs">
                  Exclude verified reviewed pairs
                </option>
              </select>
            </label>
            <button
              ref={calculateButton}
              className="button primary"
              type="submit"
            >
              {busy ? "Comparing…" : "Compare selected periods"}
            </button>
          </div>
        </fieldset>
        <p>
          Inclusive transaction dates. Periods cannot overlap. Amounts are not
          adjusted for duration.
        </p>
      </form>
      <div className="patterns-links">
        <button
          className="button"
          disabled={busy}
          onClick={() => {
            setInvalidated(true);
            void onRefresh();
          }}
        >
          Refresh workspace for comparison
        </button>
      </div>
      {changed && (
        <p className="patterns-warning" role="status">
          Scope edits are not applied. Results still use the periods and filters
          shown below.
        </p>
      )}
      {error && (
        <p className="alert error" role="alert">
          {error}
        </p>
      )}
      {stale && (
        <p className="patterns-warning" role="alert">
          Comparison is stale or could not be revalidated. Refresh and compare
          again; source drillthrough is disabled.
        </p>
      )}
      {!result && (
        <p className="muted">
          No comparison yet. Enter two periods and calculate; no default dates
          or scope are silently applied.
        </p>
      )}
      {result && (
        <div className={stale ? "patterns-stale" : ""}>
          <div
            className="comparison-applied"
            aria-label="Applied comparison scope"
          >
            <div>
              <span>A / BASELINE · {result.baseline_days} DAYS</span>
              <strong>{periodLabel(result.request.baseline)}</strong>
              <p>{result.baseline_transaction_count} source rows</p>
            </div>
            <div>
              <span>B / COMPARISON · {result.comparison_days} DAYS</span>
              <strong>{periodLabel(result.request.comparison)}</strong>
              <p>{result.comparison_transaction_count} source rows</p>
            </div>
          </div>
          <p className="comparison-direction">
            Delta = comparison − baseline · {result.gap_days} calendar gap days
            · {result.request.account ?? "All accounts"} ·{" "}
            {result.request.currency ?? "All currencies"} ·{" "}
            {result.request.transfers === "include"
              ? "Transfers included"
              : "Verified reviewed transfers excluded"}
          </p>
          {result.unequal_duration && (
            <p className="patterns-warning">
              Unequal period lengths: {result.baseline_days} versus{" "}
              {result.comparison_days} days. Observed totals are not normalized.
            </p>
          )}
          {result.request.baseline.from > result.request.comparison.from && (
            <p className="patterns-warning">
              Baseline is chronologically later. Delta still means comparison
              minus baseline.
            </p>
          )}
          <p>
            {result.workspace_transaction_count} workspace rows /{" "}
            {result.account_currency_transaction_count} match account and
            currency / {result.outside_account_currency_count} outside those
            filters.
          </p>
          <div className="patterns-links">
            {rowsButton(
              "Outside selected periods",
              result.outside_period_rows.map((row) => row.transaction_id),
              "Outside selected periods",
              "Account/currency-matching ledger metadata before, between or after the selected periods. These rows do not contribute to either total; this calculation did not verify their originals unless they support a transfer.",
            )}
            {rowsButton(
              "Verified transfer counterparts",
              result.verified_transfer_peers.map((row) => row.transaction_id),
              "Verified transfer counterparts",
              "Supporting reviewed transfer peers, including peers outside the selected periods or filters. This list is not an additional contribution to the totals.",
            )}
          </div>
          <p className="muted">
            Missing statements cannot be inferred. Zero accepted totals do not
            establish no activity or complete coverage.
          </p>
          {!result.groups.length && (
            <p className="comparison-empty">
              No imported transactions match either selected period and the
              applied filters. No account/currency totals were invented.
            </p>
          )}
          {result.groups
            .slice(groupPage * 25, (groupPage + 1) * 25)
            .map((group) => {
              const title = `${group.account} / ${group.currency}`;
              return (
                <section
                  className="comparison-group"
                  key={JSON.stringify([group.account, group.currency])}
                  aria-label={`Period totals ${title}`}
                >
                  <h3>
                    Account {group.account} / {group.currency}
                  </h3>
                  <div
                    className="table-scroll"
                    role="region"
                    aria-label={`Exact period amounts ${title}`}
                    tabIndex={0}
                  >
                    <table>
                      <caption>Observed accepted amounts · {title}</caption>
                      <thead>
                        <tr>
                          <th>Measure</th>
                          <th>A / Baseline</th>
                          <th>B / Comparison</th>
                          <th>B − A</th>
                          <th>Exact relative change</th>
                        </tr>
                      </thead>
                      <tbody>
                        {(["credits", "debits", "net"] as const).map(
                          (measure) => (
                            <tr key={measure}>
                              <th scope="row">
                                {measure === "debits"
                                  ? "Debit magnitude"
                                  : measure === "credits"
                                    ? "Credits"
                                    : "Net"}
                              </th>
                              <td>{group[measure].baseline}</td>
                              <td>{group[measure].comparison}</td>
                              <td>{group[measure].delta}</td>
                              <td>{ratio(group[measure])}</td>
                            </tr>
                          ),
                        )}
                      </tbody>
                    </table>
                  </div>
                  <div className="comparison-partitions">
                    {partition("Baseline", group.baseline, title)}
                    {partition("Comparison", group.comparison, title)}
                  </div>
                </section>
              );
            })}
          <Pager
            page={groupPage}
            total={result.groups.length}
            setPage={setGroupPage}
            label="comparison groups"
          />
          <details>
            <summary>Calculation rules and limitations</summary>
            <p>
              Rules: <code>{result.rules_version}</code>. Exact ratios are shown
              without division or rounding. No currency conversion or automatic
              duration adjustment.
            </p>
            <ul>
              {result.limitations.map((limit) => (
                <li key={limit}>{limit}</li>
              ))}
            </ul>
          </details>
        </div>
      )}
      {drill && result && (
        <Dialog
          wide
          label="Comparison source rows"
          onClose={() => setDrill(null)}
          restoreFocus={() => {
            if (handoff.current)
              document
                .querySelector<HTMLButtonElement>(
                  '[aria-label="Transaction review"] button',
                )
                ?.focus();
            else
              (opener.current?.isConnected &&
              !(
                opener.current instanceof HTMLButtonElement &&
                opener.current.disabled
              )
                ? opener.current
                : calculateButton.current
              )?.focus();
          }}
        >
          <div className="patterns-drill">
            <div className="patterns-heading">
              <div>
                <p className="eyebrow">
                  PERIOD COMPARISON / SOURCE DRILLTHROUGH
                </p>
                <h2>{drill.label}</h2>
              </div>
              <button
                className="close"
                aria-label="Close comparison source rows"
                onClick={() => setDrill(null)}
              >
                ×
              </button>
            </div>
            <p>
              Calculation revision {result.workspace_revision}. Baseline{" "}
              {periodLabel(result.request.baseline)}; comparison{" "}
              {periodLabel(result.request.comparison)}.
            </p>
            <p>{drill.detail}</p>
            {stale ? (
              <p className="patterns-warning" role="alert">
                Workspace changed. Source rows are unavailable for this stale
                result. Close and compare again.
              </p>
            ) : (
              <>
                <p className="eyebrow">
                  SOURCE ROWS / {drill.ids.length} TOTAL
                </p>
                <AnalysisSourceRows
                  ids={drill.ids.slice(sourcePage * 25, (sourcePage + 1) * 25)}
                  versions={versions}
                  annotations={annotations}
                  revision={result.workspace_revision}
                  onInspect={(row) => {
                    handoff.current = true;
                    setDrill(null);
                    onInspect(row);
                  }}
                />
                <Pager
                  page={sourcePage}
                  total={drill.ids.length}
                  setPage={setSourcePage}
                  label="source rows"
                />
              </>
            )}
          </div>
        </Dialog>
      )}
    </section>
  );
}
