# Python compatibility probe — development source contract

EW-07 / issue #11. This is a **test-only, development** macOS arm64
compatibility harness for the exact offline assembled prefix. It introduces no
application command, canonical protocol, automatic dependency installation or
production Python worker. The ignored Rust test requires a deliberately invoked
runner and a fresh artifact directory. Ordinary test discovery never executes the
candidate interpreter or imports its third-party packages.

## Reviewed assignment and permissions

The fixed recipe is `python-compatibility-v1`, pinned to installed manifest
`4dc6fd171e842d1f9254be7fc5cb16e2e01203896403dcd9839a8aec69dad822`:
CPython 3.13.15 and the 58 reviewed wheels, 11,320 ordinary files and 601,821,300
bytes. The runner independently verifies the complete installed manifest and
RECORDs; the Rust launcher also checks the exact manifest, complete tree,
single-link ordinary assets, modes, bounded sizes and every SHA-256 before launch
and after successful processing. No concurrent same-user modification of the
reviewed build tree is assumed. This is a development test trust assumption, not
protection against an administrator replacing the supervisor or runtime.

The launch/profile and six assertions follow
[the reviewed proposal](../../packaging/PYTHON-NATIVE-PROBE-PROPOSAL.md).
The `#[cfg(test)]` module is compiled only under the existing macOS supervisor.
Only its fixed copied bootstrap, compatibility adapter and transaction adapter
are executable Python inputs. The worker receives three immutable JSON
inputs including its fresh assignment; the parent's expected-results fixture is
not copied into the worker assignment. There are six assigned files in total:
three code files, fixture JSON, reader JSON and assignment JSON.

The command is `/usr/bin/sandbox-exec -f <job>/worker.sb
<prefix>/install/bin/python3.13 -I -S -B <job>/code/bootstrap.py`. The profile
allows prefix and system loader reads, assigned code/input/scratch reads and
scratch-only writes; it has no network grant and denies fork. Actual `HOME` is
passed unchanged to the launcher without granting access to its contents.
`TMPDIR` is assigned scratch; `OMP_THREAD_LIMIT`, `OMP_NUM_THREADS` and
`OPENBLAS_NUM_THREADS` are fixed at one. No Python/DYLD/user-credential environment
is passed. The native process configuration closes inherited descriptors and
uses the existing process-group termination/reaping and explicit cleanup paths.

The system-private `dyld-support.sb` was read as data during this source review.
Its SHA-256 was
`06215a5d32689aefe395c29710e182eb54ba22162f50df8b4842290f8a19bf1c`.
It grants system Cryptex loader reads/maps, ancestor reads, selected loader
syscalls/fcntl operations and a root-directory read used for `openat`. These
system grants are part of the effective profile. Apple's file explicitly warns
that its private interface can change. Review its actual contents for the
native host; a profile source digest does not prove hostile-access denial.

## Assertions and inspected package APIs

The result must exactly match the parent's compiled expected JSON, including all
58 versions, imported module names, Unicode character offsets with pending
review state, accepted graph path/assertion IDs, missing path, exact transaction
totals and source IDs, review exclusions, transfer rejection and selected
registry results. Boolean/integer substitution, unknown fields, duplicate
fields, wrong manifest/job identity or oversized results fail. All eight phase
checkpoints must reach `complete` before acceptance.

The pinned APIs were inspected as source bytes, not executed during authoring:

| Site-packages path | Version | SHA-256 | Reviewed use |
| --- | --- | --- | --- |
| `spacy/util.py` | 3.8.11 | `da1338a9da8e21646b3844c42e3c383425911de4146f6314e39de93860d359cc` | `registry.get` resolves selected architecture, layer and reader groups. |
| `catalogue/__init__.py` | 2.0.10 | `0eb8dd35905d1af2717e1f99a6cb5883862b86b6c2fb3d2de9f480fb90c9376c` | Registry resolution loads an exact named metadata entry point. |
| `srsly/_json_api.py` | 2.5.3 | `88272f1e7ad3673f372d281151495d66c3e3b56c384c18daddef3fa50844747b` | `read_json` reads the assigned synthetic reader fixture. |
| `spacy_legacy/architectures/tok2vec.py` | 3.0.12 | `b8c9a26183b82d11849af8ea38267dbbe27808c53c1654a99a1fa0d6a98cc52b` | Resolve `Tok2Vec_v1`; do not invoke the model factory. |
| `spacy_legacy/layers/staticvectors_v1.py` | 3.0.12 | `d87d9663e3ef2f74056aa1f43b1a410c3b997ea3221f16650bf88471b9fccc26` | Resolve `StaticVectors_v1`; do not invoke the layer factory. |
| `networkx/utils/backends.py` | 3.6.1 | `2035cd26305639f72a3e6dc09f2e7e91fc14f616531f7dd1d7a82bdef67ac271` | Select `backend='networkx'`; no optional `nx_loopback` test backend. |

`spacy.blank('en')` plus PhraseMatcher requires no downloaded language model.
Six relevant metadata groups are compared case-sensitively; optional logging,
training, GPU, pytest and console entry points are not executed or claimed.
Splink is imported/version-checked only. Parquet uses fixed string columns,
no compression and at most 16 rows; PyArrow's CPU/I/O thread counts are one.
The reviewed exact-decimal transaction function runs with Decimal context
precision two to exercise its independence from ambient decimal rounding.

## Limits, identities and failure handling

The existing supervisor enforces 30 seconds wall time, 30 CPU seconds,
256 descriptors, disabled core dumps and a 64 MiB per-file resource limit.
It monitors the complete assigned job tree every 20 ms against 128 MiB,
512 entries and its existing depth bound. The read-only prefix is separately
inventoried and is not charged as writable scratch. There is no claimed hard
resident-memory ceiling.

Code/JSON inputs are capped at 64 KiB each (transaction adapter 32 KiB), Parquet
at 1 MiB and the accepted structured result at 1 MiB. **128 KiB per diagnostic
stream is a post-reap acceptance/read bound, not an independently enforced live
write cap.** During execution the existing file/job limits apply. Both streams
must satisfy the smaller read bound to pass. Compiler/host-test logs are file
backed with a 32 MiB post-return acceptance bound. The runner's 300-second build
and 120-second native-test timeouts do not extend the worker's 30-second limit.

Before preparation, the native receipt records a fresh job UUID, the runner's
fresh campaign UUID and an unsuccessful outcome. Before spawn it records:

- fixed relative interpreter name, verified size and SHA-256;
- SHA-256 of the actual generated profile with assigned paths;
- all six assigned relative names, sizes and SHA-256 values;
- exact pinned runtime-manifest identity and native architecture;
- phase and termination state, initially `unconfirmed` at spawn.

The outer receipt adds actual macOS product version/architecture, source
commit/tree/parents, source-file hashes and the compiled test-binary hash.
Nonignored untracked files make the source preflight fail. Native acceptance
requires the exact campaign UUID, the same job UUID in the result, all fixed
input/code digests, expected interpreter identity and confirmed termination.
Both receipts begin unsuccessful; a retained earlier success cannot satisfy a
new campaign. On failure, validated identities and fixed phase/category fields
remain available without arbitrary paths, exception text or user data.

Worker diagnostics/results are read only after confirmed termination. Cleanup
failure remains a failure. Unverified termination retains assignment recovery
state and does not read worker scratch. An outer test-runner timeout is also
unverified: the runner may have killed the Rust test while its separately grouped
child remained alive. It does not retry, delete assignments or claim the child
was reaped; independent inspection/recovery is required. The runner may read the
bounded parent-owned native receipt for already recorded identities in that case.
Raw stdout/stderr/build/test logs remain private local artifacts, never copied
wholesale into the sanitized receipt.

## Source validation and later execution

Safe source checks are focused ordinary Rust tests/strict Clippy plus
`python -m unittest discover -s scripts/tests -p test_python_probe_contracts.py`.
The tests use synthetic bytes and mocked launch/build results; they cover
injected search paths, duplicate/oversized JSON, immutable output, metadata
ambiguity, receipt bindings, dirty source, failure-before-launch, timeouts and
artifact-directory traversal. They do not constitute native compatibility proof.

After the final source/profile/fixture review, the explicit native invocation is
`python3 scripts/test_python_compatibility.py --prefix <exact-reviewed-prefix>
--artifacts <fresh-private-artifacts> --execute-reviewed-probe`. The script's
flag is an execution guard, not permission by itself. The initial source handoff was unexecuted. The first subsequent native campaign
failed as recorded below. Any future observation must retain its
exact source checkout, binary, profile, runtime, fixture identities and negative
outcomes. A failed import does not authorize broader sandbox permissions or
larger budgets.

Hostile file/network controls with positive controls, fresh relocation, hard RSS
limits, supervisor-crash recovery, canonical protocol activation, clean offline
installation, signing, supported OS minima, Intel Mac/Windows and complete
third-party notices remain separate unmet checks. Completing compatibility
assertions inside this profile alone cannot establish those claims.

## First native observation — 2026-09-25

The first campaign at signed source `fdf7265398535593f3001e60cc15ca125cb8bd29`
**failed** after reaching `imports`, with `quota-exhausted` and confirmed worker
termination/cleanup. Its exact [outer receipt](../../packaging/evidence/python-compatibility-first-2026-09-25.json)
and [native receipt](../../packaging/evidence/python-compatibility-first-native-2026-09-25.json)
are retained verbatim. A preceding missing ignored artifact-parent preparation
failed before campaign creation/build/spawn; after that ordinary precondition was
repaired, exactly one actual candidate campaign ran. There was no native retry.

The actual host was macOS 26.6.2 arm64. Both diagnostic streams were empty. The
test log records 68.90 seconds for the entire test, including prefix verification,
preparation and cleanup; worker-only elapsed and the particular quota category
were not captured. The `imports` checkpoint follows successful bootstrap
flags/path checks and all 58 version comparisons, but does not identify any
completed individual import. The PhraseMatcher, graph, transaction and plugin
assertions were not reached, and no structured compatibility result was accepted.
Other development Clippy/render work ran concurrently; its effect is unmeasured
and is not established as the cause. No limit or permission was increased.

Independent post-failure verification found the exact prefix unchanged: 11,320
files, 601,821,300 bytes and 58 installed RECORDs; see the retained
[integrity receipt](../../packaging/evidence/python-compatibility-first-integrity-2026-09-25.json).
The inventory's `assembled-unexecuted` / `package_code_executed: false` fields
describe the original assembly/verifier operation, not the later campaign's
history. The failed native execution is recorded separately and does not mutate
that immutable installed manifest. Raw build/native logs remain private local
artifacts. No job directories remained after confirmed cleanup.
