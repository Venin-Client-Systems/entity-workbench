import { useEffect, useRef, useState } from "react";
import { command } from "./api";
import { Dialog } from "./Dialog";
import { SourceContent } from "./SourceContent";
import type { Evidence } from "./types";
import { DurableCollectionSession } from "./durable-collection-session";
import {
  availabilityLabels,
  canQueue,
  runLabels,
  validateInspection,
  type CollectionInspection,
  type CollectionRequest,
  type Receipt,
  collectionDate as date,
} from "./durable-collection-types";
const executionLabels: Record<
  CollectionInspection["execution"]["phase"],
  string
> = {
  unavailable: "Unavailable",
  idle: "No active request",
  running: "Request in progress",
  settling: "Saving observed response",
  settlement_pending: "Response awaits saving",
  faulted: "Processing failed",
  recovery_required: "Recovery required",
  stopped: "Stopped",
  unpublished: "Result not published",
};
export function RunReview({
  id,
  revision,
  session,
  evidence,
  onClose,
  refresh,
}: {
  id: string;
  revision: number;
  session: DurableCollectionSession;
  evidence: Evidence[];
  onClose: () => void;
  refresh: () => Promise<boolean>;
}) {
  const [result, setResult] = useState<CollectionInspection | null>(null),
    [error, setError] = useState(""),
    [working, setWorking] = useState(false),
    [tick, setTick] = useState(0),
    [source, setSource] = useState<Evidence | null>(null),
    [notice, setNotice] = useState("");
  const mounted = useRef(true),
    generation = useRef(0),
    mutating = useRef(false);
  useEffect(() => {
    mounted.current = true;
    return () => {
      mounted.current = false;
      generation.current++;
    };
  }, []);
  useEffect(() => {
    let active = true;
    const current = ++generation.current;
    let timer: ReturnType<typeof setTimeout> | undefined;
    if (mutating.current) return;
    setError("");
    void session.inspection
      .read(() =>
        command<CollectionInspection>({
          action: "inspect_collection_run",
          job_id: id,
        }),
      )
      .then((value) => {
        validateInspection(value, id, revision);
        if (active && current === generation.current) {
          setResult(value);
          if (
            canQueue(value.availability) &&
            ["queued", "running"].includes(value.run.state)
          )
            timer = setTimeout(() => setTick((n) => n + 1), 2000);
        }
      })
      .catch((e) => {
        if (active && current === generation.current) {
          setResult(null);
          setError(String(e));
        }
      });
    return () => {
      active = false;
      clearTimeout(timer);
    };
  }, [tick, id, revision, session]);
  async function control(
    action:
      "cancel_collection" | "resume_collection" | "retry_collection_settlement",
  ) {
    if (!result || mutating.current) return;
    const prior = result;
    let acknowledged = false;
    mutating.current = true;
    generation.current++;
    setWorking(true);
    setError("");
    setNotice("");
    try {
      const value = await command<CollectionInspection>({
        action,
        job_id: id,
        expected_generation: prior.run.generation,
        ...(action === "retry_collection_settlement"
          ? { request_sequence: prior.execution.request_sequence }
          : {}),
      });
      validateInspection(value, id, revision);
      acknowledged = true;
      if (!mounted.current) return;
      setResult(value);
      setNotice(
        `Current recorded outcome: ${runLabels[value.run.state]}.${value.run.cancellation_requested ? " Cancellation requested; this is not proof that processing has stopped." : ""}`,
      );
      const ok = await refresh().catch(() => false);
      if (mounted.current && !ok)
        setNotice(
          (n) =>
            n + " Workspace refresh failed; the returned outcome is retained.",
        );
    } catch (e) {
      if (mounted.current) {
        setResult(null);
        setError(
          `Control outcome is unconfirmed. Refresh before taking another action. ${String(e)}`,
        );
      }
    } finally {
      mutating.current = false;
      if (mounted.current) {
        setWorking(false);
        if (acknowledged) setTick((n) => n + 1);
      }
    }
  }
  return (
    <>
      <Dialog label="Durable collection review" wide onClose={onClose}>
        <div className="durable-collection">
          <header className="collection-review-header">
            <div>
              <p className="eyebrow">Discovery / retained collection</p>
              <h2>Collection review</h2>
            </div>
            <button className="button" onClick={onClose}>
              Close collection review
            </button>
          </header>
          <p>
            <code>{id}</code>
          </p>
          <button
            className="button"
            disabled={working}
            onClick={() => setTick((n) => n + 1)}
          >
            Refresh collection
          </button>
          {error && (
            <p role="alert" className="alert error">
              {error}
            </p>
          )}
          {notice && <p role="status">{notice}</p>}
          {!result && !error && (
            <p role="status">Loading verified collection record…</p>
          )}
          {result && (
            <>
              <p className="alert" role="status">
                {availabilityLabels[result.availability]}{" "}
                {result.run.mode === "synthetic" &&
                  "Retained synthetic specimen."}
              </p>
              <div className="durable-metrics">
                <div>
                  <span>Recorded outcome</span>
                  <strong>{runLabels[result.run.state]}</strong>
                </div>
                <div>
                  <span>Current execution</span>
                  <strong>{executionLabels[result.execution.phase]}</strong>
                </div>
              </div>
              <div className="durable-metrics">
                <div>
                  <span>Charged requests</span>
                  <strong>
                    {result.run.requests_used} / {result.run.input.max_requests}
                  </strong>
                </div>
                <div>
                  <span>Pages retained</span>
                  <strong>{result.run.pages_retained}</strong>
                </div>
                <div>
                  <span>Frontier remaining</span>
                  <strong>{result.run.frontier_remaining}</strong>
                </div>
              </div>
              <h3>Selected scope</h3>
              {result.run.input.urls.map((u, i) => (
                <p key={i}>
                  <code>{u}</code>
                </p>
              ))}
              <p>
                Limits: {result.run.input.max_hops} hops ·{" "}
                {result.run.input.max_requests} requests ·{" "}
                {result.run.input.max_seconds} seconds
              </p>
              <p>
                First started: {date(result.run.first_started_at_ms)} ·
                Deadline: {date(result.run.deadline_at_ms)}
              </p>
              {result.run.cancellation_requested && (
                <p className="alert">
                  Cancellation was requested. Read the recorded outcome and
                  current execution separately; request alone does not prove
                  completion.
                </p>
              )}
              <div className="actions">
                <button
                  className="button"
                  disabled={working || !result.controls.can_cancel}
                  onClick={() => void control("cancel_collection")}
                >
                  Cancel collection
                </button>
                <button
                  className="button"
                  disabled={working || !result.controls.can_resume}
                  onClick={() => void control("resume_collection")}
                >
                  Resume collection
                </button>
                <button
                  className="button"
                  disabled={
                    working ||
                    !result.controls.can_retry_settlement ||
                    result.execution.request_sequence === null
                  }
                  onClick={() => void control("retry_collection_settlement")}
                >
                  Retry saving response
                </button>
              </div>
              <p className="context-note">
                Resume preserves spent budget and the first deadline, visiting
                only eligible remaining work. Retry saving response publishes
                only the already observed response and sends no new request.
              </p>
              {result.execution.phase === "settlement_pending" && (
                <p className="alert">
                  An observed response awaits saving.{" "}
                  {result.execution.publication_retries} of{" "}
                  {result.execution.publication_retry_limit} publication retries
                  used. No new external request is authorized by retrying this
                  save.
                </p>
              )}
              <h3>Retained request history</h3>
              <p>
                Charged attempts remain visible, including unknown or incomplete
                outcomes. Retained text is unreviewed and creates no accepted
                facts.
              </p>
              {!result.requests.length && <p>No request has been reserved.</p>}
              {result.requests.map((request) => (
                <RequestRow
                  key={request.sequence}
                  request={request}
                  source={
                    request.original
                      ? evidence.find(
                          (e) =>
                            e.id === request.original!.evidence_id &&
                            e.sha256 === request.original!.sha256 &&
                            e.bytes === request.original!.bytes,
                        )
                      : undefined
                  }
                  onSource={setSource}
                />
              ))}
              {result.limitations.length > 0 && (
                <section aria-label="Collection limitations">
                  <h3>Limitations</h3>
                  <ul>
                    {result.limitations.map((s, i) => (
                      <li key={i}>{s}</li>
                    ))}
                  </ul>
                </section>
              )}
            </>
          )}
        </div>
      </Dialog>
      {source && (
        <Dialog
          label="Retained collection source"
          wide
          onClose={() => setSource(null)}
        >
          <h2>Retained collection source</h2>
          <button className="button" onClick={() => setSource(null)}>
            Close retained source
          </button>
          <SourceContent evidence={source} />
        </Dialog>
      )}
    </>
  );
}
const stopLabels: Record<string, string> = {
  cancelled: "Cancellation observed",
  deadline: "Time budget exhausted",
  clock_changed: "Clock changed",
  timeout: "Request timed out",
  network: "Network failure",
  policy: "Blocked by policy",
  body_limit: "Body limit reached",
  resolver_unavailable: "Address resolution unavailable",
  busy: "Processing capacity unavailable",
  quiescence_unverified: "Completion unverified",
  recovery_required: "Recovery required",
};
function RequestRow({
  request,
  source,
  onSource,
}: {
  request: CollectionRequest;
  source: Evidence | undefined;
  onSource: (e: Evidence) => void;
}) {
  const progress = request.progress;
  const receipt = progress.state === "observed" ? progress.receipt : null;
  const fetch = progress.state === "settled" ? progress.result : null;
  const head =
    receipt?.outcome.head ?? (fetch && fetch.kind !== "failed" ? fetch : null);
  const outcome =
    progress.state === "reserved"
      ? "Awaiting a recorded outcome"
      : progress.state === "interrupted_unknown"
        ? "Interrupted · outcome unknown"
        : receipt
          ? receipt.outcome.kind === "complete"
            ? "Complete body"
            : (stopLabels[receipt.outcome.reason] ?? "Stopped")
          : fetch?.kind === "complete"
            ? "Complete body"
            : fetch?.kind === "incomplete"
              ? "Incomplete body"
              : `Failed: ${fetch?.kind === "failed" ? fetch.reason : ""}`;
  return (
    <article className="durable-request">
      <header>
        <strong>
          {String(request.sequence + 1).padStart(2, "0")} /{" "}
          {request.entry.purpose} / {outcome}
        </strong>
      </header>
      <div>
        <h4>{request.entry.url}</h4>
        <p>
          Reserved {date(request.reserved_at_ms)} · hop {request.entry.hop} ·{" "}
          {request.entry.redirects} redirects
          {request.entry.parent !== null
            ? ` · follows request ${request.entry.parent + 1}`
            : ""}
        </p>
        {head && (
          <p>
            HTTP {head.status} · {head.media_type ?? "Media type unavailable"}
          </p>
        )}
        {progress.state === "interrupted_unknown" && (
          <p className="alert">
            The request may have reached the website. Its charge remains spent;
            no response is invented. Recovered {date(progress.recovered_at_ms)}.
          </p>
        )}
        {receipt && <ReceiptFacts receipt={receipt} />}
        {request.original ? (
          <>
            <dl>
              <dt>Original SHA-256</dt>
              <dd>
                <code>{request.original.sha256}</code>
              </dd>
              <dt>Retained bytes</dt>
              <dd>{request.original.bytes}</dd>
            </dl>
            <button
              className="button"
              disabled={!source}
              onClick={() => source && onSource(source)}
            >
              Inspect retained source
            </button>
            {!source && (
              <p>
                Refresh the workspace to locate this exact retained original.
              </p>
            )}
          </>
        ) : (
          <p>No complete response original was retained for this attempt.</p>
        )}
      </div>
    </article>
  );
}
function ReceiptFacts({ receipt }: { receipt: Receipt }) {
  return (
    <>
      <p>
        Observed {date(receipt.observed_wall_ms)} · elapsed{" "}
        {receipt.elapsed_milliseconds} milliseconds
      </p>
      <p>
        {receipt.http_delivery === "definitively_before_http"
          ? "Stopped before HTTP delivery."
          : "The request may have reached the website."}{" "}
        {receipt.locally_quiescent
          ? "Local processing completed."
          : "Processing completion is unverified."}
      </p>
      {receipt.stop_observed && (
        <p className="alert">
          {stopLabels[receipt.stop_observed]} after observation. A complete body
          does not erase this stop condition.
        </p>
      )}
      {receipt.resolved && (
        <details>
          <summary>Observed destination addresses</summary>
          <ul>
            {receipt.resolved.addresses.map((a) => (
              <li key={a}>
                <code>{a}</code>
              </li>
            ))}
          </ul>
          <p>
            These observations are not an authoritative complete address set.
          </p>
        </details>
      )}
      {receipt.resolver_uncertainty && (
        <p className="alert">
          Address resolution completion was not verified. New work requires
          recovery.
        </p>
      )}
    </>
  );
}
