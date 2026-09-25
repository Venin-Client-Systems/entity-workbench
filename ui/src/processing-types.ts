/** Rust processing-job.v1–v5 and extraction contracts. Mutations remain canonical commands. */
export type ProcessingState =
  | "queued"
  | "running"
  | "partial"
  | "blocked"
  | "quota_exhausted"
  | "failed"
  | "cancelled"
  | "completed";
export type DocumentProcessingInput = {
  evidence_id: string;
  sha256: string;
  bytes: number;
} & (
  | { operation: "parse_document" | "image_ocr" | "image_ocr_regions" }
  | { operation: "pdf_page_ocr"; page_number: number; dpi: number }
);
export type GraphProcessingInput = {
  operation: "shortest_connection_path";
  source_id: string;
  target_id: string;
  requested_revision: number;
  queued_revision: number;
};
export type ProcessingInput = DocumentProcessingInput | GraphProcessingInput;
export type ProcessingFailure =
  | "interrupted"
  | "input_unavailable"
  | "runtime_unavailable"
  | "worker_failed"
  | "invalid_result"
  | "unsupported_format"
  | "document_failed"
  | "image_decode_failed"
  | "encrypted_document"
  | "pdf_render_failed"
  | "cancelled_by_analyst"
  | "cleanup_failed"
  | "worker_exit_unverified"
  | "recovery_required"
  | "derivative_unavailable"
  | "stale_graph_capture"
  | "scheduling_or_runtime_unavailable";
export type ProcessingJob = {
  schema_version: number;
  id: string;
  request_key: string;
  input: ProcessingInput;
  state: ProcessingState;
  attempt: number;
  retry: { automatic: boolean; max_attempts: number };
  cancellation_requested: boolean;
  created_at: string;
  updated_at: string;
  started_at: string | null;
  finished_at: string | null;
  failure: ProcessingFailure | null;
  detail: string;
  result_ids: string[];
};
export type ProcessingJobPage = {
  jobs: ProcessingJob[];
  total: number;
  limit: number;
};
export type DocumentProcessingJob = ProcessingJob & { input: DocumentProcessingInput };
/** Positive selection: unsupported operations must never enter document controls. */
export const isDocumentProcessingJob = (job: ProcessingJob): job is DocumentProcessingJob =>
  job.input.operation === "parse_document" ||
  job.input.operation === "image_ocr" ||
  job.input.operation === "image_ocr_regions" ||
  job.input.operation === "pdf_page_ocr";
export type ParseLimitation =
  | "no_source_anchors"
  | "embedded_documents_excluded"
  | "ocr_not_performed"
  | "text_limit"
  | "metadata_limit"
  | "page_limit"
  | "font_substituted"
  | "font_coverage_unverified";
export type Extraction = {
  schema_version: 1 | 2;
  id: string;
  job_id: string;
  attempt: number;
  input: ProcessingInput & { operation: "parse_document" };
  created_at: string;
  result_sha256: string;
  result: {
    protocol_version: 1;
    job_id: string;
    content_sha256: string;
    source_bytes: number;
    parser: string;
    media_type: string;
    status: "complete" | "partial" | "unsupported" | "failed";
    text: string;
    metadata: Record<string, string[]>;
    limitations: ParseLimitation[];
    error:
      | "malformed_document"
      | "encrypted_document"
      | "archive_limits"
      | "text_extraction_restricted"
      | "font_asset_unavailable"
      | null;
  };
};
export const processingStates: Record<ProcessingState, string> = {
  queued: "Queued",
  running: "Running",
  partial: "Partial",
  blocked: "Blocked",
  quota_exhausted: "Quota exhausted",
  failed: "Failed",
  cancelled: "Cancelled",
  completed: "Completed",
};
export const jobLabel = (job: ProcessingJob) =>
  job.state === "running" && job.cancellation_requested
    ? "Cancellation requested"
    : processingStates[job.state];
export const activeJob = (job: ProcessingJob) =>
  job.state === "queued" || job.state === "running";
export const processingMethodLabel = (input: ProcessingInput) =>
  input.operation === "shortest_connection_path"
    ? "Graph connection path"
    : input.operation === "pdf_page_ocr"
    ? `PDF page OCR · page ${input.page_number} · ${input.dpi} DPI · English`
    : input.operation === "image_ocr_regions"
      ? "Image OCR + word regions · English"
      : input.operation === "image_ocr"
        ? "Image OCR · English"
        : "Document parsing";
export const retryableJob = (job: ProcessingJob) =>
  isDocumentProcessingJob(job) &&
  job.failure !== "worker_exit_unverified" &&
  job.failure !== "recovery_required" &&
  !activeJob(job) &&
  job.state !== "completed" &&
  job.attempt < job.retry.max_attempts;

/** A cancellation response can be an unchanged terminal failure from a racing caller. */
export const cancellationNotice = (job: ProcessingJob) =>
  job.state === "running" && job.cancellation_requested
    ? "Cancellation requested. Waiting for the worker outcome."
    : `Cancellation command acknowledged. Current state: ${jobLabel(job)}${job.failure && job.failure !== "cancelled_by_analyst" ? `; ${job.failure.replaceAll("_", " ")}` : ""}.`;
