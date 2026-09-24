# Confined OCR word-region engine

`Runtime::ocr_regions` and `ocr_regions_with_cancel` return a separate
`ocr_regions::OcrRegions` value. The existing OCR text result, document/image/PDF
extractions, commands, enabled jobs and schema files are unchanged. This adapter
accepts only the existing canonical grayscale PGM contract; decoding and original
page mapping remain separate operations. It does not write canonical records.

## Exact engine recipe

The same inventoried app-local Tesseract 5.5.2, English model and native libraries
used by [OCR-WORKERS.md](OCR-WORKERS.md) execute with fixed LSTM mode, single
uniform block segmentation and a 300 DPI recognition assumption. The supplied
raster is not resized. The selected PDF render DPI is not inferred from this
recognition setting. The new fixed recipe enables both TSV and text output via
`tessedit_create_tsv=1` and `tessedit_create_txt=1`; it does not load arbitrary
configurations or require new runtime assets.

Tesseract documents TSV's page/block/paragraph/line/word hierarchy and coordinate
columns in its [command-line guide](https://tesseract-ocr.github.io/tessdoc/Command-Line-Usage.html#tsv-output).
The parser follows the pinned version's
[GetTSVText implementation](https://github.com/tesseract-ocr/tesseract/blob/5.5.2/src/api/baseapi.cpp)
for hierarchy resets, structural confidence `-1` and word score formatting. This
is an intentionally strict subset of that engine output. Different layouts or
future engine formats can be rejected rather than silently repaired.

## Version 1 result

The typed result has `protocol_version: 1`, job UUID, exact canonical raster
SHA-256/byte count/dimensions, engine/language/model/runtime-manifest identities,
`recognized` or `no_text_recognized` status, original companion `text`, TSV
SHA-256/byte count, ordered `regions` and fixed limitations. Each region carries:

- Level: page, block, paragraph, line or word.
- Tesseract's page, block, paragraph, line and word numbers. Page is always **1
  for this raster**, never an asserted original-document page.
- Integer pixel bounds: left/top/width/height relative to the raster's top-left.
  Right and bottom are represented by left+width and top+height.
- `engine_confidence`: a finite 0–100 Tesseract word score, or null for structural
  rows. It is not a calibrated probability, extraction-quality guarantee or
  analyst confidence.
- Exact word text; structural rows have empty text.

`OcrRegions` also returns bounded transient TSV bytes, allowing
`validate_result(result, raster, tsv)` to independently reparse and compare the
whole result. No raster or TSV persistence is implied. A later canonical
integration must retain or otherwise establish the required derivative and
original/page mapping; this API does not create source anchors, accepted facts or
reviewed word regions.

## Bounds and acceptance

Input remains at most 12 million pixels, dimensions at most 8,192, with exact
8-bit PGM length. Output is bounded to less than 2,000,000 TSV bytes, at most
20,000 total rows and 10,000 words, 2,000 bytes per word, and the existing
512,000-byte / 128,000-character text limit. Every field is validated before it
becomes a typed region. The adapter requires:

- Exact header, twelve fields per row, UTF-8, final newlines in nonblank text/TSV and one full-size
  page row matching the assigned raster.
- Sequential hierarchy numbering with correct resets, existing parents,
  complete nonempty branches, and no duplicate page/region IDs.
- Positive integer boxes inside their parent and raster, with checked edge
  arithmetic. Exact duplicate word-text/rectangle pairs are rejected; repeated
  words at different positions remain separate.
- Empty structural text and structural confidence `-1`; canonical six-decimal
  finite word scores from 0 through 100; bounded nonempty words without controls
  or embedded whitespace. Valid but unsupported output fails visibly.
- The ordered TSV words must equal the whitespace-separated companion text.
  Blank recognition is a page-only TSV and empty/whitespace companion text.

Malformed fields, missing parents, unfinished rows/hierarchies, reordered or
missing words, nonfinite scores, extra pages, integer overflow and oversized
output reject the result. Complete-row truncation of one output is caught by
comparison to the other. A successful, explicitly reaped process is also required;
output from a failed or cancelled process is never accepted. These checks cannot
prove recognition completeness or accuracy, nor detect a coherently falsified
pair of outputs. No confidence threshold automatically accepts or drops words.

## Confinement and tests

The new recipe changes writable outputs only to the fixed `result.txt` and
`result.tsv` paths plus assigned scratch. The old text recipe still grants only
`result.txt`. Runtime/model inventory verification, cleared environment,
descriptor closure, denied fork/network/outside file content access, CPU/file/
wall bounds, cancellation, explicit termination verification and cleanup are
shared. Each output has a 2,000,000-byte OS file limit in this recipe; the tighter
companion-text limit is enforced before reading/acceptance. The combined tree is
also checked by the existing supervisor. No application PATH fallback or runtime
download is added.

`scripts/test_ocr_regions.py --runtime <app-local-engines>` relocates the real
runtime and records exact source/runtime/fixture identities. Native tests
recognize the synthetic reference number and amount, verify actual word boxes
cover raster ink, verify blank output, and run a disposable hostile native probe
through the exact new recipe. The probe checks outside/index/original writes,
network, inherited descriptors/environment, forking, unexpected output,
termination after cancellation and the output bound. Ordinary tests exercise
hierarchy, full-row truncation, malformed/control/oversized data, confidence,
source/TSV/result tampering, duplicate boxes, legitimate repeated words and
word/row boundary counts. Failure observations replace stale success projections
while UUID-based history retains each run even with identical clock timestamps.

Recognition remains unreviewed, English-only and optimized for the fixed
single-block recipe. Complex layout handling and accuracy benchmarks are not
established here. Development `sandbox-exec` evidence is not signed-helper or
clean-install release approval; Intel Mac, Windows runtime integration, hard RSS
and instantaneous aggregate disk ceilings, and supervisor-crash descendant
termination retain their separate gates. This slice does not complete EW-13.
