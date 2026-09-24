import type { ReactNode } from "react";
import type { Anchor, Evidence } from "./types";
import type { CitationAnchor, CitationSummary } from "./citation-types";

export type CitationSourceOpener = (
  evidence: Evidence,
  anchor?: Anchor,
) => void;
export function citationLabel(item: CitationSummary): string {
  switch (item.kind) {
    case "observation":
      return `${item.entity_name ?? item.entity_id} · ${item.field}: ${item.value}`;
    case "transaction":
      return `${item.description} · ${item.amount} ${item.currency}`;
    case "evidence":
      return item.source.name;
  }
}
function anchorLabel(anchor: CitationAnchor): string {
  switch (anchor.kind) {
    case "text":
      return `lines ${anchor.line_start}–${anchor.line_end}`;
    case "cell":
      return `${anchor.sheet}, row ${anchor.row}, ${anchor.column}`;
    case "page":
      return `page ${anchor.page}${anchor.region ? ` · region ${anchor.region.join(", ")}` : ""}`;
    case "message":
      return `message ${anchor.message_id}`;
    case "capture":
      return `capture selector ${anchor.selector}`;
  }
}
export function CitationRow({
  item,
  evidence,
  onSource,
  disabled,
  children,
}: {
  item: CitationSummary;
  evidence: Evidence[];
  onSource: CitationSourceOpener;
  disabled: boolean;
  children?: ReactNode;
}) {
  // SourceContent still needs full retained text/acquisitions. Never synthesize
  // an empty Evidence from the metadata projection or claim this lookup rehashes it.
  const source = evidence.find(
    (e) =>
      e.id === item.source.id &&
      e.sha256 === item.source.sha256 &&
      e.bytes === item.source.bytes,
  );
  const anchor = item.kind === "evidence" ? undefined : item.anchor;
  const detail =
    item.kind === "observation"
      ? `Observation · ${item.review}`
      : item.kind === "transaction"
        ? `Transaction · ${item.date} · account ${item.account} · ${item.review}`
        : `Whole source · ${item.source.extraction_status}`;
  return (
    <div className="citation-row" data-citation-id={item.id}>
      <div>
        <strong>{citationLabel(item)}</strong>
        <p>
          {detail}
          {anchor ? ` · ${item.source.name} · ${anchorLabel(anchor)}` : ""}
        </p>
        {!source && (
          <p>
            Source content unavailable in this workspace view. Refresh before
            inspection.
          </p>
        )}
      </div>
      <div className="citation-controls">
        <button
          type="button"
          className="button"
          disabled={disabled || !source}
          onClick={() => {
            if (source) onSource(source, anchor);
          }}
        >
          Inspect source
        </button>
        {children}
      </div>
    </div>
  );
}
