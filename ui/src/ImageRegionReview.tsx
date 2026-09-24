import { useEffect, useMemo, useRef, useState } from "react";
import { command } from "./api";
import { Dialog } from "./Dialog";
import type { ImageRegionInspection, OcrRegion } from "./image-region-types";
import {
  readImageRegionRaster,
  verifyRegionRaster,
  type RasterPixels,
} from "./image-region-raster";
import "./image-regions.css";

const outcome = (value: ImageRegionInspection) =>
  value.result.decoder.status === "decoded"
    ? value.result.recognition?.status === "recognized"
      ? "Words recognized · unreviewed"
      : "No text recognized · unreviewed"
    : {
        unsupported: "Image input unsupported",
        failed: "Image decoding failed",
        quota_exhausted: "Image resource limit exhausted",
      }[value.result.decoder.status];

function RegionCanvas({
  raster,
  words,
  selected,
  setSelected,
  onError,
}: {
  raster: RasterPixels;
  words: OcrRegion[];
  selected: number | null;
  setSelected: (index: number) => void;
  onError: (message: string) => void;
}) {
  const ref = useRef<HTMLCanvasElement>(null);
  const [fit, setFit] = useState(true),
    [boxes, setBoxes] = useState(true);
  const pixelData = useMemo(() => {
    try {
      const data = new ImageData(raster.width, raster.height);
      for (let i = 0; i < raster.pixels.length; i++) {
        const offset = i * 4;
        data.data[offset] =
          data.data[offset + 1] =
          data.data[offset + 2] =
            raster.pixels[i];
        data.data[offset + 3] = 255;
      }
      return data;
    } catch {
      return null;
    }
  }, [raster]);
  useEffect(() => {
    try {
      const canvas = ref.current;
      if (!canvas || !pixelData) throw new Error();
      const context = canvas.getContext("2d");
      if (!context) throw new Error();
      context.putImageData(pixelData, 0, 0);
      if (boxes)
        words.forEach((word, i) => {
          const b = word.bounds;
          context.strokeStyle = i === selected ? "#a64814" : "#397079";
          context.lineWidth = i === selected ? 4 : 2;
          context.strokeRect(b.left, b.top, b.width, b.height);
        });
    } catch {
      onError(
        "Verified raster could not be allocated or drawn. No preview is available.",
      );
    }
  }, [pixelData, words, selected, boxes, onError]);
  return (
    <section aria-label="Retained local raster" className="region-raster">
      <div className="region-toolbar">
        <h3>Local raster / encoded pixels</h3>
        <div>
          <button
            className="button"
            aria-pressed={fit}
            onClick={() => setFit(true)}
          >
            Fit
          </button>
          <button
            className="button"
            aria-pressed={!fit}
            onClick={() => setFit(false)}
          >
            100%
          </button>
          <button
            className="button"
            aria-pressed={boxes}
            onClick={() => setBoxes(!boxes)}
          >
            {boxes ? "Hide boxes" : "Show boxes"}
          </button>
        </div>
      </div>
      <div
        className="region-raster-scroll"
        tabIndex={0}
        aria-label="Raster viewport; scroll at 100 percent"
      >
        <canvas
          ref={ref}
          width={raster.width}
          height={raster.height}
          style={{ width: fit ? "100%" : `${raster.width}px` }}
          aria-label="Verified grayscale raster; words are also selectable in the adjacent list"
          role="img"
          onClick={(event) => {
            if (!boxes) return;
            const rect = event.currentTarget.getBoundingClientRect();
            const x = ((event.clientX - rect.left) * raster.width) / rect.width,
              y = ((event.clientY - rect.top) * raster.height) / rect.height;
            const index = words.findIndex(
              ({ bounds: b }) =>
                x >= b.left &&
                x < b.left + b.width &&
                y >= b.top &&
                y < b.top + b.height,
            );
            if (index >= 0) setSelected(index);
          }}
        />
      </div>
      <p className="region-caption">
        Top-left origin · pixels · right/bottom excluded
      </p>
      <p>EXIF orientation was not applied. No PDF page is inferred.</p>
    </section>
  );
}

/** Whole-chain read and raster digest validation complete before any derivative is displayed. */
export function ImageRegionReview({
  extractionId,
  sourceName,
  onClose,
}: {
  extractionId: string;
  sourceName: string;
  onClose: () => void;
}) {
  const [loaded, setLoaded] = useState<{
    value: ImageRegionInspection;
    raster: RasterPixels | null;
  } | null>(null);
  const [error, setError] = useState(""),
    [generation, setGeneration] = useState(0);
  const [selected, setSelected] = useState<number | null>(null),
    [page, setPage] = useState(0);
  const [copied, setCopied] = useState(""),
    [copying, setCopying] = useState(false);
  const textRef = useRef<HTMLTextAreaElement>(null);
  const listRef = useRef<HTMLOListElement>(null);
  const current = useRef(true);
  useEffect(() => {
    current.current = true;
    let active = true;
    setLoaded(null);
    setError("");
    setSelected(null);
    setPage(0);
    setCopied("");
    void (async () => {
      try {
        const value = await command<ImageRegionInspection>({
          action: "inspect_image_region_extraction",
          extraction_id: extractionId,
        });
        if (!active) return;
        if (value.extraction.id !== extractionId)
          throw new Error("Inspection identity changed.");
        const binding = value.result.decoder.raster;
        const raster = value.extraction.raster
          ? await verifyRegionRaster(
              await readImageRegionRaster(extractionId),
              value.extraction.raster,
              binding!.width,
              binding!.height,
            )
          : null;
        if (!active) return;
        const words =
          value.result.recognition?.regions.filter((r) => r.level === "word") ??
          [];
        if (
          words.length > 10_000 ||
          (value.result.recognition?.regions.length ?? 0) > 20_000
        )
          throw new Error("Region display exceeds its bound.");
        setLoaded({ value, raster });
      } catch (cause) {
        if (active) {
          setLoaded(null);
          setError(String(cause));
        }
      }
    })();
    return () => {
      active = false;
      current.current = false;
    };
  }, [extractionId, generation]);
  const value = loaded?.value,
    record = value?.extraction,
    decoder = value?.result.decoder,
    recognition = value?.result.recognition;
  const words = useMemo(
    () => recognition?.regions.filter((r) => r.level === "word") ?? [],
    [recognition],
  );
  useEffect(() => {
    if (listRef.current) listRef.current.scrollTop = 0;
  }, [page]);
  useEffect(() => {
    const list = listRef.current;
    const row = list?.querySelector<HTMLElement>(
      `[data-word-index="${selected}"]`,
    );
    if (!list || !row) return;
    const outer = list.getBoundingClientRect(),
      inner = row.getBoundingClientRect();
    if (inner.top < outer.top) list.scrollTop += inner.top - outer.top;
    else if (inner.bottom > outer.bottom)
      list.scrollTop += inner.bottom - outer.bottom;
  }, [selected, page]);
  const word = selected === null ? null : words[selected];
  const recognized = recognition?.status === "recognized";
  const choose = (index: number) => {
    setSelected(index);
    setPage(Math.floor(index / 50));
  };
  const copy = async () => {
    if (!recognized || !recognition || error) return;
    setCopying(true);
    setCopied("");
    try {
      await navigator.clipboard.writeText(recognition.text);
      if (current.current)
        setCopied("Unreviewed OCR text copied to the clipboard.");
    } catch {
      if (current.current) {
        setCopied(
          "Clipboard unavailable. Use your system copy shortcut with the selected text.",
        );
        textRef.current?.focus();
        textRef.current?.select();
      }
    } finally {
      if (current.current) setCopying(false);
    }
  };
  return (
    <Dialog
      wide
      label="Image word regions"
      onClose={onClose}
      preventClose={copying}
    >
      <div className="processing-review region-review">
        <div className="processing-heading">
          <div>
            <p className="eyebrow">EVIDENCE / IMMUTABLE DERIVATIVE</p>
            <h2>Image word regions</h2>
          </div>
          <button
            className="close"
            disabled={copying}
            onClick={onClose}
            aria-label="Close image word regions"
          >
            ×
          </button>
        </div>
        <h3 className="processing-filename">{sourceName}</h3>
        <p className="processing-id">{extractionId}</p>
        {error ? (
          <div role="alert" className="error">
            <strong>Derivative could not be verified</strong>
            <p>{error}</p>
            <p>
              Preview, recognized text and word controls are unavailable. This
              is a read failure, not an empty recognition result.
            </p>
            <button
              className="button"
              onClick={() => setGeneration((n) => n + 1)}
            >
              Retry inspection
            </button>
          </div>
        ) : !loaded ? (
          <p role="status">Loading verified image regions…</p>
        ) : (
          value &&
          record &&
          decoder && (
            <>
              <div className="processing-banner">
                <strong>{outcome(value)}</strong>
                <p>
                  Unreviewed recognition. Selecting a word only inspects the
                  saved derivative. No accepted observation, original-document
                  anchor or confidence assessment is created.
                </p>
              </div>
              <dl className="processing-metrics">
                <div>
                  <dt>Recognized words</dt>
                  <dd>
                    {recognition
                      ? words.length.toLocaleString("en-US")
                      : "OCR did not run"}
                  </dd>
                </div>
                <div>
                  <dt>Raster</dt>
                  <dd>
                    {loaded.raster
                      ? `${loaded.raster.width} × ${loaded.raster.height} px`
                      : "Not retained"}
                  </dd>
                </div>
                <div>
                  <dt>Engine / language</dt>
                  <dd>
                    {recognition
                      ? `${recognition.engine} / English`
                      : "Not run"}
                  </dd>
                </div>
                <div>
                  <dt>Recorded attempt</dt>
                  <dd>{record.attempt}</dd>
                </div>
              </dl>
              {decoder.status !== "decoded" && (
                <p className="context-note">
                  {decoder.failure?.replaceAll("_", " ")}. Decoder rejection
                  retained this typed result only. No raster, TSV, recognition
                  or word boxes are available.
                </p>
              )}
              {loaded.raster && (
                <div className="region-explore">
                  <RegionCanvas
                    raster={loaded.raster}
                    words={words}
                    selected={selected}
                    setSelected={choose}
                    onError={setError}
                  />
                  <section
                    className="region-words"
                    aria-label="Words in reading order"
                  >
                    <h3>Words / reading order</h3>
                    <div className="region-list-heading">
                      <span>Word</span>
                      <span>Engine score</span>
                    </div>
                    <ol ref={listRef} start={page * 50 + 1}>
                      {words
                        .slice(page * 50, page * 50 + 50)
                        .map((item, index) => {
                          const absolute = page * 50 + index;
                          return (
                            <li key={absolute}>
                              <button
                                className="region-word"
                                data-word-index={absolute}
                                aria-pressed={selected === absolute}
                                aria-label={`Select word ${absolute + 1}: ${item.text}`}
                                onClick={() => choose(absolute)}
                              >
                                <span className="region-word-number">
                                  {absolute + 1}
                                </span>
                                <span>{item.text}</span>
                                <span>{item.engine_confidence}</span>
                              </button>
                            </li>
                          );
                        })}
                    </ol>
                    {!words.length && (
                      <p>
                        No word boxes were returned. An empty OCR result does
                        not establish that the original contains no relevant
                        information. Inspect the retained raster.
                      </p>
                    )}
                    <p className="region-count">
                      {words.length
                        ? `${page * 50 + 1}–${Math.min(page * 50 + 50, words.length)} of ${words.length}`
                        : "0 of 0"}{" "}
                      words
                    </p>
                    {words.length > 50 && (
                      <div className="region-pagination">
                        <button
                          className="button"
                          disabled={page === 0}
                          onClick={() => setPage((n) => n - 1)}
                        >
                          Previous words
                        </button>
                        <button
                          className="button"
                          disabled={(page + 1) * 50 >= words.length}
                          onClick={() => setPage((n) => n + 1)}
                        >
                          Next words
                        </button>
                      </div>
                    )}
                  </section>
                </div>
              )}
              {word ? (
                <section className="region-selected" aria-label="Selected word">
                  <h3>
                    Selected word {selected! + 1} / {word.text}
                  </h3>
                  <strong>Engine score {word.engine_confidence} / 100</strong>
                  <p>
                    An engine score is not a probability or analyst confidence.
                  </p>
                  <dl className="processing-facts">
                    <dt>Raster rectangle</dt>
                    <dd>
                      Left {word.bounds.left} · top {word.bounds.top} · width{" "}
                      {word.bounds.width} · height {word.bounds.height}
                    </dd>
                    <dt>Engine hierarchy</dt>
                    <dd>
                      Raster page {word.page_number} · block {word.block_number}{" "}
                      · paragraph {word.paragraph_number} · line{" "}
                      {word.line_number} · word {word.word_number}
                    </dd>
                  </dl>
                  <p>
                    Selecting a word only highlights its box; the immutable
                    derivative is unchanged.
                  </p>
                </section>
              ) : (
                !!words.length && (
                  <p className="context-note">
                    Select a word to inspect its exact raster coordinates and
                    raw engine score.
                  </p>
                )
              )}
              <div className="processing-heading">
                <h3>Recognized text / read only</h3>
                <button
                  className="button"
                  disabled={!recognized || copying}
                  onClick={() => void copy()}
                >
                  Copy text
                </button>
              </div>
              {recognized ? (
                <textarea
                  ref={textRef}
                  className="processing-text"
                  readOnly
                  value={recognition!.text}
                  aria-label="Unreviewed OCR text"
                />
              ) : (
                <p>
                  {recognition
                    ? "Whitespace-only recognition remains in the immutable result. No useful text was recognized."
                    : "Recognition did not run for this rejected image."}
                </p>
              )}
              {copied && <p role="status">{copied}</p>}
              <dl className="processing-facts">
                <dt>Original SHA-256</dt>
                <dd>
                  <code>{record.input.sha256}</code>
                </dd>
                <dt>Original bytes</dt>
                <dd>{record.input.bytes}</dd>
                <dt>Raster SHA-256</dt>
                <dd>
                  <code>{record.raster?.sha256 ?? "No raster retained"}</code>
                </dd>
                <dt>TSV SHA-256</dt>
                <dd>
                  <code>{record.tsv?.sha256 ?? "No TSV retained"}</code>
                </dd>
                <dt>Result SHA-256</dt>
                <dd>
                  <code>{record.result.sha256}</code>
                </dd>
              </dl>
              <details className="processing-details">
                <summary>Full processing provenance and limitations</summary>
                <p>
                  Raster coordinates describe encoded image pixels. EXIF
                  orientation was not applied; transparency is composited onto
                  white before grayscale conversion. Source image index zero and
                  TSV page one are not document pages or reviewed source
                  anchors. English recognition uses a single uniform text block.
                </p>
                <dl className="processing-facts">
                  <dt>Extraction ID</dt>
                  <dd>{record.id}</dd>
                  <dt>Job ID</dt>
                  <dd>{record.job_id}</dd>
                  <dt>Evidence ID</dt>
                  <dd>{record.input.evidence_id}</dd>
                  <dt>Created at</dt>
                  <dd>{record.created_at}</dd>
                  <dt>Source image index</dt>
                  <dd>{decoder.raster?.source_image_index ?? "No raster"}</dd>
                  <dt>Decoder</dt>
                  <dd>{decoder.decoder}</dd>
                  <dt>Java runtime</dt>
                  <dd>{decoder.java_runtime}</dd>
                  <dt>Decoder worker job</dt>
                  <dd>{decoder.job_id}</dd>
                  <dt>OCR worker job</dt>
                  <dd>{recognition?.job_id ?? "Not run"}</dd>
                  <dt>Model SHA-256</dt>
                  <dd>{recognition?.model_sha256 ?? "Not run"}</dd>
                  <dt>Runtime manifest SHA-256</dt>
                  <dd>{recognition?.runtime_manifest_sha256 ?? "Not run"}</dd>
                  <dt>Retained raster bytes</dt>
                  <dd>{record.raster?.bytes ?? "Not retained"}</dd>
                  <dt>Retained TSV bytes</dt>
                  <dd>{record.tsv?.bytes ?? "Not retained"}</dd>
                  <dt>Retained result bytes</dt>
                  <dd>{record.result.bytes}</dd>
                </dl>
                <p>
                  Embedded previews and image metadata were not extracted. Word
                  regions and recognition remain unreviewed; raw engine
                  confidence has no acceptance threshold. The retained
                  derivative does not change the original, accepted findings or
                  previous report snapshots.
                </p>
              </details>
            </>
          )
        )}
      </div>
    </Dialog>
  );
}
