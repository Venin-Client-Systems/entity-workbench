import { command } from "./api";
import {
  digest,
  type LedgerScope,
  type Matching,
} from "./transaction-ledger-types";

export type CsvPolicy = "reject" | "allow_selected";
export type CsvRequest = {
  selection: Omit<LedgerScope, "page_size">;
  non_accepted: CsvPolicy;
};
type CsvColumn = {
  name: string;
  logical_type: string;
  prefix: string;
  nullable: boolean;
  meaning: string;
};
export type CsvDictionary = {
  schema_version: 1;
  encoding: string;
  delimiter: string;
  record_terminator: string;
  quote_policy: string;
  null_literal: string;
  columns: CsvColumn[];
  limitations: string[];
};
export type CsvIdentity = {
  workspace_revision: number;
  request: CsvRequest;
  matching: Matching;
  selection_sha256: string;
  format: "typed_literal_v1";
  format_sha256: string;
  dictionary: CsvDictionary;
  row_count: number;
  bytes: number;
  sha256: string;
};
export type CsvExport = CsvIdentity & { schema_version: 1; csv: string };
export const CSV_FORMAT = "typed_literal_v1" as const;
// Exact v1 Rust dictionary identity. Changing its meanings or column order needs
// an explicitly supported contract, not a silently accepted response dictionary.
const FORMAT_SHA256 =
  "a79b97802d789b1aa0e6828823aa18f5826bc03ac7613369f8ec0d943bcb73c4";
const MAX_BYTES = 256 * 1024 * 1024;
export function csvRequest(scope: LedgerScope, policy: CsvPolicy): CsvRequest {
  return {
    selection: { query: scope.query, filter: scope.filter, order: scope.order },
    non_accepted: policy,
  };
}
export const exactKeys = (value: unknown, names: string[]): boolean =>
  !!value &&
  typeof value === "object" &&
  !Array.isArray(value) &&
  Object.keys(value).sort().join(",") === [...names].sort().join(",");
export function equalContract(a: unknown, b: unknown): boolean {
  if (a === b) return true;
  if (Array.isArray(a) && Array.isArray(b))
    return a.length === b.length && a.every((v, i) => equalContract(v, b[i]));
  if (!a || !b || typeof a !== "object" || typeof b !== "object") return false;
  const aa = a as Record<string, unknown>,
    bb = b as Record<string, unknown>;
  return (
    Object.keys(aa).length === Object.keys(bb).length &&
    Object.keys(aa).every(
      (k) => Object.hasOwn(bb, k) && equalContract(aa[k], bb[k]),
    )
  );
}
const unsigned = (value: unknown): value is number =>
  Number.isSafeInteger(value) && Number(value) >= 0;
const boundedText = (value: unknown): value is string =>
  typeof value === "string" && value.length <= 1024;
export async function sha256(bytes: Uint8Array<ArrayBuffer>): Promise<string> {
  if (!globalThis.crypto?.subtle)
    throw new Error(
      "WebCrypto is unavailable; export integrity could not be verified.",
    );
  return Array.from(
    new Uint8Array(await crypto.subtle.digest("SHA-256", bytes)),
    (n) => n.toString(16).padStart(2, "0"),
  ).join("");
}
function sortedJson(value: unknown): unknown {
  if (Array.isArray(value)) return value.map(sortedJson);
  if (value && typeof value === "object")
    return Object.fromEntries(
      Object.entries(value)
        .sort(([a], [b]) => (a < b ? -1 : a > b ? 1 : 0))
        .map(([k, v]) => [k, sortedJson(v)]),
    );
  return value;
}
export async function validateCsvIdentity(
  value: CsvIdentity,
  request: CsvRequest,
  revision: number,
  count: number,
  matching: Matching,
) {
  const d = value?.dictionary;
  if (
    !value ||
    value.workspace_revision !== revision ||
    value.row_count !== count ||
    !unsigned(value.row_count) ||
    !equalContract(value.request, request) ||
    !equalContract(value.matching, matching) ||
    value.format !== CSV_FORMAT ||
    value.format_sha256 !== FORMAT_SHA256 ||
    !digest(value.selection_sha256) ||
    !digest(value.sha256) ||
    !unsigned(value.bytes) ||
    value.bytes > MAX_BYTES ||
    !exactKeys(d, [
      "schema_version",
      "encoding",
      "delimiter",
      "record_terminator",
      "quote_policy",
      "null_literal",
      "columns",
      "limitations",
    ]) ||
    d.schema_version !== 1 ||
    ![
      d.encoding,
      d.delimiter,
      d.record_terminator,
      d.quote_policy,
      d.null_literal,
    ].every(boundedText) ||
    !Array.isArray(d.columns) ||
    d.columns.length !== 15 ||
    d.columns.some(
      (c) =>
        !exactKeys(c, [
          "name",
          "logical_type",
          "prefix",
          "nullable",
          "meaning",
        ]) ||
        typeof c.nullable !== "boolean" ||
        ![c.name, c.logical_type, c.prefix, c.meaning].every(boundedText),
    ) ||
    !Array.isArray(d.limitations) ||
    d.limitations.length !== 5 ||
    !d.limitations.every(boundedText)
  )
    throw new Error(
      "CSV export identity, format, dictionary, scope or count did not match.",
    );
  const formatHash = await sha256(
    new TextEncoder().encode(JSON.stringify(sortedJson([value.format, d]))),
  );
  if (formatHash !== FORMAT_SHA256)
    throw new Error("CSV data dictionary did not match the supported format.");
}

/** Validate the fixed quoted transport format without numeric/date conversion or
 * materializing a second ledger. Rust remains the only transaction serializer. */
function validateCsvRows(
  csv: string,
  dictionary: CsvDictionary,
  expected: number,
) {
  if (!csv.startsWith("\ufeff"))
    throw new Error("CSV export is missing its UTF-8 marker.");
  let at = 1,
    rows = 0;
  while (at < csv.length) {
    for (let column = 0; column < dictionary.columns.length; column++) {
      if (csv[at++] !== '"')
        throw new Error("CSV export has an invalid quoted field.");
      const start = at;
      let closed = false;
      while (at < csv.length) {
        if (csv[at++] !== '"') continue;
        if (csv[at] === '"') {
          at++;
          continue;
        }
        closed = true;
        break;
      }
      if (!closed) throw new Error("CSV export is truncated.");
      const end = at - 1,
        definition = dictionary.columns[column];
      if (rows === 0) {
        if (csv.slice(start, end) !== definition.name)
          throw new Error("CSV export header does not match its dictionary.");
      } else {
        const missing =
          definition.nullable &&
          end - start === 4 &&
          csv.startsWith("null", start);
        if (!missing && !csv.startsWith(definition.prefix, start))
          throw new Error(
            "CSV export field is missing its declared literal prefix.",
          );
      }
      if (column < dictionary.columns.length - 1) {
        if (csv[at++] !== ",")
          throw new Error("CSV export column count did not match.");
      } else if (csv.slice(at, at + 2) !== "\r\n")
        throw new Error("CSV export record terminator did not match.");
      else at += 2;
    }
    rows++;
    if (rows > expected + 1)
      throw new Error("CSV export contains more rows than declared.");
  }
  if (rows !== expected + 1)
    throw new Error("CSV export row count did not match its content.");
}
export async function readTransactionCsv(
  scope: LedgerScope,
  policy: CsvPolicy,
  revision: number,
  count: number,
  matching: Matching,
) {
  const request = csvRequest(scope, policy);
  const value = await command<CsvExport>({
    action: "export_transaction_csv",
    request,
    expected_revision: revision,
  });
  if (
    !exactKeys(value, [
      "schema_version",
      "workspace_revision",
      "request",
      "matching",
      "selection_sha256",
      "format",
      "format_sha256",
      "dictionary",
      "row_count",
      "bytes",
      "sha256",
      "csv",
    ]) ||
    value.schema_version !== 1 ||
    typeof value.csv !== "string" ||
    value.csv.length > MAX_BYTES
  )
    throw new Error("CSV export response is unavailable or unsupported.");
  await validateCsvIdentity(value, request, revision, count, matching);
  const bytes = new TextEncoder().encode(value.csv);
  if (bytes.length !== value.bytes || (await sha256(bytes)) !== value.sha256)
    throw new Error("CSV export bytes did not match their identity.");
  validateCsvRows(value.csv, value.dictionary, count);
  return value;
}
