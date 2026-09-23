import React, { useCallback, useEffect, useRef, useState } from "react";
import { createRoot } from "react-dom/client";
import { command } from "./api";
import type {
  Response,
  Transaction,
  Evidence,
  Anchor,
  SourceExcerpt,
} from "./types";
import { Graph, LocalMap, TotalsChart } from "./Visuals";
import { AssessmentWorkbench } from "./AssessmentWorkbench";
import { Dialog } from "./Dialog";
import { ReviewSurface } from "./ReviewSurface";
import { EntityWorkbench } from "./EntityWorkbench";
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
  const [data, setData] = useState<Response | null>(null),
    [section, setSection] = useState<Section>("Overview"),
    [error, setError] = useState(""),
    [notice, setNotice] = useState(""),
    [busy, setBusy] = useState(false);
  const [query, setQuery] = useState(""),
    [reviewFilter, setReviewFilter] = useState("all"),
    [currency, setCurrency] = useState("all"),
    [selected, setSelected] = useState<Transaction | null>(null),
    [evidence, updateEvidence] = useState<Evidence | null>(null);
  const [sourceAnchor, setSourceAnchor] = useState<Anchor | undefined>(
    undefined,
  );
  const setEvidence = (item: Evidence | null, anchor?: Anchor) => {
    updateEvidence(item);
    setSourceAnchor(anchor);
  };
  const [why, setWhy] = useState(""),
    [corrected, setCorrected] = useState(""),
    [transfer, setTransfer] = useState(""),
    [entityId, setEntityId] = useState(""),
    [seeds, setSeeds] = useState("https://example.com/"),
    [previewed, setPreviewed] = useState(false);
  const [searchHits, setSearchHits] = useState<
    { id: string; name: string; score: number }[] | null
  >(null);
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
  const run = useCallback(async (action: Record<string, unknown>) => {
    setBusy(true);
    setError("");
    setNotice("");
    try {
      const response = await command<Response>(action);
      if (response.workspace) setData(response);
      else
        setNotice(
          "Recoverable backup saved in the workspace backups directory.",
        );
      return true;
    } catch (e) {
      setError(String(e));
      return false;
    } finally {
      setBusy(false);
    }
  }, []);
  useEffect(() => {
    void run({ action: "view" });
  }, [run]);
  const navigate = (s: Section) => {
    setSelected(null);
    setSection(s);
    setQuery("");
    setSearchHits(null);
  };
  const inspectTransaction = (t: Transaction) => {
    setSelected(t);
    setWhy("");
    setCorrected(t.amount);
    setTransfer("");
  };
  const selectEntity = useCallback((id: string) => {
    setEntityId(id);
    setSection("Entities");
  }, []);
  const selectCurrency = useCallback((c: string) => {
    setCurrency(c);
    setSection("Transactions");
  }, []);
  const download = (content: string, name: string, type: string) => {
    const url = URL.createObjectURL(new Blob([content], { type }));
    const a = document.createElement("a");
    a.href = url;
    a.download = name;
    a.click();
    setTimeout(() => URL.revokeObjectURL(url), 1000);
  };
  const w = data?.workspace,
    a = data?.analysis;
  const act = async (action: Record<string, unknown>) => {
    const ok = await run({ ...action, expected_revision: w?.revision });
    if (ok) {
      setSelected(null);
      setWhy("");
    }
  };
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
  const transactions =
    w?.transactions.filter(
      (t) =>
        (reviewFilter === "all" || t.review === reviewFilter) &&
        (currency === "all" || t.currency === currency) &&
        `${t.description} ${t.account} ${t.date}`
          .toLowerCase()
          .includes(query.toLowerCase()),
    ) ?? [];
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
              {s === "Transactions" && a && <small>{a.pending}</small>}
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
              {w.entities.length === 0 && (
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
                      value={a.pending}
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
                          {a.balance_checks.filter((c) => !c.reconciled)
                            .length + a.duplicate_candidates}
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
                            {
                              a.balance_checks.filter((c) => !c.reconciled)
                                .length
                            }{" "}
                            balance discrepancies · {a.duplicate_candidates}{" "}
                            possible duplicate rows
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
                          <h3>Namesake comparison</h3>
                          <p>
                            Conflicting birth years remain separate observations
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
                          <h3>Branch ambiguity</h3>
                          <p>No location selected by proximity</p>
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
                  <EvidenceRows evidence={evidenceList} onOpen={setEvidence} />
                  <p className="context-note">
                    UTF-8 text and mapped CSV/TSV statements are active.
                    Statement imports are previewed locally before publication.
                    Other formats are preserved and labelled unsupported until
                    isolated parsing workers are available.
                  </p>
                </section>
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
                          detail={`${t.transaction_ids.length} included · ${t.excluded_transfer_ids.length} matched transfer rows excluded`}
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
                      value={a.pending}
                      detail="Excluded from reviewed totals"
                    />
                    <Stat
                      label="Balance discrepancies"
                      value={
                        a.balance_checks.filter((c) => !c.reconciled).length
                      }
                      detail="Compared in source row order"
                    />
                  </div>
                  <section className="panel">
                    <div className="toolbar">
                      <input
                        aria-label="Filter transactions"
                        placeholder="Description, account or date…"
                        value={query}
                        onChange={(e) => setQuery(e.target.value)}
                      />
                      <select
                        aria-label="Review filter"
                        value={reviewFilter}
                        onChange={(e) => setReviewFilter(e.target.value)}
                      >
                        {[
                          "all",
                          "pending",
                          "accepted",
                          "rejected",
                          "deferred",
                        ].map((v) => (
                          <option key={v}>{v}</option>
                        ))}
                      </select>
                      <select
                        aria-label="Currency filter"
                        value={currency}
                        onChange={(e) => setCurrency(e.target.value)}
                      >
                        {[
                          "all",
                          ...new Set(w.transactions.map((t) => t.currency)),
                        ].map((v) => (
                          <option key={v}>{v}</option>
                        ))}
                      </select>
                      <button
                        className="button subtle"
                        onClick={() =>
                          download(
                            JSON.stringify(transactions, null, 2),
                            "transactions.json",
                            "application/json",
                          )
                        }
                      >
                        Export JSON
                      </button>
                    </div>
                    <div
                      className="table-scroll"
                      role="region"
                      aria-label="Transaction ledger"
                      tabIndex={0}
                    >
                      <table>
                        <caption>
                          {transactions.length} transactions in the current view
                          · amounts retain their original currency
                        </caption>
                        <thead>
                          <tr>
                            <th>Date</th>
                            <th>Original description</th>
                            <th>Account</th>
                            <th className="numeric">Amount</th>
                            <th>Checks</th>
                            <th>Review</th>
                          </tr>
                        </thead>
                        <tbody>
                          {transactions.map((t) => (
                            <tr
                              key={t.id}
                              className={
                                selected?.id === t.id
                                  ? "selected-row"
                                  : undefined
                              }
                              onClick={() => inspectTransaction(t)}
                            >
                              <td>{t.date}</td>
                              <td>
                                <button
                                  className="cell-button"
                                  id={`transaction-${t.id}`}
                                  onClick={() => inspectTransaction(t)}
                                >
                                  {t.description}
                                </button>
                                <small>
                                  {t.posting_date
                                    ? `Posted ${t.posting_date}`
                                    : ""}
                                </small>
                              </td>
                              <td>
                                <code>{t.account}</code>
                              </td>
                              <td className="numeric">
                                <strong>{t.amount}</strong>
                                <small>{t.currency}</small>
                              </td>
                              <td>
                                {t.duplicate_candidates.length > 0 && (
                                  <span className="pill warning">
                                    Possible duplicate
                                  </span>
                                )}
                                {a.balance_checks.some(
                                  (c) =>
                                    c.transaction_id === t.id && !c.reconciled,
                                ) && (
                                  <span className="pill warning">
                                    Balance mismatch
                                  </span>
                                )}
                                {t.transfer_peer && (
                                  <span className="pill">Matched transfer</span>
                                )}
                              </td>
                              <td>
                                <span className={`pill ${t.review}`}>
                                  {t.review}
                                </span>
                              </td>
                            </tr>
                          ))}
                        </tbody>
                      </table>
                    </div>
                    <p className="context-note">
                      Repeated purchases are retained. No currency conversion is
                      performed. Select a row to inspect its source, review it
                      or propose a correction.
                    </p>
                  </section>
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
                  <section className="panel">
                    <h2>Collection history</h2>
                    {w.jobs.length === 0 ? (
                      <p className="muted">
                        No collection jobs have run. Local indexing requires
                        collected or imported sources.
                      </p>
                    ) : (
                      w.jobs.map((j) => (
                        <article className="list-card" key={j.id}>
                          <span className="pill">
                            {j.state.replaceAll("_", " ")}
                          </span>
                          <h3>{j.queries.join(", ")}</h3>
                          <p>{j.detail}</p>
                          <small>
                            {j.requests_used} / {j.max_requests} requests
                          </small>
                        </article>
                      ))
                    )}
                  </section>
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
        <ReviewSurface
          onClose={() => setSelected(null)}
          restoreFocus={() =>
            document.getElementById(`transaction-${selected.id}`)?.focus()
          }
        >
          <button
            className="close"
            aria-label="Close review"
            onClick={() => setSelected(null)}
          >
            ×
          </button>
          <p className="eyebrow">
            TRANSACTION REVIEW · VERSION {selected.version}
          </p>
          <h2>{selected.description}</h2>
          <div className="amount-large">
            {selected.amount} <span>{selected.currency}</span>
          </div>
          <p>
            {selected.date} · {selected.account} · {selected.review}
          </p>
          {!transactions.some((t) => t.id === selected.id) && (
            <p className="alert" role="status">
              Selected transaction is outside the current filters.
            </p>
          )}
          <div className="disclosure">
            <strong>Source anchor</strong>
            <p>
              {selected.anchor.sheet}, row {selected.anchor.row}, column{" "}
              {selected.anchor.column}
            </p>
            <button
              className="text-button"
              onClick={() =>
                setEvidence(
                  w.evidence.find(
                    (e) => e.id === selected.anchor.evidence_id,
                  ) ?? null,
                  selected.anchor,
                )
              }
            >
              Inspect preserved source ↗
            </button>
          </div>
          <TransactionExcerpt
            key={selected.id + ":" + selected.version}
            transaction={selected}
          />
          <label>
            Decision reason
            <input
              aria-label="Transaction decision reason"
              value={why}
              onChange={(e) => setWhy(e.target.value)}
            />
          </label>
          <div className="actions">
            {(["accepted", "rejected", "deferred"] as const).map((state) => (
              <button
                className={`button ${state === "accepted" ? "primary" : ""}`}
                key={state}
                disabled={busy || !why || !!selected.transfer_peer}
                onClick={() =>
                  void act({
                    action: "review_transaction",
                    id: selected.id,
                    state,
                    reason: why,
                  })
                }
              >
                {state === "accepted"
                  ? "Accept"
                  : state === "rejected"
                    ? "Reject"
                    : "Defer"}
              </button>
            ))}
          </div>
          <hr />
          <label>
            Corrected amount
            <input
              aria-label="Corrected amount"
              value={corrected}
              onChange={(e) => setCorrected(e.target.value)}
            />
          </label>
          <button
            className="button"
            disabled={busy || !why || corrected === selected.amount}
            onClick={() =>
              void act({
                action: "correct_transaction",
                id: selected.id,
                amount: corrected,
                reason: why,
              })
            }
          >
            Save correction for review
          </button>
          <p className="muted">
            Original evidence stays intact. A correction returns the transaction
            to pending review and invalidates dependent findings.
          </p>
          <hr />
          <label>
            Internal transfer counterpart
            <select
              aria-label="Transfer counterpart"
              value={transfer}
              onChange={(e) => setTransfer(e.target.value)}
            >
              <option value="">Select a reviewed transaction</option>
              {w.transactions
                .filter(
                  (t) =>
                    t.id !== selected.id &&
                    t.account !== selected.account &&
                    t.review === "accepted",
                )
                .map((t) => (
                  <option key={t.id} value={t.id}>
                    {t.account} · {t.description} · {t.amount} {t.currency}
                  </option>
                ))}
            </select>
          </label>
          <button
            className="button"
            disabled={busy || !why || !transfer}
            onClick={() =>
              void act({
                action: "match_transfer",
                first: selected.id,
                second: transfer,
                reason: why,
              })
            }
          >
            Match internal transfer
          </button>
        </ReviewSurface>
      )}
      {statementFile && w && (
        <StatementImport
          file={statementFile}
          profiles={w.statement_profiles}
          onClose={() => setStatementFile(null)}
          onImported={(response, count) => {
            setData(response);
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
            {evidence.extraction_status.replaceAll("_", " ")}
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
              {e.extraction_status.replaceAll("_", " ")}
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

function TransactionExcerpt({ transaction }: { transaction: Transaction }) {
  const [excerpt, setExcerpt] = useState<SourceExcerpt | null>(null);
  const [error, setError] = useState("");
  useEffect(() => {
    let active = true;
    void command<SourceExcerpt>({
      action: "inspect_source",
      anchor: transaction.anchor,
    })
      .then((value) => {
        if (active) setExcerpt(value);
      })
      .catch((value) => {
        if (active) setError(String(value));
      });
    return () => {
      active = false;
    };
  }, [transaction.anchor]);
  return (
    <section
      className="original-excerpt"
      aria-label="Original transaction excerpt"
    >
      <h3>Preserved source value</h3>
      {error ? (
        <p role="alert">{error}</p>
      ) : excerpt ? (
        <>
          <pre className="source-quote">{excerpt.quote}</pre>
          <p className="muted">{excerpt.location} · original evidence</p>
        </>
      ) : (
        <p role="status">Resolving source anchor…</p>
      )}
    </section>
  );
}
