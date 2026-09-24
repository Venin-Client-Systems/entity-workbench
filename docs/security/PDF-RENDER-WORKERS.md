# Confined PDF page rendering foundation

This development adapter renders one explicitly selected, 1-based PDF page into a canonical grayscale PGM raster. It can pass that raster to the separate packaged English OCR worker. It does not add a canonical job, command, review workflow or export format. It does not automatically accept OCR text or create word regions.

The tested mechanism is the existing experimental macOS `sandbox-exec` launcher. This is not the supported signed-helper release design. Other platforms fail closed. Signed helpers, both Mac architectures, Windows, clean installed packages and whole-release approval remain separate gates.

## Public engine contract

`Runtime::render_pdf_page(scratch, original_bytes, page_number, dpi)` and its `with_cancel` variant return `RenderedPdf { result, raster }`. The typed `PdfRenderResult` v1 contains:

- Job UUID, original SHA-256 and byte count, renderer identity and Java runtime version.
- Requested 1-based page number, requested DPI and the available page count.
- `rendered`, `encrypted`, `unsupported`, `failed` or `quota_exhausted`, with a typed reason for every non-rendered result.
- For success only: effective CropBox, normalized quarter-turn rotation, the PDF-to-raster affine, dimensions, canonical raster path, raster SHA-256 and byte count.
- Fixed limitations: scan-focused subset, annotations excluded, grayscale conversion, unreviewed raster and no word regions.

`pdf_render::validate_result(result, original_bytes, page_number, dpi, raster)` checks strict transport fields, original/request binding, geometry/dimensions consistency, exact raster bytes and honest status combinations. The geometry is the worker's extracted interpretation of the original. Validation does not independently parse the PDF in Rust or establish pixel-perfect equivalence to every other PDF viewer.

`Runtime::ocr_pdf_page` and its `with_cancel` variant run rendering and recognition sequentially in separate disposable processes. Rejected PDFs never enter OCR. A successful OCR result retains the exact raster hash; its `no_original_document_mapping` limitation remains intact. The enclosing render record links that raster to the PDF page. There are no word boxes or accepted page-region anchors. The caller receives a bounded in-memory raster; this adapter does not retain it in canonical storage.

## Supported rendering subset and provenance

The initial subset covers one or more raster image XObjects on a page and ordinary vector paths. Images must be 8-bit DeviceGray or DeviceRGB, with unfiltered, Flate or DCT/JPEG data. Flate predictor row selectors must be 0–4; selectors outside that range fail even if PDFBox would accept them. Non-image streams with prediction are unsupported. Flate predictor dimensions are checked, image byte counts must match the dimensions, and JPEG headers must agree with their PDF dictionaries. The exact JDK JPEG reader checks decoded dimensions, multiple images and warnings before PDFBox uses the image.

Fonts (including embedded fonts), form XObjects, inline images, masks, transparency groups, optional content, patterns, shadings, non-device color spaces, chained filters and optional image codecs are unsupported. No fallback/system font lookup is permitted: preflight rejects font resources and a refusing `FontMapper` provides a second check. Unknown content operators and non-image XObjects (including PostScript) are rejected. Every supported operator has explicit operand count/type checks; line caps/joins, widths, miter limits and bounded dash patterns are validated before dispatch. A bounded pass through PDFBox’s own streaming token parser applies those same checks and rejects leftover operands at end-of-stream. Missing resources and drawing errors fail the whole raster instead of using PDFBox's default log-and-continue behavior. General PDF compatibility is not claimed.

Preflight rejects document actions/scripts and external/embedded file specifications. The renderer does not execute document scripts, activate links or submit requests. Annotations are excluded explicitly, including their appearance streams. Conservative whole-document preflight can reject a selected page because another reachable resource uses an unsupported feature; this is visible as `unsupported`.

Only `UserUnit=1` and valid quarter-turn page rotation are supported. CropBox is the effective PDFBox box, clipped against MediaBox. Coordinates are PDF points; the raster's origin is top-left. The stored six-term affine uses `x'=a*x+c*y+e`, `y'=b*x+d*y+f`. The worker captures the actual rendering transform, preserving float-rounded box dimensions and subsequent translation order; it does not simplify those translations directly to the upper box coordinates. Core JSON parsing enables `float_roundtrip` so the exact Java doubles survive transport, including fractional box origins across zero. Scale uses the same single-precision `dpi/72` as PDFBox. Width and height are floored before 90/270-degree dimension swapping, with a minimum of one pixel; the derivative is not resized afterward. RGB pages have a white background and are converted using `(299*r + 587*g + 114*b + 500)/1000`.

The implementation follows the pinned [PDFBox 3.0.8 renderer](https://github.com/apache/pdfbox/blob/3.0.8/pdfbox/src/main/java/org/apache/pdfbox/rendering/PDFRenderer.java), [page geometry](https://github.com/apache/pdfbox/blob/3.0.8/pdfbox/src/main/java/org/apache/pdfbox/pdmodel/PDPage.java), [strict parser](https://github.com/apache/pdfbox/blob/3.0.8/pdfbox/src/main/java/org/apache/pdfbox/pdfparser/PDFParser.java) and [operator error handling](https://github.com/apache/pdfbox/blob/3.0.8/pdfbox/src/main/java/org/apache/pdfbox/contentstream/PDFStreamEngine.java). The [PDFBox 3 migration notes](https://pdfbox.apache.org/3.0/migration.html) describe the incremental parsing/cache behavior that limits our memory guarantees.

## Bounds and confinement

| Resource | Bound or behavior |
|---|---|
| Original PDF | Nonempty; at most 16 MiB; exact `%PDF-` prefix for supported format |
| Selected page | 1–1,000; document page count at most 1,000 |
| DPI | Integer 72–300; explicit analyst request, no automatic page selection |
| Page raster and each embedded image | At most 8,192 on either axis and 12,000,000 pixels |
| Result JSON | At most 8 KiB; unknown/duplicate/trailing fields rejected by typed deserialization |
| Raster | Exact P5 header followed by width × height bytes; at most 12,000,032 bytes |
| Reachable object preflight | At most 50,000 object identities; depth 64; arrays/dictionaries at most 50,000 entries |
| Stream preflight | 32 MiB aggregate unfiltered/Flate-expanded bytes; DCT counts compressed bytes and separately checks decoded pixels |
| Selected-page operators | At most 100,000; fixed allowlist and operand checks; dash patterns at most 64 entries; no form recursion |
| Worker | 30 seconds wall time, 30 seconds CPU, 256 MiB JVM heap; cleaned environment and inherited descriptors |
| Files | Input/request read-only; private scratch and two exact outputs writable; no workspace/index access |
| Network/processes | No direct networking or process fork; no external providers or runtime downloads |

PDFBox may parse/decompress cross-reference or object streams before preflight can walk reachable objects. The 32 MiB preflight budget is therefore not a universal decoder allocation ceiling. JVM heap, wall time, CPU and process confinement remain necessary. Hard RSS, instantaneous aggregate disk ceilings and automatic termination after a supervisor crash are not proven. Out-of-memory or abnormal worker exits produce a worker failure; they do not fabricate a successful typed quota result.

The PDF recipe has its own `pdf-render` classpath. It grants no parser, image-decoder, OCR or Lucene classpath access. The common supervisor checks output trees, limits individual files, explicitly stops/reaps cancellation and timeout cases, and verifies scratch cleanup. `TerminationUnverified` preserves the private assignment and blocks result acceptance; cleanup failure also rejects the result. Search-worker permissions are unchanged.

## App-local staging and dependency review

`scripts/stage_pdf_worker.py` uses an explicitly supplied installed compiler and existing local Java/JAR assets. It never downloads. Its dedicated component contains original adapter classes plus exactly:

| Dependency | Version |
|---|---|
| PDFBox, PDFBox IO, FontBox | 3.0.8 |
| Commons Logging | 1.4.0 |
| Jackson annotations | 2.22 |
| Jackson core, databind | 2.22.3 |

The stager extracts each JAR's available LICENSE/NOTICE files and hashes every component file into `manifest.json`. At execution the core verifies the complete bounded ordinary-file inventory, required JAR names and hashes, rejecting omitted, extra, changed or linked assets. This detects incomplete/corrupt staging; a mutable manifest is not signed publisher authentication. Java itself uses the existing trusted packaged-runtime assignment; native observations bind its executable, modules and release file hashes.

These dependencies carry Apache-2.0 primary licences; the unmodified PDFBox JAR also includes third-party data/font terms in its retained LICENSE/NOTICE, including the SIL Open Font License. Keeping the JAR intact and retaining those notices matters even though this profile refuses font rendering. Java and OCR runtime/model licences remain with their separately staged components. Development staging is not a complete approved distribution, release SBOM or signing result.

```sh
python3 scripts/stage_pdf_worker.py \
  --javac /path/to/jdk/bin/javac \
  --java-runtime /path/to/existing/java \
  --dependencies /path/to/cached/jars \
  --runtime runtime/development/engines
python3 scripts/test_pdf_evidence.py
python3 scripts/test_pdf_workers.py --runtime runtime/development/engines
```

The native suite also requires the already staged `ocr` component beside Java and the renderer. It relocates all three before execution, uses synthetic fixtures only, and records exact source, fixture, JAR/notice, Java and OCR manifest hashes. Failure observations replace the latest success and append collision-safe UUID history. Raw local diagnostics stay in ignored artifacts; sanitized JSON contains no user paths.

## Verification scope

The eight native/contract cases cover scanned Flate and JPEG PDFs followed by actual English OCR; selected second-page rendering; fractional crop geometry at 72 and 123 DPI for all four quarter-turns with actual black/white pixel checks, including a fractional box spanning zero; source/page/DPI/rotation/hash/dimension/result-schema tampering; encrypted PDFs with both nonempty and empty user passwords; malformed PDFs; missing resources; scripts and external-file references; unsupported fonts/filters/UserUnit/rotation/operators/PostScript XObjects; malformed drawing operands, extra operands and unfinished operand lists; valid and invalid PNG predictor selectors; page/pixel/operator/structure/stream quotas; mismatched JPEG dimensions and oversized JPEG headers; short and long Flate image data; and missing/modified/unlisted/linked runtime assets.

An actual hostile worker under the PDF recipe attempts disposable outside reads, original/input writes, sibling/index access, direct loopback networking, environment leakage and process creation. Cancellation and wall timeout verify the recorded worker PID is gone before cleanup. A deliberate output overrun verifies the per-file ceiling. These are development probes on the observed host, not independent security approval.

The source-level suite also checks missing-runtime fail-closed behavior and malformed result rejection. `test_pdf_evidence.py` covers rejected inventory, timeout, incomplete suites and two observations at the same clock tick. The parser/search native runner excludes the new PDF suite and remains independently runnable. Canonical PDF jobs, analyst page selection UI, reviewed regions, other PDF features and clean native release packages remain future work.
