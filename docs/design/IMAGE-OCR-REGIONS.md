# Image OCR word regions — editable design handoff

This design adds an explicit, opt-in image OCR method that retains a local
grayscale raster, exact TSV and immutable recognition result. Analysts can
inspect a word alongside its raster box. Selection is read-only: it does not
accept an observation, create an original-document anchor, correct OCR text or
set analyst confidence. The development interface implements these controls with real canonical
reads and synthetic browser verification. Native transport and supported-platform
execution retain separate verification steps.

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
| [19 / Instrument — Image region workflow states, 45:1399](https://www.figma.com/design/O50ISV0LGG8nKDcFORNMbO?node-id=45-1399) | 1120 × 1200 | [Editable vector source](review/image-ocr-regions/source-states.svg), [recovered native Figma 1× PNG](review/image-ocr-regions/figma-states.png) |

In the initial campaign, the first two PNGs were obtained through Figma's Copy as PNG action and
visually inspected. Their dimensions are 2240 × 2540 and 1280 × 2960 pixels.
The third frame's complete canvas, editable text children, name and position
(31420, −500) were observed through native Chrome controls. During final
export Figma reported a connection issue affecting saving; browser controls
also timed out. Its native PNG export and remote sync are therefore
unconfirmed. The original Figma tab was not intentionally reloaded or closed.
The committed SVG sources preserve all three designs independently of that
connection. That failed export/sync observation is retained as history.

On 25 September, a separate fresh view in the original signed-in browser
profile loaded the saved document and frame 45:1399 with its original name,
1120 × 1200 dimensions and (31420, −500) position. Its complete canvas was
inspected, then Figma's native Export action produced the PNG linked above.
The extracted 105,881-byte image was inspected at its actual dimensions with
no clipped text or overlapping controls observed. This verifies the third
frame's saved remote content and native export without reloading, closing or
overwriting either preserved original tab. The desktop and compact frames
were listed in the fresh editor; their earlier PNGs were not re-exported in
this recovery check. The [checksum manifest](review/image-ocr-regions/checksums.json)
keeps the initial failure and this narrower later observation distinct.

These Figma frames are design review specimens. Application comparison
artifacts and scoped accessibility results are recorded separately below;
neither establishes owner acceptance or complete release approval. The third
frame's export and remote-save uncertainty is resolved; wider design and
accessibility acceptance remains separate.

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

The display adapter calls the full-chain verified raster reader through a
read-only coordinator accessor. Native `image_region_raster(extraction_id)`
returns a Tauri binary response; its only input is a canonical extraction
digest. The development-only same-origin route calls the same workspace
reader through a fixed debug CLI branch. Neither accepts a frontend file path,
`file://` URL, arbitrary file reader or worker-provided network URL.

The browser accepts at most 12,000,032 response bytes, requires canonical P5
headers, dimensions of 1–8192 and at most 12 million pixels, verifies the exact
pixel count, and checks SHA-256 against the inspected immutable record before
allocating RGBA pixels. WebCrypto absence fails explicitly. RGBA storage is
bounded to 48 million bytes; this is not a whole-process memory claim.

The development bridge permits only loopback same-origin POSTs with a bounded
extraction-only JSON body. It buffers bounded child output and returns binary
bytes only after successful process closure. Timeout, output/stderr overflow
and client disconnection stop the child; closure is observed before any
response is published. Errors are generic, bounded and excluded from binary
stdout. The helper accepts no extra CLI arguments and is absent from release
builds.

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

## Implementation verification

The browser suite uses actual Rust inspection/queue commands and the fixed
debug-only `seed-image-region-review` helper. The helper requires an empty
workspace and uses canonical import, queue, claim, finish, cancellation and
retry methods. Its twelve jobs and eight derivatives are explicit synthetic
state specimens; no worker runs. Actual confined decoder/OCR execution remains
separate native evidence. The box coordinates and recognition in these
specimens test interface binding, not OCR fidelity to the displayed raster.

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


The production UI build passed, with the existing large-bundle warning. All
59 browser workflows passed: the 50 previous workflows and nine region cases.
Those nine cover actual binary byte equality and malformed P5/digest refusal;
separate provenance; inert text and Copy; fit/100% box hit testing and unchanged
revision; complete paginated word counts; compact keyboard/focus/overflow;
whitespace-only and decoder rejection states; actual missing raster and altered
TSV refusal; late binary completion after closing; lost queue acknowledgement
across navigation and terminal state; canonical cancel/retry; same-origin input
refusal; and explicit failure without WebCrypto. No external requests were
observed in the recognized-result workflow.

The first targeted run passed seven of eight tests. The delayed-response test
ended before its intercepted real response finished, causing Playwright to
report an already-handled route during teardown. The test now awaits that
actual fulfillment before checking the new inspector; the next eight tests
passed. Adding the WebCrypto case produced nine passes, followed by the full
59-test pass. Visual inspection then verified the four-column desktop metrics,
selected-row highlight and list-scroll reset included in the final full run.

After that browser pass, peer review added a narrow development-bridge guard:
already-aborted requests or destroyed responses are refused before child
creation. No browser rerun is attributed to that subsequent guard.

Two focused Rust tests passed for canonical seed publication and the
coordinator's read-only binary accessor, including revision preservation and
shutdown refusal. Strict debug and release core Clippy passed. The first full core test run
exposed an existing coordinator restart ownership-lock failure: 106 passed,
one failed and 21 native tests ignored. Its exact failure excerpt is retained
in [core-initial-failure.txt](review/image-ocr-regions/core-initial-failure.txt).
The coordinator owner reproduced inherited-lock contention with an actual
fork and repaired explicit release after joined shutdown in `199e34ec`. That
repair was integrated locally as `a8d7db8`. The final ordinary core suite
passed 176 tests (110 unit and 66 integration); all 21 native tests remained
ignored. The earlier failed run remains visible and is not relabelled.

The retained comparisons include the [desktop viewport](review/image-ocr-regions/recognized-desktop.png),
[complete desktop inspector](review/image-ocr-regions/recognized-full.png),
[selected-word/text details](review/image-ocr-regions/recognized-details.png),
and [compact expanded inspector](review/image-ocr-regions/many-compact.png).
Alternate captures show [no text](review/image-ocr-regions/empty.png),
[unsupported input](review/image-ocr-regions/unsupported.png),
[decode failure](review/image-ocr-regions/failed.png),
[resource exhaustion](review/image-ocr-regions/quota.png), and
[unavailable storage](review/image-ocr-regions/unavailable.png).

The application retains the existing 920px modal width, adapts the desktop
frame's side-by-side sections, and stacks them in compact windows. Ordinary
checks use 1440×1000 and 720×900 viewports, plus 720×500 containment. Complete
captures use 1440×2600 and 720×3800 to expose scrollable contents without changing
application styles. These dimensions do not simulate browser zoom. Both scoped
[desktop](review/image-ocr-regions/accessibility-desktop.json) and
[compact](review/image-ocr-regions/accessibility-compact.json) axe checks report
zero WCAG 2 A/AA and 2.1 AA violations.

A full native Tauri binary-transport run, native WebKit/WebView2 behavior,
platform memory measurements, manual screen-reader review, restored Figma
remote sync and the final states-frame export remain separate verification
work. No accepted word anchors, OCR accuracy claim or complete release pass is
established by this increment.


### Combined native verification

Root integration `3536859` passed all 60 real-core browser workflows, including the final disconnect guard. The native Mac app built at clean `0b4125e` then ran actual decoder/OCR workers and displayed the verified binary raster with seven recognized words. Exact box selection, fit/100% viewing, text-copy acknowledgement and normal quit/restart retention were observed. See [the integrated verification record](../VERIFICATION.md#native-image-region-review-integration--25-september-2026). This closes the current Mac development binary-transport gap only; Figma remote sync, the third frame export and cross-platform release acceptance remain open.
