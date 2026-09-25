# Search execution ownership

This source increment contains uncertain local-search execution. It does not change
Java's ranked result contract, capture a bounded corpus snapshot, enable Windows
Search, or establish a new native confinement or performance result. Search remains
the existing synchronous macOS development recipe, with its existing per-worker
30-second and resource limits. Preparation, rebuilding and querying are not one
30-second whole-command deadline.

## Admission and persisted intent

Standalone store Search acquires the existing workspace `processing.lock` owner.
Coordinator Search borrows its exact existing owner; it never creates a second
coordinator. Both refuse quarantined/released/cross-workspace ownership, retained
processing exit uncertainty and collection recovery-required state. A standalone
owner also refuses retained Running processing/collection claims and the existing
legacy interrupted-without-lease category. Search does not turn these records into
confirmed stop observations or perform canonical recovery writes.

Under the index's existing `coordinator.lock`, a new search sequence creates
`indexes/lucene/execution-intent.v1.json` without replacement. The small fixed record
contains its version, fresh attempt UUID, assigned workspace revision and fixed
`index_then_search_v1` identity. It contains no cleanup path, query, PID or authority
to signal a process. The intent file and containing directory are synced before any
index rebuild, staged input or worker launch. One cache lease covers both optional
indexing and the query. The cache directory and created control files have retained
filesystem identities; linked/reparse paths, special files and hardlinked control
files are refused. The worker profile is unchanged: it cannot write this parent
intent or sibling assignments.

Presence of any intent requires recovery, including empty, malformed, unknown-version,
linked or unreadable intent paths. Startup applies this check when it acquires the
workspace owner, so a surviving intent blocks ordinary processing/graph/collection
admission too. The intent is never parsed as permission to recover. Its presence is
not a claim that a process definitely launched: a crash between reservation and
launch intentionally leaves a conservative refusal. An invalid *revision cache
marker* may still cause a rebuild when there is no execution intent; it is a separate
cache-validity mechanism.

## Completion, errors and process loss

The internal completion returns the original typed result plus a closed disposition:
`Released` or `RecoveryRequired`. Coordinator policy uses that disposition directly,
not error-message text or a filesystem lookup after failure. Recovery disposition
quarantines the workspace before its mutex is released; the coordinator then cancels
other owned executors through its existing quarantine mechanism. Existing read-only
inspection and already-owned publication/settlement remain governed by their prior
ownership rules.

* Unknown child exit preserves `TerminationUnverified` unchanged. It precedes input
  removal, result acceptance and all assignment/index/intent cleanup. A second worker
  in the sequence is not launched. The unresolved cache lock and workspace owner are
  retained until process exit; the pre-launch intent persists after that OS lock
  disappears. No after-error marker write is needed.
* Confirmed execution errors retain their category when owned cleanup succeeds.
  Successful execution also requires owned input/assignment cleanup and safe intent
  removal before the existing Search result can be returned.
* A cleanup or finalization failure rejects success and requires recovery. Cleanup
  diagnostics retain the preceding error. If unlink succeeded but the final directory
  sync failed, the intent may already be absent; the in-process owner remains
  quarantined. Unlink was reached only after confirmed stop and assignment cleanup,
  so this is not permission inferred from an uncertain child.
* Search assignments are explicitly retained immediately after temporary-directory
  creation. A panic cannot trigger `TempDir`'s implicit deletion. An incomplete
  search permit quarantines workspace ownership on unwinding. A panic or crash is
  never promoted into a confirmed termination result.

There is no automatic or user-facing intent-clearing/recovery action in this slice.
An obtainable lock, elapsed timeout, revision match or recorded numeric PID would not
establish that the old worker stopped. Same-user administrator tampering remains
outside the existing worker threat boundary; no sandbox or filesystem permission
is broadened by this change.

## Shutdown and scope

Synchronous Search is not one of the scheduler worker threads. Shutdown first sets
stopping, cancels known tokens, and joins its existing worker registry. It then takes
the workspace mutex as a final Search barrier before releasing workspace ownership.
Joining happens **before** this barrier because graph and collection publication can
need the workspace. Search rechecks stopping under that same mutex. A poisoned
barrier or quarantined owner refuses normal release. This does not add a Search
cancellation API or a new latency/concurrency guarantee.

No canonical record, workspace revision, database schema, historical schema or public
Search DTO changes. Backups continue to omit index caches and restore only into a
fresh destination. Neither backup nor restore proves termination in the original
workspace or clears its unresolved intent.

## Source verification

Synthetic tests cover no-launch refusal, shared and standalone ownership, bootstrap
quarantine, existing orphan/unknown categories, both sequence phases, exact unknown
error precedence, failed cleanup/sync/write, input/control identity, retained panic
assignments, subsequent refusal and the shutdown barrier. The actual assignment
wrapper is tested with closures and files, without starting a worker. Existing native
Lucene fixtures use an ordinary canonical temporary parent rather than the macOS
`/var` alias; the ignored native test is not executed for this handoff. The native
runner's source list includes the extracted modules, preserving honest future source
binding.

Initial extraction/import compile failures and the first focused run's two error-kind
failures are retained with the source evidence. The latter refused execution but
returned Validation instead of the required Blocked; the refusal categories were
repaired. Native search execution, supervisor-crash descendant containment, verified
intent recovery, bounded/off-lock corpus capture, ranked query receipts and Search
responsiveness/concurrency accounting remain separate work. Existing release gates
remain unchanged.

Final local source verification passed 29 focused lifecycle cases and the complete
core suite: 583 unit plus 79 integration tests, with 31 specialized/native tests
ignored. Strict all-target host Clippy, formatting and diff checks passed. Python
source tests passed in normal and optimized modes: 345 tests in each, including
four platform-dependent skips. Source-list membership and digest readback passed.
The initial Clippy attempt found a test-only non-octal permission literal, corrected
without changing permission semantics. A Windows-target check stopped in the `ring`
build prerequisite because the local MinGW compiler was unavailable; it did not
verify Windows core Rust or native execution. Logs, including earlier failures, are
pinned separately in the source-bound evidence handoff.
