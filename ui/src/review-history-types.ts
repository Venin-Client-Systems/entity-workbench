import type { ReviewDecision } from "./types";
export type { ReviewDecision } from "./types";
export type ReviewDecisionTargetKind =
  | "entity"
  | "observation"
  | "hypothesis"
  | "finding"
  | "transaction"
  | "processing_job"
  | "merge";
export type ReviewDecisionPage = {
  schema_version: 1;
  workspace_revision: number;
  target_id: string;
  resolved_target_kind: ReviewDecisionTargetKind;
  scope_count: number;
  query_sha256: string;
  rows: ReviewDecision[];
  next_cursor: string | null;
};
