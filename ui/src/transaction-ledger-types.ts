import type { ReviewCounts, ReviewState, Transaction } from "./types";
export type LedgerFilter = {
  date_from: string | null;
  date_to: string | null;
  account: string | null;
  currency: string | null;
  review: ReviewState | null;
};
export type LedgerOrder = "date_ascending" | "date_descending";
export type LedgerScope = {
  query: string;
  filter: LedgerFilter;
  order: LedgerOrder;
  page_size: number;
};
export type Matching = {
  algorithm: "unicode_default_lowercase_literal_v1";
  unicode_version: [number, number, number];
};
export type TransactionPage = {
  schema_version: 1;
  workspace_revision: number;
  query_sha256: string;
  scope_count: number;
  review_counts: ReviewCounts;
  selected_count: number;
  rows: Transaction[];
  next_cursor: string | null;
};
export type SearchPage = {
  schema_version: 1;
  matching: Matching;
  page: TransactionPage;
};
export type TransferPage = SearchPage & {
  target_id: string;
  target_version: number;
};
export type BalanceState =
  | { state: "no_balance" }
  | { state: "no_prior_balance" }
  | {
      state: "checked";
      previous_id: string;
      previous_version: number;
      contributing_row_count: number;
      difference: string;
      reconciled: boolean;
    };
export type Balances = {
  schema_version: 1;
  workspace_revision: number;
  rows: { id: string; version: number; balance: BalanceState }[];
};

/** Response shape/identity only. Rust remains the reconciliation calculator. */
export function validateBalances(
  value: Balances,
  wanted: { id: string; expected_version: number }[],
  revision: number,
) {
  const positive = (number: unknown) =>
    Number.isSafeInteger(number) && Number(number) > 0;
  const validState = (state: BalanceState) => {
    if (!state || typeof state !== "object") return false;
    if (state.state === "no_balance" || state.state === "no_prior_balance")
      return true;
    return (
      state.state === "checked" &&
      typeof state.previous_id === "string" &&
      new TextEncoder().encode(state.previous_id).length > 0 &&
      new TextEncoder().encode(state.previous_id).length <= 256 &&
      positive(state.previous_version) &&
      positive(state.contributing_row_count) &&
      typeof state.difference === "string" &&
      state.difference.length <= 80 &&
      /^-?[0-9]+(?:\.[0-9]+)?$/.test(state.difference) &&
      typeof state.reconciled === "boolean"
    );
  };
  if (
    value.schema_version !== 1 ||
    value.workspace_revision !== revision ||
    !Array.isArray(value.rows) ||
    value.rows.length !== wanted.length ||
    value.rows.some(
      (row, i) =>
        row.id !== wanted[i].id ||
        row.version !== wanted[i].expected_version ||
        !validState(row.balance),
    )
  )
    throw new Error(
      "Balance response did not match the complete row set or supported annotation states.",
    );
}
export type TransactionExport = {
  schema_version: 1;
  workspace_revision: number;
  request: Omit<LedgerScope, "page_size">;
  matching: Matching;
  query_sha256: string;
  row_count: number;
  bytes: number;
  sha256: string;
  json: string;
};
export type TransactionSelection = { row: Transaction; revision: number };
export const emptyFilter = (): LedgerFilter => ({
  date_from: null,
  date_to: null,
  account: null,
  currency: null,
  review: null,
});
export const digest = (value: string) => /^[0-9a-f]{64}$/.test(value);
export function validatePage(
  value: SearchPage,
  revision: number,
  limit: number,
) {
  const p = value.page;
  if (
    value.schema_version !== 1 ||
    value.matching.algorithm !== "unicode_default_lowercase_literal_v1" ||
    !Array.isArray(value.matching.unicode_version) ||
    value.matching.unicode_version.length !== 3 ||
    p.schema_version !== 1 ||
    p.workspace_revision !== revision ||
    !digest(p.query_sha256) ||
    ![p.scope_count, p.selected_count, ...Object.values(p.review_counts)].every(
      (v) => Number.isSafeInteger(v) && v >= 0,
    ) ||
    Object.values(p.review_counts).reduce((a, b) => a + b, 0) !==
      p.scope_count ||
    p.selected_count > p.scope_count ||
    !Array.isArray(p.rows) ||
    p.rows.length > limit ||
    p.rows.length > p.selected_count ||
    new Set(p.rows.map((row) => row.id)).size !== p.rows.length ||
    p.rows.some(
      (row) => !row.id || !Number.isSafeInteger(row.version) || row.version < 1,
    ) ||
    (p.next_cursor !== null &&
      (typeof p.next_cursor !== "string" ||
        p.next_cursor.length === 0 ||
        p.rows.length === 0))
  )
    throw new Error(
      "Transaction page identity or counts did not match the request.",
    );
}
export function outsideStructuredScope(row: Transaction, filter: LedgerFilter) {
  return (
    (filter.currency !== null && row.currency !== filter.currency) ||
    (filter.account !== null && row.account !== filter.account) ||
    (filter.review !== null && row.review !== filter.review) ||
    (filter.date_from !== null && row.date < filter.date_from) ||
    (filter.date_to !== null && row.date > filter.date_to)
  );
}
