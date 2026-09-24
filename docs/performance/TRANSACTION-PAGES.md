# Development transaction-page measurements

This harness measures real `PageTransactions` and the full presentation response
on isolated copies of the frozen 100,000-row ledger from the [original campaign](TRANSACTION-BASELINE.md).
It is a backend development measurement, not completion of EW-36, a UI benchmark
or a claim that the overall workspace presentation has been paginated.

Run with the existing retained, closed workspace; no data is imported from an
external service and no canonical review decisions are fabricated:

```sh
cargo test --locked -p workbench-core --example transaction_page_benchmark
python3 -m unittest discover -s scripts/tests -p test_transaction_pages.py -v
python3 scripts/transaction_page_benchmark.py --source-workspace PATH_TO_RETAINED_CASE
```

The source database and frozen original must be ordinary files without live
SQLite sidecars. The runner hashes these and any adjacent original campaign
report/fixture files before and after. It copies the database and original to a
fresh UUID-named directory, opens/migrates that copy through the canonical Rust
store outside timing, verifies frozen transaction counts and source identity,
then gives each operation another exact database/original copy. Original retained
evidence is never opened for writing. Source revision, dirty status, Rust source
hashes, runner hashes, compiler, retained release executable and before/after
workspace hashes bind each observation. Failed and partial runs remain retained.
`--reuse-build` requires a matching source/binary build receipt.

Each operation has a separate process, one first-call sample and 20 repeated
samples on the same handle. These labels do not mean cold OS caches: copying,
opening and SQLite integrity checks precede timing. Calls run sequentially. No
power, thermal, filesystem-cache or whole-host contention controls are claimed.
The fixed mix covers 200-row all-ledger first/second pages, descending date order,
accepted rows for account `0006`/AUD/calendar year 2021 first/second pages,
pending-only selection, an empty account scope, stale revision rejection and the
complete presentation `View` response. A continuation is obtained by an actual
preceding page outside timing. Repeated continuations intentionally reuse that
same valid cursor and revision; this is not a full traversal benchmark.

Successful timings include the actual Rust command dispatch, canonical checks,
JSON value construction and complete JSON serialization into a bounded counting
SHA-256 writer. This avoids retaining another huge response buffer, while counting
the exact bytes produced by the serializer. It excludes Tauri IPC, JavaScript
parsing, UI rendering, command construction and post-call oracle verification.
Stale-revision timings stop at the actual conflict result; no success payload is
reported for a rejected request. No report export is created.

Outside timing, the frozen fixture generator supplies an independent expected
selection, stable date/sequence order, all four review denominators, exact money
strings, IDs and CSV source anchors. Every returned page must match it and leave
the revision unchanged. The complete presentation is checked for its row count,
revision and original identity; existing domain tests supply broader analytical
correctness. A successful process must return all 21 verified samples and its
completion record. Read operations must leave database/original hashes unchanged.

Warm p50 and p95 use nearest rank over 20 observations, with all raw samples
retained. This short single-host run provides no statistically qualified tail
latency or two-second release gate. `/usr/bin/time` peak RSS describes each entire
measurement child, including workspace open, fixture-oracle preparation and
result verification. It is neither per-query allocation nor total desktop/worker
memory. The 180-second cap covers the complete child, not each query independently.

The runner publishes no large corpus, local paths, raw transaction payloads or
executable. Selected sanitized observations can be committed only after review;
full local logs, copied workspaces and executables stay under ignored artifacts.

A separate bundled-SQLite diagnostic executes the two fixed empty-account SQL
statements from the baseline page reader. It retains query digests, query plans,
SQLite version and 21 individual phase timings. These copied, read-only probes
identify the baseline scan cost; they are not an alternate application query
implementation and are excluded from command timings. They remain fixed when a
production optimization removes one scan, so the original cost remains inspectable.

## Retained 25 September 2026 observation

The [paired observation](observations/2026-09-25-transaction-pages.json) contains
all raw timings and identity hashes from clean signed source `26e03df` (baseline)
and `5464823` (one query shortcut). Both ran on the same Apple M1 Mac with 16 GiB
physical memory. The original campaign files remained unchanged. Both runs used
prepared database SHA-256
`fa3f6c17db05502d0a853c1f22f00802e3bf1f851e69cfcbfc79591e58aa7955`
at revision 95,102, with the same original CSV. All 189 command samples per run
passed their checks; every response byte count and response SHA-256 matched
between runs.

| Actual operation | Baseline warm p50 / p95 (ms) | Optimized warm p50 / p95 (ms) | JSON bytes |
| --- | ---: | ---: | ---: |
| All rows, first page | 330.322 / 368.508 | 313.935 / 332.994 | 94,696 |
| All rows, second page | 344.674 / 440.055 | 317.464 / 334.109 | 94,711 |
| Descending date, first page | 305.315 / 327.024 | 315.525 / 350.506 | 94,740 |
| Account/currency/year/accepted, first page | 190.052 / 196.272 | 190.829 / 212.773 | 94,803 |
| Same filter, second page | 190.608 / 200.215 | 190.565 / 195.914 | 94,809 |
| Pending selection, first page | 304.281 / 344.780 | 315.123 / 319.877 | 94,694 |
| Empty account scope | 158.190 / 164.865 | 78.585 / 83.660 | 262 |
| Stale revision, expected conflict | 0.009 / 0.010 | 0.007 / 0.010 | No success payload |
| Full presentation response | 952.327 / 991.064 | 952.671 / 1,005.394 | 83,986,629 |

The 200-row first page contains about **887 times fewer serialized bytes** than
the complete presentation. This does not mean the application's default response
has been changed or that its UI is 887 times faster. Decisions, evidence text,
analysis and the remaining whole-workspace response are outside this paging slice.

The concrete query finding was an unnecessary second scan after an empty selected
count was already known. Bundled SQLite 3.50.2 reported a `kind` index search plus
temporary grouping/ordering trees. The separately measured baseline empty count
scan had p50/p95 78.367/80.562 ms; its candidate scan had 79.980/84.026 ms. Returning
an empty page after **full cursor validation** removed that second scan. Observed
empty-command p50 fell by 50.3%, and p95 by 49.3%. A regression verifies that a
cursor naming a real but excluded row still fails in both empty and pending-only
scopes. No schema, index, cursor format, denominator or money semantics changed.

Nonempty query paths were not changed. Their timing differences between runs are
not evidence of an improvement; host/cache variance was not controlled. Those
queries still scan and sort SQLite JSON records. This experiment does not justify
a major storage migration or establish a statistically qualified p95.

Peak RSS is retained per child in the observation. It includes preparation of the
fixture oracle and the complete process lifetime, and varies across runs; no
query-allocation or overall desktop-memory claim follows from it. It excludes
other application processes. The full presentation still constructs its large
JSON value, even though the diagnostic serializer avoids a second giant buffer.

After these successful measurements, peer review found that an early host/git
metadata failure could precede creation of a failure report. The runner now saves
a minimal report first and has a regression for that failure. This reporting-only
follow-up does not replace or relabel the measured source/executable identities.
