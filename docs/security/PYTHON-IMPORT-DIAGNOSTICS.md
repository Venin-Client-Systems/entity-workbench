# Bounded Python import diagnostics — source-only increment

EW-07 / issue #11. This changes only the fixed test harness. The candidate
interpreter has **not** executed with this diagnostic revision. It does not
activate a canonical worker, establish reliable startup, or change a release gate.
The original runtime, moved runtime and all prior receipts remain unchanged.

The retained [relocation failure](../../packaging/evidence/python-relocation-first-native-2026-09-26.json)
exhausted the **whole worker's 30-second wall limit**, with a final
`spacy:before` checkpoint and 30,044 ms from spawn through reaping. That interval
also includes bootstrap, version inspection, DuckDB and NetworkX. The old
checkpoints did not time those stages. It is not a measured spaCy import duration.
The [earlier original-prefix success](../../packaging/evidence/python-compatibility-second-native-2026-09-25.json)
took 22,844 ms supervised. These observations do not establish why they differed.
Concurrent host work, cold filesystem/native loading, source compilation and
particular transitive imports remain hypotheses, not causes established by a
checkpoint or static source inspection.

## Fixed observations and cost bounds

The six direct imports and all compatibility assertions are unchanged:
`duckdb`, `networkx`, `spacy`, `click`, `splink`, `pyarrow`. The twelve existing
before/after files now also contain `elapsed_ms` and `process_cpu_ms` integers.
Both clocks start when the diagnostic object is created in the fixed bootstrap,
after isolated path validation and before version inspection. They are not
absolute timestamps or the parent's spawn clock. CPU time is process-wide;
neither wall/CPU ratios nor individual samples diagnose the cause of a wait.
Values are rounded down to milliseconds, so equal consecutive values are valid.
Per-import durations may be derived only from a matched direct before/after
pair. The final before-only pair in a failed run has no completed duration.

A Python audit observer is installed immediately before the direct-import loop
and disabled before the mention fixture. It records only the first observed
`import` attempt for each of sixteen literal module names:

```text
numpy                          numpy._core._multiarray_umath
catalogue                      confection
thinc                          thinc.compat
thinc.backends.numpy_ops       blis
blis.cy                        srsly
pydantic_core                  pydantic_core._pydantic_core
spacy.pipeline                 spacy.language
spacy.cli                      weasel
```

Each record contains only that enum value, a sequential ordinal, and the two
clocks. It never copies import filenames, argument tuples, exception strings,
frame data, environment values or arbitrary names. Before membership lookup it
rejects non-string and over-64-character names. Unselected events, duplicates,
inactive callbacks and reentrant callbacks do no clock reads or output I/O.
The observer stops recording after sixteen selected names. It does not replace
an import resolver, preload packages, change import order, add a thread/timer,
enumerate stacks or invoke package entry points.

An attempt record is **not an import-completion record**. Cached modules, imports
which do not emit a selected event, disabled/reentrant callbacks, an interrupted
write or a diagnostic failure may leave no record. Absence stays unknown. A last
attempt does not prove that module is still running or that it caused a stall.
Only the direct loop has paired before/after observations.

The twelve top-level records remain at most 512 bytes each. The new attempts
have a separate maximum of **16 × 512 bytes = 8 KiB**. Both streams use fresh,
no-clobber fixed filenames. The Rust reader checks every ordinary single-link
file after confirmed reaping, validates contiguous order, known names, exact
fields, nondecreasing clocks and record counts, and rejects symlinked ancestors,
extra diagnostic filenames, duplicate fields and partial/oversized records.
The 120,000 ms numeric acceptance ceiling is not an execution allowance:
the worker still has its existing 30-second deadline.

The audit callback catches its own observation errors so it cannot replace an
exception raised by the actual package import. It then disables itself and marks
the diagnostic state failed; the fixed bootstrap cannot publish success if the
observer failed. If the worker is killed before that check, missing records are
still unknown. Rust retains each independently validated contiguous prefix and
marks malformed streams invalid. A pre-existing timeout/termination/cleanup error
keeps precedence. Successful compatibility still requires all twelve valid
top-level records and the original exact result assertions; it does not require
sixteen attempt events. Invalid or absent observations cannot create a pass.

There is bounded observer overhead: callbacks filter all audit events while
active, and selected events add at most sixteen clock pairs and small file
writes. That overhead is included in the same worker deadline. Its native cost
has not been measured. These are diagnostic observations, not performance
benchmarks. The sandbox profile, environment, CPU/wall/file/tree/descriptor
limits, 120-second outer timeout and 300-second build timeout are unchanged.
The hostile recipe has no import observer and leaves `import_diagnostics` null.
Historical receipts use their historical source-bound shape and are not rewritten
or passed off as newly collected data.

## Pinned source basis and later packaging questions

The following primary source bytes were read from the already verified prefix,
manifest `4dc6fd171e842d1f9254be7fc5cb16e2e01203896403dcd9839a8aec69dad822`.
No package was imported during this source inspection.

| Retained source relative to site-packages | Relevant startup behavior | SHA-256 |
| --- | --- | --- |
| `spacy/__init__.py` | Imports errors, Thinc API, pipeline, language and CLI | `e62742bbacbc16277b0ad780755061bef8385d15b5706c4aa0153227789a19b0` |
| `spacy/errors.py` | Imports compat before warning setup | `cf1082df1cd443d77f45251afdaeae97b8ab62eb37c4873dddb32f9c1453076f` |
| `spacy/compat.py` | Imports Thinc utility/API | `f23cf60d6a355e0e59dc1a301ef530d61a8eef003bc6b2ecce0e93190f03fd70` |
| `thinc/__init__.py` | Imports NumPy and config | `53dafbd25814afe3cf277cc29ae43699833299a5b3671ecadc480a27af170121` |
| `thinc/config.py` | Imports catalogue and confection | `dd12e8393884cd0f700d69f234ac38949c18cb04db708c0462d5552c12977015` |
| `catalogue/__init__.py` | Enumerates entry-point metadata at import | `0eb8dd35905d1af2717e1f99a6cb5883862b86b6c2fb3d2de9f480fb90c9376c` |
| `thinc/backends/__init__.py` | Imports compatibility and native NumPy operations | `7d36cc58e4e72e99f3fc05b10cb068da14f57c544fda8e3637203d853e5bb23f` |
| `thinc/backends/numpy_ops.pyx` | Declares BLIS native dependencies | `68abb578f79575527d785091e860a78cb8e68be8d84dea4e3f48e5af95bb9dc1` |
| `blis/__init__.py` | Imports and calls native initialization | `34e67b6a5e6454be5daddc6112103e0ed1eef3ac5cc93f48d54b7329536fd621` |
| `numpy/_core/multiarray.py` | Imports merged native extension | `afd14181b927aa10800a0c5dab5f456f52e219726847dbef379e313419029d49` |
| `pydantic_core/__init__.py` | Imports native validation extension | `9cad6292b75254af606a970aae9bff6e5ae9f0b08089cd63acafa391b60794d2` |
| `spacy/util.py` | Imports srsly and registry setup | `da1338a9da8e21646b3844c42e3c383425911de4146f6314e39de93860d359cc` |
| `weasel/__init__.py` | Imports CLI entry point | `4cdc57b133e06f9d8e85a90875af704d187dd5caa806a810471e53c63cd04055` |

The retained CPython `install/lib/python3.13/importlib/_bootstrap_external.py`
has SHA-256 `9623292c307de38721ab92efcb2ba44bbb663f9140edd585aaf29179841f16a8`.
Its `SourceLoader.get_code` reads/validates available bytecode, otherwise compiles
source, and avoids cache writes when `sys.dont_write_bytecode` is true. Static
inspection found zero `.pyc` files in this installed site-packages tree. This
does not show how much source compilation contributed to any observed run.

Later work may evaluate checked-hash bytecode generated at build time with
explicit source/provenance/relocation verification. Nothing here compiles,
installs or modifies bytecode in either prefix. A separately reviewed
single-engine startup recipe is also needed to distinguish practical worker
startup from this deliberately combined all-engine compatibility campaign.
Neither future option can erase the existing failed combined campaign.

## Source verification

Tests use only the host's trusted stdlib and synthetic files. No test registers
the audit observer globally: installation is mocked and callbacks are invoked
directly. Coverage includes ordered timing, equal/regressing/invalid clocks,
sixteen-event saturation, ignored unsafe arguments, reentrancy, no-clobber writes,
observer failure versus original import exception, malformed/partial/duplicate
records, links, excess names/counts and retention of the original wall-time
failure with validated partial diagnostics. Native ignored tests remain unrun
until a clean signed source and exact execution recipe receive separate review.

At this source handoff, 41 targeted Python contract tests pass on the trusted
host Python 3.13.11 and 3.14.2, both normally and with `-O`. Full Python 3.13
discovery passes in both modes: 269 tests, including four explicit platform/input
skips. The ordinary Rust probe selection passes 13 tests, with all three explicit
native tests ignored. Strict core Clippy across all targets and Rust formatting
checks pass. These are source checks, not a new candidate-runtime observation.
