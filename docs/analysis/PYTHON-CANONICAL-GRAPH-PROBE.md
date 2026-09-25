# Test-only canonical graph native recipe

This EW-17 increment implements source for one fixed native test:
`engines::supervision::python_probe::canonical_graph::native_python_canonical_graph`.
It is ignored in ordinary Rust tests and had **not been executed** at the source
handoff documented first below; the later finite native observation follows at the end. Earlier NetworkX compatibility successes do not verify this new recipe.
There is no application command, production queue, migration, canonical result
publication or release-gate change.

The outer recipe is `python-canonical-graph-v1`. The analytical operation remains
[the existing closed graph protocol](PYTHON-GRAPH-ADAPTER.md):
`shortest_connection_path_v1`, policy
`accepted_undirected_all_retained_time_v1`, NetworkX 3.6.1. Runtime identity remains
`4dc6fd171e842d1f9254be7fc5cb16e2e01203896403dcd9839a8aec69dad822`.
A successful response echoing that identity is insufficient: the trusted native
supervisor verifies the entire immutable prefix and assigned code, and the parent
records the clean source tree, actual compiled binary and expanded profile hashes.

## Fixed canonical ownership

The trusted Rust test creates a fresh, retained `canonical-workspace` under its
owned artifact directory. It imports synthetic text and creates six entities,
six accepted assertions (including two parallel assertions), and three rejected,
pending or deferred shortcuts. The private `CanonicalProbe` holds the real
Workspace and its non-deserializable, single-use captured handle. It invokes the
existing capture API at revision 2 for `a` to `c`; no manually assembled request
can replace that authority. Only the request bytes enter the worker assignment.

The worker has no workspace/database/original read grant. It receives fixed code,
the selected 58-version fixture, an assigned request, and a host-created assignment
with independent campaign UUID, job UUID, capture nonce and request size/hash.
The request contains IDs and undirected connectivity, not source text, evidence
paths, SQL, query expressions or executable instructions. The synthetic workspace
is a sibling of the assigned job and is retained on failure for inspection.

After confirmed termination, Rust validates the **original raw result bytes**
through the private handle. It independently requires the shortest path `a,b,c`,
all hop provenance `[[r1,r1-parallel],[r2]]`, and the existing historical-period
limitation. The accepted graph can combine disjoint historical periods; it does
not establish a contemporaneous, directed or causal relationship.

The host also compares canonical logical content before and after execution:
exact ordered rows in `meta`, `records`, `history`, `events`, `derivative_objects`
and `sqlite_sequence`, the schema and user version. A new table is refused rather
than omitted. This includes sequence columns, event timestamps and complete
history bodies, not merely row counts. Raw SQLite-file layout is not asserted
unchanged. Original evidence identities are checked by the normal capture and
result-validation APIs. Validation consumes its capture and performs no writes;
a stale revision or changed provenance refuses acceptance.

## Import and process boundaries

The existing `python_probe` profile, process configuration, environment and wait/
cleanup functions are reused. The profile bytes generated for a given prefix and
job are unchanged. It grants reads to the verified prefix and assigned code/input,
and writes only to job scratch; network and process fork remain denied. The child
uses `-I -S -B`, exactly the reviewed standard-library paths, verified site-packages
and assigned adapter directory. Existing HOME handling remains unchanged; no
HOME-content read grant is added. No `.pth` file or console wrapper is processed.

Before any NetworkX import, the fixed Python recipe validates the full request,
its assignment identity and all 58 selected distribution versions. It then refuses
an already imported NetworkX or any `NETWORKX_*` environment key, rejects every
`networkx.backend_info` entry, and requires exactly the reviewed built-in
`networkx.backends` entry: `nx_loopback` owned by NetworkX 3.6.1, value
`networkx.classes.tests.dispatch_interface:backend_interface`. The check reads
metadata; it never calls `EntryPoint.load`. This matters because upstream NetworkX
executes discovered backend-info providers during its top-level import, before
an algorithm's `backend='networkx'` option can take effect.

After import, the pinned module must resolve under the verified site directory,
with no discovered or loaded backend and only core backend information. The
existing adapter explicitly dispatches to the NetworkX core implementation. All
engine package bytes and plugin metadata remain bound by the full prefix manifest.
The new malicious-metadata tests use inert fake entries whose `load` method would
fail if called; they run only trusted host stdlib and execute no plugin code.

## Results, diagnostics and failure preservation

The new recipe adds no grant or limit increase. Limits remain 30 seconds for the
whole supervised process, 120 seconds for the outer native test, and 300 seconds
for its offline release build. Existing process/file/job limits still apply. The
fixed request is capped at 64 KiB; the graph response at 128 KiB; the wrapper at
64 KiB. These are acceptance/read bounds within the existing scratch monitor,
not a new hard heap limit or per-file live write quota.

Closed checkpoints are `bootstrap`, `versions`, `metadata`, `imports`,
`init-ready`, `operation`, `complete`. Four ordered monotonic/process-CPU records
pair initialization before/ready and operation before/after. Initialization ready
means all imports needed for this graph operation are complete. Operation after
means the worker computation and result-file serialization completed; authoritative
Rust validation occurs only after process termination. Timings begin at diagnostic
construction, not spawn, and paired differences describe this one operation only.

The trusted host retains bounded raw `captured-request.json`,
`captured-result.json` and `captured-wrapper.json` with independent size/SHA-256
identities. The wrapper has a closed typed shape and must match job, campaign,
capture nonce, runtime, versions and actual raw input/result identities. Graph
JSON goes directly to the canonical validator, preserving duplicate fields for
rejection. The parent separately checks retained bytes, fixed topology, code
hashes, wrapper fields and host-validated parallel provenance. An echoed checksum
never grants the worker canonical ownership.

The initial receipt is failed/not-started before preparation or launch. Runtime,
profile, assignment, job and capture identities are recorded before spawning.
Every failure stays failed; there is no retry or alternate recipe. On unknown
termination the native supervisor returns before reading output, diagnostics or
assigned files, and preserves its quarantined job rather than cleaning it. The
outer runner may inspect only the bounded trusted native receipt; it performs no
candidate-output reads or prefix post-verification. A timeout or an uncertain
lifecycle never authorizes another campaign.

For confirmed execution, the existing supervisor cleans the owned assignment;
cleanup failure defeats success. The parent independently verifies both the true
input prefix and the new moved prefix. The canonical workspace, moved prefix and
receipts remain retained under the artifact directory for diagnosis. A confirmed
ordinary failure preserves diagnostics and remains a failure after integrity
checks.

## Future reviewed invocation

The existing `scripts/test_python_isolation.py` runner has one additional closed
case, `--case canonical-graph`. It requires the original verified prefix, a fresh
artifact child and `--execute-reviewed-probe`. It first copies a fresh moved prefix
without executing its interpreter, verifies the copy, builds one exact release
native test and launches it once. Existing compatibility, hostile and engine
receipt shapes remain closed; old evidence files are unchanged.

A source review and an explicit finite test decision precede candidate execution.
An eventual success would establish this fixture's test-only capture/supervisor/
validation path. Production cancellation/publication, scale, reliable cold start,
other platforms, complete notices, signing and clean-install readiness remain
separate requirements. No spaCy, Splink or combined-startup pass is inferred.

## Ordinary verification scope

Rust tests cover real capture authority, fresh/no-clobber workspaces, stale and
cross-workspace refusal, consumed handles, complete canonical-content fingerprints,
parallel assertions, exact assignment correlations, duplicate/unknown/oversized
raw JSON, and rejection of the generic recipe path without an owned handle.
Python tests cover valid and malicious pre-import metadata, input validation before
import, full version checks, partial operation/no-clobber failure, independent
receipt hashes/correlations, old-schema refusal, one-attempt relocation and explicit
unknown-termination/timeout suppression of dependent reads. These tests do not
execute the staged interpreter. Existing cross-language fixture equality and the
historical compatibility/probe tests remain part of ordinary verification.

Source verification at this handoff passed 447 ordinary Rust library tests and
79 integration tests; all 31 native tests remained ignored. Strict core Clippy
across all targets and formatting passed. Full script discovery passed 340 tests
per run, with four explicit skips, on trusted development CPython 3.13.11 and host
CPython 3.14.2, both normally and with `-O`. The new Python module contributes ten
focused tests; the Rust additions contribute five ordinary tests and one ignored
native test. Read-only source comparison verified that the profile function,
production supervisor, graph adapter, fixtures and historical evidence are
unchanged from the base. This records source verification only.

## First finite native observation

After the source handoff above was reviewed and its signature verified, exactly
one `--case canonical-graph --execute-reviewed-probe` invocation ran from clean
signed source `f76ef907e3aa0ba5072d742e07da4cea76fc3fec`, tree
`3faa3b6a3bff5b5e341a3268980ec7dbb1d97c76`, on macOS 26.6.2 arm64. It passed.
The original reviewed prefix was supplied directly to the runner, which copied
its own fresh moved prefix before execution. There was no warm-up, fallback,
retry, source mutation, grant change or increased deadline. The exact private
invocation record and raw build/native/runner logs remain retained.

The offline release build reused a trusted Rust compilation cache and completed
in 2m 53s, within the unchanged 300-second build limit. One other agent's already
active warm compile and focused tests ended while this runner's trusted outer
receipt still reported `compile-release-native-test` and `native_test_started`
false. That observation limits the recorded overlap to preparation/build; it is
not a general idle-host claim. Neither agent ran another native candidate during
this experiment. Runtime verification and filesystem caches also preclude a cold
cache claim. The following values describe this one observation, not reliable
startup, a benchmark, a p95 target or graph capacity.

| Observation | Actual result |
| --- | --- |
| Trusted preparation | 3,125 ms |
| Initialization paired elapsed / process CPU | 384 / 361 ms |
| Operation paired elapsed / process CPU | 0 / 1 ms |
| Spawn through confirmed reaping | 1,185 ms |
| Entire native test including inventory work | 6.52 s |
| Exit status / stdout / stderr | 0 / 0 bytes / 0 bytes |
| Termination and assignment cleanup | Confirmed; no assignment directory remains |
| Runtime metadata | All 58 versions matched |
| Pre-import metadata | No backend-info entries; only the approved ignored backend entry |
| Captured revision and accepted path | 2; `a,b,c` |
| Full parallel hop provenance | `[[r1,r1-parallel],[r2]]` |

The operation's zero millisecond wall difference and one millisecond CPU
difference are arithmetic differences of separately rounded checkpoint clocks;
zero does not mean no elapsed work. Worker operation completion precedes the
separate post-reap Rust validation. The captured path retains its all-retained-
time/undirected limitation and makes no temporal or causal inference.

The parent independently accepted the native receipt and its raw retained
request, graph result and wrapper, checked all 33 source-file hashes and actual
binary identity, then verified the true original and moved prefixes again. The
before/after inventories are equal: 11,320 files / 601,821,300 bytes / 58 installed
RECORDs, under the same immutable manifest `4dc6fd…dad822`. The inventory report's
`assembled-unexecuted` and `package_code_executed: false` fields describe the
historical installation operation; they do not negate this later explicit native
execution. `runtime_components_satisfied` remains empty and no release gate is
changed.

A separate read-only SQLite check of the retained canonical workspace passed
`quick_check`, reproduced the entire logical-content hash recorded before launch,
and found 25 records, two events, one metadata row and empty history/derivative
catalogue. Same-count history/event mutation coverage comes from ordinary tests,
not from an assertion that this native fixture had historical rows. The exact
schema and sequence content were also included. The workspace, both prefixes and
all original evidence remain retained; no canonical result was written.

| Bound identity | Value |
| --- | --- |
| Campaign | `356c7b95-a336-43d7-a4ec-ffa61a4ed0d7` |
| Job | `d7a35f1e-baed-4ce1-99c1-9559cb741ab4` |
| Capture nonce | `182cc33d-8e54-4d96-857f-b3534a052d99` |
| Actual native binary SHA-256 | `1925aa7ce1aa33d0af1866d5bf4a902621aaf657033b433b5bf8bf8e2bdde02c` |
| Expanded profile SHA-256 | `754cf151ce4b3e6ca966a403cde746dd2cbc5a16759a30e32ac6226b5cb50ced` |
| Canonical snapshot SHA-256 | `4e020660c8721767f09ad6943cfdbeabcff8e4b397cba77b65688793ad4771fb` |
| Complete canonical-content SHA-256 | `f91cc489be1f51b4cd4b0c9c0817cbcc6f2040bd15714aa0c9350f72fa654f70` |
| Request SHA-256 (530 bytes) | `6c3e8e80b33345ea2dca51ff699c3fb42fa2db0eac0c1c54548a2aae4777d70f` |
| Graph response SHA-256 (453 bytes) | `65b8fad9c0d6aa04517f78878bf4655fd082634704bdcb6d05b72abc7e7ee0c8` |
| Wrapper SHA-256 (2,036 bytes) | `c26e67614049fbe000d7b7fbc9ee624c7e5103d9b0ca1363d9eb4ba7b9208a66` |
| Outer receipt SHA-256 | `70d78b7574ac9da13c80fc9072be87e716dd845d991c8d38abc6244abdc3bfd8` |
| Native receipt SHA-256 | `87ec97f88ba226ed93ccbb1cc6a4e05be7cd4230ba77c2e8e58b356ba581d8b5` |

Byte-identical retained observations and synthetic wire files:

- [Outer receipt](../../packaging/evidence/python-canonical-graph-first-2026-09-26.json)
  and [native receipt](../../packaging/evidence/python-canonical-graph-first-native-2026-09-26.json).
- [Captured request](../../packaging/evidence/python-canonical-graph-first-request-2026-09-26.json),
  [raw graph result](../../packaging/evidence/python-canonical-graph-first-result-2026-09-26.json)
  and [wrapper](../../packaging/evidence/python-canonical-graph-first-wrapper-2026-09-26.json).
- [Independent readback, inventory equality and log hashes](../../packaging/evidence/python-canonical-graph-first-integrity-2026-09-26.json)
  and [resource observation](../../packaging/evidence/python-canonical-graph-first-conditions-2026-09-26.json).

This success establishes the fixed synthetic Rust capture → confined packaged
NetworkX → Rust owned-handle validation experiment. It does not activate a durable
analytical job, production cancellation/publication, a desktop command, arbitrary
graph operations, Parquet snapshots, scale, reliable cold startup, spaCy/Splink,
wider networking coverage, hard RSS or supervisor-crash containment. Intel Mac,
Windows Python workers, supported minimum OS versions, complete notices, signed
packages and clean-install release gates remain separate. All earlier Python
startup failures and their source-bound observations remain unchanged.
