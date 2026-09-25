# One private collection lane in JobCoordinator (EW-22 / #25)

The private synthetic collection driver now runs within the existing `JobCoordinator`, sharing its workspace mutex, ownership lock and joined shutdown. There is one collection executor thread alongside the existing one or two document worker slots. This is a coordinator integration test seam, **not live application activation**: ordinary `JobCoordinator::start` cannot enable the collection lane, and only a `cfg(test)` constructor supplies its synthetic executor. No public command, UI, schema, dependency, network policy or synchronous `CollectWebsite` behavior changes.

The [lossless transport settlement contract](TRANSPORT-SETTLEMENT.md) remains authoritative for reservations, receipts, source retention, clocks and unknown outcomes. This increment uses those operations without duplicating their collection or extraction rules. It introduces no storage migration.

## Shared ownership and lock ordering

`CollectionOwnership` now owns one mutex-protected state: a held file, an explicitly released file, or a quarantined held file. It retains the existing process-local lifetime identifier and workspace binding. `JobCoordinator` acquires this guard once and shares it through `Arc`; the collection lane does not acquire another coordinator lock. Execution rejects released, poisoned, quarantined or wrong-workspace guards. Settlement alone may use the same still-locked quarantined lifetime to publish a previously owned response; released or poisoned ownership cannot authorize settlement. An old `Arc` can remain alive after successful shutdown, but it cannot authorize a later operation.

The normal ordering is **workspace mutex, then collection lane state**. Claim and active-token registration follow that order. Cancellation records its canonical request under the workspace mutex before signalling the same active token. Inspection and unrelated canonical document operations remain available while transport executes outside the workspace mutex. Shutdown takes the cancellation registries without taking the workspace mutex, signals both lane types, wakes their waits, and serializes all joins through the existing worker registry.

Successful release happens only after every document and collection thread has joined. The guard explicitly unlocks once; closing the descriptor is not sufficient on Unix when another fork inherited it. Existing real-fork and repeated-shutdown regressions remain in the suite. A failed join, poisoned ownership state or unverified collection operation prevents release. Quarantine has no reset API: at most one held ownership handle per affected workspace is deliberately retained until process exit, even if the coordinator is dropped. This prevents another owner from launching work behind an uncertain executor. It does not claim that a shared DNS daemon or remote server has stopped.

The document lane preserves its existing canonical invalid-result, cleanup and unverified-worker classifications. A document `TerminationUnverified` now quarantines the shared guard before signalling active tokens; its typed `worker_exit_unverified` failure still publishes under the workspace mutex. Both lanes refuse new execution, while read-only inspection stays available. Startup also reads the same canonical suspension predicate after document recovery, preventing collection behind an already persisted document uncertainty. An already owned collection response can still settle with exact bindings; canonical cancellation prevents promotion. Ordinary cancellation and confirmed-stop cleanup failures still join and release normally.

A collection executor panic leaves the charged request unresolved, marks the private lane recovery-required and quarantines the shared ownership guard; it never manufactures a cancelled or completed acquisition. The existing synchronous `CollectWebsite` dispatch is outside this private lane's admission path and remains unchanged until the later replacement; this increment does not claim a unified production networking switch.

## Claim, cancellation and recovery

The private queue method publishes version-2 synthetic jobs under the shared workspace mutex. The single collection lane chooses the oldest queued synthetic v2 run in canonical sequence order, validates it, starts its existing generation/lease, and registers the active cancellation token before releasing the mutex. The driver then commits each request charge before invoking its injected transport. There is no independent collection service or unbounded concurrency.

Only explicitly enabled synthetic coordinator startup performs collection recovery, under exclusive ownership. Recovery selects running synthetic v2 jobs; historical v1 jobs remain unchanged. The default production coordinator does not recover or start private collection runs. Recovered interrupted work requires explicit resume with its expected generation. Resume uses the existing fixed first-start deadline and counts. An uncertain robots request remains charged and cannot be silently fetched again or treated as permission for pages.

Running cancellation records the request and signals the active token. A complete response racing cancellation is retained through the existing canonical receipt path without text promotion or expansion. Cancellation does not imply terminal state while the synthetic executor is still running. The same shutdown token registration check prevents a missed cancellation if shutdown starts between claiming and execution. Queue/resume/retry methods reject a stopping or released coordinator.

Unknown resolver completion quarantines shared execution immediately, before the first attempt to publish its original uncertainty. A blocked database write cannot leave document execution active while the collection receipt waits for explicit retry. Active document tokens are cancelled; confirmed-stop document work records `Interrupted` without a derivative, including a late valid result. A document's own unverified exit and cleanup failure retain their higher-priority classifications. No second queued run starts. Once published, the canonical collection recovery-required state persists across opening the workspace, and joined shutdown still refuses to release the ownership guard because local-operation completion is unverified. This test uses a typed synthetic resolver observation; it is not execution evidence for native Windows DNS cancellation.

## Bounded publication failure

After transport returns, the lane owns one `PendingSettlement` and performs one publication attempt. Failure changes the private lane to `settlement_pending`; it does not rerun transport, retry on a timer, or start another job. The pending bytes remain bounded by the existing per-response contract.

An explicit retry must identify the exact current job, generation and request sequence. At most **three explicit retries per pending response** are admitted; repeated signals before the worker consumes one are coalesced. Retry remains available for a quarantined, still-locked publication guard; it grants no execution authority. Released, poisoned or stopping ownership refuses the retry. Every attempt revalidates the ownership lifetime and canonical lease/request/receipt bindings. The private state distinguishes running, settling, pending publication, faulted, recovery-required, stopped and unpublished-at-shutdown outcomes. It contains no raw OS error or worker output.

Shutdown signals cancellation and wakes the pending wait. If publication still fails, it performs exactly one final attempt and then joins the lane. A quiescent but unpublished response leaves its canonical reservation charged and reports the private `unpublished` state; shutdown completion does not claim that all acquisitions were published. The response is no longer retained in memory after that join. Bytes written before a failed database transaction may remain an unreferenced original under the existing lifecycle. A later exclusively owned recovery records the unresolved request as unknown; a previously requested cancellation becomes cancelled without inventing a response hash or acquisition. If the observation's local quiescence was unverified, ownership remains quarantined even when final publication also fails.

Before settling during shutdown or quarantine, the coordinator journals its stop
intent. If its current clock sample precedes the already validated checkpoint,
that private cancellation uses the checkpoint's exact journal timestamp. It does
not fabricate a transport completion time or change the owned receipt's raw wall
sample. This narrowly scoped operation accepts no supplied timestamp: it requires
the same held publication ownership, exact running generation/lease and reserved
request sequence/URL, and a replay-validated synthetic v2 record. It reuses the
ordinary Cancel transition and journal bounds. Ordinary cancellation timestamps
still reject backwards or future values. Reusing the stored anchor also handles
a rollback larger than the ordinary five-second future-clock tolerance.

The cancellation remains committed if later receipt publication fails, so an
exact retry neither fetches again nor loses shutdown intent. Unknown local
quiescence still takes precedence over a clock change or cancellation; a known
backwards transport clock remains a Failed receipt, and a complete response
observed before the stop remains retained without promotion. Tests reproduce the
former preliminary-cancellation failure, exercise all three outcomes, preserve
the exact receipt on replay, and use a consistent historical fixture to model a
large rollback without changing the host clock. A separate injected database
failure verifies that the cancellation and later receipt retry remain distinct.

Claim/reservation or integrity errors stop the lane with a fault instead of repeatedly performing writes or requests. Public retry/recovery ergonomics and persistent operator-facing failure reasons are still future command/UI work; the private lane state is not a replacement for a public receipt contract.

## Verification and remaining activation

Synthetic tests cover a real loopback TLS lifecycle through the coordinator; one collection lane; canonical acquisition counts; responsive cancellation and complete-body races; explicit exact-ticket publication retries and their cap; failure through final shutdown/reopen; original deadline/generation preservation on resume; default production nonactivation; untouched v1 running records; both document and collection executors held until joined shutdown; stale guard refusal; panicked join and poisoned-guard quarantine; unverified resolver quarantine; document uncertainty with active and queued collection; persisted document uncertainty at startup; normal cleanup release; and preservation of the existing ownership tests. Three pre-existing tests initially expected successful shutdown after an unverified executor or mixed unfinished fixture. Their typed-result assertions remain, and their shutdown assertions now require quarantine refusal; ordinary cancellation retains successful shutdown. Failed observations are retained in the verification record.

No external DNS or HTTP requests, native resolver campaign, browser action or package acceptance test is part of this increment. Remaining activation work is concrete: adapt the private lane to the production transport after native resolver verification; add reviewed queue/preview/progress/cancel/resume/publication-retry commands and schemas; expose the exact uncertainty and disclosure boundaries in the designed UI; verify harmless selected-source execution and restart through the actual application; then replace the synchronous collection dispatch. All complete-release gates remain unchanged.
