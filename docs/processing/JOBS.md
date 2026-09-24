# Durable local document jobs

The Rust coordinator owns the queue and publishes immutable, unreviewed extraction records. Parsing does not overwrite original bytes, promote extracted text into accepted evidence, or create observations automatically. This is the first EW-10 execution path; collection, OCR and analytical jobs still need integration.

## Contract

Command v5 adds `queue_document_parse`, `list_processing_jobs`, `inspect_processing_job`, `inspect_extraction`, `cancel_processing_job` and `retry_processing_job`. Requests are strict JSON. The job and extraction schemas are version 1. Canonical records remain in the existing version 3 SQLite store, so this additive change needs no database migration.

| Action | Behaviour |
|---|---|
| Queue | Canonical UUID request key; the same key and input returns the existing job without a revision change. Reusing it for another input fails. The original's retained digest and size are checked. At most 64 jobs may be queued or running. |
| Execute | A process-wide coordinator holds an exclusive workspace lock. One or two threads claim jobs and release the workspace mutex while a disposable parser runs. The engine receives copied, hash-checked original bytes and its own scratch area. |
| Inspect | Job listing returns the newest 200 records and the total count. Full extraction text is fetched separately. The parser's source binding, status and limitations remain visible. |
| Cancel | The expected attempt must match. Queued jobs stop immediately; running jobs remain running with `cancellation_requested` until the worker has returned after termination/reaping. A successful result that races with cancellation is discarded. |
| Retry | Analyst reason required. Only partial, failed, blocked, quota-exhausted or cancelled jobs can be retried. Three allocated attempts maximum (including cancellation before launch); retries never happen automatically. Prior derivatives remain immutable. |
| Publish | A private attempt ticket binds job, attempt number and lease. Rust revalidates the original and parser result. A single transaction writes the derivative and terminal job state. Identical successful replay does not add records or advance the revision; conflicting replay fails. |
| Recover | Only the exclusive coordinator recovers a previous running attempt. It becomes failed/interrupted with worker exit explicitly unverified, including when cancellation had been requested; there is no automatic retry. Opening another ordinary workspace view does not invalidate a live processing attempt. |

The native desktop starts two worker slots. A missing or unsupported confined runtime produces a visible blocked job. The command-line development bridge can queue and inspect records, but does not start an ephemeral background coordinator. Production execution belongs to the native host.

The saved input includes the evidence ID, digest and byte count. Extraction IDs derive from the canonical job and attempt, and each extraction records its own content hash, engine result and creation time. The opaque attempt lease is not an API authorization mechanism: there is no public finish/publish command. Workers cannot write to the workspace database.

## Failure and recovery limits

Temporary database errors while publishing cause the coordinator to retry publication of the bounded result without restarting the engine. If the application stops before publication succeeds, the durable claim is recovered as interrupted next time. The desktop exit callback explicitly cancels and joins active executors; it does not rely on Rust destructors after a runtime process exit. Queued work remains queued. Attempt numbers are reserved on queue/retry so stale cancellation cannot affect a queued retry. There is no background network operation in this queue.

An engine result with `TerminationUnverified` takes precedence over cancellation, shutdown and input errors. It produces `failed` / `worker_exit_unverified`, retains private assignment files and atomically blocks queued jobs with `recovery_required`. New queue and retry requests are refused, including after application restart. This prevents additional workers from escaping the concurrency ceiling while an earlier process might still exist. Verified process recovery and release of that execution suspension are not implemented yet; the application does not silently clear it or claim cancellation succeeded.

Current macOS worker confinement is a development mechanism. Signed helpers, supervisor-crash descendant cleanup, Windows runtime integration and clean installed-package execution retain their separate release gates. An interrupted canonical record proves that no derivative was published; it alone does not prove that every OS process from a crashed supervisor has terminated.

Long synchronous legacy commands, including collection and search, still hold the workspace lock. This slice keeps document parsing outside that lock; it does not claim that all existing commands are cancellable. Full queue pagination, workflow recipes, per-attempt history presentation and an editable-design job/extraction review interface remain outstanding.

## Verification

Synthetic core tests cover request-key conflicts, cancellation/success races, stale attempt rejection, bounded manual retries, a transaction failure after derivative insertion, replay without duplication, missing/altered inputs, exclusive coordinator ownership, crash recovery and backup/restore of jobs, derivatives and original evidence. Coordinator tests use a controlled executor to establish responsiveness, the two-worker ceiling and joined shutdown. Real parser confinement and cancellation tests are separate and are documented in [the parser boundary](../security/PARSER-WORKERS.md).

No complete-release gate is changed by this implementation.
