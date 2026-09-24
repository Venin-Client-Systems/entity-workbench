import { useEffect, useMemo, useRef, useState } from "react";
import { command } from "./api";
import { Dialog } from "./Dialog";
import { AnalysisSourceRow, Pager } from "./TransactionAnalysisSource";
import {
  defaultPatternRequest,
  type PatternRequest,
  type RecurringCandidate,
  type TransactionAnalysis,
} from "./transaction-analysis-types";
import type { Transaction, Workspace } from "./types";
import "./transaction-patterns.css";
type Drill = { label: string; ids: string[]; candidate?: RecurringCandidate };
const pageSize = 25;
export function TransactionPatterns({
  workspace,
  onInspect,
  onRefresh,
}: {
  workspace: Workspace;
  onInspect: (row: Transaction) => void;
  onRefresh: () => Promise<unknown>;
}) {
  const [draft, setDraft] = useState<PatternRequest>({
    ...defaultPatternRequest,
  });
  const [result, setResult] = useState<TransactionAnalysis | null>(null);
  const [invalidated, setInvalidated] = useState(false);
  const [busy, setBusy] = useState(false),
    [error, setError] = useState("");
  const [drill, setDrill] = useState<Drill | null>(null),
    [sourcePage, setSourcePage] = useState(0);
  const [groupPage, setGroupPage] = useState(0),
    [candidatePage, setCandidatePage] = useState(0);
  const sequence = useRef(0),
    revision = useRef(workspace.revision),
    mounted = useRef(true),
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
    Object.entries(draft).some(
      ([key, value]) => result.request[key as keyof PatternRequest] !== value,
    );
  const transactions = useMemo(
    () => new Map(workspace.transactions.map((row) => [row.id, row])),
    [workspace.transactions],
  );
  const annotations = useMemo(
    () => new Map(result?.rows.map((row) => [row.transaction_id, row])),
    [result],
  );
  const edit = <K extends keyof PatternRequest>(
    key: K,
    value: PatternRequest[K],
  ) => setDraft({ ...draft, [key]: value });
  const calculate = async () => {
    const ticket = ++sequence.current,
      expected = workspace.revision;
    setBusy(true);
    setError("");
    setDrill(null);
    try {
      const response = await command<TransactionAnalysis>({
        action: "analyze_transactions",
        request: draft,
        expected_revision: expected,
      });
      if (!mounted.current || ticket !== sequence.current) return;
      if (revision.current !== expected) {
        setError(
          "Workspace changed during calculation. Refresh and calculate again.",
        );
        return;
      }
      setInvalidated(false);
      setResult(response);
      setGroupPage(0);
      setCandidatePage(0);
    } catch (cause) {
      if (mounted.current && ticket === sequence.current) {
        setInvalidated(true);
        setError(String(cause));
      }
    } finally {
      if (mounted.current && ticket === sequence.current) setBusy(false);
    }
  };
  const show = (
    label: string,
    ids: string[],
    candidate?: RecurringCandidate,
  ) => {
    opener.current =
      document.activeElement instanceof HTMLElement
        ? document.activeElement
        : null;
    handoff.current = false;
    setSourcePage(0);
    setDrill({ label, ids, candidate });
  };
  const rowsButton = (label: string, ids: string[], title = label) => (
    <button
      className="text-button"
      disabled={stale || busy || !ids.length}
      onClick={() => show(title, ids)}
    >
      {label} ({ids.length})
    </button>
  );
  const sourceIds =
    drill?.ids.slice(sourcePage * pageSize, (sourcePage + 1) * pageSize) ?? [];
  return (
    <section className="panel patterns" aria-label="Transaction patterns">
      <div className="patterns-heading">
        <div>
          <p className="eyebrow">LOCAL ANALYSIS / REVIEWED TRANSACTIONS</p>
          <h2>Transaction patterns</h2>
        </div>
        <span className="patterns-revision">
          {result
            ? `REVISION ${result.workspace_revision} / ${stale ? "STALE" : "CURRENT"}`
            : "NOT CALCULATED"}
        </span>
      </div>
      <p>
        Exact amounts, currencies kept separate, source rows behind every
        result. This analysis uses its own scope below; ledger filters do not
        apply.
      </p>
      <form
        className="patterns-scope"
        onSubmit={(event) => {
          event.preventDefault();
          void calculate();
        }}
      >
        <fieldset disabled={busy}>
          <legend>Analysis scope</legend>
          <div className="patterns-fields">
            <label>
              From date
              <input
                type="date"
                value={draft.date_from ?? ""}
                onChange={(e) => edit("date_from", e.target.value || null)}
              />
            </label>
            <label>
              Through date
              <input
                type="date"
                value={draft.date_to ?? ""}
                onChange={(e) => edit("date_to", e.target.value || null)}
              />
            </label>
            <label>
              Analysis account
              <select
                value={draft.account ?? ""}
                onChange={(e) => edit("account", e.target.value || null)}
              >
                <option value="">All accounts</option>
                {[...new Set(workspace.transactions.map((t) => t.account))]
                  .sort()
                  .map((a) => (
                    <option key={a}>{a}</option>
                  ))}
              </select>
            </label>
            <label>
              Analysis currency
              <select
                value={draft.currency ?? ""}
                onChange={(e) => edit("currency", e.target.value || null)}
              >
                <option value="">All currencies</option>
                {[...new Set(workspace.transactions.map((t) => t.currency))]
                  .sort()
                  .map((c) => (
                    <option key={c}>{c}</option>
                  ))}
              </select>
            </label>
            <label>
              Transfer treatment
              <select
                value={draft.transfers}
                onChange={(e) =>
                  edit(
                    "transfers",
                    e.target.value as PatternRequest["transfers"],
                  )
                }
              >
                <option value="include">Include all rows</option>
                <option value="exclude_reviewed_pairs">
                  Exclude verified reviewed pairs
                </option>
              </select>
            </label>
          </div>
          <div className="patterns-fields patterns-tolerances">
            <label>
              Minimum occurrences
              <input
                type="number"
                min="3"
                max="12"
                step="1"
                required
                value={draft.minimum_occurrences}
                onChange={(e) =>
                  edit("minimum_occurrences", e.target.valueAsNumber)
                }
              />
            </label>
            <label>
              Date tolerance (days)
              <input
                type="number"
                min="0"
                max="3"
                step="1"
                required
                value={draft.date_tolerance_days}
                onChange={(e) =>
                  edit("date_tolerance_days", e.target.valueAsNumber)
                }
              />
            </label>
            <label>
              Absolute amount spread
              <input
                type="text"
                inputMode="decimal"
                required
                maxLength={40}
                value={draft.amount_tolerance}
                onChange={(e) => edit("amount_tolerance", e.target.value)}
              />
            </label>
            <button
              ref={calculateButton}
              id="calculate-patterns"
              className="button primary"
              type="submit"
            >
              {busy ? "Calculating…" : "Calculate reviewed patterns"}
            </button>
            <button
              className="button"
              type="button"
              onClick={() => void onRefresh()}
            >
              Refresh workspace
            </button>
          </div>
        </fieldset>
        <p className="context-note">
          Inclusive transaction dates. Amount spread uses each currency’s units.
          Accepted source rows contribute; pending, rejected and deferred rows
          remain in the denominator. Repeated purchases are retained.
        </p>
      </form>
      {error && (
        <p className="error" role="alert">
          Calculation unavailable. {error} Displayed results, if any, are from
          the prior calculation.
        </p>
      )}
      {stale && (
        <p className="patterns-warning" role="status">
          Workspace changed or validation failed. These results are stale;
          recalculate before using totals or opening source rows.
        </p>
      )}
      {changed && (
        <p className="patterns-warning" role="status">
          Scope edits are not applied. Displayed results use the applied
          parameters below.
        </p>
      )}
      {!result && !busy && (
        <p className="context-note">
          Choose a scope and calculate to inspect patterns. No classification or
          finding is created.
        </p>
      )}
      {result && (
        <div className={stale ? "patterns-stale" : undefined}>
          <div className="patterns-applied">
            <strong>
              {result.scope_transaction_count} in scope /{" "}
              {result.workspace_transaction_count} workspace rows
            </strong>
            <p>
              Applied: {result.request.date_from ?? "Any start date"} through{" "}
              {result.request.date_to ?? "any end date"} ·{" "}
              {result.request.account ?? "all accounts"} ·{" "}
              {result.request.currency ?? "all currencies"} ·{" "}
              {result.request.transfers === "include"
                ? "transfers included"
                : "verified reviewed transfers excluded"}
            </p>
            <p>
              Recurrence: {result.request.minimum_occurrences}+ occurrences · ±
              {result.request.date_tolerance_days} days · amount spread ≤{" "}
              {result.request.amount_tolerance}.{" "}
              {rowsButton(
                "All scoped rows",
                result.rows.map((r) => r.transaction_id),
              )}
            </p>
          </div>
          <h3>01 / Currency instruments</h3>
          {!result.currencies.length && (
            <p>No transaction rows match this scope.</p>
          )}
          {result.currencies.map((c) => (
            <section
              key={c.currency}
              className="patterns-currency"
              aria-label={`${c.currency} analysis`}
            >
              <div className="patterns-currency-grid">
                <h4>
                  {c.currency}
                  <small>{c.scope_count} scoped rows</small>
                </h4>
                <dl>
                  <dt>Credits</dt>
                  <dd>{c.total.credits}</dd>
                </dl>
                <dl>
                  <dt>Debits</dt>
                  <dd>{c.total.debits}</dd>
                </dl>
                <dl>
                  <dt>Net</dt>
                  <dd>{c.total.net}</dd>
                </dl>
                <dl>
                  <dt>Cash candidate</dt>
                  <dd>{c.cash_candidates.debits}</dd>
                  <dd>
                    {rowsButton(
                      "Cash source rows",
                      c.cash_candidates.transaction_ids,
                      `${c.currency} cash candidates`,
                    )}
                  </dd>
                </dl>
                <dl>
                  <dt>Refund / reversal candidate</dt>
                  <dd>{c.refund_candidates.credits}</dd>
                  <dd>
                    {rowsButton(
                      "Refund source rows",
                      c.refund_candidates.transaction_ids,
                      `${c.currency} refund candidates`,
                    )}
                  </dd>
                </dl>
              </div>
              <div className="patterns-links">
                {rowsButton(
                  "Included",
                  c.total.transaction_ids,
                  `${c.currency} included rows`,
                )}
                {rowsButton(
                  "Pending",
                  c.pending_ids,
                  `${c.currency} pending rows`,
                )}
                {rowsButton(
                  "Rejected",
                  c.rejected_ids,
                  `${c.currency} rejected rows`,
                )}
                {rowsButton(
                  "Deferred",
                  c.deferred_ids,
                  `${c.currency} deferred rows`,
                )}
                {rowsButton(
                  "Excluded transfers",
                  c.excluded_transfer_ids,
                  `${c.currency} excluded transfers`,
                )}
                {rowsButton(
                  "Neither cash nor refund hint",
                  c.unclassified_ids,
                  `${c.currency} unclassified rows`,
                )}
              </div>
            </section>
          ))}
          <h3>02 / Description groups</h3>
          <p>
            Original descriptions grouped by ASCII case and whitespace only.
            Merchant and branch identity are unconfirmed; digits and punctuation
            remain distinct.
          </p>
          <div
            className="table-scroll"
            role="region"
            aria-label="Description totals"
            tabIndex={0}
          >
            <table>
              <caption>
                {result.merchant_groups.length} description groups; currencies
                remain separate
              </caption>
              <thead>
                <tr>
                  <th>Description / accounts</th>
                  <th>Currency</th>
                  <th>Credits</th>
                  <th>Debits</th>
                  <th>Net</th>
                  <th>Source rows</th>
                </tr>
              </thead>
              <tbody>
                {result.merchant_groups
                  .slice(groupPage * pageSize, (groupPage + 1) * pageSize)
                  .map((g) => (
                    <tr key={g.id}>
                      <td>
                        {g.description_group}
                        <small>{g.accounts.join(", ")}</small>
                      </td>
                      <td>{g.currency}</td>
                      <td>{g.total.credits}</td>
                      <td>{g.total.debits}</td>
                      <td>{g.total.net}</td>
                      <td>
                        {rowsButton(
                          "Inspect group",
                          g.total.transaction_ids,
                          `${g.description_group} / ${g.currency}`,
                        )}
                      </td>
                    </tr>
                  ))}
              </tbody>
            </table>
          </div>
          <Pager
            page={groupPage}
            total={result.merchant_groups.length}
            setPage={setGroupPage}
            label="description groups"
          />
          <h3>03 / Recurring candidates</h3>
          <p>
            Cadence candidates do not establish subscriptions or purpose. A full
            account/currency/description group must fit; same-day repeats and
            missing periods can suppress a candidate.
          </p>
          <div className="patterns-links">
            {rowsButton("Eligible debit rows", result.recurrence_eligible_ids)}
            {rowsButton(
              "Unmatched debit rows",
              result.recurrence_unclassified_ids,
            )}
          </div>
          {!result.recurring_candidates.length && (
            <p>
              No groups fit the applied cadence and amount limits. This does not
              establish the absence of recurring payments.
            </p>
          )}
          {result.recurring_candidates
            .slice(candidatePage * pageSize, (candidatePage + 1) * pageSize)
            .map((c) => (
              <div className="patterns-candidate" key={c.id}>
                <div>
                  <strong>
                    {c.description_group} / account {c.account} / {c.currency}
                  </strong>
                  <p>
                    {c.cadence} · {c.total.transaction_ids.length} debits ·{" "}
                    {c.total.debits} {c.currency}
                  </p>
                </div>
                <button
                  className="button"
                  disabled={stale || busy}
                  onClick={() =>
                    show("Recurring candidate", c.total.transaction_ids, c)
                  }
                >
                  Review cadence {c.account} {c.currency} {c.description_group}
                </button>
              </div>
            ))}
          <Pager
            page={candidatePage}
            total={result.recurring_candidates.length}
            setPage={setCandidatePage}
            label="recurring candidates"
          />
          <details>
            <summary>Calculation rules and limitations</summary>
            <p>
              Rules: <code>{result.rules_version}</code>. Read-only calculation;
              no review decisions are changed.
            </p>
            <ul>
              {result.limitations.map((l) => (
                <li key={l}>{l}</li>
              ))}
            </ul>
          </details>
        </div>
      )}
      {drill && result && (
        <Dialog
          wide
          label="Pattern source rows"
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
                  TRANSACTION PATTERNS / SOURCE DRILLTHROUGH
                </p>
                <h2>{drill.label}</h2>
              </div>
              <button
                className="close"
                aria-label="Close pattern source rows"
                onClick={() => setDrill(null)}
              >
                ×
              </button>
            </div>
            <p>
              Calculation revision {result.workspace_revision}. Amounts are
              exact original-currency values; source excerpts are escaped text.
            </p>
            {stale ? (
              <p className="patterns-warning" role="alert">
                Workspace changed. Source rows are unavailable for this stale
                result. Close and recalculate.
              </p>
            ) : (
              <>
                {drill.candidate && (
                  <>
                    <h3>
                      {drill.candidate.description_group} / account{" "}
                      {drill.candidate.account} / {drill.candidate.currency}
                    </h3>
                    <p className="patterns-warning">
                      Heuristic / {drill.candidate.cadence}. A matching cadence
                      does not establish a subscription, purpose or merchant
                      identity.
                    </p>
                    <p>
                      Total debits {drill.candidate.total.debits}{" "}
                      {drill.candidate.currency} · range{" "}
                      {drill.candidate.minimum_debit}–
                      {drill.candidate.maximum_debit} · applied tolerance ±
                      {result.request.date_tolerance_days} days /{" "}
                      {result.request.amount_tolerance} amount.
                    </p>
                    <div
                      className="table-scroll"
                      role="region"
                      aria-label="Anchored cadence schedule"
                      tabIndex={0}
                    >
                      <table>
                        <caption>
                          Anchored schedule · current source page
                        </caption>
                        <thead>
                          <tr>
                            <th>Expected</th>
                            <th>Actual</th>
                            <th>Deviation</th>
                          </tr>
                        </thead>
                        <tbody>
                          {drill.candidate.expected_dates
                            .slice(
                              sourcePage * pageSize,
                              (sourcePage + 1) * pageSize,
                            )
                            .map((date, i) => {
                              const index = sourcePage * pageSize + i;
                              return (
                                <tr key={index}>
                                  <td>{date}</td>
                                  <td>
                                    {drill.candidate!.actual_dates[index]}
                                  </td>
                                  <td>
                                    {drill.candidate!.deviations_days[index]}{" "}
                                    days
                                  </td>
                                </tr>
                              );
                            })}
                        </tbody>
                      </table>
                    </div>
                  </>
                )}
                <p className="eyebrow">
                  SOURCE ROWS / {drill.ids.length} TOTAL
                </p>
                {sourceIds.map((id) => {
                  const t = transactions.get(id),
                    a = annotations.get(id);
                  return (
                    <AnalysisSourceRow
                      key={id}
                      id={id}
                      transaction={t}
                      expectedVersion={a?.version}
                      annotation={a}
                      onInspect={(row) => {
                        handoff.current = true;
                        setDrill(null);
                        onInspect(row);
                      }}
                    />
                  );
                })}
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
