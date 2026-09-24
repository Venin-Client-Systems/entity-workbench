import type { ReviewState } from "./types";
export type PatternRequest = {
  date_from: string | null;
  date_to: string | null;
  account: string | null;
  currency: string | null;
  transfers: "include" | "exclude_reviewed_pairs";
  minimum_occurrences: number;
  date_tolerance_days: number;
  amount_tolerance: string;
};
export const defaultPatternRequest: PatternRequest = {
  date_from: null,
  date_to: null,
  account: null,
  currency: null,
  transfers: "include",
  minimum_occurrences: 3,
  date_tolerance_days: 2,
  amount_tolerance: "0",
};
export type MoneyTotal = {
  credits: string;
  debits: string;
  net: string;
  transaction_ids: string[];
};
export type PatternRow = {
  transaction_id: string;
  version: number;
  disposition:
    | "included"
    | "reviewed_transfer_excluded"
    | Exclude<ReviewState, "accepted">;
  verified_transfer_peer: string | null;
  unverified_transfer_match: boolean;
  has_duplicate_candidates: boolean;
  cash_rule: string | null;
  refund_rule: string | null;
};
export type RecurringCandidate = {
  id: string;
  account: string;
  currency: string;
  description_group: string;
  cadence: "weekly" | "fortnightly" | "monthly";
  total: MoneyTotal;
  minimum_debit: string;
  maximum_debit: string;
  expected_dates: string[];
  actual_dates: string[];
  deviations_days: number[];
};
export type TransactionAnalysis = {
  schema_version: 1;
  rules_version: string;
  workspace_revision: number;
  request: PatternRequest;
  workspace_transaction_count: number;
  scope_transaction_count: number;
  rows: PatternRow[];
  currencies: {
    currency: string;
    scope_count: number;
    total: MoneyTotal;
    pending_ids: string[];
    rejected_ids: string[];
    deferred_ids: string[];
    excluded_transfer_ids: string[];
    cash_candidates: MoneyTotal;
    refund_candidates: MoneyTotal;
    unclassified_ids: string[];
  }[];
  merchant_groups: {
    id: string;
    currency: string;
    description_group: string;
    accounts: string[];
    total: MoneyTotal;
  }[];
  recurring_candidates: RecurringCandidate[];
  recurrence_eligible_ids: string[];
  recurrence_unclassified_ids: string[];
  limitations: string[];
};
