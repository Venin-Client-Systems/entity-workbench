import type { ReviewState } from "./types";

export type CitationAnchor =
  | { kind: "text"; evidence_id: string; line_start: number; line_end: number }
  | {
      kind: "cell";
      evidence_id: string;
      sheet: string;
      row: number;
      column: string;
    }
  | {
      kind: "page";
      evidence_id: string;
      page: number;
      region: [number, number, number, number] | null;
    }
  | { kind: "message"; evidence_id: string; message_id: string }
  | { kind: "capture"; evidence_id: string; selector: string };
export type CitationEvidence = {
  id: string;
  name: string;
  sha256: string;
  bytes: number;
  media_type: string;
  origin_group: string;
  imported_at: string;
  extraction_status: string;
};
export type CitationSummary =
  | {
      kind: "observation";
      id: string;
      entity_id: string;
      entity_name: string | null;
      field: string;
      value: string;
      review: ReviewState;
      anchor: CitationAnchor;
      source: CitationEvidence;
    }
  | {
      kind: "transaction";
      id: string;
      description: string;
      amount: string;
      currency: string;
      date: string;
      account: string;
      review: ReviewState;
      version: number;
      anchor: CitationAnchor;
      source: CitationEvidence;
    }
  | { kind: "evidence"; id: string; source: CitationEvidence };
export type CitationSelections = {
  schema_version: 1;
  workspace_revision: number;
  rows: CitationSummary[];
};
export type CitationCataloguePage = CitationSelections & {
  matching: {
    algorithm: "unicode_default_lowercase_literal_v1";
    unicode_version: [number, number, number];
  };
  query_sha256: string;
  scope_count: number;
  next_cursor: string | null;
};
export type CitationRole = "none" | "supporting" | "contradicting";
