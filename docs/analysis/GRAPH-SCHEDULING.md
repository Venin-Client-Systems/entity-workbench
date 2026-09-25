# Exclusive graph scheduling — source implementation

This EW-17 increment adds a private scheduling interval and a complete synthetic
coordinator lifecycle for `shortest_connection_path_v1`. Production still selects
`GraphExecution::Unavailable`. It neither discovers Python through environment
variables or PATH nor attaches a runtime. An unavailable graph is durably Blocked
before reserving a drain, creating Running, capturing input or invoking an engine.
No public graph command, desktop control, database migration or public schema was
added. The earlier [graph-job source record](GRAPH-JOBS.md) remains historical.

The source base includes the signed prepared public collection cancellation
implementation. That cancellation route retains its off-lock preparation; its
short locked capture and commit phases now observe the same lock order as Resume
inspection and scheduling. Collection transport activation is unchanged.

## Ownership and ordering

The existing active-processing mutex now protects tokens and one scheduling state:

- **Open:** normal automatic claims are allowed. The oldest queued processing
  record is selected first. If it is a graph and a compatible queued collection
  record has an older canonical `records.sequence`, the collection lane starts
  first. Existing record sequence is stable on update; no new ordering subsystem
  or claim based on timestamps was introduced.
- **Draining:** freeze the graph's complete queued identity, existing processing
  registrations and the existing current/admitted collection ticket (job,
  generation, lease). New automatic claims stop. A ticket already admitted by
  Resume may enter its existing run; no newer ticket may do so. Existing
  processing tokens remain registered through terminal publication, and the
  collection driver finishes its bounded run, including off-lock preparation,
  reservation, response ownership and exact settlement retries.
- **Owned:** after processing registrations are empty and collection is strictly
  Idle with no current/admitted ticket, token, retry or recovery state, one atomic
  graph claim/capture commits. The interval stays owned through execution and
  publication, including PublicationPending. Automatic document and collection
  claims remain gated before their first possible canonical write.

A Faulted, Unpublished, RecoveryRequired or Stopped collection lane is checked
before queue priority. It visibly blocks an unclaimed graph rather than leaving
it behind an older collection record that cannot progress. SettlementPending is
owned work awaiting explicit settlement, not an idle lane. No new graph is batched
into a completed interval: success or a known-stopped durable terminal outcome
returns to Open and re-arbitrates canonical queues.

Lock order is **workspace → processing activity → collection lane**. All drain
and claim decisions hold the workspace lock. Collection inspection receives the
already-observed scheduling admission state; it never acquires activity while
holding the lane. Shutdown and execution quarantine cancel tokens under activity,
release it, then signal the lane. A separate graph condition variable waits with
the activity mutex; the existing workspace condition variable remains paired
with the workspace mutex.

A new public or internal Resume is refused before any canonical Start/Resume
write while Draining or Owned; `can_resume` reports the same refusal. Analyst
queue, cancellation and correction writes remain permitted. They can
conservatively stale an already captured graph. Cancel during Draining cancels
only the still-queued graph; it does not signal other frozen processes or claim
that a graph worker stopped. Joined shutdown releases an unclaimed drain without
an execution claim.

## Claim, result and retry lifecycle

R (requested), Q (queued), C (captured) and P (published) retain their existing
meanings. The private exclusive claim performs the following in one
`BEGIN IMMEDIATE` transaction:

1. Re-read and compare the complete queued job with the reserved identity.
2. Write Running with a fresh lease, advance actual meta revision to C and append
   the claim event.
3. Capture at that actual C using the existing non-nesting canonical reader.
4. Commit only if capture succeeds. Any error rolls back Running, event and C.

The coordinator holds the registration mutex before claim, registers the token
before releasing workspace ownership and retains the non-Clone GraphAttempt.
A capture failure may be recorded as a pre-launch Blocked outcome. If storage also
refuses that outcome, the queued identity and drain remain owned for retry; no
unregistered Running claim is left behind.

The worker receives only the existing bounded request. The privately held attempt,
exact raw result and coordinator ownership lifetime survive storage publication
errors. The engine is never rerun to retry publication. Strict validation at C and
atomic P=C+1 result/job publication are unchanged; there is no automatic rebase.
The output is capped at the existing 128 KiB before retention.

Private host inspection exposes job, attempt, lease, request digest, phase,
retry count and whether an exact retry is possible. It exposes no authority or
raw result. Up to three explicitly requested result-publication retries are
coalesced and bound to those identities. After exhaustion the attempt/result stay
owned. One new cancellation-triggered terminal settlement and one final shutdown
settlement remain available; neither invokes the executor. If shutdown was
already observed before initial publication, that publication is itself the one
final settlement. Spurious wakes do not create attempts.

Unknown exit takes precedence over cleanup failure, then cancellation. It
quarantines the existing workspace ownership before canonical recording and
prevents interval release. A known-stopped cleanup failure cannot publish a
result. If final shutdown settlement cannot commit, an inspectable
UnpublishedKnownStopped state retains the bounded attempt/result in memory and
shutdown fails under the existing quarantine policy. This does not establish a
durable known-stop record: after process loss the durable Running claim is
conservatively recovered as WorkerExitUnverified. No in-memory interval or
capture capability is reconstructed from persisted JSON. This slice introduces
no new file-lock leak or release override; existing quarantined ownership retains
its existing process-lifetime behavior.

## Source verification and limits

The new tests use real canonical SQLite workspaces and synthetic in-process
executors. They do not execute a packaged interpreter, create a native worker or
make network requests. Covered cases include two concurrent coordinator threads
serializing graph jobs, actual synthetic collection full-run draining, retained
collection settlement, exact Resume refusal with unchanged revision, prepared
Cancel during a drain, strict stale rejection after an analyst queue write,
atomic claim rollback, retained storage failures, exact result retry identities,
retry exhaustion, counted final shutdown attempts and unknown-exit quarantine.

The configured native adapter is a separate implementation and review boundary.
Its required seam is an opaque verified app-local runtime, exact request bytes,
coordinator-owned scratch root and caller cancellation token; only a bounded raw
result after confirmed termination, assignment checks and cleanup may reach this
lifecycle. No arbitrary recipe, shell, path, Python plugin or SQL is accepted.
The former fixed native graph experiment remains evidence for its own source and
recipe, not for this coordinator change.

Scheduling exclusivity does not promise responsiveness or a 30-second total job.
The existing canonical capture can verify up to 1,000 originals, each bounded at
64 MiB, without a separate aggregate original-byte ceiling (approximately
62.5 GiB in the extreme). Those reads currently occur while holding workspace and
SQLite transaction ownership. A future native adapter's prefix verification and
staging are also separate from its 30-second child interval. This source change
does not lower the accepted evidence domain or claim a performance benchmark.
Reliable native coordinator cancellation, runtime attachment, platform packaging,
Windows/Intel compatibility and release/security gates remain separate work.

## Verification record

The completed source checks cover 19 new scheduling tests and the repaired
existing shutdown-admission regression. The full ordinary core run passed 528
unit tests plus 79 integration tests; 31 native tests remained ignored. The
coordinator subset passed 67 tests with six ignored. The unchanged fixed graph
adapter also passed 13 stdlib contract tests normally and under `python -O`,
and ten real NetworkX tests in the existing locked development environment.
These are not executions of the packaged candidate interpreter. All 102 existing
schema files remain byte-identical.

Retained negative observations include the initial incorrect Cargo package name,
a missing test module while formatting a draft, a fixture field compile error,
the first Clippy enum-size refusal, the first full run's obsolete
claim-before-registration test assumption, an intermediate renamed-variable
compile error and a final needless-lifetime Clippy refusal. The fixes preserve
admission and result boundaries; there was no native retry or limit change.
The source-bound evidence companion pins final files, schemas and successful and
failed logs, distinguishing transcript-only early commands from retained files.
