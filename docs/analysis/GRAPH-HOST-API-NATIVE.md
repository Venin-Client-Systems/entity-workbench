# Native host constructor and public graph proof

The source-only contract below is preserved. A later
[native observation](NATIVE-GRAPH-HOST-RESULTS.md) now passes with separate
source, executable, workspace and runtime evidence.

This is a source-only, explicitly ignored development experiment. No candidate
interpreter has been executed for this slice. It is designed to test the actual
`JobCoordinator::start_with_development_app_resources` constructor together with
the published graph commands. It does not build or control a Tauri application,
change normal startup availability, or establish a release claim.

The [earlier coordinator campaign](NATIVE-GRAPH-COORDINATOR.md) constructed a
verified capability directly. This experiment instead starts an empty coordinator
through the resource constructor, which verifies its fixed `engines/python`
child. No capability, worker implementation, runtime verifier or document
executor is injected into this native case. The only observation attachment is
test-only and passive: a per-runtime `OnceLock` accepts the existing observer
once before the first public queue command. Duplicate attachment fails. The
existing consume-self observation API remains available for the prior campaign.
There is no global observation hook or new worker IPC.

## Single finite scenario

The fixed synthetic fixture has six entities, nine accepted observations, and
nine assertions: six accepted, one pending, one rejected and one deferred. Its
one retained original is fictional. No URLs are fetched and no competing
document or collection job is submitted.

The experiment performs this sequence:

1. Capture the complete seeded canonical state at revision 2. Start the actual
   resource constructor and require that startup preserved the state.
2. Attach the passive observer to the actual configured capability. Dispatch
   `QueueGraphPath` for `a` to `c` with revision 2 and a fresh canonical UUID.
   Require a queued acknowledgement at revision 3 and availability `ready`.
3. Observe one adapter call and one child launch. Wait for confirmed process
   termination, confirmed cleanup and the end of the owned publication interval
   before reading output bytes, canonical records or originals again.
4. Dispatch `PageGraphJobs` at revision 5, select its one job through
   `InspectGraphJob`, then pass that response's exact result ID and request/result
   digests to `InspectGraphAnalysis`.
5. Require the frozen `a → b → c` path, both parallel first-hop assertions and
   the second-hop assertion. Bind the actual raw request/result and host wrapper
   to the observer, saved record and resource manifest. Require revisions
   requested 2, queued 3, captured 4 and published 5.
6. Replay the original public queue command, including its old revision and
   exact UUID. Require the same completed inspection, no canonical write and
   still only one worker call/launch. No publication-retry command is issued.
7. Join shutdown, reopen the workspace, and repeat immutable inspection through
   the public dispatcher with the same digests. Require identical saved data,
   unchanged originals and empty processing scratch.

The expected canonical delta is specific: add the request mapping, processing
job and graph result; retain the queued and running job bodies in history; append
exactly the graph queue, claim and finish events. The receipt compares schema,
storage version, metadata, records, history, events, derivative objects and
SQLite sequences, plus every original filename, byte count and digest. Queue
and completed acknowledgements, discoverable result metadata and full inspection
are bound to the actual canonical bodies, not merely detached JSON files.

## Resource admission and failure handling

The runner requires an absolute, canonical UUID-named artifact directory already
containing only `resources/engines/python`. The artifact directory, `resources`
and `engines` must be ordinary private directories owned by the current user.
The Python child must be an ordinary directory. The explicitly supplied true
original prefix must be separate, with no ancestor overlap. There is no PATH,
environment-based runtime discovery, copying, staging, package installation or
download in this runner. Preparing the reviewed resource copy is a separate
explicit operation before any future invocation.

Any retained workspace, receipt, log or unexpected entry causes admission to
fail before writing anything. An inadmissible directory is not adopted merely
to save an error report; refusal is returned on standard output. After admission,
an initial failed report is saved before source metadata, inventory or build
work. The Rust test independently checks the exact runner-created file set
before creating its workspace or native-start receipt.

The runner binds the full clean tracked Git tree, all file hashes, the original
and resource runtime inventories, and the actual retained release test binary.
It rechecks source/binary identity and both prefixes after successful, confirmed
completion. The runtime manifest remains
`4dc6fd171e842d1f9254be7fc5cb16e2e01203896403dcd9839a8aec69dad822`.
Existing fixed graph assets, no-network development confinement, nonce/raw-byte
validation and supervisor remain unchanged.

There is one native invocation, no warm-up and no automatic retry. The offline
build deadline is 600 seconds; the outer native-test deadline is 300 seconds;
the coordinator wait is 180 seconds. The adapter retains its existing 30-second
child wall/CPU limits. These are not preparation-inclusive deadlines: resource
verification, startup, capture and final inventory verification are separate.
Existing request/result/wrapper limits remain 1 MiB, 128 KiB and 64 KiB. The old
canonical probe's 64 KiB request limit is unchanged.

Unknown termination, failed cleanup, a missing/malformed lifecycle receipt or an
outer native timeout prevents subsequent output, database, original and runtime
traversal. No cleanup or retry is attempted in that state. The fixture deliberately
retains its coordinator instead of letting `Drop` perform implicit teardown when
supervision has not established stop and cleanup. Parent-written bounded failure
receipts are retained; they are not worker output. A successful receipt requires
joined shutdown and read-only reopen checks in addition to adapter success.

## Source checks and remaining proof

Offline Python tests exercise directory admission, refusal to adopt prior state,
source-failure retention, exact response/reference/revision binding, complete
canonical deltas and detached-body rejection, raw-byte identity, and timeout or
uncertain-termination suppression of all post reads. A Rust source-only test uses
real canonical queue/claim/publication operations with a closed synthetic result
to validate the expected table delta; it is separate from the ignored native
case and invokes no worker. Observer tests verify one-shot attachment and the
unchanged consume-self API without validating or executing a runtime prefix.

An exact clean signed source and all offline checks must be reviewed before a
separately coordinated single native invocation. Even a future passing receipt
would prove only this host-constructor/public-command experiment. Packaged Tauri
startup and UI, normal default activation, signed-helper confinement, unsupported
platforms, hard RSS enforcement and general release readiness remain separate.

## Retained source verification

The ordinary core suite passed 700 tests (621 unit and 79 integration); all 34
native tests remained ignored. Strict all-target Clippy passed in host debug and
release configurations and for the Intel macOS cross target. Formatting passed.
The normal and optimized Python suites each ran 384 tests: 380 passed and four
platform cases were skipped. These include nine new offline host-proof guards.
All 110 historical schema-directory files match the integration base exactly.

The first broad Python discovery run failed because its previously loaded test
module shadowed the imported prior runner. That first failure is retained. The
repair gives the new runner the distinct name `run_graph_host_native.py` and
loads the prior runner through its exact source path. Both final complete Python
runs include the repaired discovery path. These checks executed no candidate
interpreter and staged no runtime; the ignored native proof remains unobserved.
