# Confined document parser development slice

This is a bounded implementation for **EW-12 / issue #16**, exposed through the Rust engine layer. It supports UTF-8 text decoding, PDF text extraction with PDFBox 3.0.8, and DOCX extraction with Tika 3.3.2/POI. The application coordinator owns job persistence, originals, review and canonical publication. The worker only returns an untrusted derivative.

## Public engine contract

`Runtime::parse(scratch_root, original_bytes)` creates a disposable request. `Runtime::parse_with_cancel(scratch_root, original_bytes, &CancellationToken)` additionally supports cooperative cancellation. The token is cloneable and can be cancelled from a coordinator thread. The Mac supervisor checks it before launch and during its wait loop, kills the worker process group and tracked leader, and reaps before returning. Cancellation returns `Error::Blocked`; the coordinator decides the durable terminal state and resolves publication races against a persisted cancellation request.

`engines::parser::ParseResult` is a versioned, strict serde/JSON-schema type containing:

- `protocol_version`, `job_id`, `content_sha256` and `source_bytes` bind the result to the assigned job and original.
- `parser`, `media_type`, `status`, `text` and bounded `metadata` describe the derivative.
- Typed `limitations` and optional typed `error` distinguish incomplete coverage, unsupported bytes and failed extraction.

`validate_result(result, expected_sha, expected_bytes)` is shared with canonical acceptance. Transport acceptance additionally requires the exact generated job ID. Unknown fields, inconsistent types/statuses, incorrect source bindings and excessive outputs fail validation. No page, cell, message or region anchors are returned or invented.

| Status | Meaning in this slice |
| --- | --- |
| `complete` | The supported UTF-8 decoding operation completed without truncation. This does not claim structured extraction or source-region anchors. |
| `partial` | PDF/DOCX extraction or bounded/truncated UTF-8 output; limitations remain explicit. All PDF results disclose missing OCR, embedded extraction and source anchors. DOCX results disclose missing embedded extraction and source anchors. |
| `unsupported` | Bytes do not match an enabled format, or the ZIP is not an identified DOCX package. No extracted claims are returned. |
| `failed` | Malformed, encrypted or restricted content, or rejected archive limits. No text/metadata claims are returned. |

A killed, timed-out or otherwise unsuccessful process returns an engine error without pretending it produced a valid failed extraction record. Missing runtime/confinement and cancellation use `Blocked`; the durable cancellation flag distinguishes cancellation. Observed wall-time, tree and result-byte overruns use `QuotaExhausted`. Scratch cleanup failures return typed `Error::Cleanup` so the coordinator can preserve that diagnostic even when the terminal state is Cancelled. Nonzero exits and invalid output remain worker failures when the precise cause cannot be established; an exit code is not guessed to mean memory or CPU exhaustion. All output remains unreviewed. Text and metadata can contain hostile markup or misleading content and must be displayed as escaped data. Tika explicitly treats its output as untrusted extraction and does not constitute a security boundary. [Apache Tika security model](https://tika.apache.org/security-model.html).

## Independent parser process and access

The parser launches a fresh JVM for each original using `parser/workers-0.1.0.jar` and `parser/lib/*`. Its profile has no index path and permits no search-library access. The existing search worker continues to use a separate JVM, search classpath and operation-specific index permissions. The staging script selects only parser/protocol/probe classes and excludes Lucene dependencies from the parser classpath.

Each parser receives only its staged input, stdin request, result file and private scratch directory. Originals and canonical databases are not exposed. Direct networking, forks, outside reads and original writes remain denied. The shared supervisor preserves the descriptor/environment cleaning, timeout, process-group cleanup, final tree validation and explicit verified scratch cleanup described in [MACOS-WORKERS.md](MACOS-WORKERS.md). Search behavior is covered by the combined native regression run.

This implementation still uses deprecated `sandbox-exec` for **development Mac testing only**. It is not a supported signed-helper package, clean-install test or release security sign-off. Other platforms return a blocked parser capability until a verified implementation is integrated.

## Bounds and format handling

| Resource | Current bound |
| --- | --- |
| Original | 16 MiB |
| Request wall/CPU time | 30 seconds |
| JVM heap | 256 MiB; not a proven total resident-memory bound |
| Result file | 2 MiB, read with no-follow/regular-file/one-link validation |
| Text | At most 128,000 UTF-16 units in the adapter, 128,000 Unicode scalar values and 512,000 UTF-8 bytes at Rust acceptance |
| Metadata | 32 keys, eight values per key, 128-byte keys, 4,096-byte values and 32,768 bytes overall |
| PDF text pages | First 100 pages; additional pages produce a limitation |
| DOCX ZIP members | 1,000 members, 16 MiB per expanded member, 64 MiB expanded total; duplicates/traversal/absolute/unsafe names rejected |
| Embedded/nested documents | Excluded; no recursive extraction |

PDFBox streams text through a bounded writer and stops text accumulation at its limit. A PDF without recoverable text stays partial with the OCR limitation. DOCX archives are inspected and drained within expansion limits before Tika/POI parses them. ZIP preflight failures identify the preflight adapter rather than falsely identifying unknown ZIP content as DOCX. Limits on metadata/text are disclosed rather than silently claiming complete extraction. PDF loading uses the PDFBox 3 `Loader` interface. [PDFBox migration guide](https://pdfbox.apache.org/3.0/migration.html).

This slice does not provide OCR, tables/cells, page rendering, location anchors, attachments, exported email, spreadsheets, legacy Office formats or layout fidelity. The former unexposed `render_pdf` worker branch is not part of this versioned API. It must receive its own bounded protocol and verified result contract before integration.

## Local staging and verification

Build the current Java adapter using an already installed JDK/Maven and the locally available locked dependencies. End users do not perform this step. Stage Java and search through the existing development workflow, then stage the parser into a fresh parser directory:

```sh
python3 scripts/stage_parser_engines.py --worker-target /path/to/current/java/target
python3 scripts/generate_parser_fixtures.py
python3 scripts/test_parser_workers.py
cargo test --locked -p workbench-core
cargo clippy -p workbench-core --all-targets -- -D warnings
```

The staging helper performs no download and refuses to overwrite an existing parser directory. `--runtime` selects an existing staged Java/search runtime. Missing parser assets fail closed. `fixtures/parser/manifest.json` contains deterministic hashes for synthetic UTF-8, text-PDF, no-text-PDF, DOCX and traversal-ZIP inputs.

The native runner executes twelve engine/supervisor tests, including the runtime-dependent tests normally ignored by source CI. It checks actual supported extraction, source hashes, limitations, unsupported/malformed/archive cases, text truncation and cleanup. A hostile probe inside the parser profile confirms outside/index reads, original/input writes, networking and child spawning are denied. A real search succeeds while the separate hostile parser process is still running; cancellation then terminates and reaps that parser.

Timestamped reports are retained under `artifacts/parser/`, with the latest copy at `artifacts/parser-result.json`. They record source and runtime hashes, fixture manifest hash, actual OS/architecture and named test results. Raw local diagnostics at `artifacts/parser-test-output.txt` may include development paths. The reports always retain `complete_release: false`.

The observed development run on 24 September 2026 passed on **macOS 26.6.2 arm64** with the existing staged Java 21.0.12.1 runtime and current locally compiled adapters. No runtime downloads were required. Signed helpers, Intel Mac execution, Windows execution, clean installed artifacts, OCR/rendering, the broader document-format matrix, hard aggregate disk/RSS enforcement and independent release review remain open.
