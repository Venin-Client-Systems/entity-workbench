# Durable selected-page PDF OCR

Rust queues, executes and atomically publishes an analyst-selected page through
the existing offline processing coordinator. The confined PDF renderer and OCR
run as separate, sequential workers in one of the two shared executor slots.
Their runtime and format limits are defined in
[PDF-RENDER-WORKERS.md](../security/PDF-RENDER-WORKERS.md). This is a development
implementation of the scan-focused PDF subset, not general PDF rendering or an
approved release package.

## Commands and records

Command v9 adds strict `queue_pdf_page_ocr` with `evidence_id`, canonical UUID
`request_key`, `page_number` (1-based, 1–1,000) and `dpi` (72–300), and
`inspect_pdf_extraction` with `extraction_id`. Queue bounds do not promise that a
particular document has that page or fits the raster limit. The renderer checks
the actual document and records an explicit typed outcome.

The request key binds the operation, original evidence ID/digest/byte count,
page and DPI. Reusing the same key and input returns its existing job without a
revision change, including after a lost acknowledgement. Reusing it for any
other page, DPI or operation fails. Newly queued jobs use processing-job v3;
parse v1 and parse/image v2 jobs remain readable. Historical command v1–v8,
processing-job v1/v2 and parse/image extraction schemas are unchanged. Generic
SQLite records need no database schema migration.

The input is `operation: "pdf_page_ocr"` with `evidence_id`, `sha256`, `bytes`,
`page_number` and `dpi`. The immutable pdf-extraction v1 record contains
`schema_version`, `id`, `job_id`, `attempt`, `input`, `created_at`,
`result_sha256`, and `result`:

- `render`: renderer identity, exact original digest/bytes, selected page/DPI,
  page count when known, typed status/failure/limitations and, on success,
  raster digest/dimensions and crop/quarter-turn affine geometry.
- `recognition`: bounded OCR text and typed status, engine/model/runtime
  identities, limitations and the same raster binding; null after a rejected
  rendering outcome.
- `raster_retained: false`: Rust validates the actual canonical PGM bytes in
  memory before publication, then discards them. A retained hash identifies the
  ephemeral raster; it does not mean the raster is in the workspace or backup.

There is no public finish command, worker database access or arbitrary result
fixture interface. Rust rechecks the retained original, then validates the exact
requested page/DPI, actual raster bytes, affine geometry and recognition. The
renderer and OCR must have separate worker job IDs. A rendered page without
recognition, a rejected page with a raster or recognition, or any mismatched
binding fails acceptance without creating a derivative. One transaction writes
both the immutable derivative and terminal job. Identical replay does not add a
record or revision; altered replay fails.

## Outcomes, cancellation and recovery

| Renderer/OCR outcome | Canonical outcome |
|---|---|
| Rendered, recognized | Completed; unreviewed text and provenance retained |
| Rendered, no text recognized | Completed; explicit `no_text_recognized`, including whitespace-only raw OCR text |
| Encrypted | Blocked / `encrypted_document`; typed outcome retained, no OCR |
| Unsupported format or feature | Blocked / `unsupported_format`; typed outcome retained, no OCR |
| Malformed or invalid selected page | Failed / `pdf_render_failed`; typed outcome retained, no OCR |
| Renderer resource bound | Quota exhausted / `worker_failed`; typed specific rendering failure retained, no OCR |
| Missing runtime, invalid result, cancellation or execution failure | Shared typed job failure rules; no new extraction |

PDF jobs share the 64 pending-job ceiling, two worker slots, analyst-only
three-attempt retry ceiling, attempt/lease ownership, joined shutdown and
publication rules in [JOBS.md](JOBS.md). Running cancellation remains pending
until the executor returns after worker termination. It suppresses a valid result
that races with cancellation. Unverified termination takes precedence over
cancellation/shutdown, preserves private scratch and suspends the shared queue.
An orphaned running claim after a supervisor crash is likewise unverified, not
assumed stopped. There is no automatic recovery override.

Prior attempt records remain immutable through retries. Compact text/provenance,
job references and exact originals survive backup/restore. No OCR text replaces
`Evidence.text`, and no observation, accepted fact or word-region anchor is
created. The page transform describes the rendered raster only; it is not a
reviewed location for any recognized word. OCR remains English-only here.

## Verification and review fixtures

`scripts/test_pdf_jobs.py --runtime <app-local-engines>` records source hashes,
Java/PDF/OCR runtime inventories and the exact nine-case suite. Its native case
runs actual Flate and JPEG scan PDF pages, an explicitly selected blank second
page, encrypted/unsupported/malformed inputs and a pixel-limit rejection through
the two-slot coordinator; it inspects canonical results and restores their
originals and provenance from backup. Ordinary tests cover atomic rollback,
wrong source/page/DPI/raster/recognition, request-key conflicts, legacy schemas,
replay, cancelled results, stale attempts/leases, retries, worker panic/unknown
exit suspension and mixed parse/image/PDF coordination. Sanitized failures replace
the latest observation while collision-safe history retains prior reports.

`ew-dev seed-pdf-processing-review <fresh-workspace>` is debug-only and creates
fixed canonical UI specimens. It refuses nonempty workspaces and accepts no
result payload. Its 11 originals are named `pdf-review-{mode}.pdf` for
`recognized`, `empty`, `encrypted`, `unsupported`, `failed`, `quota`, `cancelled`,
`retry`, `running`, `cancel_requested`, and `queued`. The eight retained results
include failed and successful retry attempts. Recognized/empty specimens use
page 2, DPI 144, a two-page source, synthetic renderer/OCR identities and
explicit **NO WORKER RAN** text; empty text preserves newline/form-feed bytes.
These fixtures test states and rendering safety, not recognition. Actual OCR is
established by the separate native coordinator case.

Signed helpers, Intel Mac and Windows runtime integration, clean installed
packages, general PDFs/font rendering, retained rasters, source-region review,
recognition accuracy and supervisor-crash process cleanup remain unverified or
unimplemented. This slice does not complete EW-13 or change a release gate.
