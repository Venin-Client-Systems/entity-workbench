# Durable local document and image jobs

The Rust coordinator owns the queue and publishes immutable, unreviewed extraction records. Parsing does not overwrite original bytes, promote extracted text into accepted evidence, or create observations automatically. Document parsing and PNG/JPEG image OCR share the bounded offline queue. Collection and analytical jobs still need integration.

## Contract

Command v5 adds `queue_document_parse`, `list_processing_jobs`, `inspect_processing_job`, `inspect_extraction`, `cancel_processing_job` and `retry_processing_job`. Command v7 adds `queue_image_ocr` and `inspect_image_extraction`. Requests are strict JSON. Newly queued jobs use processing-job v2; legacy parse-only jobs v1 remain readable. The parse-only extraction v1 schema remains unchanged. Image results use image-extraction v1. Historical command/job schemas are not overwritten by the generator. Canonical records remain in the existing version 3 SQLite store, so this additive change needs no database migration.

| Action | Behaviour |
|---|---|
| Queue | Canonical UUID request key; the same key and input returns the existing job without a revision change. Reusing it for another input fails. The original's retained digest and size are checked. At most 64 jobs may be queued or running. |
| Execute | A process-wide coordinator holds an exclusive workspace lock. One or two threads claim jobs and release the workspace mutex while a disposable parser, or sequential image decoder and OCR workers, run. The engine receives copied, hash-checked original bytes and its own scratch area. |
| Inspect | Job listing returns the newest 200 records and the total count. Full extraction text is fetched separately. The parser's source binding, status and limitations remain visible. |
| Cancel | The expected attempt must match. Queued jobs stop immediately; running jobs remain running with `cancellation_requested` until the worker has returned after termination/reaping. A successful result that races with cancellation is discarded. |
| Retry | Analyst reason required. Only partial, failed, blocked, quota-exhausted or cancelled jobs can be retried. Three allocated attempts maximum (including cancellation before launch); retries never happen automatically. Prior derivatives remain immutable. |
| Publish | A private attempt ticket binds job, attempt number and lease. Rust revalidates the original and parser result, or the original, actual raster and OCR result together. A single transaction writes the derivative and terminal job state. Identical successful replay does not add records or advance the revision; conflicting replay fails. |
| Recover | Only the exclusive coordinator recovers a previous running attempt. It becomes failed/worker_exit_unverified and suspends queued work, including when cancellation had been requested. New queue and retry actions stay blocked until verified process recovery. Opening another ordinary workspace view does not invalidate a live processing attempt. |

The native desktop starts two worker slots. A missing or unsupported confined runtime produces a visible blocked job. The command-line development bridge can queue and inspect records, but does not start an ephemeral background coordinator. Production execution belongs to the native host.

The saved input includes the evidence ID, digest and byte count. Extraction IDs derive from the canonical job and attempt, and each extraction records its own content hash, engine result and creation time. The opaque attempt lease is not an API authorization mechanism: there is no public finish/publish command. Workers cannot write to the workspace database.

## Image OCR results

`queue_image_ocr` accepts an evidence ID and canonical UUID request key. Request
keys bind the operation as well as original identity, so a document-parse key
cannot silently become an OCR job. PNG/JPEG support and format/expansion limits
are defined in [the confined image boundary](../security/IMAGE-WORKERS.md).
The same 64 pending-job limit, two executor slots, manual three-attempt ceiling,
lease ownership and cancellation/publication precedence apply to both operations.
A decoder exits and cleans its assignment before the OCR stage starts.

The original is verified again when the result reaches canonical publication.
The image result must match those exact bytes; its raster binding must match the
actual canonical PGM bytes; recognition must match those same raster bytes and
have a different worker job identity. A decoded image without an OCR result, or a
rejected image with recognition, fails acceptance. The coordinator borrows the
bounded result while retrying temporary publication errors rather than cloning
its raster or rerunning workers.

`inspect_image_extraction` returns an immutable record containing its canonical
job/attempt/input, timestamp, result digest and typed `result`:

- `decoder`: original identity, decoder identity, outcome, limitations and raster
  digest/dimensions/encoded-pixel mapping when decoding succeeded.
- `recognition`: the bounded typed OCR result, including recognized text,
  raster binding, engine/model/runtime identities and limitations; null when the
  decoder did not succeed.
- `raster_retained: false`: raster bytes were validated in memory and discarded
  after the publication attempt completes. The digest is provenance for an
  ephemeral derivative, not proof that its bytes are retained in the workspace
  or backup.

The record carries source image index zero and unapplied EXIF orientation; it
contains no invented document page, word regions or acceptable image-region
anchors. Publication leaves `Evidence.text`, source bytes and observations
unchanged. Recognition remains unreviewed. No command lets a frontend or worker
submit arbitrary completion results.

| Engine outcome | Canonical job / retained result |
|---|---|
| Recognized text | Completed; decoder and unreviewed recognition retained |
| No text recognized | Completed with explicit no-text detail; typed `no_text_recognized` result retained |
| Unsupported format or multiple images | Blocked / unsupported_format; typed decoder outcome retained, no OCR |
| Malformed image or decoder warning | Failed / image_decode_failed; typed decoder outcome retained, no OCR |
| Decoder pixel/container limit | Quota exhausted; typed decoder outcome retained, no OCR |
| Missing runtime, engine error, cancellation, invalid result | Existing failure precedence applies; no new derivative is published |

Previously retained image results survive manual retries unchanged. Their compact
text/provenance records and referenced originals survive database backup/restore;
no raster is silently added to the backup. A caught executor panic is treated as
worker-exit-unverified because destructor cleanup alone cannot establish process
termination. It suspends the shared queue just like an orphaned running claim.

The debug-only `ew-dev seed-image-processing-review <fresh-workspace>` helper
creates fixed canonical state specimens for UI tests, including recognized,
no-text, unsupported, failed, quota, cancelled, retry history, running,
cancellation-requested and queued jobs. It refuses a nonempty workspace and
accepts no result payload. These records explicitly use synthetic protocol values
and **do not claim OCR execution**. They share retained synthetic PNG/GIF originals.
Actual recognition is established separately by the native coordinator test.

## Failure and recovery limits

Temporary database errors while publishing cause the coordinator to retry publication of the bounded result without restarting the engine. If the application stops before publication succeeds, the durable claim is recovered as worker-exit-unverified next time, suspending further execution. The desktop exit callback explicitly cancels and joins active executors; it does not rely on Rust destructors after a runtime process exit. Queued work remains queued. Attempt numbers are reserved on queue/retry so stale cancellation cannot affect a queued retry. There is no background network operation in this queue.

An engine result with `TerminationUnverified` takes precedence over cancellation, shutdown and input errors. It produces `failed` / `worker_exit_unverified`, retains private assignment files and atomically blocks queued jobs with `recovery_required`. New queue and retry requests are refused, including after application restart. This prevents additional workers from escaping the concurrency ceiling while an earlier process might still exist. An orphaned running claim follows the same suspension path: a process may outlive a crashed supervisor, and a CPU limit does not bound an idle orphan. Earlier development records marked interrupted with a cleared lease are reclassified conservatively on coordinator startup; a joined shutdown retains its attempt lease and remains manually retryable. Verified process recovery and release of that execution suspension are not implemented yet; the application does not silently clear it or claim cancellation succeeded.

Current macOS worker confinement is a development mechanism. Signed helpers, supervisor-crash descendant cleanup, Windows runtime integration and clean installed-package execution retain their separate release gates. An interrupted canonical record proves that no derivative was published; it alone does not prove that every OS process from a crashed supervisor has terminated.

Long synchronous legacy commands, including collection and search, still hold the workspace lock. Document parsing and image OCR execute outside that lock; it does not claim that all existing commands are cancellable. Full queue pagination and workflow recipes remain outstanding. Image review presentation is a separate integration task; this canonical slice does not add UI behavior.

## Verification

Synthetic core tests cover request-key conflicts, cancellation/success races, stale attempt rejection, bounded manual retries, a transaction failure after derivative insertion, replay without duplication, missing/altered inputs, exclusive coordinator ownership, crash recovery and backup/restore of jobs, derivatives and original evidence. Coordinator tests use a controlled executor to establish responsiveness, the two-worker ceiling and joined shutdown. Real parser confinement and cancellation tests are separate and are documented in [the parser boundary](../security/PARSER-WORKERS.md).

Native coordinator tests execute the real confined PNG and JPEG decoder/OCR chain
with two slots, inspect the public command results, and restore the recognized
text, immutable provenance and original bytes from backup. The existing real PDF
parser coordinator scenario remains a separate regression. State tests cover
atomic rollback, invalid source/raster/OCR bindings, operation-key collisions,
empty recognition, retry immutability, mixed parse/image concurrency and panic
suspension. `scripts/test_image_jobs.py` records these bounded native observations
with source/runtime identities and collision-safe history. The ordinary core
suite remains the broader regression check.

No complete-release gate is changed by this implementation.
