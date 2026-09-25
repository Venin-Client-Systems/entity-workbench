# Durable graph analysis source foundation

This EW-17 increment adds internal queue admission and immutable result storage for
`shortest_connection_path_v1`. **Automatic graph execution is disabled.** The
production claim loop records `blocked / scheduling_or_runtime_unavailable` before
creating a Running claim, capturing inputs or calling any executor. No desktop
command, UI, Python launcher, package discovery, network use or new SQL table is
introduced. The previously executed [fixed native experiment](PYTHON-CANONICAL-GRAPH-PROBE.md)
remains separate evidence; it does not activate this job path or establish that an
application-local runtime is installed on any supported platform.

## Why execution remains blocked

The existing coordinator has two processing workers and a separate collection
lane. Claiming or finishing any job writes a workspace revision. If graph A
captures at C1 and graph B claims at C2, A becomes stale; settling A can then make
B stale. Silently ignoring those revisions would break the reviewed capture
contract. Two queued graph jobs therefore receive explicit availability outcomes
without either entering Running. There is no loop of automatic stale-result
retries and no change to scheduling for existing document/image/collection work.

A later scheduling slice must reserve an exclusive analytical interval after
already-running processing and collection work drain. Both lanes must honor the
same reservation before automatic claims and progress writes, retaining it across
publication retries until terminal commit. Analyst writes may still stale a
capture. This source increment does not implement that reservation.

## Revision and ownership contract

Queue admission is an internal Rust API taking canonical endpoint IDs, a canonical
UUID request key and expected revision R. Its immediate transaction writes the
request mapping and schema-5 Queued job at Q = R + 1. Exact replay requires the
same request key, endpoints **and requested R**; it returns the existing job without
writing, even when the workspace has advanced. A new key with stale R fails.
Manual retry retains requested R and records its new queued revision Q separately.
Only explicit retries are allowed, with the existing three-attempt limit.

The publication tests use a private `cfg(test)` claim seam, unavailable to the
application. It opens `BEGIN IMMEDIATE`, checks the current revision, writes the
Running job and a fresh host lease, advances the actual stored `meta.revision` to
C, and writes that revision's event. It then captures under **that same
transaction**, reading actual C. It neither nests a transaction nor labels an
earlier revision as C. Capture failure rolls back the job, revision and event to
their previous state, so no unowned Running claim escapes. Capture commits only
after all bounded canonical reads and original checks succeed. C need not equal
Q + 1 if other writes occurred before this private claim; it must be later than Q.

The resulting `GraphAttempt` is private, non-Clone and non-serializable. It freezes
the job ID, request key, input/endpoints/R/Q, attempt and host lease, plus the
existing Rust-owned capture with workspace-instance identity, nonce, exact C,
snapshot digest and complete canonical fingerprints. A result cannot provide or
reconstruct that authority. Settlement compares the canonical claim to this held
identity, including same-revision job-body substitutions.

Publication uses the existing `Workspace.change` immediate transaction. At that
point `meta.revision` must still equal captured C. The private borrowed validator
rechecks the workspace owner, complete canonical fingerprint/topology digest,
retained originals and closed raw worker result, including shortest length and
every parallel accepted assertion. It then writes an immutable graph record and
the Completed job in the same transaction, followed by the normal revision/event
advance to P = C + 1. No accepted entity, observation or assertion is created.

The original consume-on-validation graph API remains unchanged. Borrowing a
capture for publication is confined to the private attempt; it is retained only
so an aborted transaction can be retried with the same result and authority.
Database errors from recapture, writes or commit propagate without consuming it.
Terminal commit consumes the capture. A repeated matching completed delivery is
a no-write replay, bound to the same job, attempt, host lease and raw result hash.
There is no automatic rebase, fresh capture or engine rerun.

## Result size, provenance and historical inspection

`graph-analysis.v1` retains requested R, queued Q, captured C and published P;
job/request/attempt/host-lease/capture-nonce correlation; the fixed recipe, engine,
policy and expected runtime-manifest identity; exact UTF-8 request/result bytes
and SHA-256 digests; complete input fingerprints and topology; frozen entity,
accepted assertion, observation and evidence values; assertion review
denominators; and the validated outcome. Runtime identity in this protocol is an
expected recipe identity, **not an execution attestation**. A future activated
adapter still needs verified prefix/code/assignment and lifecycle evidence.

Workers receive only the existing bounded topology request. The frozen display
values and original provenance remain in Rust and the canonical workspace.
Connectivity is undirected, includes accepted relationships from all retained
time periods, and may combine disjoint periods. It does not establish a directed,
contemporaneous or causal relationship. Unsupported selected provenance is
rejected as a whole; this first graph scope supports text anchors only.

The entire serialized immutable record is capped at 16 MiB using a bounded
writer, including JSON escaping. Its ID is SHA-256 of its typed serialized value
with the ID field blank, then the final record is size-checked again. This is a
content-integrity identifier, not authentication of arbitrary worker claims.
An oversized retained result aborts publication with **no canonical writes** and
keeps the attempt. A future lifecycle owner may explicitly settle the confirmed
stopped attempt as quota exhausted; it must not truncate provenance or rerun the
engine. Inspection checks the stored byte bound before decoding, strict schema,
content identity, revision arithmetic, raw hashes and job linkage.

Later canonical corrections do not erase history or require today's entity or
observation JSON to equal old fingerprints. Inspection returns frozen values and
independently verifies the retained originals identified by that record:

| Condition | Original integrity | Freshness |
| --- | --- | --- |
| Originals valid and current revision equals P | `verified` | `current_at_publication` |
| Originals valid and workspace has advanced beyond P | `verified` | `workspace_advanced` |
| A retained original is missing, linked, changed or unreadable | `unavailable` | absent |

`current_at_publication` means the derived record was accepted for its captured C
and no subsequent workspace revision exists; it never relabels its inputs as P.
Thus its own publication does not make it immediately stale. After later writes,
the old result remains an identifiable historical snapshot. Corrupted record/job
identity causes inspection refusal; original loss remains a separately visible
integrity outcome with no freshness claim.

## Failure and compatibility boundaries

Unverified worker exit takes precedence over cleanup failure and cancellation,
retains the existing coordinator quarantine semantics, and suspends queued
processing. Confirmed cleanup failure prevents publication. Cancellation discards
even a valid result. Stale capture, invalid worker result, unavailable input,
runtime unavailability and quota exhaustion remain distinct. No lifecycle outcome
in these source tests proves an actual process was started or stopped.

The additive `processing-job.v5` recognizes the fixed graph input; existing
document/image jobs still emit their existing schema versions. Versions 1–4 and
the graph request/result v1 bytes are not rewritten. SQL workspace schema remains
unchanged: graph results use the existing canonical records table, and ordinary
backup/restore includes their records and referenced originals. Unsupported older
code must not be used to execute new graph jobs; downgrades are not supported.

## Verification scope

The focused ordinary Rust tests use synthetic canonical workspaces and forged
result bytes, with no Python process. They cover queue/replay R binding; two-job
production blocking; atomic claim rollback; actual R→Q→C→P; parallel provenance
and review denominators; result and recapture database rollback with retained
authority; a deterministic second canonical connection refused during immediate
publication; stale/foreign/nonce/duplicate/oversize results; original corruption;
unchanged historical display after corrections; cancellation/cleanup/unknown-exit
precedence; exact-size retention refusal; job-body substitution; completed replay
retargeting; manual retry/orphan recovery; and backup/restore with a schema-4 job.

Run the focused suite with `cargo test --offline --locked -p workbench-core
processing::graph_tests --lib`. The existing capture tests and full ordinary core
suite protect the earlier read-only interface. No candidate interpreter, native
confinement, performance, all-platform bundled-installation or release-acceptance
claim follows from these tests. Production scheduling, real supervised job
ownership and shutdown/retry handling, verified runtime availability, explicit
platform activation and application commands remain separate work.

Local source verification completed with 20 focused graph-job tests; the full
ordinary core suite (467 library tests and 79 integration tests, with 31 native
tests left ignored); strict all-target Clippy; formatting and diff checks; and 23
Python graph contract tests in both normal and optimized modes. Schema generation
added only the two new files; all 100 historical schema files remained byte
identical. A separate read-only code review checked the repaired ownership,
revision, failure, replay and historical-inspection paths. These are development
source checks on the available host, not platform release approval.

Earlier development checks were not counted as passes: the first 12-case run had
one failure while the test tried to write a read-only original; the synthetic
corruption now explicitly removes and replaces that file. An 18-case run exposed
an invalid-JSON test fixture and two authorizer tests that did not reach the
intended recapture point. The large fixture now remains valid JSON, and both
authorizer tests require an observed `BEGIN` followed by a recapture read, with
explicit hit assertions. A dotted Python test invocation passed 13 adapter tests
but failed to import the canonical test's sibling helper; the normal discovery
invocation subsequently passed all 23 in both modes without Python source
changes. None of those earlier failures is presented as native execution evidence
or silently converted into a success observation.
