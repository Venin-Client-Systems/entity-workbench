/** Command v23 projections. Rust owns normalization, execution and publication. */
export type CollectionInput = {
  urls: string[];
  max_hops: number;
  max_requests: number;
  max_seconds: number;
};
export type CollectionAvailability =
  | "native_disabled"
  | "standalone_unavailable"
  | "synthetic_fixture"
  | "ready"
  | "recovery_required"
  | "execution_unavailable"
  | "stopping";
export type CollectionPreview = {
  schema_version: 1;
  collector_policy: string;
  input: CollectionInput;
  selected_hosts: string[];
  robots_urls: string[];
  disclosure: {
    dns_hostnames: boolean;
    connection_metadata: boolean;
    selected_and_followed_urls: boolean;
    automatic_case_contents: boolean;
    followed_hosts: string;
  };
  preview_sha256: string;
};
export type RunState =
  | "queued"
  | "running"
  | "interrupted"
  | "recovery_required"
  | "cancelled"
  | "blocked"
  | "quota_exhausted"
  | "failed"
  | "partial"
  | "successful"
  | "successful_no_results";
export type CollectionRun = {
  id: string;
  request_key: string;
  record_version: number;
  mode: "live" | "synthetic";
  collector_policy: string;
  input: CollectionInput;
  created_at_ms: number;
  updated_at_ms: number;
  state: RunState;
  generation: number;
  first_started_at_ms: number | null;
  deadline_at_ms: number | null;
  cancellation_requested: boolean;
  requests_used: number;
  pages_retained: number;
  frontier_remaining: number;
};
export type CollectionRunPage = {
  schema_version: 1;
  workspace_revision: number;
  availability: CollectionAvailability;
  native_execution_enabled: boolean;
  scope_count: number;
  rows: CollectionRun[];
  next_cursor: string | null;
};
type Head = {
  status: number;
  media_type: string | null;
  redirect_url: string | null;
};
export type StopReason =
  | "cancelled"
  | "deadline"
  | "clock_changed"
  | "timeout"
  | "network"
  | "policy"
  | "body_limit"
  | "resolver_unavailable"
  | "busy"
  | "quiescence_unverified"
  | "recovery_required";
export type Receipt = {
  schema_version: 1;
  outcome:
    | { kind: "complete"; sha256: string; bytes: number; head: Head }
    | { kind: "stopped"; reason: StopReason; head: Head | null };
  phase: "before_request" | "pacing" | "dns" | "connect_tls_headers" | "body";
  http_delivery: "definitively_before_http" | "may_have_been_sent";
  elapsed_milliseconds: number;
  observed_wall_ms: number;
  resolved: {
    addresses: string[];
    method: string;
    authoritative_complete_set: boolean;
  } | null;
  resolver_uncertainty: { method: string; caller_context: string } | null;
  stop_observed: StopReason | null;
  locally_quiescent: boolean;
};
type Fetch =
  | ({ kind: "complete"; sha256: string; bytes: number } & Head)
  | ({ kind: "incomplete" } & Omit<Head, "redirect_url">)
  | { kind: "failed"; reason: "network" | "policy" | "quota" | "interrupted" };
export type CollectionRequest = {
  sequence: number;
  generation: number;
  entry: {
    url: string;
    hop: number;
    redirects: number;
    purpose: "robots" | "seed" | "link" | "redirect";
    parent: number | null;
  };
  reserved_at_ms: number;
  progress:
    | { state: "reserved" }
    | { state: "settled"; ended_at_ms: number; result: Fetch }
    | { state: "observed"; receipt: Receipt }
    | { state: "interrupted_unknown"; recovered_at_ms: number };
  original: { evidence_id: string; sha256: string; bytes: number } | null;
};
export type CollectionInspection = {
  schema_version: 1;
  workspace_revision: number;
  availability: CollectionAvailability;
  native_execution_enabled: boolean;
  run: CollectionRun;
  execution: {
    phase:
      | "unavailable"
      | "idle"
      | "running"
      | "settling"
      | "settlement_pending"
      | "faulted"
      | "recovery_required"
      | "stopped"
      | "unpublished";
    request_sequence: number | null;
    publication_retries: number;
    publication_retry_limit: number;
  };
  controls: {
    can_cancel: boolean;
    can_resume: boolean;
    can_retry_settlement: boolean;
  };
  requests: CollectionRequest[];
  limitations: string[];
};
export const runLabels: Record<RunState, string> = {
  queued: "Queued",
  running: "Running",
  interrupted: "Interrupted",
  recovery_required: "Recovery required",
  cancelled: "Cancelled",
  blocked: "Blocked",
  quota_exhausted: "Quota exhausted",
  failed: "Failed",
  partial: "Partial",
  successful: "Successful",
  successful_no_results: "Successful with no searchable pages",
};
export const availabilityLabels: Record<CollectionAvailability, string> = {
  native_disabled:
    "Live collection is not enabled in this build. You can preview scope and review retained collections.",
  standalone_unavailable:
    "This development view can preview and review collections. It cannot start or control collection work.",
  synthetic_fixture:
    "Synthetic test collection only. No public website is contacted.",
  ready: "Collection is available. Review the exact scope before queueing.",
  recovery_required:
    "Completion could not be verified. New collection work is suspended; retained records remain available.",
  execution_unavailable:
    "Collection processing is unavailable. Retained records remain available.",
  stopping:
    "The application is stopping collection work. New requests are unavailable.",
};
export const canQueue = (availability: CollectionAvailability | null) =>
  availability === "ready" || availability === "synthetic_fixture";
const integer = (n: unknown) =>
  typeof n === "number" && Number.isSafeInteger(n) && n >= 0;
const sha = (s: unknown) => typeof s === "string" && /^[a-f0-9]{64}$/.test(s);
const supported = (a: CollectionAvailability) =>
  Object.hasOwn(availabilityLabels, a);
export function validatePreview(p: CollectionPreview) {
  if (
    p?.schema_version !== 1 ||
    !sha(p.preview_sha256) ||
    !Array.isArray(p.input?.urls) ||
    p.input.urls.length < 1 ||
    p.input.urls.length > 10 ||
    !Array.isArray(p.selected_hosts) ||
    !Array.isArray(p.robots_urls) ||
    !p.disclosure?.dns_hostnames ||
    !p.disclosure.connection_metadata ||
    !p.disclosure.selected_and_followed_urls ||
    p.disclosure.automatic_case_contents !== false ||
    p.disclosure.followed_hosts !== "selected_hosts_only"
  )
    throw new Error("Collection disclosure is unavailable or unsupported.");
}
export function validateRunPage(
  p: CollectionRunPage,
  revision: number,
  offset: number,
) {
  if (
    p?.schema_version !== 1 ||
    p.workspace_revision !== revision ||
    !supported(p.availability) ||
    !integer(p.scope_count) ||
    !Array.isArray(p.rows) ||
    p.rows.length > 25 ||
    offset + p.rows.length > p.scope_count ||
    (p.next_cursor !== null &&
      (typeof p.next_cursor !== "string" ||
        p.next_cursor.length > 1024 ||
        p.rows.length === 0)) ||
    new Set(p.rows.map((r) => r.id)).size !== p.rows.length
  )
    throw new Error("Collection catalogue is unavailable or stale.");
}
export function validateInspection(
  p: CollectionInspection,
  id: string | null,
  revision: number,
) {
  if (
    p?.schema_version !== 1 ||
    !integer(p.workspace_revision) ||
    p.workspace_revision < revision ||
    !supported(p.availability) ||
    !p.run ||
    !isCanonicalUuid(p.run.id) ||
    !isCanonicalUuid(p.run.request_key) ||
    (id !== null && p.run.id !== id) ||
    !integer(p.run.generation) ||
    !Array.isArray(p.requests) ||
    p.requests.length > 50 ||
    !p.controls ||
    !p.execution ||
    !Array.isArray(p.limitations)
  )
    throw new Error("Collection inspection is unavailable or stale.");
}

export function collectionDate(n: number | null) {
  if (n === null) return "Not started";
  const d = new Date(n);
  return Number.isNaN(d.getTime())
    ? `Timestamp ${n} milliseconds`
    : d.toISOString();
}

export const isCanonicalUuid = (value: unknown): value is string =>
  typeof value === "string" &&
  /^[0-9a-f]{8}-[0-9a-f]{4}-[0-9a-f]{4}-[0-9a-f]{4}-[0-9a-f]{12}$/.test(value);
