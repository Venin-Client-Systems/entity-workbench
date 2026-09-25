# Durable graph dispatcher API

Command v26 exposes the existing fixed `shortest_connection_path_v1` lifecycle.
The normal coordinator still selects `GraphExecution::Unavailable`. No runtime
path, configuration, worker bytes, publication capability or activation control
is accepted through the dispatcher. No database migration or historical schema
rewrite is required. Full, presentation and summary dispatch modes return the
same direct graph responses, independent of the workspace refresh envelope.

## Commands and direct responses

| Command | Selection and guard | Result |
| --- | --- | --- |
| `queue_graph_path` | canonical UUID `request_key`, `source_id`, `target_id`, explicit `expected_revision` R | `GraphJobInspection` |
| `page_graph_jobs` | request `page_size` 1..25 and optional cursor; optional `expected_revision` | `GraphJobPage` |
| `inspect_graph_job` | canonical UUID `job_id` | `GraphJobInspection` |
| `cancel_graph_job` | `job_id`, positive `expected_attempt` | `GraphJobInspection` |
| `retry_graph_publication` | `job_id`, `expected_attempt`, canonical UUID `host_attempt_lease`, lowercase `request_sha256` | `GraphJobInspection` |
| `inspect_graph_analysis` | content-derived `id`, exact `expected_request_sha256` and `expected_result_sha256` | existing `GraphAnalysisInspection` |

A queue acknowledgement means the request was retained. It does not mean a
runtime is available or a worker started. Normal coordinator scheduling records
its existing pre-launch `blocked / scheduling_or_runtime_unavailable` outcome,
without Running, capture or engine invocation. Standalone dispatch can retain a
Queued request but has no scheduler. Responses distinguish standalone
unavailability, runtime unavailability, a privately configured runtime, synthetic
test execution and recovery-required ownership.

New admission uses existing canonical endpoint and pending-queue validation.
Endpoints are distinct active entity IDs, 1..128 UTF-8 bytes with no controls or
surrounding whitespace. Exact request-key replay binds the original R and both
endpoints, checks mapping/key/body identity, and returns the current durable job
without another write. A known exact acknowledgement can be recovered under
coordinator quarantine; a new request cannot. Reusing a key with another input
or R fails. UUIDs use their exact lowercase hyphenated representation.

## Bounded catalogue and status

The catalogue includes all graph jobs, including blocked, failed, cancelled and
completed jobs. It returns canonical job metadata, never graph topology, raw
worker JSON or frozen provenance vectors. Schema-5 rows and rows identifying the
graph operation must decode as supported graph jobs; malformed selected metadata
fails the whole page. Other document/image jobs are excluded.

Revision, total count and rows come from one SQLite read snapshot. Canonical
record sequence ascends; timestamp ties cannot reorder rows. The closed cursor
binds the family, revision, page size and last returned sequence/key, and must
identify a real graph row. Changed revision or page size returns an explicit
conflict. This provides continuation consistency, not authentication. A page has
at most 25 rows; each stored job body is at most 64 KiB before Rust decoding, and
the serialized response at most 2 MiB. No truncation or silent skipping occurs.
Lookahead metadata is used only to report a next page, so an oversized next row
fails when requested instead of hiding it or rejecting the preceding valid page.

Job identity, operation/version, attempt/retry bounds, endpoint sizes, optional
lease UUID, result-ID hashes, timestamps (at most 64 bytes) and detail (at most
4096 bytes) are validated. Result references are limited to the existing three
attempts. SQL/SQLite may still scan and materialize stored values internally;
this is a bounded returned-metadata contract, not a constant-memory database
scan or a global corruption audit. Metadata reads do not rehash originals.

`InspectGraphJob` (and Queue/Cancel acknowledgements) includes at most three
metadata-only result references: ID, request/result digests, captured revision
and published revision. These share the job's SQLite snapshot and make the full
inspection selection discoverable. Missing, oversized, wrongly linked or
malformed referenced metadata fails the entire inspection. Fixed scalar SQL
projections check types and byte lengths before Rust string allocation; listing
references neither decodes the frozen model nor rehashes originals. The page
continues to expose result IDs; inspect the selected job to obtain its references.
Full immutable inspection remains necessary to verify their claimed contents.

Inspection separates canonical state from ephemeral execution. An execution
phase is shown only for the exact same job, attempt and lease. A historical
completed job cannot acquire another job's live phase. Controls come from the
actual ownership and retained interval. Lock order remains workspace, then
activity; no activity lock is held while acquiring the workspace.

## Cancellation, publication retry and historical results

Cancellation delegates the existing canonical state transition with a graph-kind
check and exact expected attempt. Coordinator dispatch records the intent and
signals the matching token under the same workspace/claim ordering. Token signaling
and graph/writer notifications precede response projection: a missing or corrupt
result reference can make the response unavailable after cancellation commits,
but cannot prevent cancellation or leave a publication waiter asleep. A stale
attempt cannot cancel a later attempt. Standalone cancellation only records the
intent; a Running record does not prove an active worker or a confirmed stop.
Cancelled-before-start is terminal immediately. Repeated matching cancellation
is read-only. Failure precedence and stop/cleanup supervision are unchanged.

Publication retry is available only to the live coordinator retaining the exact
private attempt/result. It accepts no worker bytes. Job, attempt, host lease and
request digest must all match the pending publication; at most three explicit
retries are admitted. Consuming a retry, cancellation settlement or final shutdown
settlement changes the phase to Publishing under the activity lock before
waiting for workspace. A duplicate cannot enter that lock handoff and spend
another retry slot. It retries settlement of the same bytes and never captures
again or invokes another worker. Standalone dispatch refuses. A command
acknowledges retry admission, not successful publication; inspect the job for its
subsequent outcome. No public graph worker retry/resume command is added here;
the existing generic processing-job contract remains unchanged.

Immutable inspection delegates the existing content-ID, raw digest, R/Q/C/P and
canonical job-linkage checks, then requires both caller-selected digests. The
stored record stays capped at 16 MiB. Frozen bytes remain available after later
canonical corrections, with `workspace_advanced` freshness. Original tampering
is reported separately as `original_integrity: unavailable` and null freshness;
it does not regenerate history from current workspace values. The accepted,
undirected, all-retained-dates policy and review denominators are unchanged.

## Verification boundary

New deterministic synthetic tests cover dispatcher replay/modes, unavailable
execution, pagination without omissions/duplicates, hostile input and malformed
metadata, concurrent canonical cancellation during a pinned read, stale attempt
cancellation, exact retained publication retry, original tampering and immutable
historical bytes. No native engine, network, UI or release claim is made by these
source tests. The earlier exact native coordinator campaign remains separately
pinned evidence; this additive public API does not change its historical source
identity or activate normal application execution.

Final source checks: 685 ordinary core tests passed (606 unit, 79 integration),
with 32 native cases ignored; 16 new public-API cases are included. Strict
all-target Clippy passed for host debug/release and cross-compilation to
`x86_64-apple-darwin`; Intel execution was not performed. Formatting and the
staged public audit passed. All 105 historical schema files are byte-identical
to the base; five schemas are additive (Command v26 and four response/request
v1 schemas).

The consumed-retry regression first failed by admitting a second public retry
while the first consumed retry waited for workspace. Its before/after logs are
retained separately. Peer review also found that response-projection failure
after canonical cancellation skipped token signaling. Its deterministic
corrupt-reference regression failed before the repair; both token signaling and
retained-waiter wakeup are now tested before any response projection. Initial compilation of the discoverable-reference addition
also caught a missing response initializer; that compiler failure is retained
with the final passing verification evidence. These are source/synthetic checks,
not a native runtime or product release approval.
