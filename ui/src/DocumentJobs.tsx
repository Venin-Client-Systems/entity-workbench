import { useEffect, useRef, useState } from "react";
import { isTauri } from "@tauri-apps/api/core";
import { command } from "./api";
import type { Evidence } from "./types";
import {
  activeJob,
  jobLabel,
  processingStates,
  type ProcessingJob,
  type ProcessingJobPage,
} from "./processing-types";
import { useProcessingRead } from "./useProcessingRead";
import { ProcessingJobReview } from "./ProcessingJobReview";
import "./processing.css";

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
  const [filter, setFilter] = useState("all");
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
  const queue = async () => {
    if (!input || queueing) return;
    setQueueing(true);
    setError("");
    setNotice("");
    const requestKey = requestKeys.get(input) ?? crypto.randomUUID();
    requestKeys.set(input, requestKey);
    try {
      const job = await command<ProcessingJob>({
        action: "queue_document_parse",
        evidence_id: input,
        request_key: requestKey,
      });
      requestKeys.delete(input);
      if (mounted.current) {
        await onRefresh();
        if (mounted.current) {
          setSelected(job.id);
          setNotice(
            `Document job acknowledged: ${jobLabel(job).toLowerCase()}.`,
          );
        }
      }
    } catch (cause) {
      if (mounted.current) setError(String(cause));
    } finally {
      if (mounted.current) setQueueing(false);
    }
  };
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
          Queue a retained original for bounded parsing. Derivatives remain
          unreviewed; source anchors and accepted observations are separate
          work.
        </p>
        <div className="processing-queue">
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
          <button
            className="button primary"
            disabled={
              busy ||
              queueing ||
              read.loading ||
              !!read.error ||
              !input ||
              (!requestKeys.has(input) &&
                jobs.some(
                  (job) => job.input.evidence_id === input && activeJob(job),
                ))
            }
            onClick={() => void queue()}
          >
            {queueing
              ? "Queueing…"
              : requestKeys.has(input)
                ? "Recover queue acknowledgement"
                : "Queue document"}
          </button>
        </div>
        {jobs.some(
          (job) => job.input.evidence_id === input && activeJob(job),
        ) && (
          <p className="muted">
            This original already has a queued or running job in the displayed
            history.
          </p>
        )}
        {import.meta.env.DEV && !isTauri() && (
          <p className="context-note">
            Development bridge: jobs can be queued and inspected here. Only the
            native desktop runs the available confined parsing runtime.
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
