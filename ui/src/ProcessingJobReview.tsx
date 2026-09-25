import { useRef, useState } from "react";
import { command } from "./api";
import { Dialog } from "./Dialog";
import { ExtractionReview } from "./ExtractionReview";
import { ImageRegionReview } from "./ImageRegionReview";
import { ImageExtractionReview } from "./ImageExtractionReview";
import { PdfExtractionReview } from "./PdfExtractionReview";
import {
  activeJob,
  cancellationNotice,
  jobLabel,
  processingMethodLabel,
  retryableJob,
  isDocumentProcessingJob,
  type ProcessingJob,
} from "./processing-types";
import type { Evidence } from "./types";
import { useProcessingRead } from "./useProcessingRead";

export function ProcessingJobReview({
  jobId,
  evidence,
  onClose,
  onRefresh,
  restoreFocus,
}: {
  jobId: string;
  evidence: Evidence[];
  onClose: () => void;
  onRefresh: () => Promise<unknown>;
  restoreFocus: () => void;
}) {
  const [mutating, setMutating] = useState(false);
  const [error, setError] = useState("");
  const [notice, setNotice] = useState("");
  const [retryAttempt, setRetryAttempt] = useState<number | null>(null);
  const [reason, setReason] = useState("");
  const [extraction, setExtraction] = useState<string | null>(null);
  const closeRef = useRef<HTMLButtonElement>(null);
  const retryRef = useRef<HTMLButtonElement>(null);
  const read = useProcessingRead<ProcessingJob>(
    "inspect_processing_job",
    jobId,
    !mutating,
  );
  const job = read.value && read.value.id === jobId && isDocumentProcessingJob(read.value)
    ? read.value : null;
  const mutate = async (
    action: "cancel_processing_job" | "retry_processing_job",
    expectedAttempt: number,
  ) => {
    if (mutating || !job) return;
    setMutating(true);
    setError("");
    setNotice("");
    try {
      const updated = await command<ProcessingJob>({
        action,
        job_id: jobId,
        expected_attempt: expectedAttempt,
        ...(action === "retry_processing_job" ? { reason } : {}),
      });
      if (!isDocumentProcessingJob(updated) || updated.id !== jobId || updated.request_key !== job.request_key)
        throw new Error("Acknowledgement did not identify the selected document job.");
      read.setValue(updated);
      setNotice(
        action === "retry_processing_job"
          ? `Attempt ${updated.attempt} reserved and queued.`
          : cancellationNotice(updated),
      );
      await onRefresh();
      if (action === "retry_processing_job") {
        setRetryAttempt(null);
        setReason("");
      }
    } catch (cause) {
      setError(String(cause));
    } finally {
      setMutating(false);
    }
  };
  return (
    <>
      <Dialog
        wide
        label="Document job"
        restoreFocus={restoreFocus}
        onClose={onClose}
        preventClose={mutating}
      >
        <div className="processing-review">
          <div className="processing-heading">
            <div>
              <p className="eyebrow">LOCAL PROCESSING / ATTEMPT RECORD</p>
              <h2>Document job</h2>
            </div>
            <button
              ref={closeRef}
              className="close"
              disabled={mutating}
              onClick={onClose}
              aria-label="Close document job"
            >
              ×
            </button>
          </div>
          <p className="processing-id">{jobId}</p>
          {read.error && (
            <p className="error" role="alert">
              Job could not refresh. Displayed state may be stale. {read.error}{" "}
              <button className="button" onClick={read.refresh}>
                Reload job
              </button>
            </p>
          )}
          {!job && (
            <p role="status">
              {read.value
                ? "The response does not identify this document job. Its original and extraction controls are unavailable here."
                : read.error
                ? "Job details unavailable."
                : "Loading document job…"}
            </p>
          )}
          {job && (
            <>
              <h3 className="processing-filename">
                {evidence.find((item) => item.id === job.input.evidence_id)
                  ?.name ?? "Retained original"}
              </h3>
              <div className="processing-banner">
                <strong>{jobLabel(job)}</strong>
                <p>{job.detail}</p>
                {job.cancellation_requested && job.state === "running" && (
                  <p>
                    The request is recorded. Worker exit and final cleanup are
                    not yet confirmed.
                  </p>
                )}
                {job.failure === "worker_exit_unverified" && (
                  <p>
                    Worker exit is unconfirmed. Any remaining assignment files
                    require verified recovery; no result has been published for
                    this failed attempt.
                  </p>
                )}
                {job.failure === "recovery_required" && (
                  <p>
                    Document processing is suspended until process recovery is
                    verified. Queueing and retries are blocked.
                  </p>
                )}
                {job.failure === "cleanup_failed" && (
                  <p>
                    Scratch cleanup failed. Review the failure even if
                    cancellation was requested.
                  </p>
                )}
              </div>
              <dl className="processing-metrics">
                <div>
                  <dt>Reserved attempt</dt>
                  <dd>
                    {job.attempt} / {job.retry.max_attempts}
                  </dd>
                </div>
                <div>
                  <dt>Derivatives retained</dt>
                  <dd>{job.result_ids.length}</dd>
                </div>
                <div>
                  <dt>Retry policy</dt>
                  <dd>Manual only</dd>
                </div>
              </dl>
              <dl className="processing-facts">
                <dt>Processing method</dt>
                <dd>{processingMethodLabel(job.input)}</dd>
                <dt>Original SHA-256</dt>
                <dd>
                  <code>{job.input.sha256}</code>
                </dd>
                <dt>Original bytes</dt>
                <dd>{job.input.bytes.toLocaleString("en-US")}</dd>
                <dt>Evidence ID</dt>
                <dd>
                  <code>{job.input.evidence_id}</code>
                </dd>
                <dt>Job created</dt>
                <dd>{job.created_at}</dd>
                <dt>Updated at</dt>
                <dd>{job.updated_at}</dd>
                <dt>Attempt started</dt>
                <dd>{job.started_at ?? "Not started"}</dd>
                <dt>Attempt finished</dt>
                <dd>{job.finished_at ?? "Not finished"}</dd>
                <dt>Failure category</dt>
                <dd>{job.failure?.replaceAll("_", " ") ?? "None recorded"}</dd>
              </dl>
              <div className="processing-controls">
                {activeJob(job) && (
                  <button
                    className="button"
                    disabled={
                      mutating || !!read.error || job.cancellation_requested
                    }
                    onClick={() =>
                      void mutate("cancel_processing_job", job.attempt)
                    }
                  >
                    {job.cancellation_requested
                      ? "Cancellation requested"
                      : `Cancel attempt ${job.attempt}`}
                  </button>
                )}
                {retryableJob(job) && (
                  <button
                    ref={retryRef}
                    className="button"
                    disabled={mutating || !!read.error}
                    onClick={() => {
                      setRetryAttempt(job.attempt);
                      setError("");
                    }}
                  >
                    Review retry
                  </button>
                )}
              </div>
              {!activeJob(job) &&
                job.state !== "completed" &&
                job.attempt >= job.retry.max_attempts && (
                  <p className="context-note">
                    This job has used all {job.retry.max_attempts} reserved
                    attempts. Automatic retries are disabled.
                  </p>
                )}
              {notice && <p role="status">{notice}</p>}
              {error && retryAttempt === null && (
                <p role="alert" className="error">
                  {error}
                </p>
              )}
              <section
                className="processing-derivatives"
                aria-label="Retained extractions"
              >
                <h3>Immutable derivatives</h3>
                {job.result_ids.length ? (
                  job.result_ids.map((id) => (
                    <button
                      key={id}
                      className="button processing-derivative"
                      onClick={() => setExtraction(id)}
                    >
                      Inspect extraction{" "}
                      <span className="processing-id">{id}</span>
                    </button>
                  ))
                ) : (
                  <p className="muted">
                    No extraction has been published for this job.
                  </p>
                )}
                <p className="context-note">
                  Derivatives from earlier attempts remain available. Their
                  recorded attempt and processing status appear in extraction
                  review. Extracted text is unreviewed and has no source
                  anchors.
                </p>
              </section>
            </>
          )}
        </div>
      </Dialog>
      {retryAttempt !== null && (
        <Dialog
          label="Retry document job"
          preventClose={mutating}
          onClose={() => setRetryAttempt(null)}
          restoreFocus={() =>
            (retryRef.current?.disabled === false
              ? retryRef.current
              : closeRef.current
            )?.focus()
          }
        >
          <div className="processing-review">
            <div className="processing-heading">
              <h2>Retry document job</h2>
              <button
                className="close"
                disabled={mutating}
                onClick={() => setRetryAttempt(null)}
                aria-label="Close retry"
              >
                ×
              </button>
            </div>
            <p>
              Reserve attempt {retryAttempt + 1} against recorded attempt{" "}
              {retryAttempt}. Earlier derivatives remain unchanged.
            </p>
            <label>
              Reason for retry
              <textarea
                value={reason}
                maxLength={2000}
                disabled={mutating}
                onChange={(event) => setReason(event.target.value)}
              />
            </label>
            {job && job.attempt !== retryAttempt && (
              <p role="alert" className="error">
                This job has advanced to attempt {job.attempt}. Close this
                review and inspect the current state before retrying. Your
                reason is retained.
              </p>
            )}
            {error && (
              <p role="alert" className="error">
                {error}
              </p>
            )}
            <button
              className="button primary"
              disabled={
                mutating ||
                !!read.error ||
                !reason.trim() ||
                job?.attempt !== retryAttempt ||
                !job ||
                !retryableJob(job)
              }
              onClick={() => void mutate("retry_processing_job", retryAttempt)}
            >
              {mutating ? "Reserving…" : "Reserve retry attempt"}
            </button>
          </div>
        </Dialog>
      )}
      {extraction &&
        (job?.input.operation === "image_ocr_regions" ? (
          <ImageRegionReview
            key={extraction}
            extractionId={extraction}
            sourceName={
              evidence.find((item) => item.id === job.input.evidence_id)
                ?.name ?? "Retained original"
            }
            onClose={() => setExtraction(null)}
          />
        ) : job?.input.operation === "pdf_page_ocr" ? (
          <PdfExtractionReview
            key={extraction}
            extractionId={extraction}
            sourceName={
              evidence.find((item) => item.id === job.input.evidence_id)
                ?.name ?? "Retained original"
            }
            onClose={() => setExtraction(null)}
          />
        ) : job?.input.operation === "image_ocr" ? (
          <ImageExtractionReview
            key={extraction}
            extractionId={extraction}
            sourceName={
              evidence.find((item) => item.id === job.input.evidence_id)
                ?.name ?? "Retained original"
            }
            onClose={() => setExtraction(null)}
          />
        ) : (
          <ExtractionReview
            key={extraction}
            extractionId={extraction}
            onClose={() => setExtraction(null)}
          />
        ))}
    </>
  );
}
