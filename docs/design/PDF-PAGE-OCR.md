# PDF page OCR — design and review

The Evidence area adds an explicit PDF page OCR method. An analyst selects a
retained original, a 1-based page number and render resolution before reserving
a job. The immutable derivative inspector shows the page-render outcome,
English recognition and their separate provenance. It does not accept OCR
text, create verified word regions or expose raw PDF content in the interface.

## Editable design

The following root-level frames were authored in the existing Figma file as
native editable text and vector layers, then inspected and exported through
Figma's Copy as PNG action:

| Frame | Purpose | Dimensions |
|---|---|---|
| [15 / Instrument — PDF page OCR review, 42:1098](https://www.figma.com/design/O50ISV0LGG8nKDcFORNMbO?node-id=42-1098) | Explicit unreviewed result, three hashes, geometry, inert text and alternate outcomes | 920 × 1220 |
| [16 / Instrument — PDF page queue, 42:1151](https://www.figma.com/design/O50ISV0LGG8nKDcFORNMbO?node-id=42-1151) | Page/DPI controls, one-page scope and method-specific job ledger | 1120 × 630 |

The [review export](review/pdf-page-ocr/figma-pdf-page-ocr.png) and
[queue export](review/pdf-page-ocr/figma-pdf-page-queue.png) use the established
graphite, paper and amber instrument system: square controls, thin rules,
joined measurements, restrained warning fills and Inter/JetBrains Mono.
The PNGs are native 2× exports: 1840 × 2440 and 2240 × 1260 pixels.
All names, hashes, text and measurements in the design are illustrative
synthetic specimens. Editable frames and exported comparisons do not establish
owner acceptance or native-platform accessibility.

The implementation follows the specimens' hierarchy. The inspector adds the
complete canonical identifiers, timestamps, renderer/Java/OCR identities and
model/runtime digests under an expandable provenance section. Alternate
outcomes replace the successful text surface. The design groups alternate
states together only to make them reviewable. Ordinary desktop windows scroll;
full-height comparison captures expose the complete surface.

## Queue and attempt semantics

The page field starts empty. Resolution has a visible initial value of 144 DPI.
The analyst must choose an integer page from 1–1,000 and an integer resolution
from 72–300 DPI. The interface does not infer page count from a filename or
automatically expand a request to other pages. Actual document page count and
rendering limits are checked by the confined processing path. A selected page
outside that count remains an explicit failed result.

The queue request identity includes method, evidence ID, page and DPI. Recovery
of an uncertain acknowledgement reuses the original canonical UUID for exactly
that operation. Changing any of those selections creates a separate pending
identity. Known active jobs disable duplicate submission only for the same
selection. Pending identities survive section navigation within the open
application, but not a full reload/restart. The durable job ledger remains the
source of truth after a restart.

Cancellation, retry reservations, stale attempts, suspended processing and
unverified worker exit use the existing [document job controls](DOCUMENT-JOBS.md).
Each ledger and attempt record includes the PDF page and DPI. Earlier
derivatives stay immutable and display their own recorded attempts. No
derivative is invented for a failed worker without canonical publication.

## Result and provenance semantics

The inspector distinguishes recognized text, no text recognized, encrypted
PDFs, unsupported inputs/features, failed rendering and exhausted limits.
Rendering rejections explicitly say OCR did not run. Empty recognition can
retain immutable whitespace without enabling Copy or displaying it as useful
text. Neither empty recognition nor rejection establishes an absence of
relevant information on the page or elsewhere in the original.

Original, raster and result SHA-256 values have separate labels. For a rendered
page, the geometry panel shows raster dimensions, effective CropBox in points,
quarter-turn rotation and the six affine coefficients without display rounding.
The original page interpretation comes from the renderer; the interface does
not independently parse the PDF or establish pixel equivalence to other
viewers. Recognition binds to the raster digest, while the renderer separately
binds that raster to the original and selected page. This is not a verified
text-to-region anchor.

The validated raster bytes are discarded. Its retained binding is not an
available image or exportable exhibit. The inspector does not invent an image
preview. The scan subset, excluded annotations, grayscale conversion,
unreviewed raster and lack of word regions remain explicit. Other formats,
languages, general PDF compatibility and verified region authoring retain
separate implementation and release gates.

Text is inert in a read-only textarea. No PDF, HTML, SVG, script, image or link
from collected content is executed or fetched. Copy is a deliberate action;
clipboard refusal selects the text for the system copy shortcut. It changes
neither original nor derivative and publishes no accepted observation.

The shared serial-read hook invalidates obsolete replies on unmount or
replacement. Failed PDF inspection hides the unavailable result and offers a
real retry. The nested dialog restores focus to its opener. The shared focus
loop includes native details/summary controls so provenance is reachable by
keyboard. Compact layouts stack geometry and facts and wrap long identifiers.

## Verification boundary

`ui/tests/pdf-jobs.spec.ts` uses actual Rust commands and the fixed debug-only
`seed-pdf-processing-review` helper. The helper's canonical specimens are
explicitly synthetic; they do not claim that a worker rendered or recognized
their bytes. Actual confined renderer/OCR execution belongs to the separate
native suite. Transport tests delay or discard real responses; they never
fabricate a job or extraction payload.

The executed browser checks cover exact source/raster/result identities and
geometry; inert text and explicit copying; no-text/encrypted/unsupported/failed/
quota states; integer input boundaries; real queue/cancel/retry; method,
original, page and DPI acknowledgement identity; preserved earlier results;
late replies and read errors; keyboard provenance and focus restoration;
compact containment; and scoped axe accessibility checks.

The UI was tested against the signed durable-core implementation `2a835128`,
using the actual development executable. Eight PDF browser workflows passed.
The complete browser suite passed 50 workflows, including all 42 existing
document, image, collection, transaction, identity and assessment regressions.
The UI production build passed; its existing large-bundle warning remains.

The first targeted run passed six of eight workflows. Both failures identified
the same actual contrast defect: a shared heading selector overrode the
geometry panel's light text. A more specific selector restored the intended
Figma color. The next targeted run passed all eight, followed by the complete
50-workflow pass. Tests were not weakened to accommodate the defect.

## Retained comparison evidence

The [checksum manifest](review/pdf-page-ocr/checksums.json) binds both native
Figma exports, eleven application captures and five scoped accessibility
results. The manual visual comparison checked hierarchy, hard edges, readable
geometry, separate hashes, explicit outcomes, text containment and controls.

| Surface | Application evidence |
|---|---|
| Queue and job ledger | [Desktop](review/pdf-page-ocr/queue-desktop.png), [compact](review/pdf-page-ocr/queue-compact.png) |
| Recognized page | [Desktop viewport](review/pdf-page-ocr/recognized-desktop.png), [complete inspector](review/pdf-page-ocr/recognized-full.png), [expanded provenance](review/pdf-page-ocr/recognized-provenance.png), [compact viewport](review/pdf-page-ocr/recognized-compact.png) |
| No recognized text | [Whitespace-only immutable result](review/pdf-page-ocr/empty.png) |
| Rejected rendering | [Encrypted](review/pdf-page-ocr/encrypted.png), [unsupported](review/pdf-page-ocr/unsupported.png), [failed](review/pdf-page-ocr/failed.png), [quota exhausted](review/pdf-page-ocr/quota.png) |

Normal desktop captures use a 1440 × 1000 viewport; the complete inspector and
expanded provenance use 1440 × 1800 and 1440 × 2800 comparison viewports. Compact
captures use 720 × 900, with an additional containment and keyboard check at
720 × 500. Captures crop the relevant region/dialog; the full ledger extends
beyond the ordinary viewport. The application styles were not altered for
capture. These checks do not simulate browser zoom.

All five axe runs recorded zero violations under WCAG 2 A/AA and 2.1 AA:
[desktop queue](review/pdf-page-ocr/accessibility-queue-desktop.json),
[compact queue](review/pdf-page-ocr/accessibility-queue-compact.json),
[desktop inspector](review/pdf-page-ocr/accessibility-desktop.json),
[compact inspector](review/pdf-page-ocr/accessibility-compact.json) and
[expanded compact provenance](review/pdf-page-ocr/accessibility-compact-provenance.json).

Installed native UI, WebKit/WebView2, screen-reader use, real browser zoom,
Windows/Intel execution and signed offline distributions retain separate
verification gates. The fixed browser specimens establish interface behavior;
actual renderer and OCR execution is documented separately in
[PDF-JOBS.md](../processing/PDF-JOBS.md). This increment does not establish
general PDF support, retained raster exhibits, verified text regions or whole
release acceptance.
