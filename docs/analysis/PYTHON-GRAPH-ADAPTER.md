# Fixed Python graph protocol adapter

This source-only EW-17 increment adds `workers/python/graph_path.py` for the
Rust-owned [graph capture and validation contract](GRAPH-SNAPSHOT.md). It uses
only the already locked NetworkX `3.6.1` dependency. It does not change the
experimental `worker.py` envelope, add an application command or worker launch,
select canonical records, write SQLite, or publish findings. Existing native
compatibility receipts do not cover this new adapter.

## One operation and closed wire format

The only accepted recipe is `shortest_connection_path_v1`, with policy
`accepted_undirected_all_retained_time_v1`. Rust has already selected accepted
assertions and retained all provenance before generating this request. Python
builds `nx.Graph`, includes isolated nodes, and calls `nx.shortest_path` with the
explicit `backend='networkx'`. It never filters review states, chooses sources,
invents record versions, attributes one parallel assertion to a hop, calculates
confidence or makes a temporal inference. Rust's independent shortest-path and
provenance validation remains authoritative.

`execute(request_bytes)` returns result bytes. Its request contains exactly:

- `schema_version` (integer 1), `recipe`, `policy`, `nonce` (canonical version-4
  UUID), and `workspace_revision` (an unsigned 64-bit integer).
- `snapshot_sha256` (64 lowercase hexadecimal characters), `engine` (`networkx`),
  `engine_version` (`3.6.1`) and `runtime_manifest_sha256`.
- Distinct `source_id` and `target_id`, sorted unique `nodes`, and sorted unique
  `edges` containing canonical undirected pairs. Endpoints and every edge node
  must belong to the supplied node set; self-edges are retained as ordinary data.

The fixed runtime identity is
`4dc6fd171e842d1f9254be7fc5cb16e2e01203896403dcd9839a8aec69dad822`, the reviewed
macOS Apple Silicon development prefix. **Echoing this string is not proof that
the packaged interpreter executed.** The future supervisor must independently
verify the complete runtime, assigned code and process confinement. This module
cannot authenticate a worker-supplied snapshot; only the Rust-owned handle does.

The result echoes exactly the nine identity fields and adds one `outcome`:
`{"state":"path","nodes":[...]}` or `{"state":"unreachable"}`. Only the
engine's specific `NetworkXNoPath` exception means unreachable. Import failures,
unexpected algorithm exceptions, wrong engine versions and malformed engine
results remain failures. Equally short valid paths may differ in tie selection;
Rust checks shortest distance and actual hops rather than requiring one tie.

The adapter permits at most 1 MiB input, 1,000 nodes, 5,000 undirected pairs and
128 UTF-8 bytes per ID. IDs preserve exact Unicode scalar values without
normalization. JSON duplicates, unknown fields, non-finite numbers, booleans in
integer fields, float coercion, malformed Unicode, invalid counts and unknown
recipe/version values fail **before any NetworkX import**. There are no fields
for executable expressions, query text, paths, backend selection or plugins;
identifiers are inert graph labels. These are finite protocol limits, not native
memory or processing-time guarantees.

Result encoding is capped at 128 KiB as JSON chunks are encoded, including
escaping expansion. A request within the input/node limits can still fail this
output limit. No partial path is returned to fit a quota. Error messages use only
fixed sanitized codes, not raw request contents, exception strings or local paths.
The module has no command-line entry point and does not print a success receipt.

## Referenced-file integration seam

`process_streams(source, destination)` is a bounded wrapper over **already
assigned binary streams**. It opens no path, owns neither stream, truncates no
file and does not close descriptors. It reads at most the input limit plus one
byte in chunks of at most 64 KiB, computes and serializes the complete result
before its first output write, handles short writes, and requires successful
flush before returning. I/O failures are failures even if partial bytes exist;
there is no retry of the operation and no fallback to another engine.

A later fixed bootstrap and native supervisor must own all stream/file lifecycle
policy: create fresh assigned input and scratch locations, open input read-only
and output exclusively without following links, verify code/runtime inventory,
use reviewed `-I -S -B` path setup, enforce process/time/memory/output limits,
propagate cancellation, confirm termination, inspect the output only after
quiescence, and clean or quarantine the owned assignment. An unconfirmed exit
must prevent dependent reads. Successful function return or an echoed identity
alone is insufficient for canonical publication. Rust must consume its private
capture and validate the bytes at the bound revision; any later persistence
requires the separately reviewed transactional publication lifecycle.

No `.pth` processing, console wrappers, prefix writes, extra grants or native
recipe changes are introduced here. Explicit algorithm dispatch does **not**
disable NetworkX's import-time entry-point discovery.

## NetworkX import-time discovery

In pinned NetworkX 3.6.1, `networkx/__init__.py` calls
`utils.backends._set_configs_from_environment()`. That function calls
`_get_backends("networkx.backend_info", load_and_call=True)`, which executes
`EntryPoint.load()()` for discovered info providers. The separate
`networkx.backends` discovery records entries without loading them, and removes
the built-in `nx_loopback` test entry. Therefore `backend='networkx'` fixes
algorithm dispatch but, by itself, cannot prevent import-time plugin code.

The source tests require the locked development environment to expose no
`networkx.backend_info` entry and only the built-in ignored `nx_loopback` backend
entry. The staged prefix is inspected as files and metadata only, after an
independent check against its complete manifest; its interpreter is not run.
This observation applies to those exact inspected bytes, not arbitrary future
site-packages contents or Python search paths.

Before native activation, the supervisor/bootstrap must enforce the complete
hash-bound prefix and assigned code, permit only the reviewed explicit standard
library/site/adapter paths under `-I -S -B`, and supply a controlled environment
without inherited `NETWORKX_*` settings. The pre-import runtime check must reject
unexpected backend-info/backend entry-point metadata; a version string or
post-import plugin check is too late to prevent its execution. This adapter
neither monkeypatches upstream discovery nor claims to provide that loader
boundary by itself.

## Cross-language and adversarial verification

`workers/python/fixtures/canonical-graph-cases.v1.json` contains three synthetic
requests generated by the real Rust capture code: forward path, reverse
undirected path and an isolated target. Their expected results were independently
accepted by Rust's owned-handle validator, including the two parallel assertions
on the shared hop. The source evidence's import timestamp is fixed inside the
synthetic fixture setup so the actual canonical-body fingerprint is repeatable.

The Rust regression rebuilds that canonical workspace and compares every request
field with the retained fixture. **Only its necessarily fresh nonce is normalized
for this equality test.** It then rebinds that nonce in the fixture's result and
runs the normal consumed-handle validator. Production code has no nonce override
or fixture-loading path. Python's real-engine tests must reproduce each retained
result exactly. The fixture has no private provenance or filesystem paths.

Stdlib-only source tests exercise validation before lazy import, duplicate/unknown
fields, exact scalar types and pins, Unicode/count/order bounds, core-backend
selection, isolated nodes, sanitized failures, escaping expansion, short I/O,
input overruns and partial-write/flush failure. Separate real-engine tests run
in the existing locked **development** environment, covering the Rust fixtures,
undirected ties/self-edges, Unicode identity, limit-sized graphs and complete
output refusal. They do not execute the staged candidate interpreter or establish
sandboxing, relocation, native deadlines, worker cancellation or release readiness.

The retained [static backend inspection](python-graph-backend-static.v1.json)
records 22 primary `entry_points.txt` files in the fully verified 11,320-file,
601,821,300-byte prefix. None declares `networkx.backend_info`; only NetworkX's
ignored `nx_loopback` entry declares `networkx.backends`. The two inspected
upstream source files and every inspected metadata file carry their exact
manifest-matched hashes. The embedded inventory-reader report repeats the
immutable installation manifest's original `assembled-unexecuted` state; it is
not a claim that later, separately documented compatibility campaigns never ran.
This static inspection itself executed no staged interpreter or package code.

Source checks for this increment:

- 13 focused stdlib contract/stream tests passed normally and with `python -O`.
- Full scripts discovery: 322 tests, 318 passed and 4 platform skips, both normally
  and with `python -O` on the local development Python 3.14.2.
- 10 actual NetworkX tests passed in the existing locked CPython 3.13.11 development
  virtual environment, including the exact metadata guard and Rust fixtures.
- 16 Rust graph tests passed, including the new fixture bridge; strict all-target
  core Clippy and formatting passed.

These checks do not close EW-17 or activate a Python worker. The next native
integration must retain the original source, process, runtime and captured-handle
identities and test cancellation/termination and stale-result refusal under the
reviewed supervisor before any canonical publication route is enabled.
