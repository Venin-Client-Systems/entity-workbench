import { useEffect, useRef, useState } from "react";
import { command } from "./api";
import { CitationReadLane } from "./citation-read-lane";
import {
  CitationRow,
  citationLabel,
  type CitationSourceOpener,
} from "./CitationRow";
import type {
  CitationCataloguePage,
  CitationRole,
  CitationSelections,
  CitationSummary,
} from "./citation-types";
import type { Evidence } from "./types";
import "./citation-picker.css";

const pageSize = 50,
  historyLimit = 100;
const idKey = (ids: string[]) => JSON.stringify([...ids].sort());
function validRows(rows: CitationSummary[]): boolean {
  return (
    Array.isArray(rows) &&
    new Set(rows.map((row) => row.id)).size === rows.length &&
    rows.every(
      (row) =>
        ["observation", "transaction", "evidence"].includes(row.kind) &&
        typeof row.id === "string" &&
        row.source.id === row.source.sha256 &&
        (row.kind === "evidence" ? row.id : row.anchor.evidence_id) ===
          row.source.id,
    )
  );
}

/** The key is checked during render as well as after await: a changed selection
 * cannot retain one render of apparently current metadata. No partial set is used. */
export function useCitationSelections(
  ids: string[],
  revision: number,
  stale: boolean,
) {
  const selection = idKey(ids),
    key = JSON.stringify([selection, revision]);
  const [result, setResult] = useState<{
    key: string;
    value: CitationSelections;
  } | null>(null);
  const [failure, setFailure] = useState<{
    key: string;
    message: string;
  } | null>(null);
  const [reload, setReload] = useState(0);
  const generation = useRef(0);
  const [lane] = useState(() => new CitationReadLane<CitationSelections>());
  useEffect(() => {
    lane.open();
    return () => lane.close();
  }, [lane]);
  useEffect(() => {
    const ticket = ++generation.current;
    let active = true;
    setResult(null);
    setFailure(null);
    lane.clearPending();
    if (stale || !ids.length)
      return () => {
        active = false;
        generation.current++;
      };
    const requested: string[] = JSON.parse(selection);
    void lane
      .read(() =>
        command<CitationSelections>({
          action: "read_citation_selections",
          request: { ids: requested },
          expected_revision: revision,
        }),
      )
      .then((value) => {
        if (!active || ticket !== generation.current) return;
        if (
          value.schema_version !== 1 ||
          value.workspace_revision !== revision ||
          !validRows(value.rows) ||
          value.rows.length !== requested.length ||
          idKey(value.rows.map((row) => row.id)) !== selection
        )
          throw new Error(
            "Selected citation response does not match this complete selection and revision.",
          );
        setResult({ key, value });
      })
      .catch((cause) => {
        if (active && ticket === generation.current)
          setFailure({ key, message: String(cause) });
      });
    return () => {
      active = false;
      generation.current++;
    };
  }, [key, stale, reload]);
  const ready = !stale && (!ids.length || result?.key === key);
  return {
    ready,
    rows: ready && ids.length ? (result?.value.rows ?? []) : [],
    error: !stale && failure?.key === key ? failure.message : "",
    retry: () => {
      setResult(null);
      setFailure(null);
      setReload((value) => value + 1);
    },
  };
}
export type SelectionState = ReturnType<typeof useCitationSelections>;
export function SelectionStatus({
  state,
  stale,
}: {
  state: SelectionState;
  stale: boolean;
}) {
  const status = useRef<HTMLParagraphElement>(null);
  const retryOpener = useRef<Element | null>(null);
  useEffect(() => {
    const opener = retryOpener.current;
    if (!opener || (!state.ready && !state.error && !stale)) return;
    retryOpener.current = null;
    if (
      document.activeElement === opener ||
      document.activeElement === document.body
    )
      status.current?.focus();
  }, [state.ready, state.error, stale]);
  return (
    <div className="citation-selection-status">
      <p ref={status} tabIndex={-1} role="status" className="muted">
        {stale
          ? "Citation details are stale. The selected IDs and roles are preserved."
          : state.error
            ? "Selected citation details could not be loaded."
            : state.ready
              ? "Selected citation metadata resolved. Source inspection and analyst review remain separate."
              : "Resolving selected citation details…"}
      </p>
      {state.error && (
        <div>
          <p className="alert error" role="alert">
            Selected citation details unavailable: {state.error}
          </p>
          <p className="muted">
            No partial selection or source-origin count is shown. IDs and roles
            are preserved.
          </p>
          <button
            type="button"
            className="button"
            onClick={() => {
              retryOpener.current = document.activeElement;
              state.retry();
            }}
          >
            Retry selected citations
          </button>
        </div>
      )}
    </div>
  );
}

function Role({
  id,
  label,
  supporting,
  contradicting,
  choose,
  disabled,
}: {
  id: string;
  label: string;
  supporting: string[];
  contradicting: string[];
  choose: (id: string, role: CitationRole) => void;
  disabled: boolean;
}) {
  const value = supporting.includes(id)
    ? "supporting"
    : contradicting.includes(id)
      ? "contradicting"
      : "none";
  return (
    <label className="citation-role">
      Citation role
      <select
        aria-label={`Citation role for ${label}`}
        value={value}
        disabled={
          disabled ||
          (value === "none" && supporting.length + contradicting.length >= 100)
        }
        onChange={(event) => choose(id, event.target.value as CitationRole)}
      >
        <option value="none">Not cited</option>
        <option value="supporting">Supporting</option>
        <option value="contradicting">Contradictory</option>
      </select>
    </label>
  );
}
type PickerProps = {
  supporting: string[];
  contradicting: string[];
  selection: SelectionState;
  revision: number;
  stale: boolean;
  busy: boolean;
  evidence: Evidence[];
  choose: (id: string, role: CitationRole) => void;
  onSource: CitationSourceOpener;
};
export function CitationPicker(props: PickerProps) {
  const {
    supporting,
    contradicting,
    selection,
    stale,
    busy,
    choose,
    revision,
  } = props;
  const ids = [...supporting, ...contradicting];
  const [lane] = useState(() => new CitationReadLane<CitationCataloguePage>());
  useEffect(() => {
    lane.open();
    return () => lane.close();
  }, [lane]);
  const selectedRegion = useRef<HTMLDivElement>(null);
  const focusTarget = useRef<{ id: string; opener: Element | null } | null>(
    null,
  );
  const chooseAndFollow = (id: string, role: CitationRole) => {
    focusTarget.current =
      role === "none" ? null : { id, opener: document.activeElement };
    choose(id, role);
  };
  useEffect(() => {
    if (!selection.ready || !focusTarget.current) return;
    const target = focusTarget.current;
    focusTarget.current = null;
    if (
      document.activeElement === target.opener ||
      document.activeElement === document.body
    ) {
      const row = [
        ...(selectedRegion.current?.querySelectorAll<HTMLElement>(
          "[data-citation-id]",
        ) ?? []),
      ].find((node) => node.dataset.citationId === target.id);
      row?.querySelector("select")?.focus();
    }
  }, [selection.ready, idKey(ids)]);
  const [query, setQuery] = useState("");
  const [applied, setApplied] = useState("");
  useEffect(() => {
    const timer = setTimeout(() => setApplied(query), 250);
    return () => clearTimeout(timer);
  }, [query]);
  const invalid = new TextEncoder().encode(query).length > 256;
  const pending = query !== applied;
  const row = (item: CitationSummary) => (
    <CitationRow
      key={item.id}
      item={item}
      evidence={props.evidence}
      onSource={props.onSource}
      disabled={busy || stale}
    >
      <Role
        id={item.id}
        label={citationLabel(item)}
        {...props}
        choose={chooseAndFollow}
        disabled={busy || stale}
      />
    </CitationRow>
  );
  return (
    <>
      <h3>Selected citations · {ids.length}</h3>
      <div
        ref={selectedRegion}
        role="region"
        aria-label="Selected citations"
        aria-busy={!selection.ready && !selection.error && !stale}
      >
        <SelectionStatus state={selection} stale={stale} />
        {selection.ready
          ? selection.rows.map(row)
          : ids.map((id) => (
              <div className="citation-row" key={id}>
                <div>
                  <strong>Selected citation {id}</strong>
                  <p>Details unavailable · role retained</p>
                </div>
                <Role
                  id={id}
                  label={id}
                  {...props}
                  choose={chooseAndFollow}
                  disabled={busy || stale}
                />
              </div>
            ))}
        {!ids.length && (
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
          disabled={busy || stale}
          onChange={(event) => setQuery(event.target.value)}
        />
      </label>
      {invalid && (
        <p className="alert error" role="alert">
          Citation search exceeds 256 UTF-8 bytes. Shorten the query; it has not
          been submitted.
        </p>
      )}
      {pending && !invalid && (
        <p role="status">Waiting to apply citation search…</p>
      )}
      {stale ? (
        <p className="muted">
          Available citations require a current workspace revision.
        </p>
      ) : (
        <Catalogue
          key={JSON.stringify([revision, applied, idKey(ids)])}
          {...props}
          choose={chooseAndFollow}
          query={applied}
          excluded={ids}
          hidden={pending || invalid}
          lane={lane}
        />
      )}
      <p className="muted">
        Up to 100 citations per finding. Whole-source citations do not accept
        extracted records or establish source independence.
      </p>
    </>
  );
}
type Position = { cursor: string | null; offset: number };
type Attempt = { position: Position; previous: Position[] };
const first: Attempt = { position: { cursor: null, offset: 0 }, previous: [] };
function Catalogue(
  props: PickerProps & {
    query: string;
    excluded: string[];
    hidden: boolean;
    lane: CitationReadLane<CitationCataloguePage>;
  },
) {
  const [page, setPage] = useState<CitationCataloguePage | null>(null);
  const [attempt, setAttempt] = useState<Attempt>(first);
  const [loading, setLoading] = useState(true),
    [error, setError] = useState("");
  const generation = useRef(0),
    mounted = useRef(true),
    binding = useRef<string | null>(null);
  const status = useRef<HTMLParagraphElement>(null);
  async function read(next: Attempt, focus = false) {
    const opener = focus ? document.activeElement : null,
      ticket = ++generation.current;
    setAttempt(next);
    setLoading(true);
    setError("");
    setPage(null);
    try {
      const value = await props.lane.read(() =>
        command<CitationCataloguePage>({
          action: "page_citation_catalogue",
          request: {
            query: props.query,
            excluded_ids: props.excluded,
            page_size: pageSize,
            cursor: next.position.cursor,
          },
          expected_revision: props.revision,
        }),
      );
      if (!mounted.current || ticket !== generation.current) return;
      if (
        value.schema_version !== 1 ||
        value.workspace_revision !== props.revision ||
        !validRows(value.rows) ||
        value.rows.length > pageSize ||
        value.rows.some((row) => props.excluded.includes(row.id)) ||
        !Number.isSafeInteger(value.scope_count) ||
        value.scope_count < next.position.offset + value.rows.length ||
        !/^[a-f0-9]{64}$/.test(value.query_sha256) ||
        (binding.current !== null && binding.current !== value.query_sha256) ||
        value.matching.algorithm !== "unicode_default_lowercase_literal_v1" ||
        value.matching.unicode_version.length !== 3 ||
        !value.matching.unicode_version.every(
          (v) => Number.isInteger(v) && v >= 0 && v <= 255,
        ) ||
        (value.next_cursor !== null &&
          (typeof value.next_cursor !== "string" ||
            !value.next_cursor ||
            !value.rows.length))
      )
        throw new Error(
          "Citation page does not match this query, selection and revision. Refresh the workspace.",
        );
      binding.current = value.query_sha256;
      setPage(value);
    } catch (cause) {
      if (mounted.current && ticket === generation.current)
        setError(String(cause));
    } finally {
      if (mounted.current && ticket === generation.current) {
        setLoading(false);
        if (opener)
          requestAnimationFrame(() => {
            if (
              mounted.current &&
              ticket === generation.current &&
              (document.activeElement === opener ||
                document.activeElement === document.body)
            )
              status.current?.focus();
          });
      }
    }
  }
  useEffect(() => {
    mounted.current = true;
    props.lane.clearPending();
    if (new TextEncoder().encode(props.query).length <= 256) void read(first);
    else setLoading(false);
    return () => {
      mounted.current = false;
      generation.current++;
      props.lane.clearPending();
    };
  }, []);
  const locked = props.busy || props.hidden || loading,
    offset = attempt.position.offset;
  return (
    <div
      className="citation-catalogue"
      data-citation-query={props.query}
      hidden={props.hidden}
      aria-busy={loading}
    >
      <p ref={status} tabIndex={-1} role="status">
        {loading
          ? "Loading available citations…"
          : page
            ? page.scope_count === 0
              ? "No matching uncited records."
              : `${offset + 1}–${offset + page.rows.length} of ${page.scope_count} matching uncited records`
            : "Citation catalogue unavailable."}
      </p>
      {error && (
        <div>
          <p className="alert error" role="alert">
            Citation catalogue unavailable: {error}
          </p>
          <p className="muted">
            A failed read does not mean there are no matches. A changed revision
            requires workspace refresh.
          </p>
          <button
            type="button"
            className="button"
            disabled={locked}
            onClick={() => void read(attempt, true)}
          >
            Retry citation search
          </button>
        </div>
      )}
      <div
        className="citation-picker"
        role="region"
        aria-label="Available citations"
      >
        {page?.rows.map((item) => (
          <CitationRow
            key={item.id}
            item={item}
            evidence={props.evidence}
            onSource={props.onSource}
            disabled={locked}
          >
            <Role
              id={item.id}
              label={citationLabel(item)}
              {...props}
              disabled={locked}
            />
          </CitationRow>
        ))}
      </div>
      {page && (
        <p className="muted">
          Literal lowercase matching · Rust Unicode{" "}
          {page.matching.unicode_version.join(".")} · revision{" "}
          {page.workspace_revision}. Whitespace is literal; case mapping can
          differ from another Unicode version.
        </p>
      )}
      <div className="inline-actions" aria-label="Citation catalogue pages">
        <button
          type="button"
          className="button"
          disabled={locked || !page || offset === 0}
          onClick={() => void read(first, true)}
        >
          First citation page
        </button>
        <button
          type="button"
          className="button"
          disabled={locked || !page || !attempt.previous.length}
          onClick={() => {
            const previous = attempt.previous.at(-1);
            if (previous)
              void read(
                { position: previous, previous: attempt.previous.slice(0, -1) },
                true,
              );
          }}
        >
          Previous citation page
        </button>
        <button
          type="button"
          className="button"
          disabled={locked || !page?.next_cursor}
          onClick={() => {
            if (page?.next_cursor)
              void read(
                {
                  position: {
                    cursor: page.next_cursor,
                    offset: offset + page.rows.length,
                  },
                  previous: [...attempt.previous, attempt.position].slice(
                    -historyLimit,
                  ),
                },
                true,
              );
          }}
        >
          Next citation page
        </button>
      </div>
      {attempt.previous[0]?.offset > 0 && (
        <p className="muted">
          Back navigation retains the last 100 page positions. First citation
          page remains available.
        </p>
      )}
    </div>
  );
}
