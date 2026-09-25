import type { MoneyTotal } from "./transaction-analysis-types";
import type { TransactionVersion } from "./transaction-comparison-types";
import type { ReviewState, Anchor, ReviewCounts } from "./types";

export type AccountFlowRequest = {
  date_from: string | null;
  date_to: string | null;
  account: string | null;
  currency: string | null;
};
export const defaultFlowRequest: AccountFlowRequest = {
  date_from: null,
  date_to: null,
  account: null,
  currency: null,
};
export type FlowSource = {
  transaction_id: string;
  version: number;
  account: string;
  currency: string;
  date: string;
  amount: string;
  anchor: Anchor;
  review: ReviewState;
  in_scope: boolean;
  verified_transfer_peer: TransactionVersion | null;
  unverified_transfer_match: boolean;
  has_duplicate_candidates: boolean;
};
export type AccountFlowNode = {
  id: string;
  account: string;
  currency: string;
  support_only: boolean;
  scope_count: number;
  review_counts: ReviewCounts;
  accepted_ledger: MoneyTotal;
  accepted_unmapped: MoneyTotal;
  pending_ids: string[];
  rejected_ids: string[];
  deferred_ids: string[];
  unverified_transfer_count: number;
  unverified_transfer_ids: string[];
  duplicate_candidate_count: number;
  duplicate_candidate_ids: string[];
  support_ids: string[];
};
export type AccountFlowPair = {
  id: string;
  debit: TransactionVersion;
  credit: TransactionVersion;
  debit_date: string;
  credit_date: string;
  debit_in_scope: boolean;
  credit_in_scope: boolean;
  amount: string;
};
export type AccountFlowEdge = {
  id: string;
  currency: string;
  debit_node_id: string;
  credit_node_id: string;
  amount: string;
  pairs: AccountFlowPair[];
};
export type AccountFlowResult = {
  schema_version: 1;
  rules_version: "reviewed_internal_account_flows_v1";
  workspace_revision: number;
  request: AccountFlowRequest;
  workspace_transaction_count: number;
  scope_transaction_count: number;
  support_transaction_count: number;
  nodes: AccountFlowNode[];
  edges: AccountFlowEdge[];
  sources: FlowSource[];
  limitations: string[];
};
export function sameFlowScope(a: AccountFlowRequest, b: AccountFlowRequest) {
  return (
    a.date_from === b.date_from &&
    a.date_to === b.date_to &&
    a.account === b.account &&
    a.currency === b.currency
  );
}
