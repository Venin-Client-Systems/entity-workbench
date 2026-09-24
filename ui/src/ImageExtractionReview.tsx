import { useRef, useState } from "react";
import { Dialog } from "./Dialog";
import type {
  DecodeLimitation,
  ImageExtraction,
  RecognitionLimitation,
} from "./image-processing-types";
import { useProcessingRead } from "./useProcessingRead";

const decodeLimitations: Record<DecodeLimitation, string> = {
  exif_orientation_not_applied:
    "EXIF orientation was not applied. Pixel order follows the encoded image.",
  embedded_previews_excluded:
    "Embedded thumbnails and previews were not processed.",
  metadata_not_extracted: "Image metadata was not extracted.",
  color_converted_to_gray:
    "OCR used grayscale pixels, with transparency composited onto white.",
  no_document_page_mapping:
    "Source image index zero is not a document page or verified page anchor.",
};
const recognitionLimitations: Record<RecognitionLimitation, string> = {
  unreviewed_recognition:
    "Recognition is unreviewed. Names, dates, amounts and other text may be incorrect.",
  no_word_regions: "No verified word regions or source anchors are available.",
  no_original_document_mapping:
    "Recognition is bound to the raster hash; the decoder separately binds that raster to the original image.",
};

/** Follows the editable industrial extraction review; collected image bytes never execute here. */
export function ImageExtractionReview({
  extractionId,
  sourceName,
  onClose,
}: {
  extractionId: string;
  sourceName: string;
  onClose: () => void;
}) {
  const read = useProcessingRead<ImageExtraction>(
    "inspect_image_extraction",
    extractionId,
    true,
    false,
  );
  const [copied, setCopied] = useState("");
  const [copying, setCopying] = useState(false);
  const textRef = useRef<HTMLTextAreaElement>(null);
  const record = read.value;
  const decoder = record?.result.decoder;
  const recognition = record?.result.recognition;
  const raster = decoder?.raster;
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
      label="Image OCR review"
      onClose={onClose}
      preventClose={copying}
    >
      <div className="processing-review">
        <div className="processing-heading">
          <div>
            <p className="eyebrow">LOCAL IMAGE DERIVATIVE / UNREVIEWED</p>
            <h2>Image OCR review</h2>
          </div>
          <button
            className="close"
            disabled={copying}
            onClick={onClose}
            aria-label="Close image OCR review"
          >
            ×
          </button>
        </div>
        <h3 className="processing-filename">{sourceName}</h3>
        {read.error && (
          <p className="error" role="alert">
            Image extraction unavailable. {read.error}{" "}
            <button className="button" onClick={read.refresh}>
              Reload image extraction
            </button>
          </p>
        )}
        {!record && !read.error && (
          <p role="status">Loading immutable image extraction…</p>
        )}
        {record && decoder && (
          <>
            <div className="processing-banner">
              <strong>
                {recognition?.status === "recognized"
                  ? "Text recognized · unreviewed"
                  : recognition?.status === "no_text_recognized"
                    ? "No text recognized · unreviewed"
                    : decoder.status === "unsupported"
                      ? "Image input unsupported · OCR did not run"
                      : decoder.status === "quota_exhausted"
                        ? "Image limit exhausted · OCR did not run"
                        : "Image decoding failed · OCR did not run"}
              </strong>
              <p>
                This immutable derivative creates no accepted observations or
                verified source anchors.
              </p>
              {decoder.failure && (
                <p>Decoder outcome: {decoder.failure.replaceAll("_", " ")}</p>
              )}
            </div>
            <dl className="processing-metrics">
              <div>
                <dt>OCR engine</dt>
                <dd>{recognition?.engine ?? "Not run"}</dd>
              </div>
              <div>
                <dt>Language</dt>
                <dd>{recognition ? "English" : "Not run"}</dd>
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
            <div className="context-note">
              {raster && (
                <p>
                  <strong>
                    Raster / {raster.width.toLocaleString("en-US")} ×{" "}
                    {raster.height.toLocaleString("en-US")} pixels
                  </strong>
                  <br />
                  EXIF orientation was not applied. Check the retained original
                  before relying on layout.
                </p>
              )}
              <p>
                No verified word, page or region anchors are available. Text
                remains unreviewed.
              </p>
              <p>
                {raster
                  ? "The validated raster was discarded; its binding is retained, not an exported exhibit."
                  : "No decoded raster was published for this result."}
              </p>
            </div>
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
                  ? "The OCR worker returned no text."
                  : "No OCR text was produced because image decoding did not succeed."}{" "}
                This is not evidence that the original contains no relevant
                information.
              </p>
            )}
            {copied && <p role="status">{copied}</p>}
            <details className="processing-details">
              <summary>Full processing provenance and limitations</summary>
              <section className="processing-limitations">
                <h3>Recognition and image limitations</h3>
                <ul>
                  {recognition?.limitations.map((item) => (
                    <li key={item}>{recognitionLimitations[item]}</li>
                  ))}
                  {decoder.limitations.map((item) => (
                    <li key={item}>{decodeLimitations[item]}</li>
                  ))}
                </ul>
              </section>
              <dl className="processing-facts">
                <dt>Extraction ID</dt>
                <dd>
                  <code>{extractionId}</code>
                </dd>
                <dt>Original bytes</dt>
                <dd>{record.input.bytes.toLocaleString("en-US")}</dd>
                <dt>Evidence ID</dt>
                <dd>
                  <code>{record.input.evidence_id}</code>
                </dd>
                <dt>Document job ID</dt>
                <dd>
                  <code>{record.job_id}</code>
                </dd>
                <dt>Decoder request ID</dt>
                <dd>
                  <code>{decoder.job_id}</code>
                </dd>
                <dt>Decoder</dt>
                <dd>
                  {decoder.decoder} / Java {decoder.java_runtime}
                </dd>
                <dt>Original media type</dt>
                <dd>{decoder.media_type}</dd>
                <dt>Published at</dt>
                <dd>{record.created_at}</dd>
                {raster && (
                  <>
                    <dt>Raster dimensions</dt>
                    <dd>
                      {raster.width.toLocaleString("en-US")} ×{" "}
                      {raster.height.toLocaleString("en-US")} pixels
                    </dd>
                    <dt>Raster bytes at processing</dt>
                    <dd>{raster.bytes.toLocaleString("en-US")}</dd>
                    <dt>Source image index</dt>
                    <dd>
                      {raster.source_image_index} · encoded image, not a
                      document page
                    </dd>
                    <dt>Pixel mapping</dt>
                    <dd>Encoded pixels → grayscale / white alpha</dd>
                    <dt>Raster retained</dt>
                    <dd>No · binding retained, raster bytes discarded</dd>
                  </>
                )}
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
              The original remains unchanged. OCR text is inert review material;
              it is not automatically added to accepted evidence or corpus
              search.
            </p>
          </>
        )}
      </div>
    </Dialog>
  );
}
