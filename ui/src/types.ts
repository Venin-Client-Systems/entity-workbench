import type { CollectionJob } from "./collection-types";
import type {
  StatementProfile,
  StatementImportRecord,
} from "./statement-types";
export type ReviewState = "pending" | "accepted" | "rejected" | "deferred";
export type ReviewDecision = {
  id: string;
  target_id: string;
  state: ReviewState;
  reason: string;
  at: string;
};
export type Anchor = {
  kind: string;
  evidence_id: string;
  line_start?: number;
  line_end?: number;
  page?: number;
  row?: number;
  column?: string;
  sheet?: string;
};
export type EntityKind =
  | "person"
  | "organisation"
  | "group"
  | "account"
  | "place"
  | "digital_identifier";
export type EntityInput = {
  name: string;
  kind: EntityKind;
  identifiers: { namespace: string; value: string }[];
};
export type Entity = EntityInput & {
  id: string;
  name: string;
  kind: EntityKind;
  identifiers: { namespace: string; value: string }[];
  merged_into: string | null;
};
export type Observation = {
  id: string;
  entity_id: string;
  field: string;
  value: string;
  anchor: Anchor;
  extraction_quality: number | null;
  review: ReviewState;
};
export type IdentityComparison = {
  workspace_revision: number;
  left: Entity;
  right: Entity;
  fields: {
    field: string;
    left: Observation[];
    right: Observation[];
    source_groups: string[];
    signal:
      | "insufficient_reviewed_evidence"
      | "shared_reviewed_values"
      | "different_reviewed_values"
      | "mixed_reviewed_values";
  }[];
};
export type SourceExcerpt = {
  evidence_id: string;
  workspace_revision: number;
  location: string;
  quote: string;
  truncated: boolean;
};
export type Evidence = {
  id: string;
  name: string;
  sha256: string;
  bytes: number;
  media_type: string;
  origin_group: string;
  extraction_status: string;
  text: string | null;
  acquisitions: { job_id: string; url: string; retrieved_at: string }[];
};
export type Transaction = {
  id: string;
  account: string;
  date: string;
  posting_date: string | null;
  description: string;
  amount: string;
  currency: string;
  balance: string | null;
  anchor: Anchor;
  review: ReviewState;
  duplicate_candidates: string[];
  transfer_peer: string | null;
  version: number;
};
export type Hypothesis = {
  id: string;
  question: string;
  proposition: string;
  alternatives: string[];
  gaps: string[];
};
export type Finding = {
  hypothesis_ids: string[];
  id: string;
  title: string;
  assessment: string;
  supporting_ids: string[];
  contradicting_ids: string[];
  limitations: string;
  needs_review: boolean;
};
export type Workspace = {
  revision: number;
  statement_profiles: StatementProfile[];
  statement_imports: StatementImportRecord[];
  entities: Entity[];
  evidence: Evidence[];
  transactions: Transaction[];
  observations: Observation[];
  assertions: {
    id: string;
    subject_id: string;
    object_id: string;
    predicate: string;
    review: ReviewState;
  }[];
  findings: Finding[];
  hypotheses: Hypothesis[];
  leads: {
    id: string;
    label: string;
    identifier: { namespace: string; value: string };
    state: ReviewState;
  }[];
  addresses: {
    id: string;
    label: string;
    latitude: number;
    longitude: number;
    valid_from: string;
    valid_to: string | null;
  }[];
  locations: {
    id: string;
    merchant: string;
    branch: string | null;
    latitude: number | null;
    longitude: number | null;
    review: ReviewState;
  }[];
  jobs: CollectionJob[];
  merges: {
    id: string;
    source: string;
    target: string;
    reason: string;
    reversed: boolean;
  }[];
  identity_decisions: {
    id: string;
    left_id: string;
    right_id: string;
    outcome: "keep_separate" | "defer";
    reason: string;
    at: string;
  }[];
  reports: {
    id: string;
    workspace_revision: number;
    created_at: string;
    sha256: string;
    html_bytes: number;
  }[];
  decisions: ReviewDecision[];
};
export type Analysis = {
  pending: number;
  duplicate_candidates: number;
  totals: {
    currency: string;
    credits: string;
    debits: string;
    net: string;
    transaction_ids: string[];
    excluded_transfer_ids: string[];
  }[];
  balance_checks: {
    transaction_id: string;
    previous_id: string;
    transaction_ids: string[];
    difference: string;
    reconciled: boolean;
  }[];
};
export type Response = { workspace: Workspace; analysis: Analysis };

export type ReviewCounts = Record<ReviewState, number>;
export type DesktopWorkspace = Omit<Workspace, "transactions" | "decisions"> & {
  schema_version: number;
  review_decision_count: number;
};
export type LedgerSummary = {
  transaction_count: number;
  review_counts: ReviewCounts;
  duplicate_candidate_row_count: number;
  balance_check_count: number;
  balance_discrepancy_count: number;
  totals: {
    currency: string;
    credits: string;
    debits: string;
    net: string;
    included_count: number;
    excluded_transfer_count: number;
  }[];
};
export type DesktopSummaryResponse = {
  schema_version: 1;
  workspace: DesktopWorkspace;
  analysis: LedgerSummary;
};
