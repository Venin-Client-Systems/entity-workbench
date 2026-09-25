import { useEffect, useRef, useState, useSyncExternalStore } from "react";
import { command } from "./api";
import "./docx-snapshots.css";
import { DocxCapture } from "./docx-capture";
import {
  validateDocxPage,
  type DocxSnapshot,
  type DocxSnapshotPage,
} from "./docx-snapshot-types";
import {
  nativeExportsAvailable,
  prepareNativeDocx,
  commitNativeExport,
  discardNativeExport,
  type PreparedNativeExport,
} from "./native-export";

type Position = { cursor: string | null; offset: number };
type Attempt = { position: Position; previous: Position[] };
const first: Attempt = { position: { cursor: null, offset: 0 }, previous: [] };

export function DocxSnapshots({
  revision,
  busy,
  capture,
  refresh,
}: {
  revision: number;
  busy: boolean;
  capture: DocxCapture;
  refresh: () => Promise<boolean>;
}) {
  const saved = useSyncExternalStore(capture.subscribe, capture.snapshot);
  const [reload, setReload] = useState(0);
  const [refreshing, setRefreshing] = useState(false);
  const [refreshError, setRefreshError] = useState("");
  const mounted = useRef(true);
  useEffect(() => {
    mounted.current = true;
    capture.catalogue.open();
    return () => {
      mounted.current = false;
      capture.catalogue.close();
    };
  }, [capture]);
  async function refreshCatalogue() {
    setRefreshing(true);
    setRefreshError("");
    const ok = await refresh().catch(() => false);
    if (!mounted.current) return;
    if (ok) setReload((value) => value + 1);
    else
      setRefreshError(
        "Workspace refresh failed. The catalogue has not been revalidated.",
      );
    setRefreshing(false);
  }
  return (
    <section
      className="panel docx-snapshots"
      aria-label="Editable DOCX snapshots"
    >
      <div className="panel-heading">
        <h2>Editable DOCX snapshots</h2>
        <button
          className="button primary"
          disabled={
            busy || saved.phase === "creating" || saved.phase === "resolving"
          }
          onClick={() => void capture.capture(revision, refresh)}
        >
          {saved.phase === "creating"
            ? "Capturing DOCX snapshot…"
            : saved.phase === "resolving"
              ? "Checking capture outcome…"
              : saved.phase === "uncertain"
                ? "Retry same DOCX capture"
                : "Capture DOCX snapshot"}
        </button>
      </div>
      <p className="context-note">
        Captures the current assessment, citations and calculation details at a
        fixed revision, including drafts. Earlier HTML snapshots remain
        unchanged. DOCX assembly and exhibits remain limited by the current
        report template.
      </p>
      {saved.request && saved.phase === "creating" && (
        <p role="status">
          Capture in progress for revision {saved.request.revision}. Navigation
          keeps this request: <code>{saved.request.id}</code>.
        </p>
      )}
      {saved.phase === "uncertain" && (
        <div className="docx-capture-state">
          {saved.error && (
            <p className="alert error" role="alert">
              Capture completion is unconfirmed: {saved.error}
            </p>
          )}
          <p>
            Retry retains request <code>{saved.request!.id}</code> and captured
            revision {saved.request!.revision}. Workspace refresh does not
            create a new capture. If that revision was rejected, retry cannot
            substitute the current revision. Request recovery is retained only
            while this application remains open.
          </p>
          <button
            className="button"
            disabled={busy}
            onClick={() => void capture.resolve(revision, refresh)}
          >
            Check DOCX capture outcome
          </button>
          {saved.resolution?.outcome.state === "not_recorded" && (
            <div>
              <p role="status">
                No snapshot is recorded for this request at workspace revision{" "}
                {saved.resolution.workspace_revision}.
              </p>
              {saved.resolution.workspace_revision ===
              saved.request!.revision ? (
                <p>
                  A previously sent capture may still publish at this revision.
                  Retry the same request; a replacement is not authorized.
                </p>
              ) : (
                <>
                  <p>
                    The workspace has advanced beyond the retained capture
                    revision. That old request can no longer publish. Starting a
                    new capture acknowledges this outcome and uses a new request
                    at the visible revision.
                  </p>
                  <button
                    className="button"
                    disabled={
                      busy || revision < saved.resolution.workspace_revision
                    }
                    onClick={() => void capture.startNew(revision, refresh)}
                  >
                    Start a new DOCX snapshot
                  </button>
                  {revision < saved.resolution.workspace_revision && (
                    <p>Refresh the workspace before starting a new capture.</p>
                  )}
                </>
              )}
              {saved.refreshFailed && (
                <p className="alert error" role="alert">
                  The outcome is retained, but workspace refresh failed.
                </p>
              )}
            </div>
          )}
        </div>
      )}
      {saved.phase === "resolving" && (
        <p role="status">
          Checking the retained request and its frozen artifacts. Navigation
          preserves this lookup.
        </p>
      )}
      {saved.phase === "saved" && saved.snapshot && (
        <p className="alert" role="status">
          Snapshot captured: <code>{saved.snapshot.id}</code> · source revision{" "}
          {saved.snapshot.workspace_revision}.
          {saved.refreshFailed &&
            " Workspace refresh failed; the snapshot is saved. Refresh the catalogue separately."}
        </p>
      )}
      {saved.acknowledged.length > 0 && (
        <details>
          <summary>
            Recent acknowledged capture outcomes ({saved.acknowledged.length},
            up to 20)
          </summary>
          <ul>
            {saved.acknowledged.map((item) => (
              <li key={item.request.id}>
                Request <code>{item.request.id}</code> for source revision{" "}
                {item.request.revision}: no record at revision{" "}
                {item.resolvedRevision}. A new capture was explicitly requested.
              </li>
            ))}
          </ul>
        </details>
      )}
      <button
        className="button"
        disabled={busy || refreshing}
        onClick={() => void refreshCatalogue()}
      >
        {refreshing ? "Refreshing DOCX catalogue…" : "Refresh DOCX catalogue"}
      </button>
      {refreshError && (
        <p className="alert error" role="alert">
          {refreshError}
        </p>
      )}
      <div className="docx-catalogue-context">
        <p className="docx-instrument-label">Retained metadata catalogue</p>
        <p>
          Listing a snapshot does not verify its original sources or artifact
          bytes. Native saving verifies the frozen document and DOCX; it never
          regenerates a report from the current workspace.
        </p>
      </div>
      <Catalogue
        key={`${revision}:${reload}`}
        revision={revision}
        disabled={busy || refreshing}
        capture={capture}
      />
    </section>
  );
}

function Catalogue({
  revision,
  disabled,
  capture,
}: {
  revision: number;
  disabled: boolean;
  capture: DocxCapture;
}) {
  const [page, setPage] = useState<DocxSnapshotPage | null>(null);
  const [attempt, setAttempt] = useState<Attempt>(first);
  const [loading, setLoading] = useState(true);
  const [error, setError] = useState("");
  const generation = useRef(0),
    mounted = useRef(true);
  const binding = useRef<string | null>(null);
  const status = useRef<HTMLParagraphElement>(null);
  const focus = useRef<{
    ticket: number;
    opener: Element | null;
    moved: boolean;
  } | null>(null);
  const frame = useRef<number | null>(null);
  async function read(next: Attempt, restoreFocus = false) {
    const ticket = ++generation.current;
    if (frame.current !== null) cancelAnimationFrame(frame.current);
    focus.current = restoreFocus
      ? { ticket, opener: document.activeElement, moved: false }
      : null;
    setAttempt(next);
    setLoading(true);
    setError("");
    setPage(null);
    try {
      const result = await capture.catalogue.read(() =>
        command<DocxSnapshotPage>({
          action: "page_docx_snapshots",
          request: { page_size: 20, cursor: next.position.cursor },
          expected_revision: revision,
        }),
      );
      if (!mounted.current || ticket !== generation.current) return;
      validateDocxPage(result, revision, next.position.offset, binding.current);
      if (
        result.next_cursor === next.position.cursor &&
        result.next_cursor !== null
      )
        throw new Error(
          "DOCX catalogue continuation repeated its current cursor.",
        );
      binding.current = result.query_sha256;
      setPage(result);
    } catch (cause) {
      if (mounted.current && ticket === generation.current)
        setError(String(cause));
    } finally {
      if (mounted.current && ticket === generation.current) {
        setLoading(false);
        const pending = focus.current;
        if (pending?.ticket === ticket)
          frame.current = requestAnimationFrame(() => {
            frame.current = null;
            if (focus.current !== pending) return;
            focus.current = null;
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
    const moved = (event: FocusEvent) => {
      if (
        focus.current &&
        event.target !== focus.current.opener &&
        event.target !== document.body &&
        event.target !== document.documentElement
      )
        focus.current.moved = true;
    };
    document.addEventListener("focusin", moved);
    capture.catalogue.open();
    capture.catalogue.clearPending();
    void read(first);
    return () => {
      mounted.current = false;
      generation.current++;
      document.removeEventListener("focusin", moved);
      focus.current = null;
      if (frame.current !== null) cancelAnimationFrame(frame.current);
      capture.catalogue.clearPending();
    };
  }, [capture]);
  const offset = attempt.position.offset,
    locked = disabled || loading;
  return (
    <div aria-busy={loading}>
      <p className="muted">
        Newest publication first · 20 snapshots per page · catalogue revision{" "}
        {revision}
      </p>
      <div className="docx-catalogue-toolbar">
        <p role="status" tabIndex={-1} ref={status}>
          {loading
            ? "Loading DOCX catalogue…"
            : page
              ? page.total_count === 0
                ? "No DOCX snapshots recorded."
                : `${offset + 1}–${offset + page.rows.length} of ${page.total_count} DOCX snapshots`
              : "DOCX catalogue unavailable."}
        </p>
        <div
          className="inline-actions"
          role="group"
          aria-label="DOCX catalogue pages"
        >
          <button
            className="button"
            disabled={locked || !page || offset === 0}
            aria-label="First DOCX page"
            onClick={() => void read(first, true)}
          >
            First
          </button>
          <button
            className="button"
            disabled={locked || !page || !attempt.previous.length}
            aria-label="Back to previous DOCX page"
            onClick={() => {
              const previous = attempt.previous.at(-1);
              if (previous)
                void read(
                  {
                    position: previous,
                    previous: attempt.previous.slice(0, -1),
                  },
                  true,
                );
            }}
          >
            Back
          </button>
          <button
            className="button"
            disabled={locked || !page?.next_cursor}
            aria-label="Next DOCX page"
            onClick={() => {
              if (page?.next_cursor)
                void read(
                  {
                    position: {
                      cursor: page.next_cursor,
                      offset: offset + page.rows.length,
                    },
                    previous: [...attempt.previous, attempt.position].slice(
                      -100,
                    ),
                  },
                  true,
                );
            }}
          >
            Next
          </button>
        </div>
      </div>
      {error && (
        <div>
          <p className="alert error" role="alert">
            Catalogue unavailable: {error}
          </p>
          <p>
            A failed read does not mean no snapshots exist. Refresh the
            workspace if its revision changed.
          </p>
          <button
            className="button"
            disabled={locked}
            onClick={() => void read(attempt, true)}
          >
            Retry DOCX catalogue read
          </button>
        </div>
      )}
      {page?.rows.map((row) => (
        <SnapshotCard key={row.id} row={row} />
      ))}
      {attempt.previous[0]?.offset > 0 && (
        <p className="muted">
          Back navigation retains 100 page positions. First DOCX page remains
          available.
        </p>
      )}
    </div>
  );
}

function SnapshotCard({ row }: { row: DocxSnapshot }) {
  const [loading, setLoading] = useState(false),
    [error, setError] = useState(""),
    [notice, setNotice] = useState("");
  const active = useRef(false),
    mounted = useRef(true);
  useEffect(() => {
    mounted.current = true;
    return () => {
      mounted.current = false;
    };
  }, []);
  async function save() {
    if (active.current) return;
    active.current = true;
    setLoading(true);
    setError("");
    setNotice("");
    let prepared: PreparedNativeExport | undefined,
      committed = false;
    try {
      prepared = await prepareNativeDocx(row);
      if (!mounted.current)
        throw new Error("DOCX view closed while preparing the file.");
      const receipt = await commitNativeExport(prepared);
      committed = true;
      if (mounted.current) setNotice(receipt.location);
    } catch (cause) {
      if (mounted.current) setError(String(cause));
    } finally {
      if (prepared && !committed) {
        try {
          await discardNativeExport(prepared.ticket);
        } catch (cause) {
          if (mounted.current)
            setError(
              (old) =>
                `${old} Staging cleanup was not confirmed: ${String(cause)}`,
            );
        }
      }
      active.current = false;
      if (mounted.current) setLoading(false);
    }
  }
  return (
    <article className="list-card docx-snapshot-card" data-docx-id={row.id}>
      <div className="docx-snapshot-heading">
        <h3>Source revision {row.workspace_revision}</h3>
        <time dateTime={row.created_at}>{row.created_at}</time>
      </div>
      <dl className="docx-snapshot-identities">
        <div>
          <dt>Snapshot</dt>
          <dd>
            <code>{row.id}</code>
          </dd>
        </div>
        <div>
          <dt>Document SHA-256</dt>
          <dd>
            <code>{row.document.sha256}</code>
          </dd>
        </div>
        <div>
          <dt>DOCX SHA-256</dt>
          <dd>
            <code>{row.docx.sha256}</code>
          </dd>
        </div>
      </dl>
      <p className="muted">
        {row.docx.bytes.toLocaleString()} bytes · Template{" "}
        {row.template_version}
        {" · "}generator {row.generator_version}
      </p>
      {nativeExportsAvailable() ? (
        <button
          className="button"
          disabled={loading}
          onClick={() => void save()}
        >
          {loading ? "Preparing DOCX file…" : "Save DOCX file"}
        </button>
      ) : (
        <p className="context-note">
          Open the native desktop application to save this DOCX file.
        </p>
      )}
      {error && (
        <p className="alert error" role="alert">
          {error}
        </p>
      )}
      {notice && (
        <p className="alert" role="status">
          <strong>Saved immutable DOCX:</strong>{" "}
          <code data-native-export-location>{notice}</code>
        </p>
      )}
    </article>
  );
}
