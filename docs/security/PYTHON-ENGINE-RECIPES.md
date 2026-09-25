# Fixed Python engine recipes — source-only

EW-07 / issue #11. These two test-only recipes have not executed the candidate
interpreter. They do not enable an application worker, establish reliable startup,
or pass a release gate. All earlier combined compatibility, hostile and relocation
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

## Reviewed execution seam (not executed)

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
package. Actual native observations remain pending separate source review.

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
