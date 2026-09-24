import { useEffect, useState } from "react";
import { command } from "./api";
import { Dialog } from "./Dialog";
import { SourceContent } from "./SourceContent";
import type { Evidence, Workspace } from "./types";
import type {
  CollectionExport,
  CollectionJob,
  CollectionReceipt,
  CollectionState,
  RequestReceipt,
} from "./collection-types";
import "./collection.css";

const states: Record<CollectionState, string> = {
  queued: "Queued",
  running: "Running",
  blocked: "Blocked",
  quota_exhausted: "Quota exhausted",
  failed: "Failed",
  successful_no_results: "Successful with no searchable pages",
  successful: "Successful",
  cancelled: "Cancelled",
};
const purposes: Record<RequestReceipt["purpose"], string> = {
  access_review: "Access review",
  seed: "Seed",
  link: "Link",
  redirect: "Redirect",
};
const ordinal = (sequence: number) => String(sequence + 1).padStart(2, "0");
const outcome = (request: RequestReceipt) =>
  `${request.http_status === null ? "No HTTP status" : request.http_status} · ${request.outcome === "incomplete" ? "Incomplete body" : request.outcome}`;
const tone = (state: CollectionState) =>
  state === "successful" || state === "successful_no_results"
    ? "accepted"
    : "warning";

export function CollectionHistory({
  workspace,
  busy,
  onRefresh,
}: {
  workspace: Pick<Workspace, "jobs" | "evidence">;
  busy: boolean;
  onRefresh: () => Promise<unknown>;
}) {
  const [selected, setSelected] = useState<CollectionJob | null>(null);
  return (
    <>
      <section className="panel collection-history">
        <h2>Collection history</h2>
        {workspace.jobs.length ? (
          workspace.jobs.map((job) => (
            <article className="list-card" key={job.id}>
              <span className={`pill ${tone(job.state)}`}>
                {states[job.state]}
              </span>
              <h3>{job.queries.join(", ")}</h3>
              <p>{job.detail}</p>
              <div className="collection-history-footer">
                <small>
                  {job.requests_used} / {job.max_requests} charged requests
                </small>
                <button
                  className="button"
                  disabled={busy}
                  onClick={() => setSelected(job)}
                  aria-label={`Review collection ${job.queries.join(", ")}`}
                >
                  Review collection
                </button>
              </div>
            </article>
          ))
        ) : (
          <p className="muted">
            No collection jobs have run. Local indexing requires collected or
            imported sources.
          </p>
        )}
      </section>
      {selected && (
        <CollectionReview
          key={selected.id}
          job={selected}
          evidence={workspace.evidence}
          onClose={() => setSelected(null)}
          onRefresh={onRefresh}
        />
      )}
    </>
  );
}

function CollectionReview({
  job,
  evidence,
  onClose,
  onRefresh,
}: {
  job: CollectionJob;
  evidence: Evidence[];
  onClose: () => void;
  onRefresh: () => Promise<unknown>;
}) {
  const [receipt, setReceipt] = useState<CollectionReceipt | null>(null);
  const [error, setError] = useState("");
  const [attempt, setAttempt] = useState(0);
  const [selected, setSelected] = useState(0);
  const [filter, setFilter] = useState("all");
  const [source, setSource] = useState<Evidence | null>(null);
  const [exporting, setExporting] = useState(false);
  const [exportError, setExportError] = useState("");
  const [exported, setExported] = useState<CollectionExport | null>(null);
  useEffect(() => {
    let active = true;
    setError("");
    void command<CollectionReceipt>({
      action: "inspect_collection",
      job_id: job.id,
    })
      .then((result) => {
        if (active) setReceipt(result);
      })
      .catch((cause) => {
        if (active) setError(String(cause));
      });
    return () => {
      active = false;
    };
  }, [job.id, attempt]);

  const exportCollection = async () => {
    setExporting(true);
    setExportError("");
    try {
      const result = await command<CollectionExport>({
        action: "export_collection",
        job_id: job.id,
      });
      setExported(result);
      await onRefresh();
    } catch (cause) {
      setExportError(String(cause));
    } finally {
      setExporting(false);
    }
  };
  const request = receipt?.requests.find((item) => item.sequence === selected);
  const original = request?.original_evidence_id
    ? evidence.find((item) => item.id === request.original_evidence_id)
    : undefined;
  const uniqueOriginals = new Set(
    receipt?.requests.flatMap((item) =>
      item.original_evidence_id ? [item.original_evidence_id] : [],
    ),
  ).size;
  const inspectParent = (sequence: number) => {
    setFilter("all");
    setSelected(sequence);
  };

  return (
    <>
      <Dialog
        label="Collection review"
        wide
        preventClose={exporting}
        onClose={onClose}
      >
        <div className="collection-review">
          <header className="collection-review-header">
            <div>
              <p className="eyebrow">Collection / acquisition record</p>
              <h2>Collection review</h2>
            </div>
            <button className="button" onClick={onClose} disabled={exporting}>
              Close collection review
            </button>
          </header>
          <p className="collection-identifier">
            Job {job.id}
            {receipt && (
              <>
                {" "}
                ·{" "}
                {receipt.mode === "synthetic"
                  ? "Synthetic replay"
                  : "Live collection"}
              </>
            )}
          </p>
          <div
            className={`collection-state ${tone(receipt?.state ?? job.state)}`}
          >
            <strong>{states[receipt?.state ?? job.state]}</strong>
            <p>{job.detail}</p>
          </div>
          {error ? (
            <section
              className="collection-unavailable"
              aria-label="Acquisition receipt unavailable"
            >
              <h3>Acquisition receipt unavailable</h3>
              <p role="alert">{error}</p>
              <p>
                No request trace or export can be shown. This does not mean that
                the attempt returned no results.
              </p>
              <p className="collection-url">
                Selected scope: {job.queries.join(", ")}
              </p>
              <button
                className="button"
                onClick={() => setAttempt((value) => value + 1)}
              >
                Retry receipt inspection
              </button>
            </section>
          ) : !receipt ? (
            <p role="status">Loading validated acquisition receipt…</p>
          ) : (
            <>
              <dl className="collection-metrics">
                <div>
                  <dt>Charged requests</dt>
                  <dd>
                    {receipt.requests_used} / {receipt.max_requests}
                  </dd>
                </div>
                <div>
                  <dt>Elapsed</dt>
                  <dd>
                    {receipt.elapsed_milliseconds / 1000} s /{" "}
                    {receipt.max_seconds} s
                  </dd>
                </div>
                <div>
                  <dt>Original files</dt>
                  <dd>{uniqueOriginals} unique</dd>
                </div>
                <div>
                  <dt>Retention</dt>
                  <dd>{receipt.retention_complete ? "Complete" : "Partial"}</dd>
                </div>
              </dl>
              {!receipt.retention_complete && (
                <p role="alert" className="collection-state warning">
                  Retention is incomplete. Some response originals or text
                  derivatives could not be published. Export is unavailable.
                </p>
              )}
              {receipt.time_limit_exceeded && (
                <p className="collection-state warning">
                  The collector exceeded its elapsed time limit. This is
                  recorded in the receipt.
                </p>
              )}
              <section
                className="collection-scope"
                aria-label="Selected collection scope"
              >
                <h3>Selected scope</h3>
                {receipt.selected_urls.map((url) => (
                  <p className="collection-url" key={url}>
                    {url}
                  </p>
                ))}
                <p className="muted">
                  {receipt.max_hops} expansion hops · exact selected hosts ·
                  HTTPS only
                </p>
                <dl className="collection-facts">
                  <div>
                    <dt>Started (UTC)</dt>
                    <dd>{receipt.started_at}</dd>
                  </div>
                  <div>
                    <dt>Ended (UTC)</dt>
                    <dd>{receipt.ended_at}</dd>
                  </div>
                </dl>
              </section>
              <div className="collection-ledger">
                <section
                  aria-label="Charged request ledger"
                  className="collection-requests"
                >
                  <h3>Charged requests</h3>
                  <p className="muted">
                    Includes access checks, redirects and unsuccessful attempts.
                    Uncharged reservations are absent.
                  </p>
                  <label className="collection-filter">
                    Show requests
                    <select
                      value={filter}
                      onChange={(event) => setFilter(event.target.value)}
                    >
                      <option value="all">All outcomes</option>
                      <option value="fetched">Fetched</option>
                      <option value="incomplete">Incomplete body</option>
                      <option value="blocked">Blocked</option>
                      <option value="failed">Failed</option>
                    </select>
                  </label>
                  <ol className="collection-request-list">
                    {receipt.requests
                      .filter(
                        (item) => filter === "all" || item.outcome === filter,
                      )
                      .map((item) => (
                        <li key={item.sequence}>
                          <button
                            className="collection-request"
                            aria-pressed={selected === item.sequence}
                            onClick={() => setSelected(item.sequence)}
                            aria-label={`Inspect request ${ordinal(item.sequence)} ${item.url}`}
                          >
                            <span className="collection-ordinal">
                              {ordinal(item.sequence)}
                            </span>
                            <span>
                              <strong className="collection-url">
                                {item.url}
                              </strong>
                              <span>
                                {purposes[item.purpose]}
                                {item.parent_request !== null &&
                                  ` from ${ordinal(item.parent_request)}`}{" "}
                                · hop {item.hop}
                              </span>
                              <span
                                className={
                                  item.outcome === "fetched"
                                    ? "collection-fetched"
                                    : "collection-not-fetched"
                                }
                              >
                                {outcome(item)}
                              </span>
                            </span>
                          </button>
                        </li>
                      ))}
                  </ol>
                  {!receipt.requests.length ? (
                    <p>No charged requests are recorded for this attempt.</p>
                  ) : (
                    !receipt.requests.some(
                      (item) => filter === "all" || item.outcome === filter,
                    ) && <p>No requests match this filter.</p>
                  )}
                  <p className="muted">
                    Identical response bytes share one original. This does not
                    establish source independence.
                  </p>
                </section>
                <section
                  className="collection-request-detail"
                  aria-label="Request details"
                  aria-live="polite"
                >
                  {request ? (
                    <>
                      <h3>Request {ordinal(request.sequence)}</h3>
                      <p className="collection-outcome">{outcome(request)}</p>
                      <p className="collection-url">{request.url}</p>
                      <dl className="collection-facts">
                        <div>
                          <dt>Method / purpose</dt>
                          <dd>
                            {request.method} · {purposes[request.purpose]} · hop{" "}
                            {request.hop}
                          </dd>
                        </div>
                        <div>
                          <dt>Parent request</dt>
                          <dd>
                            {request.parent_request !== null ? (
                              <button
                                className="button"
                                onClick={() =>
                                  inspectParent(request.parent_request!)
                                }
                              >
                                Inspect parent request{" "}
                                {ordinal(request.parent_request)}
                              </button>
                            ) : (
                              "None — initial request"
                            )}
                          </dd>
                        </div>
                        <div>
                          <dt>Started (UTC)</dt>
                          <dd>{request.started_at}</dd>
                        </div>
                        <div>
                          <dt>Ended (UTC)</dt>
                          <dd>{request.ended_at}</dd>
                        </div>
                        <div>
                          <dt>Response media type</dt>
                          <dd>{request.media_type ?? "Not available"}</dd>
                        </div>
                        {request.redirect_url && (
                          <div>
                            <dt>Redirect destination</dt>
                            <dd className="collection-url">
                              {request.redirect_url}
                              <p className="muted">
                                Recorded destination; following it requires a
                                separate charged request.
                              </p>
                            </dd>
                          </div>
                        )}
                        <div>
                          <dt>Complete original</dt>
                          <dd>
                            {request.original_evidence_id
                              ? `Retained · ${request.body_bytes} bytes`
                              : request.outcome === "fetched"
                                ? "Not retained — publication failed"
                                : request.outcome === "incomplete"
                                  ? "Unavailable — response body was incomplete"
                                  : "Unavailable — no complete response"}
                          </dd>
                        </div>
                        {request.body_sha256 && (
                          <div>
                            <dt>Response SHA-256</dt>
                            <dd className="collection-hash">
                              {request.body_sha256}
                            </dd>
                          </div>
                        )}
                        {request.original_evidence_id && (
                          <div>
                            <dt>Original evidence ID</dt>
                            <dd className="collection-hash">
                              {request.original_evidence_id}
                            </dd>
                          </div>
                        )}
                      </dl>
                      {original?.text !== null && original !== undefined ? (
                        <button
                          className="button"
                          onClick={() => setSource(original)}
                        >
                          Inspect retained text
                        </button>
                      ) : (
                        <p className="muted">
                          No text derivative is available for this response.
                        </p>
                      )}
                      <p className="muted">
                        Text is displayed as data. Raw pages are never executed
                        here.
                      </p>
                    </>
                  ) : (
                    <p>
                      Select a charged request to inspect its acquisition facts.
                    </p>
                  )}
                </section>
              </div>
              <section
                className="collection-notes"
                aria-label="Collection notes"
              >
                <h3>Collection notes</h3>
                {receipt.notes.length ? (
                  <ul>
                    {receipt.notes.map((note, index) => (
                      <li key={index}>{note}</li>
                    ))}
                  </ul>
                ) : (
                  <p>No additional notes.</p>
                )}
                <p className="muted">
                  Receipt v{receipt.schema_version} · application{" "}
                  {receipt.application_version} · {receipt.collector_policy} ·
                  workspace revisions {receipt.start_revision}–
                  {receipt.retained_revision}
                </p>
                <p className="muted">
                  Complete retention covers complete responses and published
                  derivatives. An incomplete body can still have no original. A
                  searchable page is not a reviewed relevant result.
                </p>
              </section>
              <section
                className="collection-export"
                aria-label="Local acquisition export"
              >
                <h3>Local acquisition export</h3>
                <p>
                  Saves this receipt and complete originals in the workspace
                  exports directory. Existing exports remain unchanged.
                </p>
                <p className="muted">
                  Acquisition facts do not establish relevance or permission to
                  republish.
                </p>
                <button
                  className="button primary"
                  disabled={exporting || !receipt.retention_complete}
                  onClick={() => void exportCollection()}
                >
                  {exporting
                    ? "Exporting acquisition bundle…"
                    : "Export acquisition bundle"}
                </button>
                {exportError && <p role="alert">{exportError}</p>}
                {exported && (
                  <div className="collection-export-result" role="status">
                    <strong>Acquisition bundle saved</strong>
                    <dl className="collection-facts">
                      <div>
                        <dt>Workspace-relative manifest</dt>
                        <dd className="collection-url">{exported.path}</dd>
                      </div>
                      <div>
                        <dt>Manifest SHA-256</dt>
                        <dd className="collection-hash">{exported.sha256}</dd>
                      </div>
                      <div>
                        <dt>Snapshot revision</dt>
                        <dd>{exported.snapshot_revision}</dd>
                      </div>
                      <div>
                        <dt>Created (UTC)</dt>
                        <dd>{exported.created_at}</dd>
                      </div>
                    </dl>
                    <p>
                      Keep the manifest and its originals folder together.
                      Inspect exported bytes as data; do not execute captured
                      content.
                    </p>
                  </div>
                )}
              </section>
            </>
          )}
        </div>
      </Dialog>
      {source && (
        <Dialog
          label="Retained collection text"
          wide
          onClose={() => setSource(null)}
        >
          <div className="collection-review-header">
            <h2>Retained collection text</h2>
            <button className="button" onClick={() => setSource(null)}>
              Close retained text
            </button>
          </div>
          <p className="collection-hash">Original SHA-256: {source.sha256}</p>
          <SourceContent evidence={source} />
        </Dialog>
      )}
    </>
  );
}
