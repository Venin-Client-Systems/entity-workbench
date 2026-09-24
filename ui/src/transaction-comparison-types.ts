import type {
  MoneyTotal,
  PatternRequest,
  PatternRow,
} from "./transaction-analysis-types";
export type DatePeriod = { from: string; through: string };
export type ComparisonRequest = {
  baseline: DatePeriod;
  comparison: DatePeriod;
  account: string | null;
  currency: string | null;
  transfers: PatternRequest["transfers"];
};
export type TransactionVersion = { transaction_id: string; version: number };
export type PeriodAccountTotal = {
  scope_count: number;
  total: MoneyTotal;
  pending_ids: string[];
  rejected_ids: string[];
  deferred_ids: string[];
  excluded_transfer_ids: string[];
  rows: PatternRow[];
};
export type AmountChange = {
  baseline: string;
  comparison: string;
  delta: string;
  relative_change:
    | { state: "defined"; numerator: string; denominator: string }
    | { state: "zero_baseline"; comparison_is_zero: boolean }
    | { state: "negative_baseline" };
};
export type AccountCurrencyComparison = {
  account: string;
  currency: string;
  baseline: PeriodAccountTotal;
  comparison: PeriodAccountTotal;
  credits: AmountChange;
  debits: AmountChange;
  net: AmountChange;
};
export type ComparisonResult = {
  schema_version: 1;
  rules_version: string;
  workspace_revision: number;
  request: ComparisonRequest;
  baseline_days: number;
  comparison_days: number;
  unequal_duration: boolean;
  gap_days: number;
  workspace_transaction_count: number;
  account_currency_transaction_count: number;
  outside_account_currency_count: number;
  baseline_transaction_count: number;
  comparison_transaction_count: number;
  outside_period_rows: TransactionVersion[];
  verified_transfer_peers: TransactionVersion[];
  groups: AccountCurrencyComparison[];
  limitations: string[];
};
