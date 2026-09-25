import { useEffect, useMemo, useRef, useState } from "react";
import { command } from "./api";
import { Dialog } from "./Dialog";
import { TransactionFacetSelect } from "./TransactionFacetSelect";
import { AnalysisSourceRows, Pager } from "./TransactionAnalysisSource";
import { defaultFlowRequest, sameFlowScope } from "./account-flow-types";
import type {
  AccountFlowEdge,
  AccountFlowRequest,
  AccountFlowResult,
} from "./account-flow-types";
import type { MoneyTotal, PatternRow } from "./transaction-analysis-types";
import type { Transaction } from "./types";
import "./transaction-patterns.css";
import "./account-flows.css";

type Drill =
  { label: string; ids: string[] } | { label: string; edge: AccountFlowEdge };
const pageSize = 25;
const range = (page: number, count: number) =>
  count
    ? `${page * pageSize + 1}–${Math.min((page + 1) * pageSize, count)} of ${count}`
    : "0 of 0";

/** Presentation only: matching, scope, partitioning and exact arithmetic remain in Rust. */
export function AccountFlows({
  revision,
  onInspect,
  onRefresh,
}: {
  revision: number;
  onInspect: (row: Transaction, revision: number) => void;
  onRefresh: () => Promise<boolean>;
}) {
  const [draft, setDraft] = useState<AccountFlowRequest>({
    ...defaultFlowRequest,
  });
  const [result, setResult] = useState<AccountFlowResult | null>(null);
  const [busy, setBusy] = useState(false),
    [error, setError] = useState("");
  const [invalidated, setInvalidated] = useState(false);
  const [edgePage, setEdgePage] = useState(0),
    [nodePage, setNodePage] = useState(0);
  const [drill, setDrill] = useState<Drill | null>(null),
    [detailPage, setDetailPage] = useState(0);
  const mounted = useRef(true),
    sequence = useRef(0),
    currentRevision = useRef(revision);
  const opener = useRef<HTMLElement | null>(null),
    calculateButton = useRef<HTMLButtonElement>(null);
  const handoff = useRef(false);
  currentRevision.current = revision;
  useEffect(() => {
    mounted.current = true;
    return () => {
      mounted.current = false;
      sequence.current++;
    };
  }, []);
  const stale =
    !!result && (invalidated || result.workspace_revision !== revision);
  const changed = !!result && !sameFlowScope(draft, result.request);
  const nodes = useMemo(
    () => new Map(result?.nodes.map((n) => [n.id, n])),
    [result],
  );
  const versions = useMemo(
    () => new Map(result?.sources.map((s) => [s.transaction_id, s.version])),
    [result],
  );
  const contexts = useMemo(
    () =>
      new Map(
        result?.sources.map((s) => [
          s.transaction_id,
          s.in_scope
            ? "Selected activity — inside the applied scope."
            : "Support only — outside the applied scope; excluded from selected activity and review counts.",
        ]),
      ),
    [result],
  );
  const annotations = useMemo(
    () =>
      new Map<string, PatternRow>(
        result?.sources.map((s) => [
          s.transaction_id,
          {
            transaction_id: s.transaction_id,
            version: s.version,
            disposition: s.review === "accepted" ? "included" : s.review,
            verified_transfer_peer:
              s.verified_transfer_peer?.transaction_id ?? null,
            unverified_transfer_match: s.unverified_transfer_match,
            has_duplicate_candidates: s.has_duplicate_candidates,
            cash_rule: null,
            refund_rule: null,
          },
        ]),
      ),
    [result],
  );
  const calculate = async () => {
    if (busy) return;
    const ticket = ++sequence.current,
      expected = revision,
      request = { ...draft };
    setBusy(true);
    setError("");
    setDrill(null);
    try {
      const response = await command<AccountFlowResult>({
        action: "analyze_account_flows",
        request,
        expected_revision: expected,
      });
      if (!mounted.current || ticket !== sequence.current) return;
      if (currentRevision.current !== expected)
        throw new Error(
          "Workspace changed during calculation. Refresh and calculate again.",
        );
      if (
        response.schema_version !== 1 ||
        response.rules_version !== "reviewed_internal_account_flows_v1" ||
        response.workspace_revision !== expected ||
        !sameFlowScope(request, response.request)
      )
        throw new Error(
          "Account-flow response does not match the requested scope and revision.",
        );
      setResult(response);
      setInvalidated(false);
      setNodePage(0);
      setEdgePage(0);
    } catch (cause) {
      if (mounted.current && ticket === sequence.current) {
        setError(String(cause));
        setInvalidated(true);
      }
    } finally {
      if (mounted.current && ticket === sequence.current) setBusy(false);
    }
  };
  const open = (value: Drill) => {
    opener.current =
      document.activeElement instanceof HTMLElement
        ? document.activeElement
        : null;
    handoff.current = false;
    setDetailPage(0);
    setDrill(value);
  };
  const rowsButton = (label: string, ids: string[], title: string) => (
    <button
      className="text-button"
      disabled={busy || stale || !ids.length}
      onClick={() => open({ label: title, ids })}
    >
      {label} ({ids.length})
    </button>
  );
  const totals = (label: string, value: MoneyTotal, currency: string) => (
    <div className="flow-money">
      <h5>{label}</h5>
      <dl>
        {(["credits", "debits", "net"] as const).map((key) => (
          <div key={key}>
            <dt>
              {key === "debits"
                ? "Debit magnitude"
                : key === "credits"
                  ? "Credits"
                  : "Net"}
            </dt>
            <dd>
              {value[key]} <small>{currency}</small>
            </dd>
          </div>
        ))}
      </dl>
    </div>
  );
  return (
    <section
      className="panel patterns account-flows"
      aria-label="Reviewed account flows"
    >
      <div className="patterns-heading">
        <div>
          <p className="eyebrow">ACCOUNT FLOWS / REVIEWED INTERNAL LINKS</p>
          <h2>Trace reviewed transfers</h2>
        </div>
        <span className="patterns-revision">WORKSPACE R{revision}</span>
      </div>
      <p>
        Only verified reciprocal reviewed transfers form directed links.
        Accepted activity without a verified peer remains unmapped; external
        counterparties and ownership are unknown.
      </p>
      <div className="patterns-scope">
        <fieldset>
          <legend>Flow scope — transaction dates, inclusive</legend>
          <div className="flow-fields">
            <label>
              Flow from
              <input
                type="date"
                value={draft.date_from ?? ""}
                onChange={(e) =>
                  setDraft({ ...draft, date_from: e.target.value || null })
                }
              />
            </label>
            <label>
              Flow through
              <input
                type="date"
                value={draft.date_to ?? ""}
                onChange={(e) =>
                  setDraft({ ...draft, date_to: e.target.value || null })
                }
              />
            </label>
            <TransactionFacetSelect
              kind="account"
              revision={revision}
              label="Flow account"
              value={draft.account}
              onChange={(account) => setDraft({ ...draft, account })}
            />
            <TransactionFacetSelect
              kind="currency"
              revision={revision}
              label="Flow currency"
              value={draft.currency}
              onChange={(currency) => setDraft({ ...draft, currency })}
            />
          </div>
        </fieldset>
        <div className="patterns-links">
          <button
            ref={calculateButton}
            className="button primary"
            disabled={busy}
            onClick={() => void calculate()}
          >
            {busy ? "Calculating flows…" : "Calculate reviewed flows"}
          </button>
          <button
            className="button"
            disabled={busy}
            onClick={() => {
              const ticket = sequence.current;
              void onRefresh()
                .then((ok) => {
                  if (!ok)
                    throw new Error(
                      "Workspace refresh was not confirmed. Refresh and calculate again.",
                    );
                })
                .catch((cause) => {
                  if (mounted.current && ticket === sequence.current) {
                    setError(String(cause));
                    setInvalidated(true);
                  }
                });
            }}
          >
            Refresh flow workspace
          </button>
        </div>
        <p>
          All review states stay in the selected denominator. Currencies are
          separate; no conversion or inferred matches.
        </p>
      </div>
      {error && (
        <p className="error" role="alert">
          Account flows unavailable. {error}
        </p>
      )}
      {busy && (
        <p role="status">
          Reading and verifying the canonical ledger at revision {revision}…
        </p>
      )}
      {!result && !busy && !error && (
        <p className="flow-empty">
          Choose a scope and calculate. No flow result has been requested.
        </p>
      )}
      {result && (
        <div className={stale ? "patterns-stale" : ""}>
          <div className="flow-applied" aria-label="Applied flow scope">
            <span className="eyebrow">
              CAPTURED R{result.workspace_revision}
            </span>
            <strong>
              {result.request.date_from ?? "Any start date"} →{" "}
              {result.request.date_to ?? "Any end date"}
            </strong>
            <p>
              Account: {result.request.account ?? "all accounts"} / Currency:{" "}
              {result.request.currency ?? "all currencies, separate"}
            </p>
          </div>
          {changed && (
            <p className="patterns-warning">
              Scope edits are not applied. The result below retains its captured
              scope; calculate again to apply the draft.
            </p>
          )}
          {stale && (
            <p className="patterns-warning" role="alert">
              This retained result is stale or unverified after a failed read.
              Source actions are disabled. Refresh and calculate again.
            </p>
          )}
          <div className="flow-counts" aria-label="Flow scope denominators">
            <div>
              <span>Selected rows</span>
              <strong>{result.scope_transaction_count}</strong>
            </div>
            <div>
              <span>Workspace rows</span>
              <strong>{result.workspace_transaction_count}</strong>
            </div>
            <div>
              <span>Support-only rows</span>
              <strong>{result.support_transaction_count}</strong>
            </div>
          </div>
          <p>
            Support-only rows explain the other side of included transfers. They
            add no selected activity or review counts. Pending, rejected and
            deferred rows are retained, not zero observed activity.
          </p>
          <h3>Directed relationships</h3>
          <p>
            {range(edgePage, result.edges.length)} relationships shown.
            Equal-width lanes do not encode amount. Each link is debit account →
            credit account; totals count each verified pair once.
          </p>
          {!result.edges.length && (
            <p className="flow-empty">
              No verified internal transfer pairs in this scope. This does not
              establish no activity or no external transfers.
            </p>
          )}
          <div className="flow-lanes">
            {result.edges
              .slice(edgePage * pageSize, (edgePage + 1) * pageSize)
              .map((edge) => {
                const from = nodes.get(edge.debit_node_id)!,
                  to = nodes.get(edge.credit_node_id)!;
                return (
                  <article
                    className="flow-lane"
                    key={edge.id}
                    aria-label={`Flow ${from.account} to ${to.account} ${edge.currency}`}
                  >
                    <div className="flow-endpoint">
                      <span>DEBIT / {edge.currency}</span>
                      <strong>{from.account}</strong>
                      <small>
                        {from.support_only
                          ? "Support-only account"
                          : "Contains selected activity"}
                      </small>
                    </div>
                    <div className="flow-link">
                      <span aria-hidden="true">→</span>
                      <strong>
                        {edge.amount} {edge.currency}
                      </strong>
                      <span>
                        Debit to credit · {edge.pairs.length}{" "}
                        {edge.pairs.length === 1 ? "pair" : "pairs"}
                      </span>
                      <button
                        className="button"
                        disabled={busy || stale}
                        onClick={() =>
                          open({
                            label: `${from.account} → ${to.account} / ${edge.currency}`,
                            edge,
                          })
                        }
                      >
                        Review pairs
                      </button>
                    </div>
                    <div className="flow-endpoint">
                      <span>CREDIT / {edge.currency}</span>
                      <strong>{to.account}</strong>
                      <small>
                        {to.support_only
                          ? "Support-only account"
                          : "Contains selected activity"}
                      </small>
                    </div>
                  </article>
                );
              })}
          </div>
          <Pager
            page={edgePage}
            total={result.edges.length}
            setPage={setEdgePage}
            label="flow relationships"
          />
          <h3>Account activity / selected and support separate</h3>
          <p>
            {range(nodePage, result.nodes.length)} account/currency groups
            shown.
          </p>
          {!result.nodes.length && (
            <p className="flow-empty">
              No imported rows match the applied scope.
            </p>
          )}
          {result.nodes
            .slice(nodePage * pageSize, (nodePage + 1) * pageSize)
            .map((node) => {
              const title = `${node.account} / ${node.currency}`;
              return (
                <section
                  key={node.id}
                  className="flow-node"
                  aria-label={`Account activity ${title}`}
                >
                  <header>
                    <h4>{title}</h4>
                    <span>
                      {node.support_only
                        ? "SUPPORT ONLY / OUTSIDE SCOPE"
                        : `${node.scope_count} SELECTED ROWS`}
                    </span>
                  </header>
                  {node.support_only ? (
                    <p>
                      No selected activity for this account/currency. Its
                      counterpart rows only support the displayed relationships.
                    </p>
                  ) : (
                    <>
                      <p>
                        Selected review counts: {node.review_counts.accepted}{" "}
                        accepted / {node.review_counts.pending} pending /{" "}
                        {node.review_counts.rejected} rejected /{" "}
                        {node.review_counts.deferred} deferred.
                      </p>
                      {!node.review_counts.accepted && (
                        <p className="patterns-warning">
                          No accepted selected rows. Zero reviewed totals do not
                          establish no activity.
                        </p>
                      )}
                      <div className="flow-totals">
                        {totals(
                          "Accepted ledger / includes mapped transfers",
                          node.accepted_ledger,
                          node.currency,
                        )}
                        {totals(
                          "Accepted unmapped / counterpart unknown",
                          node.accepted_unmapped,
                          node.currency,
                        )}
                      </div>
                      <div className="patterns-links">
                        {rowsButton(
                          "Accepted ledger",
                          node.accepted_ledger.transaction_ids,
                          `Accepted ledger / ${title}`,
                        )}
                        {rowsButton(
                          "Accepted unmapped",
                          node.accepted_unmapped.transaction_ids,
                          `Accepted unmapped / ${title}`,
                        )}
                        {rowsButton(
                          "Pending",
                          node.pending_ids,
                          `Pending / ${title}`,
                        )}
                        {rowsButton(
                          "Rejected",
                          node.rejected_ids,
                          `Rejected / ${title}`,
                        )}
                        {rowsButton(
                          "Deferred",
                          node.deferred_ids,
                          `Deferred / ${title}`,
                        )}
                      </div>
                      <p>
                        Overlapping annotations across selected review states:{" "}
                        {node.unverified_transfer_count} unverified transfer
                        markers; {node.duplicate_candidate_count} possible
                        duplicate rows. None silently removed.
                      </p>
                      <div className="patterns-links">
                        {rowsButton(
                          "Unverified links",
                          node.unverified_transfer_ids,
                          `Unverified links / ${title}`,
                        )}
                        {rowsButton(
                          "Possible duplicates",
                          node.duplicate_candidate_ids,
                          `Possible duplicates / ${title}`,
                        )}
                      </div>
                    </>
                  )}
                  <div className="patterns-links">
                    {rowsButton(
                      "Support only",
                      node.support_ids,
                      `Support only / ${title}`,
                    )}
                  </div>
                </section>
              );
            })}
          <Pager
            page={nodePage}
            total={result.nodes.length}
            setPage={setNodePage}
            label="account groups"
          />
          <details>
            <summary>Flow rules and limitations</summary>
            <p>
              Rules: <code>{result.rules_version}</code>. Exact strings come
              from the canonical calculation; these links create no new
              findings.
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
          label="Account flow sources"
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
          <div className="patterns-drill flow-drill">
            <div className="patterns-heading">
              <div>
                <p className="eyebrow">ACCOUNT FLOWS / SOURCE DRILLTHROUGH</p>
                <h2>{drill.label}</h2>
              </div>
              <button
                className="close"
                aria-label="Close account flow sources"
                onClick={() => setDrill(null)}
              >
                ×
              </button>
            </div>
            <p>
              Captured revision {result.workspace_revision}. Selected activity
              and out-of-scope support retain their original IDs, versions and
              anchors.
            </p>
            {stale ? (
              <p className="patterns-warning" role="alert">
                Workspace changed or this result is unverified. Close, refresh
                and calculate again before inspecting sources.
              </p>
            ) : "edge" in drill ? (
              <>
                <p>
                  {range(detailPage, drill.edge.pairs.length)} pairs shown.
                  Exact total {drill.edge.amount} {drill.edge.currency}; each
                  pair contributes once. Debit and credit dates may differ.
                </p>
                {drill.edge.pairs
                  .slice(detailPage * pageSize, (detailPage + 1) * pageSize)
                  .map((pair) => (
                    <article className="flow-pair" key={pair.id}>
                      <strong>
                        {pair.amount} {drill.edge.currency}
                      </strong>
                      <p>
                        Debit {pair.debit_date} —{" "}
                        {pair.debit_in_scope
                          ? "selected activity"
                          : "support only, outside scope"}
                        ; version {pair.debit.version}.
                      </p>
                      <p>
                        Credit {pair.credit_date} —{" "}
                        {pair.credit_in_scope
                          ? "selected activity"
                          : "support only, outside scope"}
                        ; version {pair.credit.version}.
                      </p>
                      <button
                        className="button"
                        onClick={() => {
                          setDetailPage(0);
                          setDrill({
                            label: `Both endpoints / ${pair.amount} ${drill.edge.currency}`,
                            ids: [
                              pair.debit.transaction_id,
                              pair.credit.transaction_id,
                            ],
                          });
                        }}
                      >
                        Inspect both source rows
                      </button>
                    </article>
                  ))}
                <Pager
                  page={detailPage}
                  total={drill.edge.pairs.length}
                  setPage={setDetailPage}
                  label="transfer pairs"
                />
              </>
            ) : (
              <>
                <p>{range(detailPage, drill.ids.length)} source rows shown.</p>
                <AnalysisSourceRows
                  ids={drill.ids.slice(
                    detailPage * pageSize,
                    (detailPage + 1) * pageSize,
                  )}
                  versions={versions}
                  annotations={annotations}
                  contexts={contexts}
                  revision={result.workspace_revision}
                  onInspect={(row) => {
                    handoff.current = true;
                    setDrill(null);
                    onInspect(row, result.workspace_revision);
                  }}
                />
                <Pager
                  page={detailPage}
                  total={drill.ids.length}
                  setPage={setDetailPage}
                  label="flow source rows"
                />
              </>
            )}
          </div>
        </Dialog>
      )}
    </section>
  );
}
