import { useState } from "react";
import { command } from "./api";
import { Dialog } from "./Dialog";
import { FindingReviewHistory } from "./FindingReviewHistory";
import type { Anchor, Evidence, Finding, Hypothesis, Workspace } from "./types";

type Props = {
  workspace: Workspace;
  busy: boolean;
  error: string;
  run: (action: Record<string, unknown>) => Promise<boolean>;
  onSource: (evidence: Evidence, anchor?: Anchor) => void;
  download: (content: string, name: string, type: string) => Promise<void>;
};
type Citation = {
  id: string;
  label: string;
  detail: string;
  source?: Evidence;
  anchor?: Anchor;
};
function citations(w: Workspace): Citation[] {
  return [
    ...w.observations.map((o) => ({
      id: o.id,
      label: `${w.entities.find((e) => e.id === o.entity_id)?.name ?? o.entity_id} · ${o.field}: ${o.value}`,
      detail: `Observation · ${o.review}`,
      source: w.evidence.find((e) => e.id === o.anchor.evidence_id),
      anchor: o.anchor,
    })),
    ...w.transactions.map((t) => ({
      id: t.id,
      label: `${t.description} · ${t.amount} ${t.currency}`,
      detail: `Transaction · ${t.date} · account ${t.account} · ${t.review}`,
      source: w.evidence.find((e) => e.id === t.anchor.evidence_id),
      anchor: t.anchor,
    })),
    ...w.evidence.map((e) => ({
      id: e.id,
      label: e.name,
      detail: `Whole source · ${e.extraction_status}`,
      source: e,
    })),
  ];
}
function CitationRow({
  item,
  onSource,
  children,
}: {
  item: Citation;
  onSource: Props["onSource"];
  children?: React.ReactNode;
}) {
  return (
    <div className="citation-row">
      <div>
        <strong>{item.label}</strong>
        <p>
          {item.detail}
          {item.anchor
            ? ` · ${item.source?.name ?? "Missing source"} · ${item.anchor.kind === "text" ? `lines ${item.anchor.line_start}–${item.anchor.line_end}` : `row ${item.anchor.row ?? "?"}, ${item.anchor.column ?? item.anchor.kind}`}`
            : ""}
        </p>
      </div>
      <div className="citation-controls">
        {item.source && (
          <button
            type="button"
            className="button"
            onClick={() => onSource(item.source!, item.anchor)}
          >
            Inspect source
          </button>
        )}
        {children}
      </div>
    </div>
  );
}
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

function ReportExport({ report, download }: {
  report: Workspace["reports"][number];
  download: Props["download"];
}) {
  const [loading, setLoading] = useState(false);
  const [error, setError] = useState("");
  async function exportReport() {
    setLoading(true);
    setError("");
    try {
      const snapshot = await command<{ id: string; sha256: string; html: string }>({
        action: "inspect_report_snapshot", report_id: report.id,
        expected_sha256: report.sha256,
      });
      await download(snapshot.html, `assessment-${snapshot.id}.html`, "text/html");
    } catch (error) {
      setError(error instanceof Error ? error.message : String(error));
    } finally {
      setLoading(false);
    }
  }
  return <>
    <button className="button" disabled={loading} onClick={exportReport}>
      {loading ? "Loading report…" : "Export self-contained HTML"}
    </button>
    <ErrorMessage error={error} />
  </>;
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
  const [query, setQuery] = useState("");
  const [reason, setReason] = useState("");
  const [submitted, setSubmitted] = useState(false);
  const all = citations(workspace);
  const choose = (id: string, role: string) => {
    setSupporting((old) => [
      ...old.filter((x) => x !== id),
      ...(role === "supporting" ? [id] : []),
    ]);
    setContradicting((old) => [
      ...old.filter((x) => x !== id),
      ...(role === "contradicting" ? [id] : []),
    ]);
  };
  const selected = all.filter(
    (c) => supporting.includes(c.id) || contradicting.includes(c.id),
  );
  const matches = all.filter(
    (c) =>
      !selected.includes(c) &&
      `${c.label} ${c.detail} ${c.source?.name ?? ""}`
        .toLowerCase()
        .includes(query.toLowerCase()),
  );
  const row = (c: Citation) => (
    <CitationRow key={c.id} item={c} onSource={onSource}>
      <label className="citation-role">
        Citation role
        <select
          aria-label={`Citation role for ${c.label}`}
          value={
            supporting.includes(c.id)
              ? "supporting"
              : contradicting.includes(c.id)
                ? "contradicting"
                : "none"
          }
          onChange={(e) => choose(c.id, e.target.value)}
        >
          <option value="none">Not cited</option>
          <option value="supporting">Supporting</option>
          <option value="contradicting">Contradictory</option>
        </select>
      </label>
    </CitationRow>
  );
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
            <h3>Selected citations · {selected.length}</h3>
            <div role="region" aria-label="Selected citations">
              {selected.map(row)}
              {!selected.length && (
                <p className="muted">
                  Choose at least one supporting or contradictory record.
                </p>
              )}
            </div>
            <label>
              Find evidence to cite
              <input
                type="search"
                value={query}
                onChange={(e) => setQuery(e.target.value)}
              />
            </label>
            <div
              className="citation-picker"
              role="region"
              aria-label="Available citations"
            >
              {matches.slice(0, 50).map(row)}
              {!matches.length && <p>No matching uncited records.</p>}
            </div>
            <p className="muted">
              Showing {Math.min(50, matches.length)} of {matches.length}{" "}
              matching uncited records. Whole-source citations do not accept
              extracted records or establish source independence.
            </p>
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
              <button type="submit" className="button primary">
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
  const all = citations(workspace);
  const find = (id: string) =>
    all.find((c) => c.id === id) ?? {
      id,
      label: `Unresolved citation ${id}`,
      detail: "Review required",
    };
  const groups = new Set(
    [...item.supporting_ids, ...item.contradicting_ids]
      .map((id) => find(id).source?.origin_group)
      .filter(Boolean),
  );
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
        {(
          [
            ["Supporting evidence", item.supporting_ids],
            ["Contradictory evidence", item.contradicting_ids],
          ] as const
        ).map(([label, ids]) => (
          <section aria-label={label} key={label}>
            <h4>{label}</h4>
            {ids.map((id) => (
              <CitationRow key={id} item={find(id)} onSource={onSource} />
            ))}
            {!ids.length && <p className="muted">None cited.</p>}
          </section>
        ))}
        <p className="muted">
          {groups.size} source-origin groups among these citations. Shared
          origin is visible; independence requires review.
        </p>
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
                <button type="submit" className="button primary">
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
          Previous snapshots remain unchanged. Editable DOCX export is a
          remaining release gate.
        </p>
      </section>
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
