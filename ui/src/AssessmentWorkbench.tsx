import { useEffect, useRef, useState } from "react";
import { command } from "./api";
import { nativeExportsAvailable, prepareNativeReport, commitNativeExport, discardNativeExport, type PreparedNativeExport } from "./native-export";
import { Dialog } from "./Dialog";
import { FindingReviewHistory } from "./FindingReviewHistory";
import { DocxSnapshots } from "./DocxSnapshots";
import type { DocxCapture } from "./docx-capture";
import { CitationRow } from "./CitationRow";
import {
  CitationPicker,
  SelectionStatus,
  useCitationSelections,
} from "./CitationPicker";
import type { CitationRole } from "./citation-types";
import type { Anchor, Evidence, Finding, Hypothesis, Workspace } from "./types";

type Props = {
  workspace: Pick<Workspace, "revision" | "findings" | "hypotheses" | "evidence" | "reports">;
  busy: boolean;
  error: string;
  run: (action: Record<string, unknown>) => Promise<boolean>;
  onSource: (evidence: Evidence, anchor?: Anchor) => void;
  download: (content: string, name: string, type: string) => Promise<void>;
  docxCapture: DocxCapture;
};
function ErrorMessage({ error }: { error: string }) {
  return error ? (
    <p className="alert error" role="alert">
      {error}
    </p>
  ) : null;
}
const lines = (value: string) =>
  value
    .split("\n")
    .map((v) => v.trim())
    .filter(Boolean);

function ReportExport({
  report,
  download,
}: {
  report: Workspace["reports"][number];
  download: Props["download"];
}) {
  const [loading, setLoading] = useState(false);
  const [error, setError] = useState("");
  const lifetime = useRef(true);
  useEffect(() => {
    lifetime.current = true;
    return () => {
      lifetime.current = false;
    };
  }, []);
  const active = useRef(false);
  const [notice, setNotice] = useState("");
  async function exportReport() {
    if (active.current) return;
    active.current = true;
    let prepared: PreparedNativeExport | undefined;
    let committed = false;
    setLoading(true);
    setError("");
    setNotice("");
    try {
      if (nativeExportsAvailable()) {
        prepared = await prepareNativeReport(report);
        if (!lifetime.current)
          throw new Error("Report view closed while preparing export.");
        const saved = await commitNativeExport(prepared);
        committed = true;
        if (lifetime.current)
          setNotice(`Saved immutable report: ${saved.location}`);
        return;
      }
      const snapshot = await command<{
        id: string;
        sha256: string;
        html: string;
      }>({
        action: "inspect_report_snapshot",
        report_id: report.id,
        expected_sha256: report.sha256,
      });
      await download(
        snapshot.html,
        `assessment-${snapshot.id}.html`,
        "text/html",
      );
    } catch (error) {
      if (lifetime.current)
        setError(error instanceof Error ? error.message : String(error));
    } finally {
      if (prepared && !committed) {
        try {
          await discardNativeExport(prepared.ticket);
        } catch (cause) {
          if (lifetime.current)
            setError(
              (old) =>
                `${old} Staging cleanup was not confirmed: ${String(cause)}`,
            );
        }
      }
      active.current = false;
      if (lifetime.current) setLoading(false);
    }
  }
  return (
    <>
      <button className="button" disabled={loading} onClick={exportReport}>
        {loading ? "Loading report…" : "Export self-contained HTML"}
      </button>
      <ErrorMessage error={error} />
      {notice && (
        <p className="alert" role="status">
          {notice}
        </p>
      )}
    </>
  );
}

function QuestionEditor({
  item,
  workspace,
  busy,
  error,
  run,
  close,
}: Props & { item?: Hypothesis; close: () => void }) {
  const [revision] = useState(workspace.revision);
  const [question, setQuestion] = useState(item?.question ?? "");
  const [proposition, setProposition] = useState(item?.proposition ?? "");
  const [alternatives, setAlternatives] = useState(
    item?.alternatives.join("\n") ?? "",
  );
  const [gaps, setGaps] = useState(item?.gaps.join("\n") ?? "");
  const [reason, setReason] = useState("");
  const [submitted, setSubmitted] = useState(false);
  return (
    <Dialog
      label={item ? "Edit question" : "Add question"}
      wide
      preventClose={busy}
      onClose={close}
    >
      <button
        className="close"
        aria-label="Close question editor"
        disabled={busy}
        onClick={close}
      >
        ×
      </button>
      <div className="assessment-workbench">
        <p className="eyebrow">Assessment / question register</p>
        <h2>
          {item ? "Edit investigation question" : "Add investigation question"}
        </h2>
        <form
          onSubmit={async (e) => {
            e.preventDefault();
            setSubmitted(true);
            if (
              await run({
                action: item ? "update_question" : "add_question",
                ...(item ? { id: item.id } : {}),
                question: {
                  question,
                  proposition,
                  alternatives: lines(alternatives),
                  gaps: lines(gaps),
                },
                reason,
                expected_revision: revision,
              })
            )
              close();
          }}
        >
          <fieldset disabled={busy} className="plain-fieldset">
            <label>
              Investigation question
              <input
                required
                value={question}
                onChange={(e) => setQuestion(e.target.value)}
              />
            </label>
            <label>
              Working hypothesis
              <textarea
                aria-label="Working hypothesis"
                required
                value={proposition}
                onChange={(e) => setProposition(e.target.value)}
              />
            </label>
            <div className="form-grid">
              <label>
                Alternative explanations · one per line
                <textarea
                  aria-label="Alternative explanations · one per line"
                  value={alternatives}
                  onChange={(e) => setAlternatives(e.target.value)}
                />
              </label>
              <label>
                Collection gaps · one per line
                <textarea
                  aria-label="Collection gaps · one per line"
                  value={gaps}
                  onChange={(e) => setGaps(e.target.value)}
                />
              </label>
            </div>
            <label>
              Question change reason
              <textarea
                aria-label="Question change reason"
                required
                value={reason}
                onChange={(e) => setReason(e.target.value)}
              />
            </label>
            {submitted && <ErrorMessage error={error} />}
            <button className="button primary" type="submit">
              Save question
            </button>
          </fieldset>
        </form>
      </div>
    </Dialog>
  );
}
function FindingEditor({
  item,
  workspace,
  busy,
  error,
  run,
  onSource,
  close,
}: Props & { item?: Finding; close: () => void }) {
  const [revision] = useState(workspace.revision);
  const [title, setTitle] = useState(item?.title ?? "");
  const [assessment, setAssessment] = useState(item?.assessment ?? "");
  const [limitations, setLimitations] = useState(item?.limitations ?? "");
  const [supporting, setSupporting] = useState(item?.supporting_ids ?? []);
  const [contradicting, setContradicting] = useState(
    item?.contradicting_ids ?? [],
  );
  const [questions, setQuestions] = useState(item?.hypothesis_ids ?? []);
  const [reason, setReason] = useState("");
  const [submitted, setSubmitted] = useState(false);
  const stale = revision !== workspace.revision;
  const selected = useCitationSelections(
    [...supporting, ...contradicting],
    revision,
    stale,
  );
  const [refreshing, setRefreshing] = useState(false);
  const [refreshError, setRefreshError] = useState("");
  const refreshButton = useRef<HTMLButtonElement>(null);
  const refreshFocus = useRef<{
    opener: HTMLButtonElement;
    moved: boolean;
  } | null>(null);
  useEffect(() => {
    const rememberFocusMove = (event: FocusEvent) => {
      const pending = refreshFocus.current;
      if (
        pending &&
        event.target !== pending.opener &&
        event.target !== document.body &&
        event.target !== document.documentElement
      )
        pending.moved = true;
    };
    document.addEventListener("focusin", rememberFocusMove);
    return () => {
      document.removeEventListener("focusin", rememberFocusMove);
      refreshFocus.current = null;
    };
  }, []);
  useEffect(() => {
    const pending = refreshFocus.current;
    if (busy || refreshing || !pending) return;
    // Both React state transitions must have re-enabled the fieldset/button.
    const frame = requestAnimationFrame(() => {
      if (refreshFocus.current !== pending) return;
      refreshFocus.current = null;
      const active = document.activeElement;
      if (
        !pending.moved &&
        pending.opener.isConnected &&
        (active === pending.opener ||
          active === document.body ||
          active === document.documentElement)
      )
        pending.opener.focus({ preventScroll: true });
    });
    return () => cancelAnimationFrame(frame);
  }, [busy, refreshing]);
  const choose = (id: string, role: CitationRole) => {
    setSupporting((old) => [
      ...old.filter((x) => x !== id),
      ...(role === "supporting" ? [id] : []),
    ]);
    setContradicting((old) => [
      ...old.filter((x) => x !== id),
      ...(role === "contradicting" ? [id] : []),
    ]);
  };
  return (
    <Dialog
      label={item ? "Edit finding" : "Add finding"}
      restoreFocus={
        item
          ? () =>
              document
                .getElementById(`finding-${item.id}`)
                ?.focus({ preventScroll: true })
          : undefined
      }
      wide
      preventClose={busy}
      onClose={close}
    >
      <button
        className="close"
        aria-label="Close finding editor"
        disabled={busy}
        onClick={close}
      >
        ×
      </button>
      <div className="assessment-workbench">
        <p className="eyebrow">Assessment / finding editor</p>
        <h2>{item ? "Edit cited finding" : "Add cited finding"}</h2>
        <form
          onSubmit={async (e) => {
            e.preventDefault();
            if (stale || !selected.ready) return;
            setSubmitted(true);
            const finding = {
              title,
              assessment,
              limitations,
              supporting_ids: supporting,
              contradicting_ids: contradicting,
              hypothesis_ids: questions,
            };
            if (
              await run({
                action: item ? "update_finding" : "add_finding",
                ...(item ? { id: item.id, finding, reason } : finding),
                expected_revision: revision,
              })
            )
              close();
          }}
        >
          <fieldset disabled={busy} className="plain-fieldset">
            <label>
              Finding title
              <input
                required
                value={title}
                onChange={(e) => setTitle(e.target.value)}
              />
            </label>
            <label>
              Assessment
              <textarea
                aria-label="Assessment"
                required
                value={assessment}
                onChange={(e) => setAssessment(e.target.value)}
              />
            </label>
            <label>
              Limitations and outstanding enquiries
              <textarea
                aria-label="Limitations and outstanding enquiries"
                required
                value={limitations}
                onChange={(e) => setLimitations(e.target.value)}
              />
            </label>
            <fieldset>
              <legend>Linked questions</legend>
              {workspace.hypotheses.map((h) => (
                <label className="check-label" key={h.id}>
                  <input
                    type="checkbox"
                    checked={questions.includes(h.id)}
                    onChange={(e) =>
                      setQuestions((old) =>
                        e.target.checked
                          ? [...old, h.id]
                          : old.filter((id) => id !== h.id),
                      )
                    }
                  />
                  {h.question}
                </label>
              ))}
              {!workspace.hypotheses.length && (
                <p>
                  Add an investigation question in the question register to link
                  it here.
                </p>
              )}
            </fieldset>
            <CitationPicker
              supporting={supporting}
              contradicting={contradicting}
              selection={selected}
              revision={revision}
              stale={stale}
              busy={busy || refreshing}
              evidence={workspace.evidence}
              choose={choose}
              onSource={onSource}
            />
            {stale && (
              <p className="alert" role="status">
                The workspace changed while this editor was open. Draft text,
                selected IDs and roles are preserved. Close and reopen before
                saving; this draft has not adopted a newer revision.
              </p>
            )}
            <button
              ref={refreshButton}
              type="button"
              className="button"
              disabled={busy || refreshing}
              onClick={async () => {
                if (refreshButton.current)
                  refreshFocus.current = {
                    opener: refreshButton.current,
                    moved: false,
                  };
                setRefreshing(true);
                setRefreshError("");
                try {
                  if (!(await run({ action: "view" })))
                    setRefreshError(
                      "Workspace refresh failed. Draft revision is unchanged.",
                    );
                } catch (cause) {
                  setRefreshError(String(cause));
                } finally {
                  setRefreshing(false);
                }
              }}
            >
              {refreshing ? "Refreshing workspace…" : "Refresh workspace"}
            </button>
            <ErrorMessage error={refreshError} />
            {item && (
              <label>
                Finding change reason
                <textarea
                  aria-label="Finding change reason"
                  required
                  value={reason}
                  onChange={(e) => setReason(e.target.value)}
                />
              </label>
            )}
            {submitted && <ErrorMessage error={error} />}
            <div className="actions">
              <button
                type="submit"
                className="button primary"
                disabled={stale || refreshing || !selected.ready}
              >
                Save finding
              </button>
              <span className="muted">
                Saved findings require an explicit review.
              </span>
            </div>
          </fieldset>
        </form>
      </div>
    </Dialog>
  );
}
function FindingReview({
  item,
  workspace,
  busy,
  error,
  run,
  onSource,
  close,
  edit,
}: Props & { item: Finding; close: () => void; edit: () => void }) {
  const [revision] = useState(workspace.revision);
  const [reason, setReason] = useState("");
  const [submitted, setSubmitted] = useState(false);
  const stale = revision !== workspace.revision;
  const selected = useCitationSelections(
    [...item.supporting_ids, ...item.contradicting_ids],
    revision,
    stale,
  );
  const find = (id: string) => selected.rows.find((row) => row.id === id);
  const groups = new Set(
    selected.rows.map((row) => row.source.origin_group).filter(Boolean),
  );
  const unknownGroups = new Set(
    selected.rows
      .filter((row) => !row.source.origin_group)
      .map((row) => row.source.id),
  ).size;
  return (
    <Dialog label="Finding review" wide preventClose={busy} onClose={close}>
      <button
        className="close"
        aria-label="Close finding review"
        disabled={busy}
        onClick={close}
      >
        ×
      </button>
      <div className="assessment-workbench">
        <p className="eyebrow">Assessment / finding review</p>
        <h2>{item.title}</h2>
        <span className={`pill ${item.needs_review ? "warning" : "accepted"}`}>
          {item.needs_review ? "Review required" : "Reviewed"}
        </span>
        <p className="preserve-lines">{item.assessment}</p>
        <h4>Limitations</h4>
        <p className="preserve-lines">{item.limitations}</p>
        <h4>Linked questions</h4>
        {item.hypothesis_ids.map((id) => (
          <p key={id}>
            {workspace.hypotheses.find((h) => h.id === id)?.question ??
              "Unresolved question"}
          </p>
        ))}
        {!item.hypothesis_ids.length && (
          <p className="muted">No questions linked.</p>
        )}
        <SelectionStatus state={selected} stale={stale} />
        {(
          [
            ["Supporting evidence", item.supporting_ids],
            ["Contradictory evidence", item.contradicting_ids],
          ] as const
        ).map(([label, ids]) => (
          <section aria-label={label} key={label}>
            <h4>{label}</h4>
            {ids.map((id) => {
              const citation = find(id);
              return citation ? (
                <CitationRow
                  key={id}
                  item={citation}
                  evidence={workspace.evidence}
                  onSource={onSource}
                  disabled={busy || stale}
                />
              ) : (
                <p key={id}>
                  Citation {id} · details unavailable; role retained.
                </p>
              );
            })}
            {!ids.length && <p className="muted">None cited.</p>}
          </section>
        ))}
        {selected.ready && (
          <p className="muted">
            {groups.size} source-origin groups among these citations. Shared
            origin is visible; independence requires review.
            {unknownGroups > 0 &&
              ` ${unknownGroups} cited sources have no recorded origin group; this is not a complete group count.`}
          </p>
        )}
        {stale && (
          <p className="alert" role="status">
            The workspace changed while this review was open. The draft reason
            is preserved. Close and reopen the finding before recording a
            decision.
          </p>
        )}
        {item.needs_review && (
          <form
            onSubmit={async (e) => {
              e.preventDefault();
              if (stale || !selected.ready) return;
              setSubmitted(true);
              if (
                await run({
                  action: "review_finding",
                  id: item.id,
                  reason,
                  expected_revision: revision,
                })
              )
                close();
            }}
          >
            <fieldset disabled={busy || stale} className="plain-fieldset">
              <label>
                Finding review reason
                <textarea
                  aria-label="Finding review reason"
                  required
                  value={reason}
                  onChange={(e) => setReason(e.target.value)}
                />
              </label>
              {submitted && <ErrorMessage error={error} />}
              <div className="inline-actions">
                <button
                  type="submit"
                  className="button primary"
                  disabled={!selected.ready}
                >
                  Mark finding reviewed
                </button>
                <button type="button" className="button" onClick={edit}>
                  Edit finding
                </button>
              </div>
            </fieldset>
          </form>
        )}
        {!item.needs_review && (
          <div className="actions">
            <button className="button" disabled={busy} onClick={edit}>
              Edit finding
            </button>
          </div>
        )}
        <p className="context-note">
          Review records an analyst decision. It does not establish identity or
          source independence. Cited observations and transactions must be
          accepted in their review screens first. Edits and workspace evidence
          changes reopen review; earlier report snapshots remain unchanged.
        </p>
        <FindingReviewHistory
          key={item.id}
          findingId={item.id}
          revision={workspace.revision}
          busy={busy}
          onRefresh={() => run({ action: "view" })}
        />
      </div>
    </Dialog>
  );
}
export function AssessmentWorkbench(props: Props) {
  const { workspace: w, busy, run, download } = props;
  const [question, setQuestion] = useState<Hypothesis | "new" | null>(null);
  const [finding, setFinding] = useState<Finding | "new" | null>(null);
  const [review, setReview] = useState<string | null>(null);
  const reviewed = w.findings.find((f) => f.id === review);
  return (
    <div className="assessment-workbench">
      <section className="panel">
        <div className="panel-heading">
          <h2>Investigation questions</h2>
          <button
            className="button"
            disabled={busy}
            onClick={() => setQuestion("new")}
          >
            Add question
          </button>
        </div>
        {w.hypotheses.map((h) => (
          <article className="list-card" key={h.id}>
            <h3>{h.question}</h3>
            <p className="preserve-lines">{h.proposition}</p>
            <div className="form-grid">
              <div>
                <h4>Alternatives</h4>
                {h.alternatives.map((a, i) => (
                  <p key={i}>{a}</p>
                ))}
                {!h.alternatives.length && <p>None recorded.</p>}
              </div>
              <div>
                <h4>Collection gaps</h4>
                {h.gaps.map((g, i) => (
                  <p key={i}>{g}</p>
                ))}
                {!h.gaps.length && <p>None recorded.</p>}
              </div>
            </div>
            <button
              className="button"
              disabled={busy}
              onClick={() => setQuestion(h)}
            >
              Edit question
            </button>
          </article>
        ))}
        {!w.hypotheses.length && (
          <p>
            Record the questions, possible explanations and gaps that guide
            collection.
          </p>
        )}
      </section>
      <section className="panel">
        <div className="panel-heading">
          <h2>Findings</h2>
          <div className="inline-actions">
            <button
              className="button"
              disabled={busy}
              onClick={() => setFinding("new")}
            >
              Add finding
            </button>
            <button
              className="button primary"
              disabled={busy}
              onClick={() => void run({ action: "save_report" })}
            >
              Save report snapshot
            </button>
          </div>
        </div>
        {w.findings.map((f) => (
          <article className="finding" key={f.id}>
            <span className={`pill ${f.needs_review ? "warning" : "accepted"}`}>
              {f.needs_review ? "Review required" : "Reviewed"}
            </span>
            <h3>{f.title}</h3>
            <p className="preserve-lines">{f.assessment}</p>
            <p className="muted">
              {f.supporting_ids.length} supporting ·{" "}
              {f.contradicting_ids.length} contradictory ·{" "}
              {f.hypothesis_ids.length} linked questions
            </p>
            <button
              className="button"
              disabled={busy}
              id={`finding-${f.id}`}
              onClick={() => setReview(f.id)}
            >
              Review finding
            </button>
          </article>
        ))}
        {!w.findings.length && (
          <p>
            No findings recorded. Cite source records and retain any
            contradictory evidence.
          </p>
        )}
      </section>
      <section className="panel">
        <div className="panel-heading">
          <h2>Immutable report snapshots</h2>
          <span className="count">{w.reports.length}</span>
        </div>
        {w.reports.map((r) => (
          <article className="list-card" key={r.id}>
            <span className="pill">REV {r.workspace_revision}</span>
            <h3>{new Date(r.created_at).toLocaleString()}</h3>
            <p>
              <code>{r.sha256}</code>
            </p>
            <ReportExport report={r} download={download} />
          </article>
        ))}
        <p className="context-note">
          Snapshots include current review status, citations and limitations,
          including drafts. Corrections flag current findings for review.
          Previous HTML snapshots remain unchanged. Editable DOCX captures are
          recorded separately below.
        </p>
      </section>
      <DocxSnapshots revision={w.revision} busy={busy} capture={props.docxCapture} refresh={() => run({ action: "view" })} />
      {question && (
        <QuestionEditor
          {...props}
          item={question === "new" ? undefined : question}
          close={() => setQuestion(null)}
        />
      )}
      {finding && (
        <FindingEditor
          {...props}
          item={finding === "new" ? undefined : finding}
          close={() => setFinding(null)}
        />
      )}
      {reviewed && (
        <FindingReview
          key={reviewed.id}
          {...props}
          item={reviewed}
          close={() => setReview(null)}
          edit={() => {
            setReview(null);
            setFinding(reviewed);
          }}
        />
      )}
    </div>
  );
}
