import { useRef, useState } from "react";
import { Dialog } from "./Dialog";
import type { Extraction, ParseLimitation } from "./processing-types";
import { useProcessingRead } from "./useProcessingRead";
const limitations: Record<ParseLimitation, string> = {
  no_source_anchors:
    "No source anchors — text has no verified page, region or cell links.",
  embedded_documents_excluded: "Embedded documents were not processed.",
  ocr_not_performed: "OCR was not performed. Scanned content may be absent.",
  text_limit: "Text limit reached. Extracted text is incomplete.",
  metadata_limit: "Metadata limit reached. Metadata is incomplete.",
  page_limit: "Page limit reached. Later pages were not processed.",
};
export function ExtractionReview({
  extractionId,
  onClose,
}: {
  extractionId: string;
  onClose: () => void;
}) {
  const read = useProcessingRead<Extraction>(
    "inspect_extraction",
    extractionId,
    true,
    false,
  );
  const [copied, setCopied] = useState("");
  const [copying, setCopying] = useState(false);
  const textRef = useRef<HTMLTextAreaElement>(null);
  const record = read.value;
  const copy = async () => {
    if (!record) return;
    setCopying(true);
    setCopied("");
    try {
      await navigator.clipboard.writeText(record.result.text);
      setCopied("Unreviewed text copied to the clipboard.");
    } catch {
      setCopied(
        "Clipboard unavailable. The text is selected; use your system copy shortcut.",
      );
      textRef.current?.focus();
      textRef.current?.select();
    } finally {
      setCopying(false);
    }
  };
  return (
    <Dialog
      wide
      label="Extraction review"
      onClose={onClose}
      preventClose={copying}
    >
      <div className="processing-review">
        <div className="processing-heading">
          <div>
            <p className="eyebrow">LOCAL DERIVATIVE / UNREVIEWED</p>
            <h2>Extraction review</h2>
          </div>
          <button
            className="close"
            disabled={copying}
            onClick={onClose}
            aria-label="Close extraction review"
          >
            ×
          </button>
        </div>
        <p className="processing-id">{extractionId}</p>
        {read.error && (
          <p className="error" role="alert">
            Extraction unavailable. {read.error}{" "}
            <button className="button" onClick={read.refresh}>
              Reload extraction
            </button>
          </p>
        )}
        {!record && !read.error && (
          <p role="status">Loading immutable extraction…</p>
        )}
        {record && (
          <>
            <div className="processing-banner">
              <strong>
                {record.result.status === "complete"
                  ? "Complete parser output"
                  : record.result.status === "partial"
                    ? "Partial parser output"
                    : record.result.status === "unsupported"
                      ? "Unsupported format"
                      : "Parser failed"}{" "}
                · unreviewed
              </strong>
              <p>
                This immutable derivative does not create accepted observations
                or verified source anchors.
              </p>
              {record.result.error && (
                <p>Parser error: {record.result.error.replaceAll("_", " ")}</p>
              )}
            </div>
            <dl className="processing-metrics">
              <div>
                <dt>Parser</dt>
                <dd>{record.result.parser}</dd>
              </div>
              <div>
                <dt>Original bytes</dt>
                <dd>{record.input.bytes.toLocaleString("en-US")}</dd>
              </div>
              <div>
                <dt>Recorded attempt</dt>
                <dd>{record.attempt}</dd>
              </div>
            </dl>
            <section className="processing-limitations">
              <h3>Extraction limitations</h3>
              <ul>
                {record.result.limitations.map((item) => (
                  <li key={item}>{limitations[item]}</li>
                ))}
              </ul>
            </section>
            <div className="processing-heading">
              <h3>Unreviewed extracted text</h3>
              <button
                className="button"
                disabled={copying || !record.result.text}
                onClick={() => void copy()}
              >
                {copying ? "Copying…" : "Copy text"}
              </button>
            </div>
            {record.result.text ? (
              <textarea
                ref={textRef}
                className="processing-text"
                aria-label="Unreviewed extracted text"
                readOnly
                spellCheck={false}
                value={record.result.text}
              />
            ) : (
              <p className="context-note">
                No text was returned. This is not evidence that the original
                contains no relevant information.
              </p>
            )}
            {copied && <p role="status">{copied}</p>}
            <h3>Original and derivative provenance</h3>
            <dl className="processing-facts">
              <dt>Original SHA-256</dt>
              <dd>
                <code>{record.input.sha256}</code>
              </dd>
              <dt>Result SHA-256</dt>
              <dd>
                <code>{record.result_sha256}</code>
              </dd>
              <dt>Evidence ID</dt>
              <dd>
                <code>{record.input.evidence_id}</code>
              </dd>
              <dt>Document job ID</dt>
              <dd>
                <code>{record.job_id}</code>
              </dd>
              <dt>Worker request ID</dt>
              <dd>
                <code>{record.result.job_id}</code>
              </dd>
              <dt>Media type</dt>
              <dd>{record.result.media_type}</dd>
              <dt>Published at</dt>
              <dd>{record.created_at}</dd>
            </dl>
            <h3>Extracted metadata · unreviewed</h3>
            {Object.entries(record.result.metadata).length ? (
              <dl className="processing-facts">
                {Object.entries(record.result.metadata).map(([key, values]) => (
                  <div className="processing-metadata" key={key}>
                    <dt>{key}</dt>
                    <dd>
                      {values.map((value, index) => (
                        <p key={index}>{value}</p>
                      ))}
                    </dd>
                  </div>
                ))}
              </dl>
            ) : (
              <p className="muted">No metadata returned.</p>
            )}
            <p className="context-note">
              Text and metadata are displayed as inert data. Complete parser
              output is not a claim of extraction accuracy, OCR coverage or
              analyst acceptance.
            </p>
          </>
        )}
      </div>
    </Dialog>
  );
}
