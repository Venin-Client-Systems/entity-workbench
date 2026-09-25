import { invoke, isTauri } from "@tauri-apps/api/core";
import {
  digest,
  type LedgerScope,
  type Matching,
} from "./transaction-ledger-types";
import {
  CSV_FORMAT,
  csvRequest,
  validateCsvIdentity,
  type CsvIdentity,
  type CsvPolicy,
} from "./transaction-csv";
import type { Workspace } from "./types";
import { docxUuid, type DocxSnapshot } from "./docx-snapshot-types";
export { isTauri as nativeExportsAvailable };

type TransactionArtifact = {
  kind: "transactions";
  workspace_revision: number;
  request: Omit<LedgerScope, "page_size">;
  matching: Matching;
  query_sha256: string;
  row_count: number;
  bytes: number;
  sha256: string;
};
type ReportArtifact = {
  kind: "html_report";
  report_id: string;
  workspace_revision: number;
  bytes: number;
  sha256: string;
};
type DocxArtifact = {
  kind: "docx_report";
  report_id: string;
  workspace_revision: number;
  document_sha256: string;
  bytes: number;
  sha256: string;
};
type CsvArtifact = CsvIdentity & { kind: "transaction_csv" };
type Artifact =
  TransactionArtifact | ReportArtifact | DocxArtifact | CsvArtifact;
export type PreparedNativeExport = {
  schema_version: 1 | 2;
  ticket: string;
  expires_after_seconds: 120;
  artifact: Artifact;
};
export type SavedNativeExport = {
  schema_version: 1 | 2;
  ticket: string;
  artifact: Artifact;
  filename: string;
  location: string;
};
const uuid = (v: unknown): v is string =>
  typeof v === "string" &&
  /^[0-9a-f]{8}-[0-9a-f]{4}-4[0-9a-f]{3}-[89ab][0-9a-f]{3}-[0-9a-f]{12}$/.test(
    v,
  );
const integer = (v: unknown): v is number =>
  Number.isSafeInteger(v) && Number(v) >= 0;
function keys(value: unknown, names: string[]): boolean {
  return (
    typeof value === "object" &&
    value !== null &&
    !Array.isArray(value) &&
    Object.keys(value).sort().join(",") === names.sort().join(",")
  );
}
function artifactValid(a: Artifact): boolean {
  if (
    !a ||
    !integer(a.bytes) ||
    a.bytes > 256 * 1024 * 1024 ||
    !digest(a.sha256) ||
    !integer(a.workspace_revision)
  )
    return false;
  if (a.kind === "transaction_csv")
    return (
      keys(a, [
        "kind",
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
      ]) &&
      integer(a.row_count) &&
      digest(a.selection_sha256) &&
      digest(a.format_sha256) &&
      a.format === CSV_FORMAT
    );
  if (a.kind === "docx_report")
    return (
      keys(a, [
        "kind",
        "report_id",
        "workspace_revision",
        "document_sha256",
        "bytes",
        "sha256",
      ]) &&
      docxUuid(a.report_id) &&
      digest(a.document_sha256) &&
      a.bytes > 0 &&
      a.bytes <= 32 * 1024 * 1024
    );
  return a.kind === "html_report"
    ? keys(a, ["kind", "report_id", "workspace_revision", "bytes", "sha256"]) &&
        uuid(a.report_id)
    : a.kind === "transactions" &&
        keys(a, [
          "kind",
          "workspace_revision",
          "request",
          "matching",
          "query_sha256",
          "row_count",
          "bytes",
          "sha256",
        ]) &&
        integer(a.row_count) &&
        digest(a.query_sha256);
}
function preparedValid(p: PreparedNativeExport): boolean {
  return (
    keys(p, [
      "schema_version",
      "ticket",
      "expires_after_seconds",
      "artifact",
    ]) &&
    p.schema_version === (p.artifact?.kind === "transaction_csv" ? 2 : 1) &&
    uuid(p.ticket) &&
    p.expires_after_seconds === 120 &&
    artifactValid(p.artifact)
  );
}
const same = (a: unknown, b: unknown): boolean => {
  if (a === b) return true;
  if (Array.isArray(a) && Array.isArray(b))
    return a.length === b.length && a.every((v, i) => same(v, b[i]));
  if (a && b && typeof a === "object" && typeof b === "object") {
    const aa = a as Record<string, unknown>,
      bb = b as Record<string, unknown>;
    return (
      Object.keys(aa).length === Object.keys(bb).length &&
      Object.keys(aa).every((k) => Object.hasOwn(bb, k) && same(aa[k], bb[k]))
    );
  }
  return false;
};
async function prepare(
  request: Record<string, unknown>,
  accepts: (a: Artifact) => boolean | Promise<boolean>,
): Promise<PreparedNativeExport> {
  const value = await invoke<PreparedNativeExport>("prepare_native_export", {
    request,
  });
  let accepted = false;
  try {
    accepted = preparedValid(value) && (await accepts(value.artifact));
  } catch {
    /* Invalid metadata must discard its stage before rejection. */
  }
  if (!accepted) {
    if (uuid(value?.ticket)) await discardNativeExport(value.ticket);
    throw new Error(
      "Prepared native export identity did not match the selected scope.",
    );
  }
  return value;
}
export function prepareNativeTransactions(
  scope: LedgerScope,
  revision: number,
  count: number,
  matching: Matching,
) {
  const request = {
    query: scope.query,
    filter: scope.filter,
    order: scope.order,
  };
  return prepare(
    {
      kind: "transactions",
      request,
      expected_revision: revision,
      expected_row_count: count,
      expected_matching: matching,
    },
    (a) =>
      a.kind === "transactions" &&
      a.workspace_revision === revision &&
      a.row_count === count &&
      same(a.request, request) &&
      same(a.matching, matching),
  );
}
export function prepareNativeCsv(
  scope: LedgerScope,
  policy: CsvPolicy,
  revision: number,
  count: number,
  matching: Matching,
) {
  const request = csvRequest(scope, policy);
  return prepare(
    {
      kind: "transaction_csv",
      request,
      expected_revision: revision,
      expected_row_count: count,
      expected_matching: matching,
      expected_format: CSV_FORMAT,
    },
    async (a) => {
      if (a.kind !== "transaction_csv") return false;
      await validateCsvIdentity(a, request, revision, count, matching);
      return true;
    },
  );
}
export function prepareNativeReport(report: Workspace["reports"][number]) {
  return prepare(
    {
      kind: "html_report",
      report_id: report.id,
      expected_sha256: report.sha256,
    },
    (a) =>
      a.kind === "html_report" &&
      a.report_id === report.id &&
      a.sha256 === report.sha256 &&
      a.bytes === report.html_bytes &&
      a.workspace_revision === report.workspace_revision,
  );
}
export function prepareNativeDocx(report: DocxSnapshot) {
  return prepare(
    {
      kind: "docx_report",
      report_id: report.id,
      expected_document_sha256: report.document.sha256,
      expected_docx_sha256: report.docx.sha256,
    },
    (a) =>
      a.kind === "docx_report" &&
      a.report_id === report.id &&
      a.workspace_revision === report.workspace_revision &&
      a.document_sha256 === report.document.sha256 &&
      a.sha256 === report.docx.sha256 &&
      a.bytes === report.docx.bytes,
  );
}
export async function commitNativeExport(
  prepared: PreparedNativeExport,
): Promise<SavedNativeExport> {
  const args = {
    ticket: prepared.ticket,
    expectedSha256: prepared.artifact.sha256,
    expectedBytes: prepared.artifact.bytes,
  };
  // The same ticket is idempotent. One retry can recover a lost acknowledgement;
  // it can never generate a new artifact or choose a different destination.
  const result = await invoke<SavedNativeExport>(
    "commit_native_export",
    args,
  ).catch(() => invoke<SavedNativeExport>("commit_native_export", args));
  const expectedName =
    prepared.artifact.kind === "transaction_csv"
      ? `transactions-typed-v1-r${prepared.artifact.workspace_revision}-${prepared.artifact.sha256}.csv`
      : prepared.artifact.kind === "transactions"
        ? `transactions-r${prepared.artifact.workspace_revision}-${prepared.artifact.sha256}.json`
        : `assessment-${prepared.artifact.report_id}-${prepared.artifact.sha256}.${prepared.artifact.kind === "docx_report" ? "docx" : "html"}`;
  if (
    !keys(result, [
      "schema_version",
      "ticket",
      "artifact",
      "filename",
      "location",
    ]) ||
    result.schema_version !== prepared.schema_version ||
    result.ticket !== prepared.ticket ||
    !same(result.artifact, prepared.artifact) ||
    result.filename !== expectedName ||
    typeof result.location !== "string" ||
    !result.location ||
    result.location.length > 32768
  )
    throw new Error(
      "Native save receipt did not match the prepared export. Completion is unconfirmed.",
    );
  return result;
}
export async function discardNativeExport(ticket: string): Promise<void> {
  const result = await invoke<
    { state: "discarded" } | { state: "saved"; receipt: SavedNativeExport }
  >("discard_native_export", { ticket });
  if (!result || (result.state !== "discarded" && result.state !== "saved"))
    throw new Error("Native export cleanup was not confirmed.");
}
