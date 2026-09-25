# Fixed application graph adapter — source foundation

This source slice separates an application-owned graph assignment from the
previous synthetic canonical graph campaign. It does **not** configure normal
application execution, attach a Python runtime, run a candidate, or close the
signed-helper/platform release gates. `GraphExecution::Unavailable` remains the
normal coordinator choice until scheduling and an explicit capability attachment
are reviewed together.

## Capability and authority

`engines::python_graph::VerifiedGraphRuntime` is crate-private, non-deserializable
and non-Clone. Its explicit `from_app_engines(root, cancellation)` constructor
derives only `root/python` and verifies the complete pinned inventory. No existing
`Runtime`, environment variable, PATH entry, developer prefix or system Python
implicitly makes it available. Unsupported platforms return `Blocked`; the only
candidate described by the pin is macOS ARM64 CPython 3.13.15 / NetworkX 3.6.1.

`execute(scratch_root, request_bytes, cancellation)` takes an existing private,
effective-user-owned, ordinary 0700 application scratch directory with no linked
ancestors. It creates one explicit 0700 assignment beneath that directory and
owns its cleanup. Neither directory nor bytes come from a generic frontend writer.
The intended coordinator retains the private `GraphAttempt` and passes its exact
captured bytes. This adapter never receives a workspace, writes canonical data,
or substitutes for the store's path/provenance/revision validator.

The fixed outer recipe is `python-graph-job-v1`; the existing inner
`shortest_connection_path_v1` bytes and algorithm are unchanged. Rust creates a
fresh canonical UUID for each launch and binds it to the captured UUID, exact
request byte count/hash, runtime pin, and fixed code assets. The strict receipt
also binds isolation flags, Python version, all 58 pinned distribution versions,
NetworkX backend metadata checks, and exact raw graph result count/hash. Duplicate
or unknown receipt fields fail. The returned graph bytes are never reserialized;
the store still rejects malformed/duplicate graph fields and unauthorized paths.

The application assignment contains six fixed files: `graph_worker.py`,
`runtime_support.py`, `graph_path.py`, the version-only JSON asset, assignment JSON,
and exact graph request. There is no campaign, expected synthetic answer, fixture
loader, recipe selector, plugin choice, path choice or arbitrary script argument.

## Shared runtime and supervision boundary

The private `engines/supervision/python.rs` extracts the inventory, profile,
staging, assigned-file checks and command setup used by the historical probes.
It calls the existing process configuration, process-group wait/stop/reap,
output-tree budget and cleanup functions. The existing development `sandbox-exec`
profile is unchanged: default deny, no fork or network allowance, prefix/system
library reads and executable mappings, assigned code/input reads and scratch-only
writes. Fixed `-I -S -B`, cleared environment with the previously required HOME,
scratch TMPDIR, single-thread environment settings and inherited-handle closure
remain in effect. No OS security configuration is changed.

`runtime_support.py` centralizes strict JSON, initial path checks, distribution
verification and the pre-import NetworkX backend guard. The latter requires no
backend-info entry and exactly the pinned `nx_loopback` metadata entry; it never
calls `EntryPoint.load`. The bootstrap loads only its host-staged sibling helper
under isolated Python before adding the exact verified site/code paths. The app
worker then validates the graph request before importing NetworkX. The original
`graph_path.py` remains the one algorithm adapter.

The full manifest is still
`4dc6fd171e842d1f9254be7fc5cb16e2e01203896403dcd9839a8aec69dad822`:
11,320 files, 601,821,300 bytes in the previously assembled prefix. This is not a
new smaller NetworkX distribution or a new packaging claim. No packages or Cargo
dependencies were added. Existing third-party notices and the complete pinned
runtime remain necessary for a future packaged attachment.

Inventory reads use `O_NOFOLLOW | O_NONBLOCK`, regular single-link checks and the
existing immutable-file metadata identity helper before/open/after/final-named
checks. Prefix file modes, exact listed set, hashes and byte counts are checked;
each open/read uses a bounded 64 KiB buffer and observes cancellation. Assigned
writes use create-new 0600 files, bounded chunks and sync. These checks detect
replacement during verification; they do not create an OS boundary against an
equivalent-user process racing a later interpreter open.

## Bounds and outcomes

| Item | Bound |
| --- | --- |
| Application request | 1 MiB |
| Historical canonical probe request | **64 KiB, unchanged** |
| Raw graph result | 128 KiB |
| Application receipt / each code or metadata asset | 64 KiB |
| Manifest | 16 MiB |
| Inventory | 20,000 file ceiling, 40,000 entries, depth 16; exact pin has 11,320 files |
| Runtime asset / total inventory | 80 MiB / 768 MiB |
| Supervised child wall / CPU | 30 seconds / 30 seconds |
| Existing output tree | 512 entries, depth 2, 64 MiB per file, 128 MiB total |

The 30-second wall limit starts with the supervised process. Initial configuration,
preparation and pre/post inventory hashing are outside it and are cooperatively
cancellable; this is not a preparation-inclusive latency guarantee or a hard RSS
limit. A configured capability is reverified on every execution, so configuration
does not cache permission to trust changed runtime files.

Success requires the complete pre-inventory, confirmed process-group termination,
post-tree/assigned-file checks, complete post-inventory, exact receipt acceptance
and successful cleanup. Cancellation is `Interrupted`, distinct from unavailable
runtime `Blocked`, without matching error text. The legacy shared wait wrapper
keeps its previous cancellation behavior; only the app caller selects the new
typed disposition. A failed or cancelled wait skips output/post-inventory reads
and makes **no verified-after claim**. Confirmed nonzero child exits also cannot
return graph bytes.

`TerminationUnverified` takes precedence and retains the assignment without
output, post-prefix or cleanup access. Confirmed-outcome cleanup failure overrides
success, cancellation or another error. No publication retry invokes the adapter
again: the intended exclusive coordinator retains the returned result and owned
attempt until its terminal write commits. This source foundation does not enable
that coordinator path.

## Historical evidence and verification boundary

Historical native receipts and evidence documents are byte-unchanged. They prove
their recorded source, runtime and campaign, not this extraction. Current probe
collectors include the new shared Rust/Python sources and require the added fixed
helper asset; prior recipe names, top-level receipt schemas, fixture expectations,
limits and hostile controls remain intact. Current source collectors deliberately
do not reinterpret an older receipt as evidence for the new code.

Source regressions cover application request sizes above 64 KiB versus the legacy
bound, exact raw byte bindings, altered/duplicate/unknown wrapper fields, missing
runtime, metadata-before-import, no-clobber output, oversized output, private
scratch admission, mid-hash cancellation, same-content named-file replacement,
FIFO substitution, hard links, assigned-file tampering, and cancellation/unknown
termination cleanup precedence. Source-only Python tests use standard-library
stubs, not the candidate interpreter or NetworkX. The first affected Rust run's
new scratch test failed because its temporary directory relied on default modes;
the retained failure is not hidden. The fixture and new app assignment now request
0700 explicitly.

The handoff checks passed 600 ordinary Rust tests (521 library and 79 integration;
31 explicit/native tests ignored), strict debug and release all-target Clippy,
and the final affected-engine subset (61 pass, 23 explicit/native ignored).
The Python suite passed 341 tests with 4 skips under both normal and optimized
execution. All 102 historical schema-directory files match the base bytes.
A local Windows GNU Clippy attempt stopped in native dependency preparation
because `x86_64-w64-mingw32-gcc` was unavailable; this is retained as an environment
failure, not Windows verification. Hosted platform verification is still required.

Before activation, a separately authorized finite native proof must bind the
exact signed source/executable/configuration/runtime and assignment identities,
run the fixed app entry on an isolated app-owned runtime copy, include a request
above 64 KiB, verify ordinary/unreachable result acceptance and confirmed
cancellation, check pre/post inventories and cleanup, then demonstrate durable
publication through the exclusive scheduler. Previous canonical probe success is
not substituted for this proof. Intel Mac and Windows runtime/confinement,
minimum-OS packaging, complete notice review, signed helper, supervisor-crash
containment, and native app activation remain separate unresolved gates.
