import { useState } from "react";
import { TransactionFacetSelect } from "./TransactionFacetSelect";
import {
  firstPosition,
  PageControls,
  useTransactionPage,
  type PagePosition,
} from "./TransactionPageControls";
import { emptyFilter, type LedgerOrder } from "./transaction-ledger-types";
import type { Transaction } from "./types";

export function TransferCandidates({
  target,
  revision,
  disabled,
  selected,
  onSelect,
  onRefresh,
}: {
  target: Transaction;
  revision: number;
  disabled: boolean;
  selected: Transaction | null;
  onSelect: (row: Transaction | null) => void;
  onRefresh: () => void;
}) {
  const { review: _review, ...filter } = emptyFilter();
  const [draft, setDraft] = useState({
    query: "",
    filter,
    order: "date_ascending" as LedgerOrder,
  });
  const [scope, setScope] = useState(draft),
    [position, setPosition] = useState(firstPosition),
    [history, setHistory] = useState<PagePosition[]>([]),
    [retry, setRetry] = useState(0);
  const result = useTransactionPage(
    {
      action: "page_transfer_candidates",
      request: {
        target_id: target.id,
        expected_target_version: target.version,
        ...scope,
        page_size: 50,
        cursor: position.cursor,
      },
      expected_revision: revision,
    },
    revision,
    50,
    retry,
    position.offset,
  );
  const invalid = new TextEncoder().encode(draft.query).length > 1024;
  const update = (key: keyof typeof filter, value: string | null) =>
    setDraft((old) => ({ ...old, filter: { ...old.filter, [key]: value } }));
  const rows = result.value?.page.rows ?? [];
  return (
    <section className="transfer-candidates" aria-label="Transfer candidates">
      <h3>Internal transfer counterpart</h3>
      <p className="muted">
        Accepted rows from other accounts. Unequal amounts, other currencies,
        matched rows and repeats remain candidates; matching requires canonical
        validation.
      </p>
      <fieldset disabled={disabled} className="plain-fieldset">
        <label>
          Candidate query
          <input
            aria-label="Transfer candidate query"
            value={draft.query}
            onChange={(event) =>
              setDraft((old) => ({ ...old, query: event.target.value }))
            }
          />
        </label>
        <TransactionFacetSelect
          kind="account"
          label="Transfer candidate account"
          revision={revision}
          value={draft.filter.account}
          onChange={(value) => update("account", value)}
          disabled={disabled}
        />
        <TransactionFacetSelect
          kind="currency"
          label="Transfer candidate currency"
          revision={revision}
          value={draft.filter.currency}
          onChange={(value) => update("currency", value)}
          disabled={disabled}
        />
        <label>
          Candidate from date
          <input
            type="date"
            aria-label="Transfer candidate from date"
            value={draft.filter.date_from ?? ""}
            onChange={(event) =>
              update("date_from", event.target.value || null)
            }
          />
        </label>
        <label>
          Candidate through date
          <input
            type="date"
            aria-label="Transfer candidate through date"
            value={draft.filter.date_to ?? ""}
            onChange={(event) => update("date_to", event.target.value || null)}
          />
        </label>
        <label>
          Candidate date order
          <select
            aria-label="Transfer candidate date order"
            value={draft.order}
            onChange={(event) =>
              setDraft((old) => ({
                ...old,
                order: event.target.value as LedgerOrder,
              }))
            }
          >
            <option value="date_ascending">Date ascending</option>
            <option value="date_descending">Date descending</option>
          </select>
        </label>
        <button
          className="button"
          disabled={invalid}
          onClick={() => {
            setScope(draft);
            setPosition(firstPosition());
            setHistory([]);
            setRetry((n) => n + 1);
          }}
        >
          Apply candidate filters
        </button>
      </fieldset>
      {invalid && (
        <p role="alert">
          Candidate query exceeds 1,024 UTF-8 bytes; it was not submitted.
        </p>
      )}
      {JSON.stringify(scope) !== JSON.stringify(draft) && (
        <p role="status">Candidate filter draft is not applied.</p>
      )}
      <p className="muted">
        Requested candidates: {scope.filter.date_from ?? "earliest"} →{" "}
        {scope.filter.date_to ?? "latest"} inclusive; account{" "}
        {scope.filter.account ?? "all other accounts"}; currency{" "}
        {scope.filter.currency ?? "all"}; {scope.order.replaceAll("_", " ")};
        query <code>{scope.query || "(empty)"}</code>.
      </p>
      <PageControls
        label="Candidate"
        value={result.value}
        error={result.error}
        position={position}
        history={history}
        pending={disabled || (!result.value && !result.error)}
        move={(next, trail) => {
          setPosition(next);
          setHistory(trail);
        }}
        retry={() => setRetry((n) => n + 1)}
        refresh={onRefresh}
      />
      <label>
        Internal transfer counterpart
        <select
          aria-label="Transfer counterpart"
          value={selected?.id ?? ""}
          disabled={disabled || !result.value}
          onChange={(event) =>
            onSelect(
              rows.find((row) => row.id === event.target.value) ??
                (selected?.id === event.target.value ? selected : null),
            )
          }
        >
          <option value="">Select a reviewed transaction</option>
          {selected && !rows.some((row) => row.id === selected.id) && (
            <option value={selected.id}>
              {selected.account} · {selected.description} · {selected.amount}{" "}
              {selected.currency} · retained selection outside this page
            </option>
          )}
          {rows.map((row) => (
            <option key={row.id} value={row.id}>
              {row.account} · {row.description} · {row.amount} {row.currency}
              {row.transfer_peer ? " · matched" : ""}
            </option>
          ))}
        </select>
      </label>
      {result.value?.page.selected_count === 0 && (
        <p>
          No accepted candidates match this scope. This does not resolve the
          transaction as a transfer.
        </p>
      )}
      {selected && (
        <p className="muted">
          Selected counterpart {selected.id}, version {selected.version},
          revision {revision}. Changing candidate pages does not silently change
          it.
        </p>
      )}
    </section>
  );
}
