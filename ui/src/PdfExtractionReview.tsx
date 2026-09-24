import { useRef, useState } from "react";
import { Dialog } from "./Dialog";
import type { RecognitionLimitation } from "./image-processing-types";
import type {
  PdfExtraction,
  PdfRenderFailure,
  PdfRenderLimitation,
} from "./pdf-processing-types";
import { useProcessingRead } from "./useProcessingRead";
import "./pdf-processing.css";

const renderLimitations: Record<PdfRenderLimitation, string> = {
  scan_focused_subset:
    "Only the scan-focused PDF subset was supported. General PDF compatibility is not established.",
  annotations_excluded:
    "Annotations, including their appearance streams, were excluded.",
  color_converted_to_gray:
    "The rendered page was converted to grayscale with a white background.",
  unreviewed_raster:
    "The renderer's interpretation is unreviewed; verify it against the original before relying on it.",
  no_word_regions:
    "No verified word regions or accepted source anchors were created.",
};
const recognitionLimitations: Record<RecognitionLimitation, string> = {
  unreviewed_recognition:
    "Recognized names, dates, amounts and other text may be incorrect.",
  no_word_regions:
    "Recognition has no word boxes or verified text-to-region mapping.",
  no_original_document_mapping:
    "Recognition is bound to the raster hash. The separate render record binds that raster to the selected PDF page.",
};
const failures: Record<PdfRenderFailure, string> = {
  encrypted_document: "This operation cannot render encrypted PDFs.",
  unsupported_format: "The retained bytes are not a supported PDF input.",
  unsupported_feature:
    "The PDF contains a feature outside the scan-focused rendering subset.",
  active_content: "Document actions or scripts were rejected during preflight.",
  external_resource:
    "An external or embedded file reference was rejected during preflight.",
  malformed_document:
    "The PDF could not be rendered without accepting malformed content.",
  page_out_of_range:
    "The selected page is beyond the reported document page count.",
  page_limit: "The document exceeds the supported page-count limit.",
  pixel_limit: "The page or an embedded image exceeds the pixel limit.",
  structure_limit: "The PDF exceeds the supported structure limit.",
  stream_limit: "The PDF exceeds the expanded-stream limit.",
  operator_limit: "The selected page exceeds the drawing-operator limit.",
};
const outcome = (record: PdfExtraction) => {
  const { render, recognition } = record.result;
  if (recognition?.status === "recognized")
    return "Text recognized · unreviewed";
  if (recognition?.status === "no_text_recognized")
    return "No text recognized · unreviewed";
  switch (render.status) {
    case "encrypted":
      return "Encrypted PDF · OCR did not run";
    case "unsupported":
      return "PDF input unsupported · OCR did not run";
    case "failed":
      return "PDF rendering failed · OCR did not run";
    case "quota_exhausted":
      return "PDF limit exhausted · OCR did not run";
    case "rendered":
      return "Page rendered · no OCR result published";
  }
};

/** Inspect validated structured records and inert text; no PDF or raster executes in the interface. */
export function PdfExtractionReview({
  extractionId,
  sourceName,
  onClose,
}: {
  extractionId: string;
  sourceName: string;
  onClose: () => void;
}) {
  const read = useProcessingRead<PdfExtraction>(
    "inspect_pdf_extraction",
    extractionId,
    true,
    false,
  );
  const [copied, setCopied] = useState("");
  const [copying, setCopying] = useState(false);
  const textRef = useRef<HTMLTextAreaElement>(null);
  // A failed canonical read must not expose a previously loaded result as available.
  const record = read.error ? null : read.value;
  const render = record?.result.render;
  const recognition = record?.result.recognition;
  const raster = render?.raster;
  const geometry = render?.geometry;
  const recognized = recognition?.status === "recognized";
  const copy = async () => {
    if (!recognized || !recognition) return;
    setCopying(true);
    setCopied("");
    try {
      await navigator.clipboard.writeText(recognition.text);
      setCopied("Unreviewed OCR text copied to the clipboard.");
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
      label="PDF page OCR review"
      onClose={onClose}
      preventClose={copying}
    >
      <div className="processing-review">
        <div className="processing-heading">
          <div>
            <p className="eyebrow">LOCAL PDF PAGE DERIVATIVE / UNREVIEWED</p>
            <h2>PDF page OCR review</h2>
          </div>
          <button
            className="close"
            disabled={copying}
            onClick={onClose}
            aria-label="Close PDF page OCR review"
          >
            ×
          </button>
        </div>
        <h3 className="processing-filename">{sourceName}</h3>
        {read.error && (
          <p className="error" role="alert">
            PDF extraction unavailable. {read.error}{" "}
            <button className="button" onClick={read.refresh}>
              Reload PDF extraction
            </button>
          </p>
        )}
        {!record && !read.error && (
          <p role="status">Loading immutable PDF extraction…</p>
        )}
        {record && render && (
          <>
            <div className="processing-banner">
              <strong>{outcome(record)}</strong>
              <p>
                One selected page. This immutable derivative creates no accepted
                observations or verified word regions.
              </p>
              <p>Scan-focused subset · annotations excluded · English OCR</p>
              {render.failure && <p>{failures[render.failure]}</p>}
            </div>
            <dl className="processing-metrics">
              <div>
                <dt>Selected page</dt>
                <dd>
                  {render.page_number} /{" "}
                  {render.page_count === null
                    ? "count unavailable"
                    : `${render.page_count} ${render.page_count === 1 ? "page" : "pages"}`}
                </dd>
              </div>
              <div>
                <dt>Render resolution</dt>
                <dd>{render.dpi} DPI</dd>
              </div>
              <div>
                <dt>Recorded attempt</dt>
                <dd>{record.attempt}</dd>
              </div>
            </dl>
            <h3>Original, raster and result</h3>
            <dl className="processing-facts">
              <dt>Original SHA-256</dt>
              <dd>
                <code>{record.input.sha256}</code>
              </dd>
              <dt>Raster SHA-256</dt>
              <dd>{raster ? <code>{raster.sha256}</code> : "Not produced"}</dd>
              <dt>Result SHA-256</dt>
              <dd>
                <code>{record.result_sha256}</code>
              </dd>
            </dl>
            {geometry && raster ? (
              <section className="pdf-geometry" aria-label="Page geometry">
                <h3>Page geometry / extracted interpretation</h3>
                <dl>
                  <div>
                    <dt>Raster dimensions</dt>
                    <dd>
                      {raster.width.toLocaleString("en-US")} ×{" "}
                      {raster.height.toLocaleString("en-US")} pixels
                    </dd>
                  </div>
                  <div>
                    <dt>Effective CropBox</dt>
                    <dd>[{geometry.crop_box.join(", ")}] points</dd>
                  </div>
                  <div>
                    <dt>Rotation</dt>
                    <dd>{geometry.rotation_degrees}°</dd>
                  </div>
                </dl>
                <p>PDF points → top-left raster pixels</p>
                <p>
                  <code aria-label="PDF to raster affine">
                    [{geometry.pdf_to_raster.join(", ")}]
                  </code>
                </p>
                <p>
                  The validated raster was discarded; its binding is retained,
                  not an exported exhibit.
                </p>
              </section>
            ) : (
              <p className="context-note">
                No rendered page raster or geometry was published for this
                result.
              </p>
            )}
            <div className="processing-heading">
              <h3>Unreviewed OCR text</h3>
              <button
                className="button"
                disabled={copying || !recognized}
                onClick={() => void copy()}
              >
                {copying ? "Copying…" : "Copy OCR text"}
              </button>
            </div>
            {recognized && recognition ? (
              <textarea
                ref={textRef}
                className="processing-text"
                aria-label="Unreviewed OCR text"
                readOnly
                spellCheck={false}
                value={recognition.text}
              />
            ) : (
              <p className="context-note">
                {recognition
                  ? "The OCR worker returned no text for this selected page."
                  : "No OCR text was published for this result."}{" "}
                This is not evidence that the page or other parts of the
                original contain no relevant information.
              </p>
            )}
            {copied && <p role="status">{copied}</p>}
            <details className="processing-details">
              <summary>Full PDF / OCR provenance and limitations</summary>
              <section className="processing-limitations">
                <h3>Rendering and recognition limitations</h3>
                <ul>
                  {render.limitations.map((item) => (
                    <li key={item}>{renderLimitations[item]}</li>
                  ))}
                  {recognition?.limitations.map((item) => (
                    <li key={`ocr-${item}`}>{recognitionLimitations[item]}</li>
                  ))}
                </ul>
              </section>
              {geometry && (
                <p className="context-note">
                  Affine order is [a, b, c, d, e, f]: x′ = a×x + c×y + e; y′ =
                  b×x + d×y + f. CropBox is the renderer’s effective box,
                  clipped to MediaBox. This records the worker’s interpretation;
                  it does not independently establish pixel equivalence to other
                  viewers or verify text-to-region anchors.
                </p>
              )}
              <dl className="processing-facts">
                <dt>Extraction ID</dt>
                <dd>
                  <code>{extractionId}</code>
                </dd>
                <dt>Evidence ID</dt>
                <dd>
                  <code>{record.input.evidence_id}</code>
                </dd>
                <dt>Original bytes</dt>
                <dd>{record.input.bytes.toLocaleString("en-US")}</dd>
                <dt>Document job ID</dt>
                <dd>
                  <code>{record.job_id}</code>
                </dd>
                <dt>Render request ID</dt>
                <dd>
                  <code>{render.job_id}</code>
                </dd>
                <dt>Renderer</dt>
                <dd>
                  {render.renderer} / Java {render.java_runtime}
                </dd>
                <dt>Render status</dt>
                <dd>{render.status.replaceAll("_", " ")}</dd>
                <dt>Failure category</dt>
                <dd>
                  {render.failure?.replaceAll("_", " ") ?? "None recorded"}
                </dd>
                <dt>Published at</dt>
                <dd>{record.created_at}</dd>
                {raster && (
                  <>
                    <dt>Raster bytes at processing</dt>
                    <dd>{raster.bytes.toLocaleString("en-US")}</dd>
                    <dt>Raster retained</dt>
                    <dd>No · binding retained, raster bytes discarded</dd>
                  </>
                )}
                <dt>OCR engine</dt>
                <dd>{recognition?.engine ?? "Not run"}</dd>
                <dt>Language</dt>
                <dd>{recognition ? "English (eng)" : "Not run"}</dd>
                {recognition && (
                  <>
                    <dt>OCR request ID</dt>
                    <dd>
                      <code>{recognition.job_id}</code>
                    </dd>
                    <dt>OCR raster SHA-256</dt>
                    <dd>
                      <code>{recognition.raster_sha256}</code>
                    </dd>
                    <dt>English model SHA-256</dt>
                    <dd>
                      <code>{recognition.model_sha256}</code>
                    </dd>
                    <dt>OCR runtime manifest SHA-256</dt>
                    <dd>
                      <code>{recognition.runtime_manifest_sha256}</code>
                    </dd>
                  </>
                )}
              </dl>
            </details>
            <p className="context-note">
              The original remains unchanged. Text is inert review material; it
              is not automatically added to accepted observations or corpus
              search. Only the selected page was requested.
            </p>
          </>
        )}
      </div>
    </Dialog>
  );
}
