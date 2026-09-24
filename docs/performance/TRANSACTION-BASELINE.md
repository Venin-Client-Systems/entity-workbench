# Development transaction performance baseline

This harness measures existing canonical Rust operations on a fixed synthetic ledger.
It is an early EW-36 / [issue #40](https://github.com/Venin-Client-Systems/entity-workbench/issues/40)
observation, not completion of the programme performance gate. No production query,
import or size limit is changed for the fixture.

The complete gate still requires a controlled 16 GB machine, 100,000 transactions,
10,000 document pages including 1,000 scans, concurrent map/graph use, indexed
search, ordinary transaction filters, import/OCR throughput, cancellation and UI
responsiveness, disk growth and total/runtime peak memory. This harness contains
no GUI, document corpus, OCR worker, map, graph or network request.

## Reproduce

From the repository root, with the normal Rust and Python development toolchains:

```sh
cargo test --locked -p workbench-core --example performance_baseline
python3 -m unittest discover -s scripts/tests -p test_transaction_performance.py -v
python3 scripts/transaction_performance.py --mode smoke
python3 scripts/transaction_performance.py --mode baseline --reuse-build
```

The smoke and full run use the same release-profile example and operation mix.
The smoke has 1,000 rows and proves harness execution only. Build reuse requires
the exact binary SHA-256 and a matching hash of all Rust core/example sources,
Cargo manifests and lockfile from the retained build receipt. A changed Rust file
requires a new build (omit `--reuse-build`). Compiler identity, source revision,
dirty status, file hashes and binary hash are recorded. The example is never
packaged or exposed as an application command.

The Python runner currently supports macOS and Linux. It records actual OS,
architecture, logical CPU count, sanitized CPU model and physical RAM where
available. It records no machine serial number or hostname. Other platforms fail
with an explicit observation; this is not Windows benchmark evidence.

## Frozen fixture, version 1

The generator is [fixture.rs](../../crates/core/examples/performance_baseline/fixture.rs).
It produces UTF-8 CSV, LF line endings, fixed row order and integer-cent amounts.
The header and every original description remain source-linked through ordinary
canonical import. No SQL injection, direct database writes or fake worker results
are used.

| Case | Rows | Original bytes | Original SHA-256 |
| --- | ---: | ---: | --- |
| Smoke | 1,000 | 50,141 | `7b1b5fe662bc6dbe7163c99d6688037a7eb13659db8ee61f5852d20230ea8aa3` |
| Baseline | 100,000 | 5,010,041 | `c587e3ad59c11445f780306b9aef370e4d97310cf66728b1aabe5f9819c5aef7` |

Every merchant has a 100-month history from January 2016 through April 2024.
Eight account labels preserve leading zeros. AUD and USD remain separate. Fixed
description groups include debit merchants, ATM hints, refund hints and credits;
two groups repeat the same source fields so legitimate repeated purchases remain
present. Two other groups form 100 explicitly reviewed, reciprocal transfers in
the full case. The smoke contains only the first ten groups and no transfer pairs.
This deliberately regular fixture exercises recurrence; it is not a representative
distribution of real investigative data or worst-case adversarial cardinality.

All rows first enter as pending through `Workspace::import`. Ordinary
`review_transaction` calls apply the fixed synthetic review plan, followed by
ordinary `match_transfer` calls. The reason explicitly says these are synthetic
performance decisions. No finding or real analyst assessment is implied.

| Case | Accepted | Pending | Rejected | Deferred | Reviewed transfer pairs |
| --- | ---: | ---: | ---: | ---: | ---: |
| Smoke | 700 | 100 | 100 | 100 | 0 |
| Baseline | 85,000 | 5,000 | 5,000 | 5,000 | 100 |

Setup separately measures canonical import, review/matching and backup. Its
300-second cap is not relaxed if the accepted fraction is expensive: a timed-out
setup retains the partial workspace and failure, and every query stays `not_run`.
Canonical readback must match every review-state count before backup. A consistent
`Workspace::backup` becomes the baseline, and `Workspace::restore` verifies its
referenced original before each measurement process. Generated source bytes and
semantic decisions are deterministic; canonical decision UUIDs/timestamps and
database bytes are not claimed to be deterministic.

## Fixed query mix and timing boundaries

| Operation | Work inside the timed region |
| --- | --- |
| `view` | Actual `Command::View`: canonical whole-workspace read, existing legacy transaction analysis, conversion to JSON value, final JSON byte serialization |
| `patterns_all` | Actual `AnalyzeTransactions` with all dates/accounts/currencies; transfers included; canonical read and original verification, exact totals, heuristic groups, complete source lists and JSON serialization |
| `patterns_filtered` | Same command for account `0006`, AUD, inclusive calendar year 2021, excluding only reviewed reciprocal transfers |
| `patterns_empty` | Same filtered command with a fixed nonexistent account; zero results is a measured valid result, not a missing measurement |
| `comparison` | Actual `CompareTransactionPeriods`, all accounts/currencies, inclusive 2020 versus 2021, excluding reviewed reciprocal transfers; both periods in one revision-bound snapshot, JSON serialization |
| `html_export` | `Workspace::save_report`: canonical view/analysis, original verification, HTML construction, complete file write/fsync, atomic publication and canonical snapshot publication |

The ordinary ledger search/review/currency filter currently runs in JavaScript
over the loaded workspace. `patterns_filtered` is the existing Rust analytical
scope calculation, **not a measurement of that UI filter**. The `view` timing helps
expose transport cost but excludes Tauri IPC, parsing in JavaScript and rendering.
HTML measurement calls the same canonical save method as the application but
excludes the command's subsequent whole-workspace response refresh and UI work.

Each operation runs in its own fresh process: one first-call sample and three
repeated-call samples. These are process/connection labels, not proof of a cold OS
page cache. Restore reads originals and copies the database before the first
sample, and the prior setup has already populated caches. No privileged cache
purge is performed. Warm queries reuse the same workspace handle. Each repeated
HTML export restores the verified backup to a distinct fresh workspace outside
the timed region, so all four exports begin at the same revision with zero prior
reports. Restore time is recorded separately; HTML remains process-warm with a
fresh connection, rather than an identical query-cache experiment.

The 60-second cap applies to each operation's **whole child process**, including
restore and verification, not independently to each timed sample. A timeout kills
the isolated measurement process group and retains the completed JSONL samples.
The other independent operations may continue from the unchanged backup. Started,
missing, failed and successful-empty observations remain distinguishable.

Outside the timed region, integer-cent calculations independently check exact
currency totals and complete included/pending/rejected/deferred/excluded source-ID
lists. Comparison checks every account/currency/period group. Read calculations
must preserve the recorded revision. Published HTML must match the canonical
snapshot bytes, revision and digest exactly. These checks are not a complete
semantic oracle for every recurrence heuristic or report citation; existing
domain/report regression tests retain that responsibility.

## Evidence and interpretation

Each run creates a unique directory under ignored `artifacts/transaction-performance/`.
`report.json` starts incomplete before expensive work and is replaced atomically
after each phase. Raw stdout JSONL, stderr, sample output hashes, binary/source
identities, fixture, canonical workspace, verified backup and HTML exports remain
available. Failures never overwrite a previous successful run. Large fixtures,
databases and exports must not be committed. Inspect local diagnostics before
publishing any selected observation: they can contain development paths.
The report also records the retained run directory's logical file bytes after
setup and at the end. These include verified backup, restored measurement copies,
SQLite files and exports. They are not allocated disk blocks, peak scratch use or
the storage requirement of one application workspace.

Raw first-call and all three repeated timings are the primary evidence. The
reported nearest-rank p95 over three warm samples is mathematically their maximum.
It has no useful tail-confidence qualification. `p95_qualified` and
`under_two_seconds_gate_passed` are always false, including when every sample is
under two seconds. Do not turn this short run into a release responsiveness claim.

When `/usr/bin/time` provides it, peak RSS is the whole measurement child lifetime,
including restore, output serialization, oracle allocation and HTML readback. It
is not per-operation allocation attribution, aggregate application/worker memory,
or a hard resident-memory limit. Missing or timeout-truncated RSS stays null. On
macOS the reported unit is bytes; on Linux kbytes are converted to bytes. No memory
claim follows from the development host merely having 16 GiB installed. Power,
thermal state, storage contention and other applications are not controlled.

Use observed misses to choose a separate profiling/repair task, preserve the
original result, and rerun this same frozen fixture after a change. Do not adjust
the fixture, bounds or measurement scope to manufacture a pass.

The first retained [100,000-row observation](OBSERVATION-2026-09-24.md) reports the
actual raw measurements and the next profiling questions.
