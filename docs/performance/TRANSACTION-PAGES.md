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
