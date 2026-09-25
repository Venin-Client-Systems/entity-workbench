# Durable collection snapshot and inert bundle

This source adds a synchronous, read-only export of one exact durable **v4** run. It is useful for inspecting acquisition evidence, including failed and unfinished work. It does not activate collection, execute a worker, acquire a second execution lock, establish benchmark eligibility, or alter a historical run. The ordinary native collection availability flag remains false.

## API and identities

`Workspace::durable_collection_snapshot(job_id)` returns a bounded typed snapshot. `Workspace::export_durable_collection(job_id, export_id)` publishes an inert bundle. The caller must retain a canonical UUID `export_id` **before** invoking publication. `Workspace::inspect_durable_collection_export(export_id)` independently validates an existing bundle. There is no public command or UI change.

The development harness exposes the same store methods:

```text
ew-dev snapshot-durable-collection WORKSPACE JOB_UUID
ew-dev export-durable-collection WORKSPACE JOB_UUID EXPORT_UUID
ew-dev inspect-durable-collection-export WORKSPACE EXPORT_UUID
```

All arguments are positional and extra arguments are refused. Paths are local development inputs; collected URLs and source names never become filesystem paths. New additive schemas are `durable-collection-snapshot.v1.schema.json`, `durable-collection-export.v1.schema.json`, and `durable-collection-export-inspection.v1.schema.json`. Prior schemas and record versions remain unchanged.

The snapshot retains the exact UTF-8 canonical run JSON, its byte length and SHA-256, the captured workspace revision, the existing standalone inspection projection, and an ordered source identity catalogue. The raw run retains input/policy/mode, journal, request ancestry, irrevocable charges, unresolved reservations, raw transport observations, deadlines, cancellation and unknown-termination state. Derived inspection controls are conservatively standalone/disabled; they are not current coordinator execution permissions. Neither a lease string in the raw JSON nor this read-only snapshot is a write or execution capability.

Each source entry preserves evidence ID, original digest and length, the SHA-256 of the complete canonical metadata row at capture, and every acquisition entry for this exact run, including duplicates. Unrelated source text, names and other jobs' acquisition history are not copied. The metadata digest is a captured reference; its full preimage is deliberately not included and cannot be recomputed from this projection. Original bytes, exact run bytes and run-specific acquisition bindings are fully checked. The bundle's self-consistent hashes detect mismatches, but are not authenticity signatures against an actor rewriting the complete bundle.

## Capture, replay and publication

The existing bounded input capture is shared as inert data. A distinct private read-only wrapper replays and validates it without acquiring execution ownership. Owned mutation capture still requires its exact live publication owner and lifetime; only that wrapper can construct a prepared mutation. Quarantined execution does not prohibit this read-only export.

Capture uses one SQLite read snapshot, preflights source metadata before copying it, validates content-addressed evidence keys, and preserves the exact raw run. Replay uses the existing v4 machine and verified original reader. A subsequent exact check rejects revision, run or source metadata drift and rehashes originals. Unlike mutation preparation, the read-only export accepts **no** cancellation suffix or other run drift.

Publication creates only `exports/durable-<UUID>/`, with fixed names:

```text
snapshot.json
originals/<sha256>.bin
complete.json
```

Every creation is no-clobber. Existing complete or partial identifiers are refused. Only inert `.bin` originals are emitted; the extension and escaping do not grant permission to execute their content. Source originals are copied from returned verified bounded bytes, rather than a reopened unverified source path.

After preparation/copy, an **IMMEDIATE** SQLite transaction excludes other SQLite writers from final exact revision/source revalidation through completion-marker publication. It makes no canonical writes and rolls back on scope exit. This interval includes up to 100 MiB of source revalidation, copied-file verification and filesystem synchronization; it has no responsiveness or short-latency claim. Snapshot-only reads use a deferred transaction and return the consistent captured revision, not a promise that the workspace remains unchanged after the method returns. Filesystem checks remain separate from SQLite writer exclusion.

The completion marker is written last and binds export UUID, job, revision, exact run hash, snapshot hash/bytes and ordered original inventory. A marker is never accepted on its own. Inspection bounded-reads every declared object, rejects extra files, verifies identities and hashes, replays the frozen v4 journal, validates acquisition/source bindings and compares the typed inspection. It uses the frozen originals, so later current-workspace metadata changes or missing/corrupted current originals cannot redefine a previously completed bundle.

## Failures, acknowledgement and recovery

Before marker creation, a failure attempts removal only of entries with the operation's retained handle identity and verified bytes. Unexpected, changed or replaced contents make cleanup fail explicitly; there is no recursive deletion. Creation followed by failed ownership registration returns `Cleanup` and leaves the unproven partial directory intact. Callers must retain the UUID and must not treat this as a free identifier.

From the start of marker creation onward, any error is an explicit unconfirmed-completion `Cleanup` result, and the directory is retained. A lost reply likewise must be resolved by inspecting the known UUID. Repeating publication refuses the existing target and cannot replace an earlier successful export. Missing/truncated/invalid markers or incomplete inventories remain failures, even if some originals are intact. There is no automatic retry, repair or adoption of partial bundles.

All ancestors and entries are checked for links; files must be ordinary, singly linked files. Unix uses no-follow bounded reads and device/inode identity; Windows uses reparse-point opening, full file identity and restrictive sharing. Cleanup checks identity before removal and refuses observed substitution. This is not a sandbox against arbitrary concurrent same-user filesystem mutation. File writes are synchronized, with directory synchronization on Unix; neither this source nor its synthetic tests establishes power-loss durability or an atomic multi-file filesystem transaction. Native Windows execution is not claimed by host compilation.

The export adds **no canonical reference or database record**, revision increment, receipt, acquisition or observation. Backups therefore cannot gain a dangling dependency on an export. Existing backups exclude export directories while retaining the run and referenced originals; restore can produce an equivalent snapshot under a new caller-chosen export UUID. Export bundles are local copies, outside canonical backup retention. General collection inspect/start/cancel/reservation semantics and historical policies are unchanged.

## Bounds and limitations

The shared capture permits a run body up to 4 MiB, at most 50 referenced originals, metadata rows up to 4 MiB each and 16 MiB aggregate metadata. Complete response bodies remain bounded at 2 MiB each and 100 MiB aggregate. A capped serialization writer rejects a snapshot above 16 MiB before allocating its serialized output; the completion marker is at most 64 KiB. Inspection imposes the same file/count bounds and checks the exact inventory. Failure is whole-result, without silent truncation or invented response bodies for unresolved charges.

These are byte/count bounds, not a total RSS, CPU or cancellation guarantee. Canonical input, typed projection, raw JSON and encoded snapshot can coexist. HTML replay and original I/O are synchronous. Calling these store methods while holding a coordinator workspace mutex retains that mutex for the call; there is no new off-lock coordinator route in this slice. Completed acquisition still has the existing source-media reinterpretation limitation, rather than a newly invented immutable web derivative.

This does not change the frozen EW-05 benchmark. In particular, interpreting its seed as the first **content** request, after charged access review, is only a proposed clarification requiring separate resolution. Cold task initialization, access-review terms/robots accounting, ranked local query/display evidence and independent labels remain later work. Labels and unexecuted tasks must remain unknown/not run; this export cannot manufacture scores or benchmark acceptance.

## Regression scope

Synthetic real-store tests cover exact raw-byte export under an existing/quarantined owner; unresolved charged and interrupted/partial/failed states; source/revision drift; WAL writer exclusion during marker publication; no-clobber and acknowledgement-loss inspection after current-source change; partial markers and invalid inventories; modified originals, linked files and observed replacement; explicit ownership-registration/cleanup failure; byte bounds; and backup/restore equivalence with unchanged canonical rows/history. Existing ownership, quarantine, cancellation and clock-rollback regression groups continue to exercise the shared capture refactor. No network or ignored native campaign is required or run for this change.
