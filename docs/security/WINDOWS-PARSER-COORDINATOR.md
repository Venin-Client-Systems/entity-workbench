# Windows canonical document processing campaign

This development harness exercises the public Rust workspace and job-coordinator
interfaces with the confined Windows Java parser recipe. It adds no production
commands, worker overrides or cancellation hooks. Its first actual Windows run
is pending. Host contract tests and cross-target compilation do not establish
native confinement, clean installation, Windows 11 compatibility or release
acceptance.

The campaign depends on the integrated Windows parser adapter, extraction record
version 2 and contextual trusted-runtime error classification. An older build
must fail its checks. The existing [AppContainer foundation](WINDOWS-WORKERS.md)
and [Java engine campaign](WINDOWS-JAVA-WORKERS.md) remain earlier, mandatory
workflow steps. Their hostile-worker and role-isolation assertions are separate
evidence from this coordinator campaign.

## Public workflow and fixed fixtures

The example `crates/core/examples/windows_parser_probe.rs` opens an owned,
synthetic workspace and attaches the staged runtime parent. It uses
`Command::Import`, `QueueDocumentParse`, `InspectProcessingJob` and
`InspectExtraction` through `JobCoordinator::dispatch`. It never calls the
private claim/completion interfaces, injects an executor or writes SQLite.
Every parse uses the existing fixed parser recipe, resource limits and runtime
inventory validation. The coordinator is started with concurrency one.

| Fixed input | Required canonical outcome | Required extraction identity/content |
| --- | --- | --- |
| `notice.txt` | Completed | Exact UTF-8 content, `utf8-v1` |
| `unreviewed.source` | Completed | Distinct inline UTF-8 content, `utf8-v1`; evidence text stays `None` |
| `notice.pdf` | Partial | `pdfbox-3.0.8-local-fonts-v1`, expected synthetic names, one page, explicit font limitations |
| `notice.docx` | Partial | `tika-ooxml-3.3.2`, expected synthetic names |
| `no-text.pdf` | Partial | Vector-only one-page PDF; exactly one Windows CRLF, closed limitations |
| `malformed.pdf` | Failed / `document_failed` | Failed extraction with `malformed_document` |
| `traversal.zip` | Failed / `document_failed` | `zip-preflight-v1`, `archive_limits`, no text/metadata |
| `unsupported.bin` | Blocked / `unsupported_format` | `unsupported-v1`, no text/metadata |
| `font-corpus.pdf` | Partial | The reviewed 21-page standard-font, encoding and ToUnicode oracle |
| `embedded-font.pdf` | Partial | Embedded-font text without false substitute-font limitations |

All ten inputs have distinct SHA-256 identities. The PDF assertions reuse the
existing Java engine campaign's closed fixture oracle, including per-case
markers and page counts. This does not establish PDF rendering or universal
script/font coverage.

Each accepted extraction must be schema version 2, match the exact job/result ID
and attempt, and bind the original evidence ID, SHA-256 and byte count. The
harness invokes the shared canonical result validator and recomputes the stored
result hash. Failed and unsupported parser outcomes remain inspectable canonical
derivatives, distinct from a missing or rejected runtime, which publishes no
extraction.

Repeating each completed request key must return the same job and leave the
workspace view and extraction unchanged. Ten fixture jobs produce ten unique
referenced results. This assertion describes the public canonical records; it
does not claim a direct database scan for unreferenced rows.

## Evidence ownership, cancellation and recovery

The full evidence record after each import must remain unchanged after parsing.
The TXT importer already records text; this is an explicit baseline, not a parser
acceptance. The distinct `.source` fixture demonstrates `None` before and after
parsing. No observation or assertion is accepted by the campaign.

A separate job is queued at attempt 1 and cancelled through the public command
before the coordinator starts. It must retain its exact cancelled record,
`cancelled_by_analyst` reason, absent start time and empty result IDs after
startup. This proves queued cancellation without a claim; it does **not** prove
in-flight cancellation or that observing `Running` identifies a native execution
checkpoint. The receipt always says `in_flight_cancellation_proven: false`.

After the first parsed fixture, the campaign creates a frozen backup through the
public backup command. After all ten, it creates a full backup. It explicitly
joins the coordinator, reopens the workspace, starts and joins another
coordinator, and compares the complete public view, jobs and referenced
extractions. Both backups are restored to separate empty destinations through
`Workspace::restore`. Their public records must equal their saved snapshots,
every referenced original must retain its byte count and SHA-256, and scratch
must be empty. The frozen backup must still contain only the cancelled job and
first parsed fixture, even after later results exist in the source workspace.

This tests completed-job restart and complete backup/restore, including originals
and extraction records. It does not simulate power loss or claim a new migration,
interrupted-running-job or in-flight cancellation result.

## Runtime controls and bounded cleanup

The missing-runtime workspace must end `Blocked` / `runtime_unavailable` with no
result. A separate control workspace uses a bounded private copy of only the
parser runtime. Before mutation, an actual successful canonical parse establishes
that this copy works. Altering one worker-JAR byte must then block with no result.
After restoring the exact JAR, a second actual success is required before changing
only the manifest role to `search`; that role mutation must independently block.
Earlier extraction records remain unchanged. The original staged manifest and
worker JAR must retain their hashes.

The harness copy is limited to 10,000 entries, depth 16 and 1 GiB total bytes,
streamed in 64 KiB blocks. It rejects links, special files, Windows reparse points
and multiply linked files. During Windows traversal, pinned directory/file handles
deny concurrent writes and deletion. Destination files are created exclusively.
The production worker still validates its own complete copied inventory and
applies its existing per-job limits; this harness does not broaden them.

Every coordinator action path attempts an explicit joined shutdown. A failed
join governs the result and prevents subsequent recovery reads or cleanup. A
failed campaign retains its owned synthetic area, including any assignment whose
termination could not be verified; a `TempDir` destructor does not traverse it.
Only a fully successful campaign removes the area, after all four coordinators
are joined and scratch emptiness is checked. A host-runner timeout is a failure,
not confirmation that native worker cleanup completed.

## Receipt and workflow contract

`scripts/test_windows_parser_coordinator.py` writes a failed receipt before
launch, removes any previous child receipt, and enforces a 900-second outer
campaign watchdog. A fixture job has a 90-second preparation/publication guard;
the actual Java worker's existing 30-second limit is unchanged. The overall
workflow remains bounded by its existing 25-minute job timeout.

Success requires all 25 named checks, ten ordered derivative identities, exactly
four joined coordinators, a clean current Git source identity, the matching
compile-time `WORKBENCH_COORDINATOR_PROBE_SOURCE`, a fresh nonce and unchanged
binary/runtime-manifest hashes. The actual Git tree and parents are retained so
a pull-request merge checkout is not confused with its branch head. Source and
fixture identity are checked again after execution. Nonignored untracked files
also make the checkout unclean. Unknown fields, duplicated
JSON keys, nonfinite numbers, oversized receipts, incomplete checks, nonzero
exit, timeout and malformed output cannot produce success. Earlier or late child
success cannot override a failed parent observation.

The raw child receipt uses a `.receipt` extension and stderr uses `.log`; neither
matches the workflow's uploaded `*.json` files. The parent only embeds validated
closed fields, hashes and fixed categories. It never exports extraction text,
local paths or raw native/JVM messages. The workflow creates a separate failed
`coordinator-report.json` before all builds and probes, so failure of an earlier
stage leaves the canonical campaign explicitly unpassed.

To execute on a prepared Windows development checkout, after the existing
foundation and Java engine campaigns:

```powershell
$env:WORKBENCH_COORDINATOR_PROBE_SOURCE = (git rev-parse HEAD).Trim()
cargo test -p workbench-core --example windows_parser_probe --locked -- --test-threads=1
cargo build -p workbench-core --example windows_parser_probe --locked
python scripts/test_windows_parser_coordinator.py --binary target/debug/examples/windows_parser_probe.exe --runtime runtime/windows-java-development --destination artifacts/windows-confinement
```

The Python unit tests use mocked subprocess outcomes to test receipt handling;
they are not native execution evidence. Actual downloadable-package testing,
Windows 11, signing, independent security review, core search activation and a
deterministic real-coordinator in-flight cancellation campaign remain separate
requirements. No success here sets `complete_release` to true.

## Source validation before the first native run

On the development host, the six Rust example tests and twelve Python runner
tests passed; the twelve Python tests also passed with optimization enabled.
Strict host Clippy, core formatting and workflow actionlint passed. The new
Windows-only runtime-copy module type-checked against the real Windows GNU
target and `windows-sys` metadata with warnings denied. The complete core example
cross-check stopped in the existing `ring` build because the local MinGW C
compiler was unavailable. This is a local verification limitation, not a native
pass or evidence that the Windows example links or runs. The actual hosted
workflow must build and execute the integrated source before any coordinator
campaign success is recorded.
