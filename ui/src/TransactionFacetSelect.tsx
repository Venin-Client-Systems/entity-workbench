import { useEffect, useId, useRef, useState } from "react";
import { command } from "./api";
// Reuse the existing generic scheduling primitive; no second request scheduler.
import { CitationReadLane as ReadLane } from "./citation-read-lane";
import "./transaction-facets.css";

type FacetKind = "account" | "currency";
type FacetPage = {
  schema_version: number;
  workspace_revision: number;
  facet: FacetKind;
  query_sha256: string;
  transaction_count: number;
  distinct_count: number;
  values: { value: string; transaction_count: number }[];
  next_cursor: string | null;
};
type Props = {
  kind: FacetKind;
  revision: number;
  label: string;
  value: string | null;
  onChange: (value: string | null) => void;
  disabled?: boolean;
};
type Position = { cursor: string | null; offset: number };
type Attempt = { position: Position; previous: Position[] };
const first: Attempt = { position: { cursor: null, offset: 0 }, previous: [] };
const pageSize = 100,
  historyLimit = 100;

/** A lane survives revision changes while the keyed page drops obsolete state. */
export function TransactionFacetSelect(props: Props) {
  const [lane] = useState(() => new ReadLane<FacetPage>());
  useEffect(() => {
    lane.open();
    return () => lane.close();
  }, [lane]);
  return (
    <FacetChoices
      key={JSON.stringify([props.kind, props.revision])}
      {...props}
      lane={lane}
    />
  );
}
function FacetChoices({
  kind,
  revision,
  label,
  value,
  onChange,
  disabled = false,
  lane,
}: Props & { lane: ReadLane<FacetPage> }) {
  const id = useId();
  const [page, setPage] = useState<FacetPage | null>(null);
  const [attempt, setAttempt] = useState<Attempt>(first);
  const [loading, setLoading] = useState(true);
  const [error, setError] = useState("");
  const mounted = useRef(true),
    generation = useRef(0),
    binding = useRef<string | null>(null);
  const status = useRef<HTMLParagraphElement>(null);
  const pendingFocus = useRef<{
    ticket: number;
    opener: Element;
    moved: boolean;
  } | null>(null);
  const focusFrame = useRef<number | null>(null);
  const plural = kind === "account" ? "accounts" : "currencies";
  async function read(next: Attempt, followFocus = false) {
    const ticket = ++generation.current;
    const opener = followFocus ? document.activeElement : null;
    if (focusFrame.current !== null) cancelAnimationFrame(focusFrame.current);
    focusFrame.current = null;
    pendingFocus.current = opener ? { ticket, opener, moved: false } : null;
    setAttempt(next);
    setPage(null);
    setError("");
    setLoading(true);
    try {
      const response = await lane.read(() =>
        command<FacetPage>({
          action: "page_transaction_facets",
          request: {
            facet: kind,
            page_size: pageSize,
            cursor: next.position.cursor,
          },
          expected_revision: revision,
        }),
      );
      if (!mounted.current || ticket !== generation.current) return;
      const offset = next.position.offset;
      // Transport/selection binding only. Rust owns canonical value validation,
      // exact BINARY ordering, counts and continuation membership.
      if (
        response.schema_version !== 1 ||
        response.workspace_revision !== revision ||
        response.facet !== kind ||
        !Number.isSafeInteger(response.transaction_count) ||
        response.transaction_count < 0 ||
        !Number.isSafeInteger(response.distinct_count) ||
        response.distinct_count < 0 ||
        response.distinct_count > response.transaction_count ||
        !Array.isArray(response.values) ||
        response.values.length > pageSize ||
        new Set(response.values.map((row) => row.value)).size !==
          response.values.length ||
        response.values.some(
          (row) =>
            typeof row.value !== "string" ||
            !row.value.length ||
            !Number.isSafeInteger(row.transaction_count) ||
            row.transaction_count < 1 ||
            row.transaction_count > response.transaction_count,
        ) ||
        offset + response.values.length > response.distinct_count ||
        (response.values.length === 0 && response.distinct_count !== 0) ||
        !/^[a-f0-9]{64}$/.test(response.query_sha256) ||
        (binding.current !== null &&
          binding.current !== response.query_sha256) ||
        (response.next_cursor !== null &&
          (typeof response.next_cursor !== "string" ||
            !response.next_cursor.length ||
            response.next_cursor.length > 2048 ||
            response.next_cursor === next.position.cursor ||
            response.values.length === 0)) ||
        (response.next_cursor !== null) !==
          offset + response.values.length < response.distinct_count
      ) {
        throw new Error(
          "Facet response does not match this selector and revision. Refresh the workspace.",
        );
      }
      binding.current = response.query_sha256;
      setPage(response);
    } catch (cause) {
      if (mounted.current && ticket === generation.current)
        setError(String(cause));
    } finally {
      if (mounted.current && ticket === generation.current) {
        setLoading(false);
        const pending = pendingFocus.current;
        if (pending?.ticket === ticket)
          focusFrame.current = requestAnimationFrame(() => {
            focusFrame.current = null;
            if (pendingFocus.current !== pending) return;
            pendingFocus.current = null;
            const active = document.activeElement;
            if (
              mounted.current &&
              ticket === generation.current &&
              !pending.moved &&
              (active === pending.opener ||
                active === document.body ||
                active === document.documentElement)
            )
              status.current?.focus();
          });
      }
    }
  }
  useEffect(() => {
    mounted.current = true;
    const rememberFocusMove = (event: FocusEvent) => {
      const pending = pendingFocus.current;
      if (
        pending &&
        event.target !== pending.opener &&
        event.target !== document.body &&
        event.target !== document.documentElement
      )
        pending.moved = true;
    };
    document.addEventListener("focusin", rememberFocusMove);
    lane.open();
    lane.clearPending();
    void read(first);
    return () => {
      mounted.current = false;
      generation.current++;
      document.removeEventListener("focusin", rememberFocusMove);
      pendingFocus.current = null;
      if (focusFrame.current !== null) cancelAnimationFrame(focusFrame.current);
      lane.clearPending();
    };
  }, []);
  const rows = page?.values ?? [];
  const retained = value !== null && !rows.some((row) => row.value === value);
  const offset = attempt.position.offset;
  const locked = disabled || loading;
  return (
    <div
      className="transaction-facet"
      data-facet-kind={kind}
      data-facet-label={label}
      aria-busy={loading}
    >
      <label htmlFor={`${id}-select`}>{label}</label>
      <select
        id={`${id}-select`}
        aria-describedby={`${id}-status ${id}-scope`}
        value={value ?? ""}
        disabled={disabled}
        onChange={(event) => onChange(event.target.value || null)}
      >
        <option value="">All {plural}</option>
        {retained && (
          <option value={value}>{value} · retained selection</option>
        )}
        {rows.map((row) => (
          <option key={row.value} value={row.value}>
            {row.value} · {row.transaction_count} rows
          </option>
        ))}
      </select>
      <p
        id={`${id}-status`}
        ref={status}
        tabIndex={-1}
        role="status"
        className="facet-status"
      >
        {loading
          ? `Loading ${plural}…`
          : page
            ? page.distinct_count === 0
              ? `No ${plural} recorded.`
              : `${offset + 1}–${offset + rows.length} of ${page.distinct_count} ${plural}`
            : `${kind === "account" ? "Account" : "Currency"} choices unavailable.`}
      </p>
      {retained && (
        <p className="facet-retained">
          Selected value retained outside the loaded choices.
        </p>
      )}
      {error && (
        <div className="facet-error">
          <p role="alert">
            {label} choices unavailable: {error}
          </p>
          <p>
            A failed read does not mean there are no values. Refresh the
            workspace if its revision changed.
          </p>
          <button
            type="button"
            className="button"
            disabled={locked}
            onClick={() => void read(attempt, true)}
          >
            Retry {label.toLowerCase()}
          </button>
        </div>
      )}
      <div className="patterns-pager facet-pager" aria-label={`${label} pages`}>
        <button
          type="button"
          className="button"
          disabled={locked || offset === 0}
          aria-label={`First ${label.toLowerCase()} page`}
          onClick={() => void read(first, true)}
        >
          First
        </button>
        <button
          type="button"
          className="button"
          disabled={locked || !attempt.previous.length}
          aria-label={`Previous ${label.toLowerCase()} page`}
          onClick={() => {
            const previous = attempt.previous.at(-1);
            if (previous)
              void read(
                { position: previous, previous: attempt.previous.slice(0, -1) },
                true,
              );
          }}
        >
          Back
        </button>
        <button
          type="button"
          className="button"
          disabled={locked || !page?.next_cursor}
          aria-label={`Next ${label.toLowerCase()} page`}
          onClick={() => {
            if (page?.next_cursor)
              void read(
                {
                  position: {
                    cursor: page.next_cursor,
                    offset: offset + rows.length,
                  },
                  previous: [...attempt.previous, attempt.position].slice(
                    -historyLimit,
                  ),
                },
                true,
              );
          }}
        >
          Next
        </button>
      </div>
      <p id={`${id}-scope`} className="facet-scope">
        {page
          ? `${page.transaction_count} whole-ledger rows · all review states`
          : "Whole-ledger choices · all review states"}
        {` · revision ${revision}`}
      </p>
      {attempt.previous[0]?.offset > 0 && (
        <p className="facet-scope">
          Back covers the last {historyLimit} pages. First restarts the list.
        </p>
      )}
    </div>
  );
}
