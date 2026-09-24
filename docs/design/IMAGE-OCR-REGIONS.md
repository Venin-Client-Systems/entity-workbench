# Image OCR word regions — editable design handoff

This design adds an explicit, opt-in image OCR method that retains a local
grayscale raster, exact TSV and immutable recognition result. Analysts can
inspect a word alongside its raster box. Selection is read-only: it does not
accept an observation, create an original-document anchor, correct OCR text or
set analyst confidence. This increment contains design materials only; it does
not implement or verify the application interface.

## Editable frames and observation limits

Three frames were authored in the existing Figma file as native editable text
and vector layers. They extend the graphite, paper and amber instrument system
with square controls, thin rules, joined measurements and Inter/JetBrains Mono.
All displayed text, hashes, scores, dimensions and boxes are illustrative
synthetic specimens, not measured OCR results.

| Frame | Dimensions | Retained evidence |
|---|---|---|
| [17 / Instrument — Image word regions, 45:1199](https://www.figma.com/design/O50ISV0LGG8nKDcFORNMbO?node-id=45-1199) | 1120 × 1270 | [Native Figma 2× PNG](review/image-ocr-regions/figma-desktop.png), [editable vector source](review/image-ocr-regions/source-desktop.svg) |
| [18 / Instrument — Image regions compact, 45:1310](https://www.figma.com/design/O50ISV0LGG8nKDcFORNMbO?node-id=45-1310) | 640 × 1480 | [Native Figma 2× PNG](review/image-ocr-regions/figma-compact.png), [editable vector source](review/image-ocr-regions/source-compact.svg) |
| [19 / Instrument — Image region workflow states, 45:1399](https://www.figma.com/design/O50ISV0LGG8nKDcFORNMbO?node-id=45-1399) | 1120 × 1200 | [Editable vector source](review/image-ocr-regions/source-states.svg); visually inspected on the Figma canvas |

The first two PNGs were obtained through Figma's Copy as PNG action and
visually inspected. Their dimensions are 2240 × 2540 and 1280 × 2960 pixels.
The third frame's complete canvas, editable text children, name and position
(31420, −500) were observed through native Chrome controls. During final
export Figma reported a connection issue affecting saving; browser controls
also timed out. Its native PNG export and remote sync are therefore
unconfirmed. The original Figma tab was not intentionally reloaded or closed.
The committed SVG sources preserve all three designs independently of that
connection. The [checksum manifest](review/image-ocr-regions/checksums.json)
records exactly which native exports exist and leaves the third unverified.

These are design review specimens. No application screenshot, accessibility
pass, owner acceptance or completed feature is implied. The final states PNG
and remote save should be verified when the design connection is restored,
without creating duplicate frames.

## Queue and retained data

The Evidence method selector offers **Image OCR + word regions · English**
separately from existing text-only image OCR. Before queueing, a visible notice
states that this method retains the canonical grayscale raster, TSV and result
locally. It handles one retained original PNG/JPEG. No automatic acceptance,
PDF page inference, EXIF orientation application or additional language is
implied.

Pending queue identity includes both method and evidence ID. Reuse the existing
workspace-lifetime acknowledgement recovery and durable job controls. An
uncertain acknowledgement must recover its existing request UUID, including
after Evidence navigation. It must not silently queue a new job. A cancelled,
failed or retried job keeps its earlier immutable derivative identity.

The intended canonical contract is `QueueImageOcrRegions { evidence_id,
request_key }` and `InspectImageRegionExtraction { extraction_id }`. Inspection
returns `{ extraction, result }`. The extraction carries job, attempt, source,
timestamp and typed digest/length references; the result carries decoder and
optional recognition data. Successful and no-text recognition retain raster,
TSV and result. Decoder rejection retains the typed result only.

At this handoff the core owner has implemented a Rust-only verified raster
reader. A bounded display IPC adapter still needs integration. The interface
must receive verified bytes tied to the inspected extraction, never a path,
`file://` URL, arbitrary file reader or worker-provided network URL. The design
does not itself create that transport contract.

## Raster, list and selection

The desktop inspector places the raster and reading-order word list beside
each other. Compact windows stack them before selected-word details, recognized
text and provenance. The modal has one vertical document scroll; the raster
viewport can pan at 100%, and the word list shows explicit page/total counts.
Use bounded pages of 50 words rather than mounting up to 10,000 focusable rows.

The raster is a verified canonical P5 grayscale image rendered to a local
canvas. The implementation should parse its fixed bounded header and exact
pixel count, then use local pixel data. It must not render collected HTML,
SVG, original image metadata or remote image references. Fit and 100% are
explicit view controls. Hide boxes changes presentation only. No control
mutates the original or saved recognition.

Box geometry uses the retained raster's encoded pixel dimensions, top-left
origin, and half-open right/bottom bounds. Scaling for display must use the
same transform for pixels, overlays and pointer hit testing. EXIF orientation
was not applied. `source_image_index = 0` identifies the original image; TSV
page 1 identifies its raster and is not a PDF page. The synthetic 1200 × 230
specimen is drawn at half scale, so the selected box's original coordinates
(296, 86, 140, 54) correspond to its displayed rectangle (148, 43, 70, 27).

Word text, engine score and hierarchy are displayed from the immutable typed
result. Preserve leading zeros and inert script-like text. Label the score
column **Engine score**; the selected detail shows the unrounded raw score on
its 0–100 scale and explicitly says it is neither a probability nor analyst
confidence. There is no score threshold, inferred correctness classification
or hidden word suppression.

A row or box selects the corresponding word, updates its detail and highlights
the matching rectangle. Selection has no publication side effect. Keyboard
selection must stay in the word list; selected styling must not rely on color
alone. A selected word on another list page moves to that explicit page. Zoom,
box visibility and pagination must not change the selected word's identity.

## Result states and provenance

| State | Visible result |
|---|---|
| Recognized | Verified local raster, word list, selected-word detail, inert recognized text and explicit Copy action |
| No text recognized | Retained raster remains inspectable; zero words; no invented boxes; Copy disabled; immutable whitespace is preserved in the result but not presented as useful text |
| Unsupported, failed or quota-limited decoder | Typed reason and result identity; no raster, TSV, recognition or word controls |
| Missing or altered published derivative | Explicit unavailable read state; hide previous preview/text/boxes; retry inspection and inspect job remain available |
| Worker failure without publication | Job failure only; do not invent an extraction or preview |
| Worker exit unverified / recovery required | Exceptional job failure and retained-file warning; do not describe the worker as stopped or ordinary retry as available |

Empty recognition is not evidence that the source contains no relevant
information. An unavailable derivative is not empty recognition. A read retry
is separate from a new processing attempt and does not repair immutable files.

Original image, retained raster, exact TSV and result JSON each have their own
digest and byte count. Full provenance should include extraction/job/attempt
identity, source index, decoder and OCR runtime/model identities, dimensions,
limitations and creation time. Long identifiers wrap without truncating their
copyable values. Decoder and recognition are distinct worker results. The
engine's region hierarchy is preserved; it is not a reviewed source anchor.

## Implementation verification required

The browser suite must use real Rust inspection/queue commands and canonical
synthetic fixtures. Fixed state specimens must say no worker ran; actual
confined decoder/OCR execution remains separate native evidence.

Required checks include exact original/raster/TSV/result bindings; leading-zero
and script-like text; selected-box geometry at fit and 100%; complete word
denominators across pagination; no-text whitespace; result-only rejections;
missing/altered raster and TSV failures; stale inspection/raster replies after
selection, retry, dialog close or unmount; source/attempt changes; lost queue
acknowledgement across navigation; inert Copy; and no external requests.

Keyboard tests must cover list selection, pagination, provenance, Escape,
focus containment and return to the opener. Compact tests must check horizontal
overflow, long words/digests and small-height scrolling. Scoped axe checks and
rendered application comparisons against these Figma frames are required
before an implemented UI handoff. Native WebKit/WebView2 behavior, real browser
zoom, screen-reader use and supported-platform packaging remain distinct
verification obligations.
