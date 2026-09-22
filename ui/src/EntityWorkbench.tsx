import { useEffect, useState } from "react";
import { command } from "./api";
import { Dialog } from "./Dialog";
import type {
  Anchor,
  Entity,
  EntityInput,
  EntityKind,
  Evidence,
  IdentityComparison,
  Observation,
  Workspace,
} from "./types";

const kinds: EntityKind[] = [
  "person",
  "organisation",
  "group",
  "account",
  "place",
  "digital_identifier",
];
const signals = {
  insufficient_reviewed_evidence: "Insufficient reviewed evidence",
  shared_reviewed_values: "Shared reviewed values",
  different_reviewed_values: "Different reviewed values",
  mixed_reviewed_values: "Shared and different reviewed values",
};
export const entityLabel = (entity: Entity) =>
  `${entity.name} · ${entity.identifiers.map((i) => `${i.namespace}:${i.value}`).join(", ") || entity.id.slice(0, 8)}`;
type Apply = (action: Record<string, unknown>) => Promise<boolean>;
type OpenSource = (e: Evidence, anchor?: Anchor) => void;

export function EntityWorkbench({
  workspace: w,
  busy,
  run,
  selectedId,
  onSource,
  error,
}: {
  workspace: Workspace;
  busy: boolean;
  run: Apply;
  selectedId: string;
  onSource: OpenSource;
  error: string;
}) {
  const [filter, setFilter] = useState("");
  const [editor, setEditor] = useState<Entity | "new" | null>(null);
  const [addFor, setAddFor] = useState<Entity | null>(null);
  const [review, setReview] = useState<Observation | null>(null);
  const [left, setLeft] = useState(selectedId || w.entities[0]?.id || "");
  const [right, setRight] = useState(
    w.entities.find((e) => e.id !== (selectedId || w.entities[0]?.id))?.id ||
      "",
  );
  const [why, setWhy] = useState("");
  const [comparison, setComparison] = useState<IdentityComparison | null>(null);
  const [comparisonError, setComparisonError] = useState("");
  const [comparing, setComparing] = useState(false);
  useEffect(() => {
    setEditor(null);
    setAddFor(null);
    setReview(null);
  }, [w.revision]);
  const apply: Apply = (action) =>
    run({ ...action, expected_revision: w.revision });
  useEffect(() => {
    if (selectedId) setLeft(selectedId);
  }, [selectedId]);
  useEffect(() => {
    let current = true;
    setComparison(null);
    setComparisonError("");
    if (!left || !right || left === right) {
      setComparing(false);
      return;
    }
    setComparing(true);
    command<IdentityComparison>({
      action: "compare_entities",
      left_id: left,
      right_id: right,
    })
      .then((result) => {
        if (current) {
          if (result.workspace_revision === w.revision) setComparison(result);
          else
            setComparisonError("Workspace changed. Reload it before deciding.");
        }
      })
      .catch((error) => {
        if (current) setComparisonError(String(error));
      })
      .finally(() => {
        if (current) setComparing(false);
      });
    return () => {
      current = false;
    };
  }, [left, right, w.revision]);
  const inspect = (o: Observation) => {
    const e = w.evidence.find((e) => e.id === o.anchor.evidence_id);
    if (e) onSource(e, o.anchor);
  };
  const label = (id: string) => {
    const e = w.entities.find((e) => e.id === id);
    return e ? entityLabel(e) : "Unavailable entity";
  };
  const recordDecision = async (action: Record<string, unknown>) => {
    const submittedReason = why;
    if (await apply({ ...action, reason: submittedReason })) {
      setWhy((current) => (current === submittedReason ? "" : current));
    }
  };
  const activePair =
    !!comparison &&
    comparison.workspace_revision === w.revision &&
    !comparison.left.merged_into &&
    !comparison.right.merged_into;
  return (
    <>
      <div className="toolbar">
        <input
          aria-label="Filter entities"
          placeholder="Name or reference number…"
          value={filter}
          onChange={(e) => setFilter(e.target.value)}
        />
        <button
          className="button primary"
          disabled={busy}
          onClick={() => setEditor("new")}
        >
          Add entity
        </button>
      </div>
      {editor && (
        <section className="panel">
          <h2>{editor === "new" ? "Create entity" : "Edit entity"}</h2>
          <EntityForm
            key={editor === "new" ? "new" : editor.id}
            initial={editor === "new" ? undefined : editor}
            busy={busy}
            onCancel={() => setEditor(null)}
            onSave={async (entity, reason) => {
              if (
                await apply(
                  editor === "new"
                    ? { action: "add_entity", entity, reason }
                    : {
                        action: "update_entity",
                        id: editor.id,
                        entity,
                        reason,
                      },
                )
              )
                setEditor(null);
            }}
          />
        </section>
      )}
      <div className="entity-grid">
        {w.entities
          .filter((e) =>
            `${e.name} ${e.identifiers.map((i) => `${i.namespace}:${i.value}`).join(" ")}`
              .toLowerCase()
              .includes(filter.toLowerCase()),
          )
          .map((entity) => (
            <section
              className={`panel entity-card ${selectedId === entity.id ? "selected" : ""}`}
              key={entity.id}
              aria-label={entityLabel(entity)}
            >
              <span className="pill">{entity.kind.replaceAll("_", " ")}</span>
              <h2>{entity.name}</h2>
              {entity.identifiers.map((i) => (
                <p key={`${i.namespace}\0${i.value}`}>
                  <span className="muted">
                    Reference Number · {i.namespace}
                  </span>
                  <br />
                  <code>{i.value}</code>
                </p>
              ))}
              {entity.merged_into && (
                <p className="alert">
                  Merged into {label(entity.merged_into)}; original observations
                  retained.
                </p>
              )}
              <div className="actions">
                <button
                  className="button"
                  disabled={busy || !!entity.merged_into}
                  onClick={() => setEditor(entity)}
                >
                  Edit record
                </button>
                <button
                  className="button"
                  disabled={busy}
                  onClick={() => setAddFor(entity)}
                >
                  Add observation
                </button>
                <button
                  className="text-button"
                  onClick={() => setLeft(entity.id)}
                >
                  Compare this record
                </button>
              </div>
              {w.observations
                .filter((o) => o.entity_id === entity.id)
                .map((o) => (
                  <article className="observation" key={o.id}>
                    <strong>{o.field.replaceAll("_", " ")}</strong>
                    <p>{o.value}</p>
                    <span className={`pill ${o.review}`}>{o.review}</span>
                    <div className="actions">
                      <button
                        className="text-button"
                        onClick={() => inspect(o)}
                      >
                        Inspect source
                      </button>
                      <button
                        className="text-button"
                        disabled={busy}
                        onClick={() => setReview(o)}
                      >
                        Review observation
                      </button>
                    </div>
                  </article>
                ))}
            </section>
          ))}
      </div>
      <section className="panel">
        <h2>Identity comparison</h2>
        <div className="form-grid">
          <label>
            First entity
            <select
              aria-label="First entity"
              value={left}
              onChange={(e) => setLeft(e.target.value)}
            >
              <option value="">Choose an entity</option>
              {w.entities.map((e) => (
                <option key={e.id} value={e.id}>
                  {entityLabel(e)}
                </option>
              ))}
            </select>
          </label>
          <label>
            Second entity
            <select
              aria-label="Second entity"
              value={right}
              onChange={(e) => setRight(e.target.value)}
            >
              <option value="">Choose an entity</option>
              {w.entities.map((e) => (
                <option key={e.id} value={e.id}>
                  {entityLabel(e)}
                </option>
              ))}
            </select>
          </label>
        </div>
        {left && right && left === right && (
          <p className="alert">Choose two different records.</p>
        )}
        {comparing && <p role="status">Comparing reviewed observations…</p>}
        {comparisonError && <p role="alert">{comparisonError}</p>}
        {comparison && (
          <>
            <p className="muted">
              Revision {comparison.workspace_revision}. Signals compare exact
              values from accepted observations on each original record. Shared
              values are not proof of identity. Source groups are not a count of
              independent sources.
            </p>
            {comparison.fields.length === 0 ? (
              <p>No observations are available for this pair.</p>
            ) : (
              comparison.fields.map((field) => (
                <article className="list-card" key={field.field}>
                  <h3>
                    {field.field.replaceAll("_", " ")} · {signals[field.signal]}
                  </h3>
                  <p className="muted">
                    {field.source_groups.length} source groups among accepted
                    observations
                  </p>
                  <div className="grid-two">
                    {[field.left, field.right].map((observations, i) => (
                      <div key={i}>
                        <h4>
                          {i === 0
                            ? entityLabel(comparison.left)
                            : entityLabel(comparison.right)}
                        </h4>
                        {observations.length ? (
                          observations.map((o) => (
                            <p key={o.id}>
                              {o.value}{" "}
                              <span className={`pill ${o.review}`}>
                                {o.review}
                              </span>{" "}
                              <button
                                className="text-button"
                                onClick={() => inspect(o)}
                              >
                                Source
                              </button>
                            </p>
                          ))
                        ) : (
                          <p>No observations</p>
                        )}
                      </div>
                    ))}
                  </div>
                </article>
              ))
            )}
          </>
        )}
        <label>
          Identity decision reason
          <textarea
            aria-label="Identity decision reason"
            value={why}
            onChange={(e) => setWhy(e.target.value)}
          />
        </label>
        <div className="actions">
          <button
            className="button"
            disabled={busy || !why.trim() || !activePair}
            onClick={() =>
              void recordDecision({
                action: "decide_identity",
                left_id: left,
                right_id: right,
                outcome: "keep_separate",
              })
            }
          >
            Keep separate
          </button>
          <button
            className="button"
            disabled={busy || !why.trim() || !activePair}
            onClick={() =>
              void recordDecision({
                action: "decide_identity",
                left_id: left,
                right_id: right,
                outcome: "defer",
              })
            }
          >
            Defer identity decision
          </button>
          <button
            className="button"
            disabled={
              busy ||
              !why.trim() ||
              !activePair ||
              comparison?.left.kind !== comparison?.right.kind
            }
            onClick={() =>
              void recordDecision({
                action: "merge",
                source: left,
                target: right,
              })
            }
          >
            Merge selected records
          </button>
        </div>
        <p className="context-note">
          A merge associates the first record with the second. Original
          observations retain their original entity IDs. Reverse a merge to
          restore separate identities.
        </p>
        {w.merges.map((m) => (
          <article className="list-card" key={m.id}>
            <h3>
              {label(m.source)} → {label(m.target)}
            </h3>
            <p>{m.reason}</p>
            <span className="pill">
              {m.reversed ? "Merge reversed" : "Active merge"}
            </span>
            {!m.reversed && (
              <div className="actions">
                <button
                  className="button"
                  disabled={busy || !why.trim()}
                  onClick={() =>
                    void recordDecision({ action: "reverse_merge", id: m.id })
                  }
                >
                  Reverse merge
                </button>
              </div>
            )}
          </article>
        ))}
        {w.identity_decisions.length > 0 && (
          <section aria-label="Identity decision history">
            <h3>Previous identity decisions</h3>
            {w.identity_decisions.map((d) => (
              <article className="list-card" key={d.id}>
                <span className="pill">
                  {d.outcome === "keep_separate" ? "Keep separate" : "Deferred"}
                </span>
                <p>
                  {label(d.left_id)} / {label(d.right_id)}
                </p>
                <p>{d.reason}</p>
                <small>{new Date(d.at).toLocaleString()}</small>
              </article>
            ))}
          </section>
        )}
      </section>
      {addFor && (
        <section className="panel">
          <h2>Add observation to {entityLabel(addFor)}</h2>
          <ObservationForm
            key={addFor.id}
            entityId={addFor.id}
            evidence={w.evidence}
            busy={busy}
            onInspect={onSource}
            onCancel={() => setAddFor(null)}
            onSave={async (fields, reason) => {
              if (
                await apply({
                  action: "add_observation",
                  observation: fields,
                  reason,
                })
              )
                setAddFor(null);
            }}
          />
        </section>
      )}
      {review && (
        <ObservationReview
          observation={review}
          evidence={w.evidence}
          busy={busy}
          apply={apply}
          error={error}
          onInspect={onSource}
          onClose={() => setReview(null)}
        />
      )}
    </>
  );
}

function EntityForm({
  initial,
  busy,
  onSave,
  onCancel,
}: {
  initial?: Entity;
  busy: boolean;
  onSave: (input: EntityInput, reason: string) => Promise<void>;
  onCancel: () => void;
}) {
  const [name, setName] = useState(initial?.name ?? "");
  const [kind, setKind] = useState<EntityKind>(initial?.kind ?? "person");
  const [identifiers, setIdentifiers] = useState(
    initial?.identifiers ?? [{ namespace: "", value: "" }],
  );
  const [reason, setReason] = useState("");
  return (
    <form
      onSubmit={(e) => {
        e.preventDefault();
        void onSave(
          {
            name,
            kind,
            identifiers: identifiers.filter((i) => i.namespace || i.value),
          },
          reason,
        );
      }}
    >
      <div className="form-grid">
        <label>
          Entity name
          <input
            required
            maxLength={300}
            value={name}
            onChange={(e) => setName(e.target.value)}
          />
        </label>
        <label>
          Entity kind
          <select
            value={kind}
            onChange={(e) => setKind(e.target.value as EntityKind)}
          >
            {kinds.map((k) => (
              <option key={k} value={k}>
                {k.replaceAll("_", " ")}
              </option>
            ))}
          </select>
        </label>
      </div>
      {identifiers.map((item, i) => (
        <div className="form-grid" key={i}>
          <label>
            Reference namespace {i + 1}
            <input
              aria-label={`Reference namespace ${i + 1}`}
              maxLength={100}
              value={item.namespace}
              onChange={(e) =>
                setIdentifiers(
                  identifiers.map((v, j) =>
                    j === i ? { ...v, namespace: e.target.value } : v,
                  ),
                )
              }
            />
          </label>
          <label>
            Reference Number {i + 1}
            <input
              aria-label={`Reference Number ${i + 1}`}
              maxLength={300}
              value={item.value}
              onChange={(e) =>
                setIdentifiers(
                  identifiers.map((v, j) =>
                    j === i ? { ...v, value: e.target.value } : v,
                  ),
                )
              }
            />
          </label>
        </div>
      ))}
      <button
        type="button"
        className="text-button"
        disabled={identifiers.length >= 50}
        onClick={() =>
          setIdentifiers([...identifiers, { namespace: "", value: "" }])
        }
      >
        Add another reference
      </button>
      <p className="muted">
        Namespaces and leading zeros are preserved. Leave both fields blank to
        omit a reference. Matching names or reference values do not merge
        records.
      </p>
      <label>
        Entity change reason
        <textarea
          required
          value={reason}
          onChange={(e) => setReason(e.target.value)}
        />
      </label>
      <div className="actions">
        <button
          className="button primary"
          disabled={busy || !reason.trim() || !name.trim()}
        >
          Save entity
        </button>
        <button type="button" className="button" onClick={onCancel}>
          Cancel entity changes
        </button>
      </div>
    </form>
  );
}

type ObservationFields = {
  entity_id: string;
  field: string;
  value: string;
  anchor: Anchor;
};
function ObservationForm({
  entityId,
  evidence,
  busy,
  initial,
  onSave,
  onCancel,
  onInspect,
}: {
  entityId: string;
  evidence: Evidence[];
  busy: boolean;
  initial?: Observation;
  onSave: (fields: ObservationFields, reason: string) => Promise<void>;
  onCancel: () => void;
  onInspect: OpenSource;
}) {
  const [field, setField] = useState(initial?.field ?? "");
  const [value, setValue] = useState(initial?.value ?? "");
  const [source, setSource] = useState(
    initial?.anchor.evidence_id ?? evidence.find((e) => e.text)?.id ?? "",
  );
  const [mode, setMode] = useState(
    initial?.anchor.kind === "cell" ? "cell" : "text",
  );
  const [lineStart, setLineStart] = useState(initial?.anchor.line_start ?? 1),
    [lineEnd, setLineEnd] = useState(initial?.anchor.line_end ?? 1);
  const [row, setRow] = useState(initial?.anchor.row ?? 2),
    [column, setColumn] = useState(initial?.anchor.column ?? "description");
  const [reason, setReason] = useState("");
  const sourceItem = evidence.find((e) => e.id === source);
  const anchor: Anchor =
    mode === "cell"
      ? { kind: "cell", evidence_id: source, sheet: "CSV", row, column }
      : {
          kind: "text",
          evidence_id: source,
          line_start: lineStart,
          line_end: lineEnd,
        };
  return (
    <form
      onSubmit={(e) => {
        e.preventDefault();
        void onSave({ entity_id: entityId, field, value, anchor }, reason);
      }}
    >
      <div className="form-grid">
        <label>
          Observation field
          <input
            required
            disabled={!!initial}
            placeholder="birth_year, alias, registration_number…"
            value={field}
            onChange={(e) => setField(e.target.value)}
          />
        </label>
        <label>
          Observation value
          <input
            required
            value={value}
            onChange={(e) => setValue(e.target.value)}
          />
        </label>
      </div>
      <label>
        Source evidence
        <select
          required
          value={source}
          onChange={(e) => {
            setSource(e.target.value);
            setMode("text");
          }}
        >
          <option value="">Choose retained evidence</option>
          {evidence
            .filter((e) => e.text !== null)
            .map((e) => (
              <option key={e.id} value={e.id}>
                {e.name}
              </option>
            ))}
        </select>
      </label>
      <label>
        Source anchor type
        <select value={mode} onChange={(e) => setMode(e.target.value)}>
          <option value="text">Text line range</option>
          {sourceItem?.media_type === "text/csv" && (
            <option value="cell">CSV cell</option>
          )}
        </select>
      </label>
      {mode === "text" ? (
        <div className="form-grid">
          <label>
            First source line
            <input
              type="number"
              required
              min={1}
              step={1}
              value={lineStart}
              onChange={(e) => setLineStart(e.target.valueAsNumber)}
            />
          </label>
          <label>
            Last source line
            <input
              type="number"
              required
              min={lineStart}
              step={1}
              value={lineEnd}
              onChange={(e) => setLineEnd(e.target.valueAsNumber)}
            />
          </label>
        </div>
      ) : (
        <div className="form-grid">
          <label>
            CSV row
            <input
              type="number"
              required
              min={2}
              step={1}
              value={row}
              onChange={(e) => setRow(e.target.valueAsNumber)}
            />
          </label>
          <label>
            CSV column
            <input
              required
              value={column}
              onChange={(e) => setColumn(e.target.value)}
            />
          </label>
        </div>
      )}
      <button
        type="button"
        className="text-button"
        disabled={!sourceItem}
        onClick={() => sourceItem && onInspect(sourceItem, anchor)}
      >
        Inspect proposed source anchor
      </button>
      <label>
        Observation change reason
        <textarea
          required
          value={reason}
          onChange={(e) => setReason(e.target.value)}
        />
      </label>
      <p className="muted">
        Saving creates a pending observation. Acceptance requires a separate
        source review. The source remains unchanged.
      </p>
      <div className="actions">
        <button
          className="button primary"
          disabled={
            busy || !reason.trim() || !source || !value.trim() || !field.trim()
          }
        >
          {initial ? "Save observation correction" : "Save observation"}
        </button>
        <button type="button" className="button" onClick={onCancel}>
          Cancel observation changes
        </button>
      </div>
    </form>
  );
}

function ObservationReview({
  observation: o,
  evidence,
  busy,
  apply,
  onInspect,
  onClose,
  error,
}: {
  observation: Observation;
  evidence: Evidence[];
  busy: boolean;
  apply: Apply;
  onInspect: OpenSource;
  onClose: () => void;
  error: string;
}) {
  const [reason, setReason] = useState(""),
    [correcting, setCorrecting] = useState(false);
  const source = evidence.find((e) => e.id === o.anchor.evidence_id);
  return (
    <Dialog label="Observation review" onClose={onClose}>
      <button
        className="close"
        aria-label="Close observation review"
        onClick={onClose}
      >
        ×
      </button>
      {error && (
        <p className="alert error" role="alert">
          {error}
        </p>
      )}
      <h2>{o.field.replaceAll("_", " ")}</h2>
      <p>{o.value}</p>
      <span className={`pill ${o.review}`}>{o.review}</span>
      <p>
        Extraction quality:{" "}
        {o.extraction_quality === null ? "not scored" : o.extraction_quality}.
        Review state is an analyst decision.
      </p>
      <button
        className="text-button"
        disabled={!source}
        onClick={() => source && onInspect(source, o.anchor)}
      >
        Inspect observation source
      </button>
      {!correcting ? (
        <>
          <label>
            Observation review reason
            <textarea
              value={reason}
              onChange={(e) => setReason(e.target.value)}
            />
          </label>
          <div className="actions">
            {(["accepted", "rejected", "deferred"] as const).map((state) => (
              <button
                className="button"
                key={state}
                disabled={busy || !reason.trim()}
                onClick={() =>
                  void apply({
                    action: "review_observation",
                    id: o.id,
                    state,
                    reason,
                  }).then((ok) => {
                    if (ok) onClose();
                  })
                }
              >
                {state === "accepted"
                  ? "Accept observation"
                  : state === "rejected"
                    ? "Reject observation"
                    : "Defer observation"}
              </button>
            ))}
          </div>
          <button className="text-button" onClick={() => setCorrecting(true)}>
            Correct observation
          </button>
        </>
      ) : (
        <ObservationForm
          entityId={o.entity_id}
          initial={o}
          evidence={evidence}
          busy={busy}
          onInspect={onInspect}
          onCancel={() => setCorrecting(false)}
          onSave={async (fields, why) => {
            if (
              await apply({
                action: "correct_observation",
                id: o.id,
                value: fields.value,
                anchor: fields.anchor,
                reason: why,
              })
            )
              onClose();
          }}
        />
      )}
    </Dialog>
  );
}
