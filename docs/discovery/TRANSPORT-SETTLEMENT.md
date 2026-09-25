# Durable request execution and lossless settlement (EW-22)

This increment connects the private durable collection store to the private Rust transport for **one committed request at a time**. Synthetic integration tests execute real HTTP over a fixture TLS connection on loopback, publish its response through the canonical workspace, and reopen or restore that workspace. Application commands, coordinator scheduling, public acquisition, UI controls and the old synchronous collector are unchanged. No live collection or release gate is passed.

All new runs still require `synthetic: true`. They use private record version 2 and policy `direct-https-durable-transport-v2`. No public command/schema, dependency, worker protocol or storage migration is introduced. Existing version-1 durable records retain their original bytes and semantics. The preceding [foundation](DURABLE-JOBS.md) and [transport](CANCELLABLE-TRANSPORT.md) documents describe their own earlier increments; the clock and receipt rules below apply to version 2.

## Concrete execution boundary

`CollectionDriver::attach` accepts an existing running version-2 run, its canonical generation/lease and the held workspace coordinator ownership guard. It creates a process-local monotonic deadline bounded by the remaining first-start wall deadline and original time limit. The driver also binds its lifetime to that particular ownership guard. It cannot be reused after the lock is dropped and reacquired, even if recovery has not yet revoked the canonical lease.

For each `next` call:

1. Under the workspace mutex, revalidate ownership, run, generation and lease, and refuse persisted recovery-required state. An already requested cancellation ends before another reservation.
2. Commit `Advance` and its charged request ticket through the canonical transaction. Failure here prevents resolver or socket execution. An unresolved earlier reservation prevents another request.
3. Release the workspace mutex. Execute the private transport once, using that ticket, immutable selected-host input, fixed deadline, cancellation token and pacing time. The transport remains verified HTTPS with a validated pinned address snapshot, no proxy, automatic redirect, retry or fallback resolver. The tests' endpoint and trust-root substitutions exist only under `cfg(test)`.
4. Return a `PendingSettlement` owning the exact observation and any complete response bytes. No canonical success is implied yet.
5. Settle under the workspace mutex. Revalidate the ownership lifetime and exact job/generation/lease/request/URL, retain complete bytes through the existing bounded original store, then atomically publish Evidence/acquisition, receipt event and resulting checkpoint. Eligible static text promotion uses the existing collector rules. A failed transaction leaves the reservation charged; the same pending observation can retry publication without any transport call.

An exact already-published receipt replay returns the current record without adding revision, acquisition or history. A conflicting reply, recovered reservation, revoked lease or changed ownership fails. Dropping the pending value loses the in-memory response; recovery retains the charged request as `interrupted_unknown`, without inventing a response, hash or end time. That request is not silently sent again. An unknown robots request cannot authorize later page requests.

The current driver is synchronous inside its future owned executor thread; it releases the workspace mutex for I/O. It is not a separately operated service or a second coordinator. Its private `next` entrypoint has no production caller. The ownership seam still uses the same lock as `JobCoordinator` and must be replaced with that coordinator's held ownership during activation.

## Receipt facts and interpretation

Each version-2 `TransportObserved` journal event contains a strict, closed receipt. Its cached charged-request state repeats the receipt and must match deterministic replay. Unknown or duplicate serialized fields fail. The receipt retains:

- Complete-body digest and exact byte count, or a stopped outcome with no original reference. Complete empty, robots, redirect, error and unsupported bodies are retained.
- Normalized response status/media type, safe resolved Location when available, and the observed identity-encoding flag. These are selected response facts, not a full raw header capture. A Location on an ordinary 200 response remains provenance and is not followed.
- Last execution phase and conservative HTTP delivery knowledge. Before-request, pacing and DNS stops are definitively before HTTP. Connect/TLS/header or body phases mean HTTP may have been sent; they do not prove server receipt or processing.
- Actual per-request monotonic elapsed milliseconds, exact final checked raw wall sample, primary stop reason, and any cancellation/deadline/clock-change observation racing completion.
- Validated candidate addresses and acquisition method, with `authoritative_complete_set: false`. The snapshot does not claim a complete A/AAAA answer set.
- Resolver uncertainty and caller-context status, plus whether the owned local operation ended. Shared OS DNS activity and remote-server processing are not claimed stopped.

Only a complete identity-encoded eligible page may become static text or expand links. Encoded responses retain their bytes but cannot supply robots rules or text. Complete bytes received during cancellation, expiry or clock failure are retained without promotion or further expansion. Partial body bytes never become an original, and a stopped response can retain its headers without a fabricated body digest. Existing observations, historical collection receipts and saved exports are not rewritten.

Unknown resolver quiescence has precedence over cancellation. It commits `recovery_required`, preserves the uncertainty and charge, and blocks starting or advancing any other collection run in that workspace, including after reopening. The underlying transport also quarantines its process. No reset/recovery command is added here. On Windows, even confirmed caller-context completion after cancelled DNS does not establish provider quiescence; an uncompleted caller context may remain retained until process exit. Native platform proof remains outstanding.

## Two clocks, one fixed deadline

The first-start wall deadline is never replaced. Time spent stopped or restarting counts. Each attached driver owns a new monotonic segment; it cannot carry a previous process's elapsed value forward as if the monotonic clock were continuous. Subsequent requests use the same segment deadline. Pacing is at least one second after the preceding execution finishes; attaching after earlier charges adds a conservative one-second delay rather than inferring historical monotonic continuity.

The journal event's `clock_anchor_ms` is the prior valid checkpoint sample. The receipt keeps the raw final transport sample separately. A backwards sample, or an explicitly observed clock reversal during the request, fails the run without clamping or discarding the raw value. Comparison is against that request's reservation, because a canonical cancellation may legitimately be journaled after the response was observed but before settlement. The checkpoint retains the latest valid actual journal/receipt sample; this does not replace the raw receipt sample or invent an end time.

Version-2 historical replay validates these relationships and timestamp representability without comparing an old stored time to today's possibly changed wall clock. New ordinary queue/start/reserve/cancel/recovery events still use the existing current-time and ordering checks. A backwards recovery clock refuses the operation. A forward jump can exhaust the unchanged deadline; complete response bytes remain inspectable. The transport's 20 ms cooperative checks do not preempt trusted DNSService/Winsock calls or promise hard syscall latency.

## Compatibility and recovery

Storage schema 5 remains unchanged from this increment's base. The existing generic records/history tables store version-2 runs, and complete responses are still ordinary Evidence published with their references in the same transaction. Current backup/restore copies the complete SQLite snapshot and every referenced Evidence original through verified bounded buffers. Files retained before a failed publication may remain unreferenced; backup excludes those orphans and never calls them a completed acquisition.

Inspection of the base recovery and backup implementation confirms that legacy recovery selects its own job kinds; it does not recover or rewrite `collection_run`. Backup copies all Evidence, not only originals it can interpret through a collection record. This increment does not change those implementations. The current binary's integration test restores a completed version-2 run with all four response originals and identical receipt/checkpoint state. The preceding foundation separately tested an older schema-4 reader; that earlier execution is not new evidence that an arbitrary older binary can inspect version-2 runs. Readers unable to open storage schema 5 continue to fail on the existing newer-schema check. No older binary is claimed to understand or execute version-2 collection semantics.

## Verification and activation path

The checked-in verification record identifies exact test observations and retained failed attempts. Synthetic tests cover committed reservation visibility from a second connection before TLS starts, unlocked workspace during I/O, robots/redirect/link ancestry, exact replay, reservation and publication rollback, cancellation before and after charging, complete-body cancellation, encoded policy refusal, truncated-body provenance, monotonic expiry, ownership handover, crash before/after response, raw clock reversal across reopen, malformed/duplicate receipt fields, stale leases, persistent quarantine, v1 byte preservation and current backup/restore. Existing collector/foundation regressions remain in the ordinary suite. No external DNS or HTTP requests are part of this verification.

The next integration should be a single reviewable consumer of this driver:

1. Use the existing `JobCoordinator` ownership and joined shutdown for one collection lane. Queue/claim and recover under its mutex, run transport outside it, and retain a pending settlement until publication succeeds or explicit recovery records uncertainty. Cancellation must set both canonical request state and the same token; shutdown must join the executor before releasing ownership. Never use the private stop-acknowledgement seam as a frontend assertion.
2. Add bounded queue/preview/inspect/cancel/resume commands and versioned progress/receipt readers exposing these exact facts, selected hosts/URLs, disclosed query values and fixed limits. Preview is not source-access approval. The public contract must preserve recovery-required, charged-unknown and body-complete-with-stop outcomes.
3. Add the designed controls, synthetic coordinator/fault tests and actual native resolver cancellation evidence. Then verify an explicitly selected harmless public source through the application, including cancellation/restart and retained receipts. The private synthetic marker must not be silently removed before that review.
4. Retire the old synchronous `CollectWebsite` dispatch when the new application path is verified, preserving historical receipt inspection/export. Do not leave two competing production collection owners.

This remains an incomplete direct-collection integration. It establishes neither broad-search relevance, legal/source-access approval, offline packaging, native resolver cancellation acceptance nor a finished release.
