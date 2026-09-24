import React, { useCallback, useEffect, useRef, useState } from "react";
import { createRoot } from "react-dom/client";
import { command } from "./api";
import { downloadExport } from "./download";
import type {
  DesktopSummaryResponse,
  ReviewState,
  Transaction,
  Evidence,
  Anchor,
} from "./types";
import { Graph, LocalMap, TotalsChart } from "./Visuals";
import { AssessmentWorkbench } from "./AssessmentWorkbench";
import { CollectionHistory } from "./CollectionReview";
import { DocumentJobs } from "./DocumentJobs";
import { Dialog } from "./Dialog";
import { TransactionLedger } from "./TransactionLedger";
import { TransactionReview } from "./TransactionReview";
import type {
  LedgerScope,
  TransactionSelection,
} from "./transaction-ledger-types";
import {
  readDesktopSummary,
  isBackupResult,
  retainNewestSummary,
} from "./desktop-summary";
import { EntityWorkbench } from "./EntityWorkbench";
import { TransactionComparison } from "./TransactionComparison";
import { TransactionPatterns } from "./TransactionPatterns";
import { StatementImport, type StatementFile } from "./StatementImport";
import { SourceContent } from "./SourceContent";
import "./tokens.css";
import "./style.css";
import "./accessibility.css";
const sections = [
  "Overview",
  "Evidence",
  "Entities",
  "Transactions",
  "Relationships",
  "Locations",
  "Discovery",
  "Assessment",
] as const;
type Section = (typeof sections)[number];
function App() {
  const [data, setData] = useState<DesktopSummaryResponse | null>(null),
    [section, setSection] = useState<Section>("Overview"),
    [error, setError] = useState(""),
    [notice, setNotice] = useState(""),
    [busy, setBusy] = useState(false);
  const [query, setQuery] = useState(""),
    [reviewFilter, setReviewFilter] = useState<ReviewState | null>(null),
    [currency, setCurrency] = useState<string | null>(null),
    [selected, setSelected] = useState<TransactionSelection | null>(null),
    [evidence, updateEvidence] = useState<Evidence | null>(null);
  const [sourceAnchor, setSourceAnchor] = useState<Anchor | undefined>(
    undefined,
  );
  const setEvidence = (item: Evidence | null, anchor?: Anchor) => {
    updateEvidence(item);
    setSourceAnchor(anchor);
  };
  const [ledgerPivot, setLedgerPivot] = useState(0);
  const [ledgerScope, setLedgerScope] = useState<LedgerScope | null>(null);
  const [visibleTransactionIds, setVisibleTransactionIds] = useState<string[]>(
    [],
  );
  const [entityId, setEntityId] = useState(""),
    [seeds, setSeeds] = useState("https://example.com/"),
    [previewed, setPreviewed] = useState(false);
  const [searchHits, setSearchHits] = useState<
    { id: string; name: string; score: number }[] | null
  >(null);
  const pendingDocumentRequests = useRef(new Map<string, string>());
  const workspaceAction = useRef(false);
  const publishSummary = useCallback((value: unknown) => {
    const next = readDesktopSummary(value);
    setData((current) => retainNewestSummary(current, next));
  }, []);
  const upload = useRef<HTMLInputElement>(null);
  const [statementFile, setStatementFile] = useState<StatementFile | null>(
    null,
  );
  const searchCorpus = async () => {
    setBusy(true);
    setError("");
    try {
      const result = await command<{
        hits: { id: string; name: string; score: number }[];
      }>({ action: "search", query });
      setSearchHits(result.hits);
    } catch (e) {
      setError(String(e));
    } finally {
      setBusy(false);
    }
  };
  const run = useCallback(
    async (action: Record<string, unknown>) => {
      if (workspaceAction.current) return false;
      workspaceAction.current = true;
      setBusy(true);
      setError("");
      setNotice("");
      try {
        const response = await command<unknown>(action);
        if (action.action === "backup") {
          if (!isBackupResult(response))
            throw new Error(
              "Invalid backup response; success was not confirmed.",
            );
          setNotice(
            "Recoverable backup saved in the workspace backups directory.",
          );
        } else publishSummary(response);
        return true;
      } catch (e) {
        setError(String(e));
        return false;
      } finally {
        workspaceAction.current = false;
        setBusy(false);
      }
    },
    [publishSummary],
  );
  useEffect(() => {
    void run({ action: "view" });
  }, [run]);
  const navigate = (s: Section) => {
    setSelected(null);
    setSection(s);
    setQuery("");
    setSearchHits(null);
  };
  const inspectTransaction = (row: Transaction, revision: number) => {
    setSelected({ row, revision });
  };
  const applyLedgerScope = useCallback(
    (scope: LedgerScope, visibleIds: string[]) => {
      setLedgerScope(scope);
      setVisibleTransactionIds(visibleIds);
      setCurrency(scope.filter.currency);
      setReviewFilter(scope.filter.review);
    },
    [],
  );
  const selectEntity = useCallback((id: string) => {
    setEntityId(id);
    setSection("Entities");
  }, []);
  const selectCurrency = useCallback((c: string) => {
    setCurrency(c);
    setLedgerPivot((value) => value + 1);
    setSection("Transactions");
  }, []);
  const download = async (content: string, name: string, type: string) => {
    setNotice("");
    const saved = await downloadExport(content, name, type);
    if (saved) setNotice(`Saved to Downloads: ${saved}`);
  };
  const w = data?.workspace,
    a = data?.analysis;
  const uploadFile = async (file: File) => {
    if (file.size > 16 * 1024 * 1024) {
      setError("Import limit is 16 MiB per file.");
      return;
    }
    const bytes = Array.from(new Uint8Array(await file.arrayBuffer()));
    if (/\.(csv|tsv)$/i.test(file.name)) {
      setSelected(null);
      setStatementFile({ name: file.name, bytes });
      return;
    }
    await run({ action: "import", name: file.name, bytes });
    setSection("Evidence");
  };
  const evidenceList =
    w?.evidence.filter((e) =>
      searchHits
        ? searchHits.some((h) => h.id === e.id)
        : `${e.name} ${e.text ?? ""}`
            .toLowerCase()
            .includes(query.toLowerCase()),
    ) ?? [];
  return (
    <div
      className={`shell${selected && section === "Transactions" ? " review-open" : ""}`}
      onClickCapture={(event) => {
        // WebKit does not focus buttons on pointer/AX activation. Establish the
        // real opener before mounting a dialog so source review can return to it.
        if (event.target instanceof Element)
          event.target
            .closest<HTMLButtonElement>("button")
            ?.focus({ preventScroll: true });
      }}
    >
      <aside className="sidebar">
        <div className="brand">
          <div>
            <strong>Entity Workbench</strong>
            <span className="brand-caption">EVIDENCE WORKSPACE</span>
          </div>
        </div>
        <div className="workspace-label">ACTIVE WORKSPACE</div>
        <div className="case-name">
          Local workspace <span>LOCAL</span>
        </div>
        <nav aria-label="Workbench sections">
          {sections.map((s, i) => (
            <button
              key={s}
              className={section === s ? "active" : ""}
              aria-current={section === s ? "page" : undefined}
              onClick={() => navigate(s)}
            >
              <span className="nav-index">
                {String(i + 1).padStart(2, "0")}
              </span>
              {s}
              {s === "Transactions" && a && (
                <small>{a.review_counts.pending}</small>
              )}
            </button>
          ))}
        </nav>
        <div className="sidebar-bottom">
          <span className="status-dot" /> Local evidence store
          <p>No telemetry · No hosted services</p>
          <span className="build-label">DEVELOPMENT 0.1</span>
        </div>
      </aside>
      <div className="workspace">
        <header>
          <div className="breadcrumbs">
            Workspace <span>/</span> {section}
          </div>
          <div className="header-actions">
            <span className="revision">REV {w?.revision ?? "—"}</span>
            <button
              className="button subtle"
              disabled={busy}
              onClick={() => void run({ action: "backup" })}
            >
              Back up
            </button>
            <button
              className="button primary"
              disabled={busy}
              onClick={() => upload.current?.click()}
            >
              ＋ Import evidence
            </button>
            <input
              ref={upload}
              type="file"
              hidden
              onChange={(e) => {
                const f = e.target.files?.[0];
                if (f) void uploadFile(f);
                e.target.value = "";
              }}
            />
          </div>
        </header>
        <main>
          <div className="page-heading">
            <div>
              <p className="eyebrow">
                WORKSPACE /{" "}
                {String(sections.indexOf(section) + 1).padStart(2, "0")}
              </p>
              <h1>
                {section === "Overview" ? "Investigation overview" : section}
              </h1>
              <p className="subtitle">
                {
                  {
                    Overview:
                      "Open questions, preserved sources and outstanding review decisions.",
                    Evidence:
                      "Preserved originals, reviewable derivatives and source anchors.",
                    Entities:
                      "Separate namesakes. Compare observations before deciding.",
                    Transactions:
                      "Exact amounts. Explicit decisions. Every total leads back to its source.",
                    Relationships:
                      "Explore assertions while retaining their evidence and review status.",
                    Locations:
                      "Historical context and uncertainty come before distance.",
                    Discovery:
                      "Collect public pages directly. Search a local corpus you can inspect.",
                    Assessment:
                      "Build cited findings and preserve an identifiable report snapshot.",
                  }[section]
                }
              </p>
            </div>
            <span className="pill synthetic">
              {w?.entities.some((e) => e.id === "person-a")
                ? "DEMO RECORDS PRESENT"
                : "LOCAL WORKSPACE"}
            </span>
          </div>
          {error && (
            <div className="alert error" role="alert">
              {error}
            </div>
          )}
          {notice && (
            <div className="alert" role="status">
              {notice}
            </div>
          )}
          {busy && (
            <div className="busy" role="status">
              Processing workspace action…
            </div>
          )}
          {!w && !busy && (
            <div className="empty">
              <h2>Workspace unavailable</h2>
              <p>
                Launch the native desktop app, or the documented local
                development bridge.
              </p>
              <button
                className="button"
                onClick={() => void run({ action: "view" })}
              >
                Retry
              </button>
            </div>
          )}
          {w && a && (
            <>
              {w.revision === 0 && (
                <div className="welcome">
                  <div>
                    <h2>Start with a reviewable example</h2>
                    <p>
                      Load fictional namesakes, a statement with an OCR error,
                      unresolved merchant branches and a research question.
                    </p>
                  </div>
                  <button
                    className="button primary"
                    disabled={busy}
                    onClick={() => void run({ action: "seed_demo" })}
                  >
                    Load synthetic investigation
                  </button>
                </div>
              )}
              {section === "Overview" && (
                <>
                  <div className="stats">
                    <Stat
                      label="Evidence items"
                      value={w.evidence.length}
                      detail="Originals retained"
                    />
                    <Stat
                      label="Entities"
                      value={w.entities.length}
                      detail="Identity decisions preserved"
                    />
                    <Stat
                      label="Transactions to review"
                      value={a.review_counts.pending}
                      detail="Exact decimals, by currency"
                    />
                    <Stat
                      label="Open collection gaps"
                      value={w.hypotheses.flatMap((h) => h.gaps).length}
                      detail="Alternative explanations visible"
                    />
                  </div>
                  <div className="grid-two">
                    <section className="panel">
                      <div className="panel-heading">
                        <h2>Investigation questions</h2>
                        <span className="pill">OPEN</span>
                      </div>
                      {w.hypotheses.map((h) => (
                        <article key={h.id} className="question">
                          <h3>{h.question}</h3>
                          <p>{h.proposition}</p>
                          <h4>Alternative explanations</h4>
                          {h.alternatives.map((t) => (
                            <p className="line-item" key={t}>
                              <span>↳</span>
                              {t}
                            </p>
                          ))}
                          <h4>Collection gaps</h4>
                          {h.gaps.map((g) => (
                            <p className="line-item" key={g}>
                              <span>○</span>
                              {g}
                            </p>
                          ))}
                        </article>
                      ))}
                    </section>
                    <section className="panel">
                      <div className="panel-heading">
                        <h2>Review priorities</h2>
                        <span className="count">
                          {a.balance_discrepancy_count +
                            a.duplicate_candidate_row_count}
                        </span>
                      </div>
                      <button
                        className="priority"
                        onClick={() => navigate("Transactions")}
                      >
                        <span className="priority-mark">!</span>
                        <div>
                          <h3>Statement reconciliation</h3>
                          <p>
                            {a.balance_discrepancy_count} balance discrepancies
                            · {a.duplicate_candidate_row_count} possible
                            duplicate rows
                          </p>
                        </div>
                        <span>↗</span>
                      </button>
                      <button
                        className="priority"
                        onClick={() => navigate("Entities")}
                      >
                        <span className="priority-mark">≋</span>
                        <div>
                          <h3>Identity review</h3>
                          <p>
                            {w.entities.length} entities · {w.observations.length} source observations
                          </p>
                        </div>
                        <span>↗</span>
                      </button>
                      <button
                        className="priority"
                        onClick={() => navigate("Locations")}
                      >
                        <span className="priority-mark">⌖</span>
                        <div>
                          <h3>Merchant locations</h3>
                          <p>{w.locations.length} candidates · {w.locations.filter((location) => location.review === "pending").length} pending review</p>
                        </div>
                        <span>↗</span>
                      </button>
                      <div className="context-note">
                        Accepted extraction, match scores, source independence
                        and analyst confidence represent different judgements.
                      </div>
                    </section>
                  </div>
                  <section className="panel">
                    <div className="panel-heading">
                      <h2>Evidence trail</h2>
                      <button
                        className="text-button"
                        onClick={() => navigate("Evidence")}
                      >
                        Open evidence register ↗
                      </button>
                    </div>
                    <EvidenceRows evidence={w.evidence} onOpen={setEvidence} />
                  </section>
                </>
              )}
              {section === "Evidence" && (
                <>
                  <section className="panel">
                    <div className="toolbar">
                      <input
                        aria-label="Search evidence"
                        placeholder="Search terms, phrase or Lucene query…"
                        value={query}
                        onChange={(e) => {
                          setQuery(e.target.value);
                          setSearchHits(null);
                        }}
                      />
                      <button
                        className="button"
                        disabled={busy || !query}
                        onClick={() => void searchCorpus()}
                      >
                        Search local index
                      </button>
                      <span>{evidenceList.length} source items</span>
                    </div>
                    <p className="muted">
                      Type to filter extracted text, or search the local Lucene
                      index with Boolean, phrase, proximity, fuzzy and fielded
                      queries. The packaged macOS development build includes the
                      local search runtime.
                    </p>
                    <EvidenceRows
                      evidence={evidenceList}
                      onOpen={setEvidence}
                    />
                    <p className="context-note">
                      UTF-8 text and mapped CSV/TSV statements are active.
                      Statement imports are previewed locally before
                      publication. Originals are retained separately. Document
                      jobs publish unreviewed derivatives with explicit parser
                      status and limitations.
                    </p>
                  </section>
                  <DocumentJobs
                    requestKeys={pendingDocumentRequests.current}
                    evidence={w.evidence}
                    busy={busy}
                    onRefresh={() => run({ action: "view" })}
                  />
                </>
              )}
              {section === "Entities" && (
                <EntityWorkbench
                  workspace={w}
                  busy={busy}
                  run={run}
                  selectedId={entityId}
                  onSource={setEvidence}
                  error={error}
                />
              )}
              {section === "Transactions" && (
                <>
                  <div className="stats">
                    {a.totals.length ? (
                      a.totals.map((t) => (
                        <Stat
                          key={t.currency}
                          label={`${t.currency} reviewed net`}
                          value={t.net}
                          detail={`${t.included_count} included · ${t.excluded_transfer_count} matched transfer rows excluded`}
                        />
                      ))
                    ) : (
                      <Stat
                        label="Reviewed totals"
                        value="—"
                        detail="Accept source rows to include them in calculations"
                      />
                    )}
                    <Stat
                      label="Pending review"
                      value={a.review_counts.pending}
                      detail="Excluded from reviewed totals"
                    />
                    <Stat
                      label="Balance discrepancies"
                      value={a.balance_discrepancy_count}
                      detail="Compared in source row order"
                    />
                  </div>
                  <TransactionLedger
                    revision={w.revision}
                    currency={currency}
                    review={reviewFilter}
                    pivot={ledgerPivot}
                    selectedId={selected?.row.id}
                    onInspect={inspectTransaction}
                    onScope={applyLedgerScope}
                    onRefresh={() => run({ action: "view" })}
                    download={download}
                  />
                  <TransactionComparison
                    workspace={w}
                    onInspect={inspectTransaction}
                    onRefresh={() => run({ action: "view" })}
                  />
                  <TransactionPatterns
                    workspace={w}
                    onInspect={inspectTransaction}
                    onRefresh={() => run({ action: "view" })}
                  />
                  {a.totals.length > 0 && (
                    <section className="panel">
                      <h2>Reviewed flow by currency</h2>
                      <TotalsChart analysis={a} onCurrency={selectCurrency} />
                      <p className="muted">
                        Chart positions use display approximations. The table
                        and report retain exact decimal amounts. Select a bar to
                        inspect that currency.
                      </p>
                    </section>
                  )}
                </>
              )}
              {section === "Relationships" && (
                <section className="panel">
                  <div className="panel-heading">
                    <h2>Entity relationships</h2>
                    <span className="pill">UNREVIEWED ASSERTIONS</span>
                  </div>
                  <Graph workspace={w} onSelect={selectEntity} />
                  <p className="context-note">
                    Dashed edges represent pending assertions. Select an entity
                    to inspect its observations. Merged records retain their
                    original identity and provenance.
                  </p>
                </section>
              )}
              {section === "Locations" && (
                <>
                  <div className="alert">
                    Regional basemap coverage is unavailable. This local
                    coordinate view makes no external tile requests.{" "}
                    {w.locations.length} merchant candidates remain unresolved.
                  </div>
                  <section className="panel">
                    <LocalMap workspace={w} />
                    <div className="map-legend">
                      <span>● Historical reference addresses</span>
                      <span>● Unresolved merchant branches</span>
                    </div>
                  </section>
                  <div className="grid-two">
                    <section className="panel">
                      <h2>Historical addresses</h2>
                      {w.addresses.map((ad) => (
                        <article className="list-card" key={ad.id}>
                          <h3>{ad.label}</h3>
                          <p>
                            {ad.valid_from} → {ad.valid_to ?? "present"}
                          </p>
                          <code>
                            {ad.latitude}, {ad.longitude}
                          </code>
                        </article>
                      ))}
                    </section>
                    <section className="panel">
                      <h2>Location assessment</h2>
                      <p>
                        Branches require their own supporting sources and valid
                        dates. An online payment does not establish physical
                        presence.
                      </p>
                      <div className="big-number">
                        0 <span>/ {w.locations.length}</span>
                      </div>
                      <p>
                        Merchant candidates accepted for distance classification
                      </p>
                      <p className="context-note">
                        The Rust domain layer uses geodesic distance with
                        uncertainty intervals and the applicable historical
                        address. The desktop review workflow is still under
                        development.
                      </p>
                    </section>
                  </div>
                </>
              )}
              {section === "Discovery" && (
                <>
                  <section className="panel">
                    <h2>Direct public-web collection</h2>
                    <p>
                      The selected URLs and normal connection metadata are
                      disclosed to those websites. Page links are followed only
                      on the selected hosts. No local case contents or search
                      terms are sent to a search provider.
                    </p>
                    <label>
                      Seed URLs, one per line
                      <textarea
                        aria-label="Seed URLs"
                        value={seeds}
                        onChange={(e) => {
                          setSeeds(e.target.value);
                          setPreviewed(false);
                        }}
                      />
                    </label>
                    <div className="scope-grid">
                      <div>
                        <strong>2 hops</strong>
                        <span>Maximum expansion</span>
                      </div>
                      <div>
                        <strong>50 requests</strong>
                        <span>Including robots and redirects</span>
                      </div>
                      <div>
                        <strong>10 minutes</strong>
                        <span>Maximum duration</span>
                      </div>
                    </div>
                    <div className="actions">
                      <button
                        className="button"
                        disabled={busy}
                        onClick={() => {
                          setPreviewed(true);
                        }}
                      >
                        Preview disclosure
                      </button>
                      {previewed && (
                        <button
                          className="button primary"
                          disabled={busy}
                          onClick={() =>
                            void run({
                              action: "collect_web",
                              urls: seeds
                                .split("\n")
                                .map((s) => s.trim())
                                .filter(Boolean),
                              max_hops: 2,
                              max_requests: 50,
                              max_seconds: 600,
                            })
                          }
                        >
                          Collect selected websites
                        </button>
                      )}
                    </div>
                    {previewed && (
                      <div className="disclosure">
                        <h3>Requests will be sent to</h3>
                        {seeds
                          .split("\n")
                          .filter(Boolean)
                          .map((url, i) => (
                            <code key={i}>
                              {url}
                              <br />
                            </code>
                          ))}
                        <p>
                          Collection honours robots rules, requires HTTPS and
                          stops at the first exhausted limit. Private, local and
                          special-use addresses are rejected. Public access and
                          robots permission do not grant republication rights.
                        </p>
                      </div>
                    )}
                  </section>
                  <CollectionHistory
                    workspace={w}
                    busy={busy}
                    onRefresh={() => run({ action: "view" })}
                  />
                  <div className="alert">
                    Search coverage is limited to the local corpus. Broad
                    open-web coverage has not been demonstrated; no third-party
                    search provider is configured.
                  </div>
                </>
              )}
              {section === "Assessment" && (
                <AssessmentWorkbench
                  workspace={w}
                  busy={busy}
                  error={error}
                  run={run}
                  onSource={setEvidence}
                  download={download}
                />
              )}
            </>
          )}
          <footer>
            Entity Workbench{" "}
            <span>Local by design. Evidence before conclusions.</span>
            <span>Security and complete packaging gates remain open.</span>
          </footer>
        </main>
      </div>
      {selected && w && (
        <TransactionReview
          key={
            selected.row.id +
            ":" +
            selected.row.version +
            ":" +
            selected.revision
          }
          selection={selected}
          currentRevision={w.revision}
          scope={ledgerScope}
          visibleIds={visibleTransactionIds}
          evidence={w.evidence}
          busy={busy}
          run={run}
          onSource={setEvidence}
          close={() => setSelected(null)}
        />
      )}
      {statementFile && w && (
        <StatementImport
          file={statementFile}
          profiles={w.statement_profiles}
          onClose={() => setStatementFile(null)}
          onImported={(response, count) => {
            publishSummary(response);
            setStatementFile(null);
            setSection("Transactions");
            setNotice(`Imported ${count} transactions as pending review.`);
          }}
        />
      )}
      {evidence && (
        <Dialog label="Evidence source" wide onClose={() => setEvidence(null)}>
          <button
            className="close"
            aria-label="Close source"
            onClick={() => setEvidence(null)}
          >
            ×
          </button>
          <p className="eyebrow">PRESERVED SOURCE</p>
          <h2>{evidence.name}</h2>
          <p className="hash">SHA-256 {evidence.sha256}</p>
          <span className="pill">
            {evidence.extraction_status === "unsupported_in_development_build"
              ? "Legacy import · not yet processed"
              : evidence.extraction_status.replaceAll("_", " ")}
          </span>
          {evidence.acquisitions.map((capture, i) => (
            <p className="hash" key={i}>
              Retrieved {capture.retrieved_at}
              <br />
              {capture.url}
            </p>
          ))}
          <SourceContent
            key={evidence.id + JSON.stringify(sourceAnchor)}
            evidence={evidence}
            anchor={sourceAnchor}
          />
        </Dialog>
      )}
    </div>
  );
}
function Stat({
  label,
  value,
  detail,
}: {
  label: string;
  value: string | number;
  detail: string;
}) {
  return (
    <div className="stat">
      <span>{label}</span>
      <strong>{value}</strong>
      <p>{detail}</p>
    </div>
  );
}
function EvidenceRows({
  evidence,
  onOpen,
}: {
  evidence: Evidence[];
  onOpen: (e: Evidence) => void;
}) {
  return (
    <div>
      {evidence.map((e, i) => (
        <button className="evidence-row" key={e.id} onClick={() => onOpen(e)}>
          <span className="file-icon">
            {e.media_type === "text/csv" ? "CSV" : "TXT"}
          </span>
          <div>
            <strong>{e.name}</strong>
            <small>
              Source {String(i + 1).padStart(2, "0")} ·{" "}
              {(e.bytes / 1024).toFixed(1)} KB ·{" "}
              {e.extraction_status === "unsupported_in_development_build"
                ? "Legacy import · not yet processed"
                : e.extraction_status.replaceAll("_", " ")}
            </small>
          </div>
          <code>{e.sha256.slice(0, 12)}…</code>
          <span>↗</span>
        </button>
      ))}
    </div>
  );
}
createRoot(document.getElementById("root")!).render(<App />);
