import { digest } from "./transaction-ledger-types";

type ArtifactRef = {
  kind: "report_document_json_v1" | "report_docx_v1";
  sha256: string;
  bytes: number;
};
export type DocxSnapshot = {
  schema_version: 1;
  id: string;
  workspace_revision: number;
  created_at: string;
  template_version: string;
  generator_version: string;
  document: ArtifactRef;
  docx: ArtifactRef;
};
export type DocxSnapshotPage = {
  schema_version: 1;
  workspace_revision: number;
  total_count: number;
  query_sha256: string;
  rows: DocxSnapshot[];
  next_cursor: string | null;
};
export type DocxCaptureResolution = {
  schema_version: 1;
  request_id: string;
  captured_revision: number;
  workspace_revision: number;
  outcome:
    { state: "saved"; snapshot: DocxSnapshot } | { state: "not_recorded" };
};
export function validateDocxResolution(
  value: DocxCaptureResolution,
  id: string,
  captured: number,
  minimumRevision: number,
): void {
  if (
    !value ||
    value.schema_version !== 1 ||
    value.request_id !== id ||
    value.captured_revision !== captured ||
    !wholeNumber(value.workspace_revision) ||
    value.workspace_revision < captured ||
    value.workspace_revision < minimumRevision ||
    (value.outcome?.state !== "not_recorded" &&
      value.outcome?.state !== "saved") ||
    (value.outcome.state === "saved" &&
      (!isDocxSnapshot(value.outcome.snapshot) ||
        value.outcome.snapshot.id !== id ||
        value.outcome.snapshot.workspace_revision !== captured ||
        value.workspace_revision <= captured))
  )
    throw new Error(
      "DOCX recovery response did not match this request and visible workspace revision.",
    );
}
export const docxUuid = (value: unknown): value is string =>
  typeof value === "string" &&
  /^[0-9a-f]{8}-[0-9a-f]{4}-[0-9a-f]{4}-[0-9a-f]{4}-[0-9a-f]{12}$/.test(value);
export const wholeNumber = (value: unknown): value is number =>
  Number.isSafeInteger(value) && Number(value) >= 0;
const boundedText = (value: unknown, maximum: number): value is string =>
  typeof value === "string" && value.length > 0 && value.length <= maximum;

/** Transport shape and selection binding only; Rust validates the frozen model. */
export function isDocxSnapshot(value: unknown): value is DocxSnapshot {
  if (!value || typeof value !== "object") return false;
  const row = value as DocxSnapshot;
  const ref = (
    value: ArtifactRef,
    kind: ArtifactRef["kind"],
    maximum: number,
  ) =>
    value?.kind === kind &&
    digest(value.sha256) &&
    wholeNumber(value.bytes) &&
    value.bytes > 0 &&
    value.bytes <= maximum;
  return (
    row.schema_version === 1 &&
    docxUuid(row.id) &&
    wholeNumber(row.workspace_revision) &&
    boundedText(row.created_at, 128) &&
    boundedText(row.template_version, 128) &&
    boundedText(row.generator_version, 128) &&
    ref(row.document, "report_document_json_v1", 16 * 1024 * 1024) &&
    ref(row.docx, "report_docx_v1", 32 * 1024 * 1024)
  );
}
export function validateDocxPage(
  value: DocxSnapshotPage,
  revision: number,
  offset: number,
  query: string | null,
): void {
  if (
    !value ||
    value.schema_version !== 1 ||
    value.workspace_revision !== revision ||
    !wholeNumber(value.total_count) ||
    !digest(value.query_sha256) ||
    (query !== null && query !== value.query_sha256) ||
    !Array.isArray(value.rows) ||
    value.rows.length > 20 ||
    value.rows.some(
      (row) => !isDocxSnapshot(row) || row.workspace_revision >= revision,
    ) ||
    new Set(value.rows.map((row) => row.id)).size !== value.rows.length ||
    offset + value.rows.length > value.total_count ||
    (value.next_cursor !== null &&
      (!boundedText(value.next_cursor, 2048) || value.rows.length === 0)) ||
    (value.next_cursor !== null) !==
      offset + value.rows.length < value.total_count ||
    (value.rows.length === 0 && offset !== value.total_count)
  ) {
    throw new Error(
      "DOCX catalogue response did not match the selected revision and page.",
    );
  }
}
