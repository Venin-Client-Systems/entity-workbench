import { useEffect, useRef, useState } from "react";
import { command } from "./api";
import { Dialog } from "./Dialog";
import type { Response } from "./types";
import type {
  Delimiter,
  StatementMapping,
  StatementPreview,
  StatementProfile,
  StatementSample,
  ValueMapping,
} from "./statement-types";

export type StatementFile = { name: string; bytes: number[] };
export function StatementImport({
  file,
  profiles,
  onClose,
  onImported,
}: {
  file: StatementFile;
  profiles: StatementProfile[];
  onClose: () => void;
  onImported: (response: Response, count: number) => void;
}) {
  const [delimiter, setDelimiter] = useState<Delimiter>(
    file.name.toLowerCase().endsWith(".tsv") ? "tab" : "comma",
  );
  const [sample, setSample] = useState<StatementSample | null>(null);
  const [mapping, setMapping] = useState<StatementMapping | null>(null);
  const [preview, setPreview] = useState<StatementPreview | null>(null);
  const [profileId, setProfileId] = useState("");
  const [saveProfile, setSaveProfile] = useState(false),
    [profileName, setProfileName] = useState("");
  const [error, setError] = useState("");
  const [phase, setPhase] = useState<
    "sampling" | "previewing" | "importing" | null
  >("sampling");
  const request = useRef(0);
  const previewHeading = useRef<HTMLHeadingElement>(null);
  useEffect(() => {
    const current = ++request.current;
    setPhase("sampling");
    setSample(null);
    setPreview(null);
    setError("");
    command<StatementSample>({
      action: "inspect_statement",
      bytes: file.bytes,
      delimiter,
    })
      .then((result) => {
        if (request.current === current) {
          setSample(result);
          setMapping((existing) => existing ?? result.suggested_mapping);
        }
      })
      .catch((e) => {
        if (request.current === current) setError(String(e));
      })
      .finally(() => {
        if (request.current === current) setPhase(null);
      });
    return () => {
      request.current++;
    };
  }, [file, delimiter]);
  useEffect(() => {
    if (preview) previewHeading.current?.focus();
  }, [preview]);
  const change = (next: StatementMapping) => {
    request.current++;
    setMapping(next);
    setPreview(null);
    setError("");
    setPhase(null);
  };
  const previewRows = async () => {
    if (!mapping) return;
    const current = ++request.current;
    setPhase("previewing");
    setError("");
    setPreview(null);
    try {
      const result = await command<StatementPreview>({
        action: "preview_statement",
        ...file,
        mapping,
      });
      if (request.current === current) setPreview(result);
    } catch (e) {
      if (request.current === current) setError(String(e));
    } finally {
      if (request.current === current) setPhase(null);
    }
  };
  const publish = async () => {
    if (!preview || !mapping) return;
    setPhase("importing");
    setError("");
    try {
      const result = await command<Response>({
        action: "import_statement",
        ...file,
        mapping,
        preview_token: preview.preview_token,
        expected_revision: preview.workspace_revision,
        save_profile_name: saveProfile ? profileName : null,
      });
      onImported(result, preview.valid_rows);
    } catch (e) {
      setError(String(e));
      setPreview(null);
    } finally {
      setPhase(null);
    }
  };
  const headers = sample?.headers ?? [];
  return (
    <Dialog
      label="Import statement"
      wide
      preventClose={phase === "importing"}
      onClose={onClose}
    >
      <div className="statement-import">
        <button
          className="close"
          aria-label="Close statement import"
          disabled={phase === "importing"}
          onClick={onClose}
        >
          ×
        </button>
        <p className="eyebrow">
          STATEMENT IMPORT / {preview ? "02 PREVIEW" : "01 MAP COLUMNS"}
        </p>
        <h2>{file.name}</h2>
        <p className="muted">
          Local CSV/TSV import. Original bytes are retained. Every transaction
          starts in pending review.
        </p>
        {error && (
          <p role="alert" className="alert error">
            {error}
          </p>
        )}
        {phase && (
          <p role="status" className="busy">
            {phase === "sampling"
              ? "Reading source columns…"
              : phase === "previewing"
                ? "Validating every source row…"
                : "Saving original evidence and pending transactions…"}
          </p>
        )}
        {!preview ? (
          <>
            <div className="form-grid">
              <label>
                Saved mapping
                <select
                  aria-label="Saved mapping"
                  value={profileId}
                  disabled={phase === "sampling"}
                  onChange={(e) => {
                    setProfileId(e.target.value);
                    setSaveProfile(false);
                    const chosen = profiles.find(
                      (p) => p.id === e.target.value,
                    );
                    if (chosen) {
                      change(chosen.mapping);
                      setDelimiter(chosen.mapping.delimiter);
                    } else if (sample) change(sample.suggested_mapping);
                  }}
                >
                  <option value="">New mapping</option>
                  {profiles.map((p) => (
                    <option key={p.id} value={p.id}>
                      {p.name}
                    </option>
                  ))}
                </select>
              </label>
              <label>
                Column separator
                <select
                  aria-label="Column separator"
                  value={delimiter}
                  onChange={(e) => {
                    request.current++;
                    setMapping(null);
                    setPreview(null);
                    setProfileId("");
                    setDelimiter(e.target.value as Delimiter);
                  }}
                >
                  <option value="comma">Comma</option>
                  <option value="semicolon">Semicolon</option>
                  <option value="tab">Tab</option>
                </select>
              </label>
            </div>
            {sample && (
              <details className="source-sample" open>
                <summary>
                  Original source sample · first five logical rows · cells
                  shortened at 240 characters
                </summary>
                <div
                  className="table-scroll"
                  role="region"
                  tabIndex={0}
                  aria-label="Statement source sample"
                >
                  <table>
                    <caption>Source columns before interpretation</caption>
                    <thead>
                      <tr>
                        {headers.map((h) => (
                          <th key={h}>{h}</th>
                        ))}
                      </tr>
                    </thead>
                    <tbody>
                      {sample.sample_rows.map((r, i) => (
                        <tr key={i}>
                          {r.map((cell, j) => (
                            <td key={j}>{cell}</td>
                          ))}
                        </tr>
                      ))}
                    </tbody>
                  </table>
                </div>
              </details>
            )}
            {mapping && sample && (
              <form
                onSubmit={(e) => {
                  e.preventDefault();
                  void previewRows();
                }}
              >
                <fieldset>
                  <legend>Interpretation</legend>
                  <div className="form-grid">
                    <label>
                      Date format
                      <select
                        aria-label="Statement date format"
                        value={mapping.date_format}
                        onChange={(e) =>
                          change({
                            ...mapping,
                            date_format: e.target
                              .value as StatementMapping["date_format"],
                          })
                        }
                      >
                        <option value="iso">YYYY-MM-DD</option>
                        <option value="day_first">DD/MM/YYYY</option>
                        <option value="month_first">MM/DD/YYYY</option>
                      </select>
                    </label>
                    <label>
                      Number format
                      <select
                        aria-label="Statement number format"
                        value={mapping.number_format}
                        onChange={(e) =>
                          change({
                            ...mapping,
                            number_format: e.target
                              .value as StatementMapping["number_format"],
                          })
                        }
                      >
                        <option value="dot_decimal">
                          1,234.56 — dot decimal
                        </option>
                        <option value="comma_decimal">
                          1.234,56 — comma decimal
                        </option>
                      </select>
                    </label>
                    <label>
                      Source row order
                      <select
                        aria-label="Source row order"
                        value={mapping.row_order}
                        onChange={(e) =>
                          change({
                            ...mapping,
                            row_order: e.target
                              .value as StatementMapping["row_order"],
                          })
                        }
                      >
                        <option value="oldest_first">Oldest first</option>
                        <option value="newest_first">Newest first</option>
                      </select>
                    </label>
                    <label>
                      Amount interpretation
                      <select
                        aria-label="Amount interpretation"
                        value={
                          mapping.amount.kind === "debit_credit"
                            ? "separate"
                            : mapping.amount.positive_is_debit
                              ? "positive_debits"
                              : "negative_debits"
                        }
                        onChange={(e) =>
                          change({
                            ...mapping,
                            amount:
                              e.target.value === "separate"
                                ? {
                                    kind: "debit_credit",
                                    debit: "",
                                    credit: "",
                                  }
                                : {
                                    kind: "signed",
                                    column:
                                      mapping.amount.kind === "signed"
                                        ? mapping.amount.column
                                        : "",
                                    positive_is_debit:
                                      e.target.value === "positive_debits",
                                  },
                          })
                        }
                      >
                        <option value="negative_debits">
                          Signed amount · negative means debit
                        </option>
                        <option value="positive_debits">
                          Signed amount · positive means debit
                        </option>
                        <option value="separate">
                          Separate debit and credit columns
                        </option>
                      </select>
                    </label>
                  </div>
                </fieldset>
                <fieldset>
                  <legend>Source columns</legend>
                  <div className="form-grid">
                    <Column
                      label="Transaction date column"
                      value={mapping.date}
                      headers={headers}
                      onChange={(date) => change({ ...mapping, date })}
                    />
                    <Column
                      label="Posting date column"
                      value={mapping.posting_date ?? ""}
                      headers={headers}
                      optional
                      onChange={(posting_date) =>
                        change({
                          ...mapping,
                          posting_date: posting_date || null,
                        })
                      }
                    />
                    <Column
                      label="Description column"
                      value={mapping.description}
                      headers={headers}
                      onChange={(description) =>
                        change({ ...mapping, description })
                      }
                    />
                    <Column
                      label="Balance column"
                      value={mapping.balance ?? ""}
                      headers={headers}
                      optional
                      onChange={(balance) =>
                        change({ ...mapping, balance: balance || null })
                      }
                    />
                    {mapping.amount.kind === "signed" ? (
                      <Column
                        label="Amount column"
                        value={mapping.amount.column}
                        headers={headers}
                        onChange={(column) => {
                          if (mapping.amount.kind === "signed")
                            change({
                              ...mapping,
                              amount: { ...mapping.amount, column },
                            });
                        }}
                      />
                    ) : (
                      <>
                        <Column
                          label="Debit column"
                          value={mapping.amount.debit}
                          headers={headers}
                          onChange={(debit) => {
                            if (mapping.amount.kind === "debit_credit")
                              change({
                                ...mapping,
                                amount: { ...mapping.amount, debit },
                              });
                          }}
                        />
                        <Column
                          label="Credit column"
                          value={mapping.amount.credit}
                          headers={headers}
                          onChange={(credit) => {
                            if (mapping.amount.kind === "debit_credit")
                              change({
                                ...mapping,
                                amount: { ...mapping.amount, credit },
                              });
                          }}
                        />
                      </>
                    )}
                  </div>
                </fieldset>
                <div className="form-grid">
                  <ValueField
                    label="Account"
                    value={mapping.account}
                    headers={headers}
                    onChange={(account) => change({ ...mapping, account })}
                  />
                  <ValueField
                    label="Currency"
                    value={mapping.currency}
                    headers={headers}
                    onChange={(currency) => change({ ...mapping, currency })}
                  />
                </div>
                <p className="context-note">
                  Dates and signs are interpreted only by these settings.
                  Separate debit/credit values must be nonnegative. Unresolved
                  rows block the entire import.
                </p>
                <div className="actions">
                  <button
                    className="button primary"
                    type="submit"
                    disabled={phase !== null}
                  >
                    Preview all rows
                  </button>
                </div>
              </form>
            )}
          </>
        ) : (
          <>
            <h3 ref={previewHeading} tabIndex={-1}>
              Review interpreted transactions
            </h3>
            <div className="stats">
              <div className="stat">
                <span>Source rows</span>
                <strong>{preview.total_rows}</strong>
              </div>
              <div className="stat">
                <span>Rows with errors</span>
                <strong>{preview.invalid_rows}</strong>
              </div>
              <div className="stat">
                <span>Balance mismatches</span>
                <strong>{preview.balance_mismatches}</strong>
              </div>
            </div>
            <p className="hash">Original SHA-256 {preview.sha256}</p>
            {preview.already_imported && (
              <p className="alert" role="alert">
                This original is already retained. Importing it again cannot add
                duplicate transactions.
              </p>
            )}
            {preview.invalid_rows > 0 && (
              <div className="alert error" role="alert">
                <strong>
                  No rows will be imported until all errors are resolved.
                </strong>
                <ul>
                  {preview.issues.map((i) => (
                    <li key={i.source_row}>
                      Row {i.source_row}: {i.message}
                    </li>
                  ))}
                </ul>
                {preview.issues_truncated && (
                  <p>Only the first 100 errors are listed.</p>
                )}
              </div>
            )}
            {preview.balance_mismatches > 0 && (
              <p className="alert">
                Available balances do not reconcile for{" "}
                {preview.balance_mismatches} rows. These checks remain visible
                after import; the first available balance has no preceding
                balance to compare.
              </p>
            )}
            <div
              className="table-scroll"
              role="region"
              tabIndex={0}
              aria-label="Statement import preview"
            >
              <table>
                <caption>
                  {preview.rows_truncated
                    ? "First 50 rows shown; every source row was validated."
                    : "Every source row shown."}{" "}
                  Amounts are exact decimals in their original currency.
                </caption>
                <thead>
                  <tr>
                    <th>Source row</th>
                    <th>Date</th>
                    <th>Description</th>
                    <th>Original amount cells</th>
                    <th className="numeric">Interpreted amount</th>
                    <th>Account</th>
                  </tr>
                </thead>
                <tbody>
                  {preview.rows.map((r) => (
                    <tr key={r.source_row}>
                      <td>{r.source_row}</td>
                      <td>
                        {r.transaction?.date ?? "—"}
                        <small>
                          {r.transaction?.posting_date
                            ? `Posted ${r.transaction.posting_date}`
                            : ""}
                        </small>
                      </td>
                      <td>{r.transaction?.description ?? r.error}</td>
                      <td>
                        {r.original_amounts.map(([column, value]) => (
                          <div key={column}>
                            <small>{column}</small>
                            <code>{value || "(empty)"}</code>
                          </div>
                        ))}
                      </td>
                      <td className="numeric">
                        {r.transaction?.amount ?? "—"}
                        <small>{r.transaction?.currency}</small>
                      </td>
                      <td>
                        <code>{r.transaction?.account}</code>
                      </td>
                    </tr>
                  ))}
                </tbody>
              </table>
            </div>
            <fieldset disabled={phase === "importing"} className="profile-save">
              <legend>Reuse this mapping</legend>
              <label className="check-label">
                <input
                  type="checkbox"
                  checked={saveProfile}
                  onChange={(e) => setSaveProfile(e.target.checked)}
                />{" "}
                Save as a reusable mapping
              </label>
              {saveProfile && (
                <label>
                  Mapping name
                  <input
                    aria-label="Mapping name"
                    value={profileName}
                    maxLength={100}
                    onChange={(e) => setProfileName(e.target.value)}
                  />
                </label>
              )}
            </fieldset>
            <div className="actions">
              <button
                className="button"
                disabled={phase === "importing"}
                onClick={() => setPreview(null)}
              >
                Edit mapping
              </button>
              <button
                className="button primary"
                disabled={
                  phase !== null ||
                  preview.invalid_rows > 0 ||
                  preview.already_imported ||
                  (saveProfile && !profileName.trim())
                }
                onClick={() => void publish()}
              >
                Import {preview.valid_rows} pending transactions
              </button>
            </div>
            <p className="context-note">
              Repeated purchases and overlapping source rows are retained and
              flagged for review. Importing does not accept transactions or
              include them in reviewed totals.
            </p>
          </>
        )}
      </div>
    </Dialog>
  );
}
function Column({
  label,
  value,
  headers,
  optional,
  onChange,
}: {
  label: string;
  value: string;
  headers: string[];
  optional?: boolean;
  onChange: (value: string) => void;
}) {
  return (
    <label>
      {label}
      <select
        aria-label={label}
        value={value}
        onChange={(e) => onChange(e.target.value)}
      >
        <option value="">
          {optional ? "Not available" : "Select a column"}
        </option>
        {headers.map((h) => (
          <option key={h} value={h}>
            {h}
          </option>
        ))}
      </select>
    </label>
  );
}
function ValueField({
  label,
  value,
  headers,
  onChange,
}: {
  label: string;
  value: ValueMapping;
  headers: string[];
  onChange: (value: ValueMapping) => void;
}) {
  return (
    <fieldset>
      <legend>{label}</legend>
      <label>
        {label} source
        <select
          aria-label={`${label} source`}
          value={value.kind}
          onChange={(e) =>
            onChange(
              e.target.value === "column"
                ? { kind: "column", column: "" }
                : { kind: "constant", value: "" },
            )
          }
        >
          <option value="column">Source column</option>
          <option value="constant">One fixed value</option>
        </select>
      </label>
      {value.kind === "column" ? (
        <Column
          label={`${label} column`}
          value={value.column}
          headers={headers}
          onChange={(column) => onChange({ kind: "column", column })}
        />
      ) : (
        <label>
          {label} value
          <input
            aria-label={`${label} value`}
            value={value.value}
            maxLength={label === "Currency" ? 3 : 300}
            onChange={(e) =>
              onChange({ kind: "constant", value: e.target.value })
            }
          />
        </label>
      )}
    </fieldset>
  );
}
