# Development verification — 2026-09-23

These observations describe the initial implementation, not completion of the full plan.

| Check | Observed result | Scope |
|---|---|---|
| Rust domain/workspace suite | 49 tests passed (42 integration, 7 unit) | Exact decimals/dates; atomic import; duplicate retention; review conflicts; correction propagation; transfer exclusion; currencies; reversible merges; backup/restore; corrupted originals; newer schemas; symlink paths; escaped reports; uncertainty; public-IP policy; worker-message limits; discovery budget; static HTML extraction; missing balances; acquisition provenance; general identity authoring; source-anchor validation/excerpts; identity decision recovery; assertion invalidation; robots/transport/rate-limit outcomes; redirect scope; bounded source expansion |
| Rust static analysis | Clippy passed with warnings denied | Workspace and test targets |
| UI production build | TypeScript and Vite passed | Bundled React/Cytoscape/ECharts/MapLibre assets, map worker and licensed Inter font |
| Browser workflows | Six passed against real Rust commands | Seed, transaction correction/acceptance, merge/reversal, local map, report snapshot and persistence; no external browser requests; no uncaught page errors. Empty-workspace authoring adds two namesakes, reviews cited observations, records separate/deferred decisions, corrects an observation and checks persistence. Statement workflows verify delimiter/mapping preview, saved profiles, original cells, duplicate rejection, invalid rows and delayed-response invalidation |
| Accessibility and keyboard | Eight tested section states and both transaction review layouts have zero axe violations; keyboard and minimum-width checks passed | Persistent desktop review, compact native modal, draft retention through resize, out-of-filter selection, nested source anchors, Escape, return to opener, 960×640 layout; remaining manual checks in `design/accessibility-results.json` |
| Native desktop build | Windows x64, macOS Apple Silicon and macOS Intel source builds passed in GitHub CI at `b6f80f0`; local Apple Silicon development app bundle built | Not signed/notarized product distributions; clean offline installation remains unverified |
| Java engines | Tika synthetic text parse and five Lucene queries passed on Java 21.0.12.1 | Boolean, phrase, proximity, fuzzy, fielded queries; revision returned |
| Rust → confined Lucene | Phrase and fuzzy query returned expected evidence and revision; native app phrase search returned the expected synthetic source | Staged app-local Java, local index, macOS development profile |
| Hostile Java development probe | Passed after dyld profile repair | Control worker could access sentinels/network; confined worker could not; assigned job I/O still worked |
| Python engines | 3 tests passed | Exact DuckDB/Parquet totals with record IDs; reviewed NetworkX paths; spaCy phrase candidates |
| Direct website collection | One public documentation page retained with original SHA-256 and static text | `https://example.com/`, 2 requests including robots policy; no search provider; not broad-web coverage |
| Complete release gate | Unpassed | Explicit machine-readable gates in `release-gates.json` |

The Java hostile probe exercises disposable synthetic sentinels only. It does not establish confinement for Python, Tesseract, Chromium, all JVM/native libraries, descendant processes or every supported OS version.

The source CI matrix targets Windows x64 and both Mac architectures. The native jobs initially passed in [the initial source run](https://github.com/Venin-Client-Systems/entity-workbench/actions/runs/35717611878). Its Ubuntu browser job could not launch Chromium under the hosted AppArmor policy. Browser verification now uses macOS with Chromium sandboxing still enabled. A later browser run exposed an intermittent identity-history assertion: tests now require the saved-history region, and decisions require a comparison at the current workspace revision. A deliberately delayed real comparison response verifies the stale-state guard.

CI source compilation is separate from clean offline installation, Windows 11 runtime acceptance, signed helper validation and downloadable artifact testing. Consult [the current pull request checks](https://github.com/Venin-Client-Systems/entity-workbench/pull/1/checks) for verification of the latest changes; successful earlier builds do not pre-claim current CI success.

The five collector unit tests use a synthetic transport script. They verify state-machine outcomes and bounded link expansion without any live network access. They are not evidence of live broad-web coverage, DNS behaviour or end-to-end source discovery.

No performance claim is made. The required 16 GB large-corpus benchmark has not been run. A locally fast synthetic test is not a substitute.

## Editable design pass

Real Figma Design frames, styles and component sets were created and exported. The first implementation applies the native foundations to the working UI; [handoff](design/HANDOFF.md) records frame links, comparisons, measured contrast pairs and remaining discrepancies. Final visual approval remains open.

The rebuilt Apple Silicon development application was launched with the new palette, bundled font and persistent transaction panel. Native source inspection returned the preserved CSV amount and exact row anchor. WebKit initially returned focus to the document when the source dialog closed; after the explicit focus fix, native accessibility readback showed focus returning to the source-inspection button.

The [complete pre-design CI run](https://github.com/Venin-Client-Systems/entity-workbench/actions/runs/35719487969) passed all four jobs at `b6f80f0`. Subsequent design changes have passed the local UI workflows, build and native inspection; their hosted checks remain independently visible on the pull request.

## Industrial design revision

The owner-selected industrial direction is implemented from native Figma frame `6:2`, with a graphite/amber palette, joined metric strips, square controls, compact ledger spacing and locally bundled JetBrains Mono. The revised UI production build, both browser workflows, all 34 Rust tests, strict Clippy and formatting checks pass locally. The eight section states and both review layouts retain zero automated axe violations. Ten declared text/background pairs meet 4.5:1; the smallest measured ratio is 5.30:1. The workflow tests retain their no-external-request and no-page-error assertions.

The Apple Silicon development bundle was rebuilt and launched. The revised native ledger and inspector display the original CSV amount and anchor; closing source inspection with Escape returns focus to the source button. Native inspection identified default WebKit select sizing, which is corrected with explicit control appearance while retaining the system option menu. These are development checks, not signed distribution or clean-install approval. Updated synthetic exports, image hashes and the exact remaining design scope are in the [current handoff](design/HANDOFF.md). Latest hosted verification remains visible on the pull request.

## Reusable statement mappings

Nine new Rust integration tests cover exact signed and separate debit/credit amounts, comma/dot decimals and strict grouping, explicit date formats, leading-zero accounts, multiline logical CSV anchors, profile reuse, overlapping imports, unchanged original bytes, whole-file rejection including errors beyond the displayed sample, stale/modified preview binding, and tab-separated BOM input with source-linked observations. The full suite is 43 tests. Strict Clippy, formatting and the production UI build pass. Java and Python adapters were unchanged.

Four browser workflows pass against real Rust commands. The new statement workflows add zero-violation axe checks for mapping and preview at 1440×1000 and 960×640, reachable import actions without horizontal page overflow, focus on the preview heading, preserved source inspection, saved-profile persistence, duplicate-original rejection, invalid-row blocking and a deliberately delayed real preview discarded after the analyst edits the mapping. No external requests or uncaught page errors occurred in the successful mapped-import flow. Screenshots and checksums are in [the statement design handoff](design/STATEMENT-IMPORT.md).

The Apple Silicon development bundle was rebuilt and launched. The existing synthetic workspace advanced from revision 7 to 8 through the guarded schema upgrade. A native semicolon statement with day-first dates, comma decimals, newest-first rows and fixed textual account/currency values imported three pending transactions at revision 9. The preview showed zero invalid rows and zero balance mismatches. Native source inspection returned `12,30` at logical CSV row 3, column `Paid out`, while the retained text correctly displayed the multiline description on separate physical lines. Escape restored focus to the source-inspection button. This is local development evidence, not clean-install or signed-distribution approval.

The v1→v2 tests verify that a backup contains both the original schema database and byte-identical referenced evidence. An injected SQLite event failure rolls back the version and revision while retaining a usable backup; restoring the v1 backup preserves evidence and then upgrades the restored workspace. Abrupt process termination and future migration paths remain unverified.

## Assessment authoring and explicit review

Six new Rust integration tests exercise question creation and revision, finding/question links, supporting and contradictory citations, explicit review decisions, pending-record rejection, stale writes, citation and field validation, altered-original rejection, correction propagation and preserved HTML snapshots. Version 2 migration tests retain the legacy database and originals, reopen legacy findings, preserve prior snapshots, test failed-migration rollback of finding status, and restore a usable workspace. The full core suite is 49 tests on this Unix development host; the existing Unix-only symlink test is omitted on Windows. Strict Clippy and formatting checks pass.

Six browser workflows pass against real Rust commands. The assessment flows author questions and findings, inspect both opposing source anchors, mark a finding reviewed, download a report, correct an observation, reject re-review of the pending correction, edit the finding and question, inspect decision history and verify that the prior report is unchanged. A concurrent import makes a stale question save fail without losing the draft or overwriting newer records. Four assessment states at 1440×1000 and 960×1000 have zero automated axe violations, with no horizontal modal overflow, external requests or uncaught page errors. The edit-flow check exposed ambiguous wrapped textarea labels when editing existing values; explicit accessible field names now make those controls stable. No test timeout increase was needed.

The editable Figma finding-review specimen, source export, application screenshots and remaining design scope are recorded in [the assessment handoff](design/ASSESSMENT-REVIEW.md). Java/Python engines and confinement policies were unchanged. Full workflow, packaging, sandbox, broad-web coverage and signed-release gates remain open. Hosted checks for this increment must be assessed on its pull request; earlier successful source builds are not evidence for a new commit.

The Apple Silicon development app was rebuilt and launched against its existing synthetic workspace. Opening advanced revision 9 to 10 through the guarded schema upgrade. Native finding review inspected `statement.csv`, row 10, column `amount`, returning the preserved `-180.00` value; Escape restored focus to the source button. Reviewing the pending transaction citation failed visibly while retaining the reason. Editing the finding then saved its revised limitations and reason at revision 11. Native development validation does not establish clean offline installation or a signed release.

The final rebuilt native app retained the revised finding after restart. Saving another recorded edit advanced to revision 12 and returned keyboard focus to **Review finding**; the fix defers focus restoration until the form's DOM changes have completed. Saving a native report snapshot then advanced the workspace to revision 13 while the snapshot correctly identifies source revision 12. The final six browser workflows also verify this editor-to-register focus path.

## Delivery tooling — EW-01

The offline runtime inventory verifier has 25 synthetic test methods. Local Apple Silicon: 24 passed, one Windows-junction check skipped as platform-specific. Tests exercise all three target policies, missing OCR/region assets, tampering, unsafe paths, links, malformed declarations, bounded errors and fail-closed behaviour under `python -O`. Native CI runs the same suite; source CI is distinct from installed-artifact acceptance.

The actual Java/Lucene staging inventory hashes 269 files / 168,205,077 bytes and fails completeness with 21 missing required components and the unlisted development marker. See the [sanitized negative report](delivery/evidence/runtime-inventory-2026-09-23.json) and [interpretation/limitations](../packaging/README.md). The tested directory is only the engine staging area, not the whole application. No release gate has changed.

The first Windows inventory run exposed zero identity/link fields from `DirEntry.stat`, which rejected ordinary files. The verifier now uses `os.stat(..., follow_symlinks=False)` to obtain real metadata without following name-surrogate reparse points. The existing positive inventories and hardlink/junction tests cover this regression on Windows. This follows the [Python filesystem API contract](https://docs.python.org/3/library/os.html#os.DirEntry.stat); the failed CI run remains available in the PR history.

The Windows rerun also exposed inconsistent path-versus-handle timestamp observations. Identity/size/link checks now bind the pathname to its opened handle; concurrent-change detection compares descriptor metadata before and after hashing. Regression tests cover differing path timestamps and a real file mutation during hashing. Exact manifest byte hashes remain mandatory.
