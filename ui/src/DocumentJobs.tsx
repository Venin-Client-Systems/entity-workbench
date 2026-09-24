import { useEffect, useRef, useState } from "react";
import { isTauri } from "@tauri-apps/api/core";
import { command } from "./api";
import type { Evidence } from "./types";
import {
  activeJob,
  jobLabel,
  processingMethodLabel,
  processingStates,
  type ProcessingJob,
  type ProcessingJobPage,
  type ProcessingInput,
} from "./processing-types";
import { useProcessingRead } from "./useProcessingRead";
import { ProcessingJobReview } from "./ProcessingJobReview";
import "./processing.css";
import "./pdf-processing.css";

export function DocumentJobs({
  evidence,
  busy,
  onRefresh,
  requestKeys,
}: {
  evidence: Evidence[];
  busy: boolean;
  onRefresh: () => Promise<unknown>;
  requestKeys: Map<string, string>;
}) {
  const [selected, setSelected] = useState<string | null>(null);
  const [input, setInput] = useState("");
  const [method, setMethod] =
    useState<ProcessingInput["operation"]>("parse_document");
  const [filter, setFilter] = useState("all");
  const [pageNumber, setPageNumber] = useState("");
  const [dpi, setDpi] = useState("144");
  const [queueing, setQueueing] = useState(false);
  const [error, setError] = useState("");
  const [notice, setNotice] = useState("");
  const inputRef = useRef<HTMLSelectElement>(null);
  const mounted = useRef(true);
  useEffect(() => {
    mounted.current = true;
    return () => {
      mounted.current = false;
    };
  }, []);
  const read = useProcessingRead<ProcessingJobPage>(
    "list_processing_jobs",
    null,
    !queueing,
  );
  const jobs = read.value?.jobs ?? [];
  const visible = jobs.filter(
    (job) =>
      filter === "all" ||
      (filter === "active" ? activeJob(job) : job.state === filter),
  );
  const name = (job: ProcessingJob) =>
    evidence.find((item) => item.id === job.input.evidence_id)?.name ??
    job.input.evidence_id;
  const isPdf = method === "pdf_page_ocr";
  const validPdfOptions =
    /^\d+$/.test(pageNumber) &&
    Number(pageNumber) >= 1 &&
    Number(pageNumber) <= 1000 &&
    /^\d+$/.test(dpi) &&
    Number(dpi) >= 72 &&
    Number(dpi) <= 300;
  const pendingKey = isPdf
    ? `${method}:${input}:${Number(pageNumber)}:${Number(dpi)}`
    : `${method}:${input}`;
  const sameActiveJob = jobs.some(
    (job) =>
      job.input.evidence_id === input &&
      job.input.operation === method &&
      (job.input.operation !== "pdf_page_ocr" ||
        (job.input.page_number === Number(pageNumber) &&
          job.input.dpi === Number(dpi))) &&
      activeJob(job),
  );
  const queue = async () => {
    if (!input || queueing || (isPdf && !validPdfOptions)) return;
    setQueueing(true);
    setError("");
    setNotice("");
    const requestKey = requestKeys.get(pendingKey) ?? crypto.randomUUID();
    requestKeys.set(pendingKey, requestKey);
    try {
      const job = await command<ProcessingJob>({
        action: isPdf
          ? "queue_pdf_page_ocr"
          : method === "image_ocr_regions"
            ? "queue_image_ocr_regions"
            : method === "image_ocr"
              ? "queue_image_ocr"
              : "queue_document_parse",
        evidence_id: input,
        request_key: requestKey,
        ...(isPdf ? { page_number: Number(pageNumber), dpi: Number(dpi) } : {}),
      });
      requestKeys.delete(pendingKey);
      if (mounted.current) {
        await onRefresh();
        if (mounted.current) {
          setSelected(job.id);
          setNotice(
            `${isPdf ? "PDF page OCR" : method === "image_ocr_regions" ? "Image word-region OCR" : method === "image_ocr" ? "Image OCR" : "Document"} job acknowledged: ${jobLabel(job).toLowerCase()}.`,
          );
        }
      }
    } catch (cause) {
      if (mounted.current) setError(String(cause));
    } finally {
      if (mounted.current) setQueueing(false);
    }
  };
  const queueButton = (
    <button
      className="button primary"
      disabled={
        busy ||
        queueing ||
        read.loading ||
        !!read.error ||
        !input ||
        (isPdf && !validPdfOptions) ||
        (!requestKeys.has(pendingKey) && sameActiveJob)
      }
      onClick={() => void queue()}
    >
      {queueing
        ? "Queueing…"
        : requestKeys.has(pendingKey)
          ? "Recover queue acknowledgement"
          : isPdf
            ? "Queue PDF page OCR"
            : method === "image_ocr_regions"
              ? "Queue word-region OCR"
              : method === "image_ocr"
                ? "Queue image OCR"
                : "Queue document"}
    </button>
  );
  return (
    <>
      <section className="panel processing-panel" aria-label="Document jobs">
        <div className="processing-heading">
          <div>
            <p className="eyebrow">LOCAL PROCESSING / DOCUMENTS</p>
            <h2>Document jobs</h2>
          </div>
          <span className="processing-local">LOCAL INPUTS</span>
        </div>
        <p className="muted">
          Queue a retained original for bounded parsing, image OCR or one
          selected PDF page. Derivatives remain unreviewed; source anchors and
          accepted observations are separate work.
        </p>
        <div className={`processing-queue${isPdf ? " pdf-queue" : ""}`}>
          <label>
            Processing method
            <select
              value={method}
              disabled={queueing}
              onChange={(event) =>
                setMethod(event.target.value as ProcessingInput["operation"])
              }
            >
              <option value="parse_document">Document parsing</option>
              <option value="image_ocr">Image OCR · English (PNG/JPEG)</option>
              <option value="image_ocr_regions">
                Image OCR + word regions · English
              </option>
              <option value="pdf_page_ocr">PDF page OCR · English</option>
            </select>
          </label>
          <label>
            Original to process
            <select
              ref={inputRef}
              value={input}
              disabled={queueing}
              onChange={(event) => setInput(event.target.value)}
            >
              <option value="">Choose a retained original</option>
              {evidence.map((item) => (
                <option key={item.id} value={item.id}>
                  {item.name}
                </option>
              ))}
            </select>
          </label>
          {isPdf ? (
            <>
              <div className="pdf-queue-options">
                <label>
                  Page number (1-based)
                  <input
                    type="number"
                    min="1"
                    max="1000"
                    step="1"
                    value={pageNumber}
                    placeholder="Choose page"
                    disabled={queueing}
                    aria-describedby="pdf-queue-limits"
                    onChange={(event) => setPageNumber(event.target.value)}
                  />
                </label>
                <label>
                  Resolution (DPI)
                  <input
                    type="number"
                    min="72"
                    max="300"
                    step="1"
                    value={dpi}
                    disabled={queueing}
                    aria-describedby="pdf-queue-limits"
                    onChange={(event) => setDpi(event.target.value)}
                  />
                </label>
                {queueButton}
              </div>
              <p className="context-note" id="pdf-queue-limits">
                Choose one page from 1–1,000 and an integer resolution from
                72–300 DPI. The document page count is checked during rendering.
                No automatic page expansion. Scan-focused subset only: fonts,
                forms and advanced graphics may be rejected. English OCR remains
                unreviewed.
              </p>
            </>
          ) : (
            queueButton
          )}
        </div>
        {method === "image_ocr_regions" && (
          <p className="context-note">
            This method retains the canonical grayscale raster, TSV and
            immutable recognition result locally. It adds unreviewed word boxes
            for one PNG/JPEG image. Existing text-only image OCR remains
            separate. Encoded pixels are used; EXIF orientation is not applied.
            No accepted source anchor is created.
          </p>
        )}
        {method === "image_ocr" && (
          <p className="context-note">
            English OCR supports one PNG or JPEG image. PDF pages, other image
            formats and other languages are not supported by this operation.
          </p>
        )}
        {sameActiveJob && (
          <p className="muted">
            This original already has a queued or running job for this method
            {isPdf ? ", page and resolution" : ""} in the displayed history.
          </p>
        )}
        {import.meta.env.DEV && !isTauri() && (
          <p className="context-note">
            Development bridge: jobs can be queued and inspected here. Only the
            native desktop runs the available confined processing runtimes.
          </p>
        )}
        {notice && <p role="status">{notice}</p>}
        {error && (
          <p className="error" role="alert">
            {error} Retrying this queue action reuses its request key until
            acknowledged.
          </p>
        )}
        <div className="processing-toolbar">
          <label>
            Show document jobs
            <select
              value={filter}
              onChange={(event) => setFilter(event.target.value)}
            >
              <option value="all">All states</option>
              <option value="active">Queued / running</option>
              {Object.entries(processingStates).map(([key, label]) => (
                <option key={key} value={key}>
                  {label}
                </option>
              ))}
            </select>
          </label>
          <span>
            {read.value
              ? `${visible.length} shown · ${jobs.length} loaded / ${read.value.total} total`
              : "Reading jobs…"}
          </span>
        </div>
        {read.error && (
          <div className="error" role="alert">
            Job history could not refresh. Displayed entries may be stale.{" "}
            {read.error}
            <button className="button" onClick={read.refresh}>
              Reload jobs
            </button>
          </div>
        )}
        {!!read.value && read.value.total > jobs.length && (
          <p className="context-note">
            Only the newest {read.value.limit} jobs are loaded. This filter
            applies to those loaded jobs.
          </p>
        )}
        <ol className="processing-list">
          {visible.map((job) => (
            <li key={job.id}>
              <div className="processing-job-name">
                <strong>{name(job)}</strong>
                <small>{processingMethodLabel(job.input)}</small>
                <small className="processing-id">{job.id}</small>
              </div>
              <div>
                <span
                  className={`pill ${job.state === "completed" ? "processing-completed" : "warning"}`}
                >
                  {jobLabel(job)}
                </span>
                <small>
                  Attempt {job.attempt} / {job.retry.max_attempts} ·{" "}
                  {job.result_ids.length} derivatives
                </small>
              </div>
              <button
                className="button"
                id={`document-job-${job.id}`}
                aria-label={`Inspect document job ${name(job)} ${job.id}`}
                onClick={() => setSelected(job.id)}
              >
                Inspect job
              </button>
            </li>
          ))}
        </ol>
        {!read.loading && !visible.length && (
          <p className="muted">
            {jobs.length
              ? "No loaded jobs match this filter."
              : "No document jobs have been queued."}
          </p>
        )}
        <p className="context-note">
          Retries require a reason and reserve a new attempt. Earlier
          derivatives stay immutable. A completed job does not establish
          extraction accuracy or analyst acceptance.
        </p>
      </section>
      {selected && (
        <ProcessingJobReview
          key={selected}
          jobId={selected}
          restoreFocus={() => {
            const opener = document.getElementById(`document-job-${selected}`);
            if (opener instanceof HTMLButtonElement) opener.focus();
            else inputRef.current?.focus();
          }}
          evidence={evidence}
          onClose={() => {
            setSelected(null);
            read.refresh();
          }}
          onRefresh={onRefresh}
        />
      )}
    </>
  );
}
