# Fixed Python engine recipes

EW-07 / issue #11. One reviewed native invocation of each test-only recipe passed
at signed source `46e6c1e`, using separate fresh verified relocated prefixes. These
observations do not enable an application worker, establish reliable startup, or
pass a release gate. All earlier combined compatibility, hostile and relocation
observations remain unchanged, including the
[relocated diagnostic timeout](../../packaging/evidence/python-relocation-diagnostic-first-native-2026-09-26.json).

## Recipe and result contracts

| Fixed recipe identity | Explicit imports before initialization-ready | Exact existing assertions |
| --- | --- | --- |
| `python-networkx-v1` | `networkx` | Accepted-edge path `a,b,c`, source assertions `r1,r2`, rejected shortcut excluded, unreachable result true; explicit `backend='networkx'` |
| `python-transactions-v1` | `duckdb`, `pyarrow`, `pyarrow.compute`, `pyarrow.parquet`, the reviewed `transaction_totals` adapter | Original per-currency exact decimal totals and contributing IDs under Decimal precision 2, and explicit rejection of an unverified accepted transfer pointer |

Each fresh process verifies the same **58 distribution versions** from the
compiled fixture before importing its operation modules. The runtime remains the
exact full prefix with manifest
`4dc6fd171e842d1f9254be7fc5cb16e2e01203896403dcd9839a8aec69dad822`.
Version metadata for spaCy/Splink does not claim that their code imported or their
features worked. The package list, bytes, notices, `.pth` files and installed
RECORDs are unchanged.

Both operations call the same fixed helper functions used by the combined
compatibility probe. Only their extraction into callable functions changed; the
combined import order, result keys, fixtures and expected results are preserved.
The new worker compares its selected result against the corresponding fields of
the original expected fixture before recording operation-after or writing a
result. Rust then parses a bounded typed result with unknown/duplicate fields
rejected and independently compares it with the compiled original expectation.
The Python parent independently checks the exact result, assigned source hashes,
recipe, job, campaign nonce, interpreter, manifest and termination status. A
NetworkX result cannot satisfy a transaction or combined recipe, or vice versa.

The bootstrap accepts only the three literal identities (the existing combined
identity and the two new identities); absent selection retains the combined
recipe. The Rust test functions choose the identity through a closed enum.
There are no arbitrary module names, entry points, commands, SQL or external
paths in this selection. NetworkX is not assigned the transaction adapter. The
separate recipes receive the compiled expected fixture and engine module; they
are not assigned the combined import observer or plugin reader fixture.

## Shared confinement, lifecycle and timing

The new recipes use the **same** existing `python_probe::run` implementation for
inventory verification, assigned files, profile generation, environment,
spawn/process group, deadline, reaping, output bounds, source rechecking and
explicit cleanup. Only the closed compiled asset/result/diagnostic selection
varies. There is no second launcher or unsandboxed fallback. Production supervisor
code and all application commands remain unchanged.

The command remains the app-local interpreter with `-I -S -B`; only verified
stdlib, the exact app-local site-packages and assigned reviewed adapter paths are
added. No `.pth` execution, console wrapper, bytecode generation, prewarming,
package modification or internet access is added. HOME is passed through unchanged
as required by the existing sandbox launcher; no HOME content grant is added.
The prefix is read-only and writes remain assigned scratch only.

Limits remain **30 seconds for the whole worker**, 120 seconds for the outer
native test and 300 seconds for compilation. Existing CPU, file/tree, descriptor
and environment limits also remain unchanged. Compilation uses the same release
build selection as the isolation runner. No hard RSS bound or independent
startup allowance is claimed. Bounded stderr/stdout retention remains a
post-reap acceptance/read bound, not a new live 128 KiB quota.

Each new recipe emits exactly four potential timed records, in this order:

1. `initialization/before` before all-version verification and operation imports.
2. `initialization/ready` after explicit operation imports, module-origin checks,
   and transaction thread-count setup where applicable.
3. `operation/before` immediately before the existing operation helper.
4. `operation/after` only after the original exact assertions agree.

Initialization-ready means these explicit imports/setup completed. A native
library can defer internal initialization or loading until its first operation;
that work belongs to the operation interval and the same whole-worker deadline.
There is no implication that every internal lazy import has completed. Both
monotonic elapsed and process CPU clocks start at the diagnostic object's
construction, after bootstrap path validation and loading the reviewed stdlib
helper module, not at parent spawn. Paired durations can be derived only when
both matching records exist. Missing after/ready records leave completion unknown.

The records have closed stage/boundary enums and integer clocks; each is at most
512 bytes, four records / 2 KiB total. The readers reject gaps, extra timing
filenames, links, hardlinks, duplicate/unknown fields, invalid/regressing clocks
and oversized/partial data. Validated prefixes survive a malformed later record.
No import observer, new thread or background work is introduced. The existing
combined receipt keeps its exact import-diagnostic shape; only the new identities
admit `engine_diagnostics`. Historical source-bound receipts are not rewritten.

A fresh failed initial observation is saved before preparation/spawn. Native
receipts bind fresh job/campaign identifiers, source/build binary, actual profile,
assigned sources and candidate interpreter. A primary quota, termination or
cleanup error takes precedence over diagnostic failure. After confirmed native
termination the runner independently verifies the prefix; unknown termination
prevents post-verification, scratch reads and automatic retry. The shared runner
also now treats a retained `unconfirmed` receipt after native test exit as
unverified, rather than using it to authorize post-prefix reads.

## Budget basis and unresolved startup

The latest combined observation measured only DuckDB's completed direct import
at 736 ms elapsed / 24 ms process CPU and NetworkX at 307 / 306 ms. These are
single observations within an already running process, not isolated cold-start
measurements. The retained spaCy before-only checkpoint and later attempt records
cannot establish a completed spaCy duration. The language-to-CLI attempt interval
was 21,035 ms elapsed but 339 ms process CPU; CPU-bound compilation is not an
established explanation for that whole interval. Native loading, filesystem
waits, scheduling and other causes remain unresolved.

The evidence supports testing independent practical operations under the existing
30-second budget. It does **not** justify a larger budget or a smaller guaranteed
startup SLA. Separating the two completed imports removes about 1,043 ms of
observed preceding import time from a prospective mention recipe, which does not
prove that spaCy will then finish. No spaCy or Splink separate recipe is added in
this increment. Required plugin checks remain part of the unresolved combined
compatibility campaign. Fresh process or relocated-prefix execution also does not
prove a cold OS cache; no cache clearing or performance benchmark is implied.

## Reviewed execution seam

A future separately reviewed invocation uses the existing runner with one of
`--case networkx` or `--case transactions`, the explicit immutable prefix,
a fresh ignored artifact child, and `--execute-reviewed-probe`. Each invocation
selects one exact ignored Rust test and creates one process. There is no all-case
loop or automatic rerun. Both tests are ignored in ordinary source CI:

- `engines::supervision::python_probe::engine_recipes::native_python_networkx`
- `engines::supervision::python_probe::engine_recipes::native_python_transactions`

Ordinary tests use only trusted host stdlib, synthetic JSON/files and mocked
module imports. Rust tests exercise typed exact results, cross-recipe rejection,
version duplicates, source role grants and malformed/partial timing reads. Python
checks cover all-version preflight, fixed imports, failed import/assertion
checkpoint retention, clock/order/no-clobber handling, source/nonce/result binding,
closed historical schemas, and confirmed versus unknown termination with no
retry. No test executes the staged interpreter or an installed third-party
package. The separately reviewed native observations follow below.

Source verification at this handoff: the ordinary Rust probe selection passed
17 tests, with all five native/measurement tests ignored; strict core Clippy
across all targets and Rust formatting passed. Full script discovery passed on
the trusted host Python 3.13.11 and 3.14.2, both normally and with `-O`: 309 tests
per run, four explicit platform/input skips. The new single-engine test module
contains ten focused tests. A read-only AST comparison against the base source
also confirmed the extracted graph and transaction operation bodies are unchanged
apart from replacing their module aliases with explicit function parameters and
returning their existing result fields. The fixtures, expected JSON, production
supervisor and historical evidence files are byte-identical to the base.


## First native observations — two finite invocations

Exactly one `networkx` invocation ran, followed by exactly one `transactions`
invocation **only after** the first worker's confirmed termination/cleanup,
source/binary identity checks and independent original/moved-prefix verification.
Both used clean signed commit `46e6c1e23d312f8e5e646441b3654635abb51b20`, tree
`9f1907f17a9f84a2c8cd93cc09eeb90111d253c3`, on macOS 26.6.2 arm64. Neither had a
retry, candidate preflight/import warm-up, changed package/profile or increased
budget. The host was reserved against other agents' heavy builds/native campaigns;
that condition does not establish any cause for the measured durations.

Before each invocation, the existing trusted relocation helper copied the exact
prefix into a fresh no-clobber artifact parent. That helper executed no candidate
code. Each runner received the new moved prefix and a separate fresh campaign
child. Therefore, the runner's `original_inventory` and `selected`/post-inventory
fields **both describe that moved prefix**. Separate preparation and integrity
receipts explicitly verify the true original prefix before and after each
campaign. Both original and both moved inventories remain unchanged at the pinned
manifest: 11,320 ordinary files / 601,821,300 bytes / 58 installed RECORDs.
No job directory remains. The moved prefixes and all private build/test logs are
retained. The inventory's assembly-state `package_code_executed: false` describes
the earlier installer operation, not these explicitly recorded native campaigns.

| Observation | NetworkX | DuckDB + PyArrow |
| --- | ---: | ---: |
| Exact recipe assertions | Passed | Passed |
| All 58 metadata versions | Matched | Matched |
| Trusted preparation | 3,417 ms | 2,603 ms |
| Initialization pair: elapsed / CPU | 401 / 387 ms | 6,340 / 400 ms |
| Operation pair: elapsed / CPU | 1 / 1 ms | 13,173 / 981 ms |
| Spawn through confirmed reaping | 1,192 ms | 20,272 ms |
| Full native test, including post-hashing | 6.90 s | 25.66 s |
| Exit status | 0 | 0 |
| stdout / stderr | 0 / 0 bytes | 0 / 0 bytes |
| Termination / cleanup | Confirmed | Confirmed |

The timings are arithmetic differences of the four retained records, rounded to
milliseconds. They are not measurements of a cold OS cache, p95 performance,
reliable startup, or 100,000-row capacity. Trusted prefix verification is separately
recorded and can populate filesystem caches. The transaction operation includes
its original synthetic Parquet writes, exact-total and negative-transfer checks,
and any deferred internal work; its wall/CPU difference does not identify a cause.

The NetworkX result was exactly `a,b,c`, assertion IDs `r1,r2`, with unreachable
true. Transactions returned AUD `1.123456788` for `aud-a,aud-b,aud-c` and USD
`5.00000000` for `usd-a,usd-b`; the unverified accepted transfer pointer was
rejected. Both passed the complete selected original fixture assertions, Rust's
typed comparison and the independent parent comparison. No spaCy, mention/plugin,
Splink matching or combined compatibility success is inferred.

Both used native test binary SHA-256
`886fada938740eb467079fec7d26cafac0ad31277ee1f502006148a9e2f48bac`.
Its source and bytes were checked before and after each invocation. The first
release build reported 3m 19s; the second reported 0.47s with the same already-built
binary. Reusing this trusted Rust binary is not a candidate warm-up. Actual
expanded profile hashes differ because job/prefix paths differ; both profiles
were generated by the unchanged reviewed profile function.

| Identity | NetworkX | Transactions |
| --- | --- | --- |
| Campaign | `f5588eac-2a62-48e9-bced-483db5d57614` | `4f92e301-49ec-462a-a54e-a2f47ea75ca2` |
| Job | `09ab2453-26ad-4572-b92d-e1cee03ad15e` | `1badd75b-2dcc-4f88-a2c4-b6fba73319c0` |
| Expanded profile SHA-256 | `7b07d064f3557940f673eac88b11eefea4b52df435fdff1f3965645af5630a0c` | `4baf4c1ee749f7f83d5db3c7086c55b33c579d0a54e9dbf199a950b9f12d1843` |
| Outer receipt SHA-256 | `83bf13dd7e5d2b4cea40ae8eee340c048dc16556d75f158859e57af71ba9f97e` | `e8e62b5635589f8efbb68d4fb60bbc478a05fb8bbee47dec67ab3f029d763bdc` |
| Native receipt SHA-256 | `0d124e55b98345454f1d8fe1490a0fdd65d53fc9f8dc7957905d2bd6efb8b73b` | `b6ebbe6c5c933b82bb40856e88ab9daae51c9b23dabbd066b723c351017b3efa` |

Retained byte-identical records:

- NetworkX: [outer](../../packaging/evidence/python-networkx-first-2026-09-26.json),
  [native](../../packaging/evidence/python-networkx-first-native-2026-09-26.json),
  [preparation](../../packaging/evidence/python-networkx-first-preparation-2026-09-26.json),
  [independent integrity/log hashes](../../packaging/evidence/python-networkx-first-integrity-2026-09-26.json).
- Transactions: [outer](../../packaging/evidence/python-transactions-first-2026-09-26.json),
  [native](../../packaging/evidence/python-transactions-first-native-2026-09-26.json),
  [preparation](../../packaging/evidence/python-transactions-first-preparation-2026-09-26.json),
  [independent integrity/log hashes](../../packaging/evidence/python-transactions-first-integrity-2026-09-26.json).

These finite successes support work on a canonical Rust-selected snapshot and
validated result protocol. They leave analytical job integration, canonical
publication, wider networking coverage, hard RSS enforcement, crash recovery,
clean installation/platform/signing checks and the earlier spaCy startup failure
unresolved. No release gate changes follow from this evidence.
