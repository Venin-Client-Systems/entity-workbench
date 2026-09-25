/** Public command.v4 / collection-receipt.v1 contracts, validated by Rust. */
export type CollectionState =
  | "queued"
  | "running"
  | "blocked"
  | "quota_exhausted"
  | "failed"
  | "successful_no_results"
  | "successful"
  | "cancelled";
export type CollectionJob = {
  id: string;
  queries: string[];
  adapters: string[];
  state: CollectionState;
  requests_used: number;
  max_hops: number;
  max_requests: number;
  max_seconds: number;
  detail: string;
};
export type RequestReceipt = {
  sequence: number;
  url: string;
  method: string;
  purpose: "access_review" | "seed" | "link" | "redirect";
  parent_request: number | null;
  hop: number;
  started_at: string;
  ended_at: string;
  outcome: "fetched" | "blocked" | "failed" | "incomplete";
  http_status: number | null;
  media_type: string | null;
  redirect_url: string | null;
  body_sha256: string | null;
  body_bytes: number | null;
  original_evidence_id: string | null;
};
export type CollectionReceipt = {
  schema_version: 1;
  job_id: string;
  mode: "live" | "synthetic";
  application_version: string;
  collector_policy: string;
  selected_urls: string[];
  max_hops: number;
  max_requests: number;
  max_seconds: number;
  requests_used: number;
  started_at: string;
  ended_at: string;
  elapsed_milliseconds: number;
  time_limit_exceeded: boolean;
  start_revision: number;
  retained_revision: number;
  state: CollectionState;
  retention_complete: boolean;
  requests: RequestReceipt[];
  notes: string[];
};
export type CollectionExport = {
  schema_version: 1;
  id: string;
  job_id: string;
  snapshot_revision: number;
  created_at: string;
  path: string;
  sha256: string;
};
