# Image OCR review

This surface adds an explicit image-processing method to the document queue and an immutable recognition inspector. It uses the Rust canonical job and image-extraction contracts. It does not accept extracted facts or provide document-page anchors.

## Editable design

[13 / Instrument — image OCR review, frame 34:885](https://www.figma.com/design/O50ISV0LGG8nKDcFORNMbO?node-id=34-885) is a native editable Figma frame, 920×1100, using the existing graphite/amber instrument system. Its text, rules, controls and measurement cells remain editable. The [export](review/image-ocr/figma-image-ocr-review.png) was visually checked and has SHA-256 `74941d5c99ca6421e134be4a9695d407b0a9a7b0b464e7cb31a041d911c35523`.

The specimen establishes the hierarchy: original name, explicit unreviewed status, engine/language/attempt cells, three separate object digests, raster/orientation limitations, read-only recognized text and a deliberate copy action. An alternate empty-recognition state is shown below the main specimen for comparison. Design hashes, counts and text are illustrative synthetic values. No owner acceptance or interactive Figma prototype is claimed.

The application follows that hierarchy and exposes complete worker identities, timestamps, model/runtime digests and all recorded limitations in an expandable provenance section. Its ordinary viewport scrolls; the full-height comparison capture exposes the complete surface. Empty and failed states replace the text area rather than appearing alongside a successful result. These are deliberate implementation differences from the condensed specimen.

## Interaction and state

The processing-method selector offers document parsing or English image OCR for a single PNG/JPEG. It does not infer support from a filename or imply PDF OCR. Queue request keys are scoped to both the original and processing method. A lost acknowledgement can be recovered after navigation without confusing the two operations. This identity lasts within the open application; it is not persisted through a full application restart.

Image review distinguishes recognized text, completed recognition without text, unsupported format, malformed decoding and exhausted image limits. No-text and failure states disable Copy and explicitly refuse an inference that the original contains no relevant information. Unsupported/failed decoding does not claim that a raster was produced or validated.

Original, raster and result digests have separate labels. The decoder binds the original to the normalized raster; recognition binds to that raster. The raster was validated during processing but its bytes are not retained. The stored binding is not an exhibit or page image. Encoded source index zero is not a document page. EXIF orientation is unapplied, transparency is composited onto white and the OCR input is grayscale. No word regions or verified source anchors are created.

Text remains inert in a read-only textarea. No collected image, SVG, HTML, script or link is executed. Copy requests the system clipboard only after an explicit action; refusal selects the text for the platform copy shortcut. The original, derivative and accepted observations are unchanged. Retry preserves previous results and their recorded attempts, including a failed derivative followed by successful recognition.

The existing modal focus containment, Escape handling, close restoration, serial reads and stale-response invalidation apply. At narrow widths, measurement cells and fact labels stack and identifiers wrap. The full provenance disclosure uses the native HTML details/summary keyboard control.

## Verification boundary

`ui/tests/image-jobs.spec.ts` uses the actual Rust development harness and fixed canonical image-job specimens. The fixture helper imports retained synthetic PNG/GIF bytes, reserves and claims real job records, and publishes fixed validated protocol values. No OS worker is launched by this helper; its recognized text explicitly identifies it as a UI specimen. Native image/OCR/coordinator execution is verified separately.

The browser workflows cover exact provenance, inert hostile-looking text, clipboard copying, no implicit observations or external requests, distinct outcomes, preserved earlier attempts, method-specific lost acknowledgement recovery after navigation, keyboard focus, compact overflow and scoped accessibility. The tests never fabricate an API response; the lost-response test executes the real request and drops only its transport acknowledgement.

Runtime accuracy, page/word anchors, PDF rasterization, other languages, accepted extraction authoring, screen-reader review, real browser zoom, Windows/WebView2 and Intel native interactions remain separate work. Automated axe checks and a visual comparison cannot establish those outcomes or release acceptance.

## Retained comparison evidence

The [checksum manifest](review/image-ocr/checksums.json) binds the Figma export, eight application crops and three scoped accessibility results. Review includes [desktop recognition](review/image-ocr/recognized-desktop.png), [full-height recognition](review/image-ocr/recognized-full.png), [expanded provenance](review/image-ocr/recognized-provenance.png), [compact recognition](review/image-ocr/recognized-compact.png), [no recognized text](review/image-ocr/no_text_recognized.png), [unsupported input](review/image-ocr/unsupported.png), [malformed input](review/image-ocr/failed.png) and [pixel quota](review/image-ocr/quota_exhausted.png).

The root visual pass compared hierarchy, hard edges, measurement cells, three distinct hashes, inert text, focus controls and explicit limitations against the editable frame. Full recognition uses a 1440×1800 comparison viewport; expanded provenance uses 1440×2600 so the final model/runtime digests remain inspectable. Ordinary desktop is 1440×1000 and compact is 720×900. The compact/short viewport checks verify horizontal containment, not real browser zoom.

All five new image browser workflows passed against the actual canonical commands. The combined suite passed 35 workflows, including existing document parsing, statements, transactions, collection, identities and assessments. Three scoped axe runs recorded zero violations: desktop, compact, and compact with expanded provenance. The combined core passed 125 ordinary tests, with 12 runtime-dependent tests explicitly excluded from that ordinary run; strict Clippy, UI production build and native host compilation passed. Separate native execution results are recorded in the processing and verification documentation.

Peer review corrected two cases before these captures: whitespace-only `no_text_recognized` output now disables Copy and shows the empty state while preserving immutable whitespace, and multiple-image rejection uses the broader heading “Image input unsupported.” Neither change alters canonical source bytes or worker statuses.
