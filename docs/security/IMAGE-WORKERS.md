# Development PNG/JPEG decoder boundary

This adapter decodes a single PNG or JPEG into a bounded, canonical grayscale
raster for the existing app-local OCR worker. It is a development foundation on
the tested Apple Silicon Mac. It does not establish a signed macOS helper,
Windows/Intel compatibility, a clean installed distribution, PDF rendering or
general image-format support.

## Data flow and identity

`Runtime::decode_image` (or `decode_image_with_cancel`) gives a separate disposable
Java process a private copy of the original, one request and its private output
area. The `image` classpath contains `ImageWorker`, the bounded protocol adapter
and three Jackson JARs. It has no Tika, PDFBox or Lucene dependency. The worker
selects only the JDK PNG/JPEG reader classes from the `java.desktop` module.
`Runtime::ocr_image` starts the decoder, validates its output and confirms its
exit/cleanup, then starts a separate Tesseract process with the raster. Neither
worker accesses canonical storage. Unsupported or failed images do not enter OCR.

The strict versioned `ImageDecodeResult` records the exact original SHA-256 and
byte length, decoder/runtime identities, media type, outcome, failure code and
limitations. A decoded result also contains the exact raster SHA-256, length,
width, height, `source_image_index: 0` and
`pixel_mapping: encoded_pixels_gray_white_alpha_v1`. Rust validates the result
against both original and raster bytes before returning it. Unknown fields,
invalid UUIDs, an unexpected job, wrong hashes/dimensions, inconsistent outcomes
and incomplete limitations are rejected. The OCR result's `raster_sha256` binds
recognition to that derivative. The two hashes must retain their distinct names.

Raster coordinates retain the encoded image's pixel order and dimensions: no
rotation, resize or crop is applied. EXIF orientation is deliberately **not
applied**; an orientation-tagged JPEG test verifies this behavior. This is source
image index zero, not an invented PDF page number or document-page mapping.
Embedded thumbnails/previews and metadata are not extraction outputs. Word
regions, page anchors, accuracy guarantees and analyst acceptance remain absent.
The OCR result retains its existing no-original-document-mapping limitation;
callers can associate it with this separately validated image binding.

Each source pixel is read through `BufferedImage.getRGB` in the default RGB color
model. For each color channel `c` and alpha `a` (integers 0–255), the white-alpha
composite is `(c*a + 255*(255-a) + 127)/255`, with integer division. Grayscale is
`(299*r + 587*g + 114*b + 500)/1000`. The derivative is exactly
`P5\n{width} {height}\n255\n` followed by `width*height` bytes. A 2×2 synthetic
RGBA fixture verifies coordinate order and exact red/green/blue/transparent output.

## Bounds and failures

| Boundary | Limit / behavior |
|---|---|
| Original | 16 MiB; PNG signature or JPEG signature; other formats are unsupported |
| Dimensions | 1–8192 pixels per axis, at most 12,000,000 pixels |
| Images | Exactly one; APNG and concatenated JPEG images are unsupported |
| PNG container | At most 1024 chunks; bounded lengths, checked CRCs, one initial IHDR, exact IEND |
| Output | Canonical PGM at most 12,000,032 bytes; typed JSON at most 4096 bytes |
| Process | 30-second wall budget, 30-second CPU limit, 256 MiB Java heap |
| Per-file writes | Kernel `RLIMIT_FSIZE` of 12,000,032 bytes for this assignment |
| Shared supervision | Clean environment, closed inherited descriptors, 256 descriptors, no core dumps; sampled tree size/count/nesting limits |

Width/height are read and rejected before pixel decoding/allocation. The bounded
PNG container check also precedes ImageIO. Memory-cache input avoids ImageIO's
temporary-file cache fallback. ImageIO's `ignoreMetadata` argument is a hint;
color profiles and other internal decoder structures may still be processed in
the disposable worker. The Java heap and process budgets are not a hard bound on
resident/native memory. Sampled tree limits are not a hard aggregate disk ceiling.

Results distinguish `decoded`, `unsupported`, `failed` and `quota_exhausted`.
PNG CRC/container corruption, malformed images and decoder warnings are rejected
without publishing a raster. Resource exhaustion or unexpected worker exit can
also produce an engine error with no result. A warning is never silently reported
as complete decoding. Missing runtimes and unsupported platform confinement fail
closed; there is no PATH fallback, dependency download or provider request.

## Confinement and cleanup

The image worker uses the existing development `sandbox-exec` supervisor with a
separate `image` assignment. Reads cover only its Java/image runtime, assigned
original/request, scratch area and documented OS facilities. Writes cover only
`result.json`, `raster.pgm` and scratch. Original/input writes, other workers,
Lucene indexes, outside workspace contents, direct networking and process-fork
are not permitted. Headless Java operation is explicit. Shared launcher tests
continue to cover the independent parser and Lucene paths.

Cancellation and every other supervised outcome require explicit tracked-leader
termination/reaping before return. Unverified termination retains the private
assignment and rejects publication. Verified execution is followed by explicit
cleanup; cleanup failure rejects success. This is not a supervisor-crash
termination guarantee and does not establish a supported signed-helper sandbox.
The [macOS worker boundary](MACOS-WORKERS.md),
[parser boundary](PARSER-WORKERS.md) and [OCR boundary](OCR-WORKERS.md) retain
their respective platform and release limitations.

## Reproduction and evidence

Use an existing Java 21 runtime, an installed build JDK and the cached Jackson
2.22/2.22.3 dependencies. `scripts/stage_image_worker.py --help` describes the
explicit local paths. Staging makes no downloads and includes `HostileProbe`
solely for development verification. It does not constitute a production bundle
or authenticate arbitrary runtime contents. Runtime packaging/signing remains a
separate release task. Java and Jackson notices remain required as in the parser
distribution; this adapter adds no third-party image library.

`scripts/GenerateImageFixtures.java` regenerates the synthetic image fixtures
from the committed synthetic OCR raster using a development JDK. Run
`python3 scripts/test_image_workers.py --runtime <staged-engines>` with the
image, Java and existing app-local OCR assets staged. The runner moves those
assets to a new path, verifies the image classpath and OCR inventory, then runs
the real decoder/OCR and same-assignment hostile worker cases. Only disposable
sentinels and a local test listener are used. Tests cover outside/input writes,
network/fork/environment denial, observed cancellation/reaping and the kernel
file-size ceiling, alongside synthetic successful and rejected images.

Timestamped `artifacts/image/*.json` observations identify source hashes,
dirty/clean revision, fixture hashes, Java launcher/modules/release hashes,
adapter/Jackson JAR hashes and the OCR runtime manifest hash. They explicitly
keep `complete_release: false`. Preflight failures, tool errors, timeouts and
incomplete suites replace the latest observation with failure while retaining
history. `scripts/test_image_evidence.py` verifies that behavior. Raw local test
output stays in ignored artifacts; sanitize any material selected for publication.

Primary API references: Oracle's
[ImageReader](https://docs.oracle.com/en/java/javase/21/docs/api/java.desktop/javax/imageio/ImageReader.html),
[MemoryCacheImageInputStream](https://docs.oracle.com/en/java/javase/21/docs/api/java.desktop/javax/imageio/stream/MemoryCacheImageInputStream.html)
and [BufferedImage](https://docs.oracle.com/en/java/javase/21/docs/api/java.desktop/java/awt/image/BufferedImage.html).
