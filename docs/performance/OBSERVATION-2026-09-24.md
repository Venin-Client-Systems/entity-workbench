# First canonical 100,000-transaction development observation

All six measured operations completed with their source/totals checks. This is a
development baseline, not the complete EW-36 performance gate. The
[methodology](TRANSACTION-BASELINE.md) defines the fixed fixture, timing boundaries
and omitted work; the [machine-readable report](observations/2026-09-24-macos-arm64-transactions.json)
retains all 24 samples, review denominators, output hashes, source/binary bindings,
setup progress and resource observations.

The report's SHA-256 is
`3c16f2e415b55a5e83df3ee2c849069b7ef65ecd7b75df1b0f6e80562a81e3ab`.
It is an exact copy of the retained local report, rather than a rewritten success
projection. Large generated originals, workspaces and exports remain under the
ignored run directory identified below.

## Observed environment and provenance

- UTC interval: 24 September 2026, 14:28:28–14:30:03 (25 September locally).
- Apple M1, arm64, eight logical CPUs, macOS 26.6.2.
- Physical RAM: 17,179,869,184 bytes (16 GiB).
- Rust 1.90.0; existing release profile, locked dependencies, no query changes.
- Production source base: `07ee9cf75d5bd7d8f4391dda1aeaf3c6d224a3c6`.
  The new harness was uncommitted during measurement, so `source_dirty` is true.
  Exact core/example sources, Cargo files, executable and runner hashes are in
  the report. This is not a claim that the base commit already contained the
  harness.
- Other agents paused heavy local work for the campaign. The desktop/browser and
  ordinary host applications remained open; a small Python evidence-test run
  occurred during setup. OS cache, power, thermal conditions and storage were
  uncontrolled. No clean-host or exclusive-reservation claim is made.
- Retained local run: `artifacts/transaction-performance/20260924T142828-b16b36cb71404e8aba02a732e4225443/`.

## Canonical setup

The frozen 5,010,041-byte CSV has SHA-256
`c587e3ad59c11445f780306b9aef370e4d97310cf66728b1aabe5f9819c5aef7`.
Its 100,000 records entered through canonical import and ordinary review/match
methods. Final revision **95,101** contains **85,000 accepted, 5,000 pending,
5,000 rejected and 5,000 deferred records**, including 100 reviewed transfer pairs.
No records were silently removed to accelerate measurement.

| Setup operation | Observed seconds |
| --- | ---: |
| Canonical CSV import | 1.567 |
| 95,000 review decisions and 100 transfer matches | 63.644 |
| Consistent canonical backup | 0.335 |

The import alone corresponds to approximately 63,818 rows/second for this single
simple generated CSV. That is not OCR throughput, statement-preview responsiveness,
or a general import performance claim. Canonical review uses individual durable
mutations and measures fixture setup, not human review speed.

## Operation observations

Every row below has one first-call fresh-process sample followed by three repeated
calls. OS page-cache cold behavior was not measured. Times are milliseconds,
rounded here to three decimals; the JSON preserves the actual recorded values.

| Operation | First call | Repeated 1 | Repeated 2 | Repeated 3 | Child-lifetime peak RSS, MiB |
| --- | ---: | ---: | ---: | ---: | ---: |
| Whole workspace View + legacy analysis + JSON | 781.161 | 709.143 | 727.266 | 747.129 | 869.08 |
| All-scope patterns + JSON | 467.271 | 455.826 | 444.944 | 465.195 | 557.67 |
| Account/currency/year patterns + JSON | 278.266 | 281.366 | 278.910 | 281.933 | 203.42 |
| Empty exact analytical scope + JSON | 270.135 | 265.963 | 264.506 | 286.846 | 194.36 |
| Two-period comparison + JSON | 408.756 | 439.353 | 415.683 | 417.574 | 272.33 |
| Canonical HTML snapshot publication | 1660.386 | 1561.266 | 1563.448 | 1752.265 | 2072.31 |

These small samples do not establish a statistically useful p95. The nearest-rank
p95 of three repeated samples is simply the largest one. The report leaves both
`p95_qualified` and `under_two_seconds_gate_passed` false. The ordinary JavaScript
ledger filter, native IPC and UI rendering are absent from these timings.

The broad result included 849 merchant-description groups and 648 recurrence
candidates. The filtered result kept 996 rows in scope: 780 accepted and included,
204 deferred and 12 explicitly excluded transfer records; it produced 65 merchant
groups and 49 recurrence candidates. The empty scope returned a valid empty result
while retaining the 100,000-row workspace denominator. Comparison denominators
remain separate for every account, currency and period in the report.

Peak RSS comes from `/usr/bin/time -l` across each complete child lifetime. It
includes restoration, verification and output/oracle allocations. In particular,
the HTML process reached **2,172,977,152 bytes**, which is not a measurement of the
native application's total memory or an isolated HTML allocation peak.

## Concrete next profiling questions

The timings expose substantial transport and artifact volume even though this
small run did not exceed two seconds inside its stated boundaries:

| Output | Exact bytes |
| --- | ---: |
| Whole workspace response | 83,986,629 |
| Broad patterns response | 58,002,467 |
| Filtered patterns response | 588,673 |
| Empty patterns response | 1,672 |
| Comparison response | 16,050,241 |
| One HTML export | 113,592,530 |

1. Measure native IPC, JSON parsing and rendered ledger/pattern interactions at
   these response sizes before treating the Rust timings as perceived latency.
   Bounded analytical pagination and narrower workspace reads are profiling
   candidates; no implementation or correctness guarantee is changed here.
2. The empty filtered calculation still took about 270 ms because current
   correctness validation reads and validates the full ledger. Profile that cost
   while preserving revision consistency and out-of-scope transfer verification.
3. Profile the HTML publication and readback path before adding more report
   content. One snapshot is approximately 108.33 MiB; repeated historical snapshots
   and whole-workspace refresh can increase memory and storage costs. The harness
   resets to zero prior snapshots for every export and therefore does not measure
   that accumulation or browser print/render behavior.
4. Run the required document/search/OCR and concurrent map/graph workload on a
   controlled host, then repeat adequate samples with cancellation and UI evidence.

The retained harness run occupied 363,781,483 logical bytes after setup and
2,890,993,827 at the end. These figures include backup and nine restored benchmark
workspaces, four complete HTML exports and logs; they are not a single-workspace
storage estimate or a peak-disk measurement. Failed runs and previous smoke runs
are separate directories and were preserved.

No domain or query algorithm was modified in response to these observations. A
follow-up change should retain this result and rerun the same fixture so any
performance improvement remains comparable and its correctness remains reviewable.
