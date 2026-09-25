# Explicit report retrieval

The desktop previously received every historical report HTML body with each workspace refresh or mutation response. The revised bridge requests a presentation response: each report has its ID, source revision, creation time, SHA-256 and UTF-8 byte count. Selecting **Export self-contained HTML** retrieves one immutable snapshot by ID and expected digest. Rust verifies its identity and actual HTML hash before returning it; integrity failures remain visible and can be retried after repair. Saved HTML never runs in the privileged interface.

`Workspace::view` and ordinary `dispatch` retain the historical full-view response. Desktop/coordinator dispatch and the synthetic browser bridge explicitly select presentation mode. Both paths share domain command handling, worker cancellation and a single SQLite read snapshot. A new report loads its canonical sources without allocating prior report HTML, which is not used by the renderer. No report, transaction or decision is removed or rewritten by this response change. The database needs no additional migration beyond retained-region storage schema 4.

Command v11 adds `inspect_report_snapshot`; the new workspace-presentation v1 schema defines report metadata. All earlier generated schema files remain byte-identical. `ew-dev <workspace> --presentation [runtime]` is an opt-in development response mode; the legacy command remains available.

## Measured byte verification

Clean source `de49ba64c89e54872196864383a44f5b615d14b2` was tested on an **isolated copy** of the retained 100,000-transaction, one-report workspace. Its original database hash was checked before and after and remained unchanged. The copy migrated from storage schema 3 to 4 with its recoverable backup. The [retained observation](observations/2026-09-25-report-presentation.json) binds the tested executable, source, runner, original database and exact response hashes.

| Response | JSON bytes, excluding the CLI newline |
|---|---:|
| Legacy full workspace | 198,449,390 |
| Desktop presentation | 83,986,848 |
| Explicit selected snapshot | 114,462,761 |

The repeated workspace response is **114,462,542 bytes smaller**. All 100,000 transactions, 95,100 review decisions, the legacy calculation response and every non-report field were identical between response modes. The selected export was byte-identical to the retained snapshot; raw HTML remains 113,592,530 bytes with SHA-256 `3be44e386ec0dea70c7ccf715f48ce23daf93b7fd8df06977507ccf5bc158f0d`.

This observation makes **no timing, memory or native UI responsiveness claim**. SQLite still reads/parses JSON records, and the remaining 84 MB workspace response is still too large for a finished large-case UI. Revision-bound summaries, pagination, selected evidence/review reads, bounded analytical snapshots and native rendering measurements remain required under EW-36.

The explicit in-memory report response bounds stored JSON and HTML to 256 MiB and returns an error above that size. It does not truncate or alter retained snapshots. Large-report streaming/export and reducing HTML repetition remain unfinished; the legacy full-view API itself has not gained an aggregate limit.

## Verification scope

Core regressions cover preserved legacy responses, presentation metadata, exact UTF-8 counts, explicit immutable retrieval after later changes, incorrect identity/digest, corrupt HTML, malformed types and a real concurrent canonical writer. The assessment browser workflow checks actual downloaded bytes against the canonical snapshot. A second browser scenario alters the retained HTML after catalogue load, confirms Rust rejects export without a download, then restores the bytes and verifies a successful retry. The real core handles both paths; success responses are not mocked.

These development checks do not pass a complete-release gate or qualify the performance target.
