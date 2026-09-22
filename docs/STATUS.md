# Implementation status

This is an initial integrated development implementation. None of the three milestones is declared complete. The supported-release target remains Windows 11 x64, macOS Apple Silicon and macOS Intel.

| Area | Implemented and verified | Remaining work |
|---|---|---|
| Product design | Native editable Figma frames, styles and component sets; applied foundation tokens and bundled Inter; desktop transaction panel and compact dialog; exported comparisons, axe and keyboard verification | Remaining frame discrepancies, full workflow designs, icon/typography refinement, manual accessibility and final visual sign-off |
| Workspace | Rust-owned SQLite, revision history, content-addressed originals, strict commands, recoverable backup/restore, newer-schema refusal | Real migration paths and failure injection; full Windows ACL implementation; workspace management UI |
| Evidence | UTF-8 text/CSV import, hash deduplication, source rows, escaped text review | OCR and page/cell-region review; complex tables; Office/email workflows; versioned derivative UI |
| Transactions | Exact decimals, separate currencies, review decisions, reversible corrections, probable duplicate examples, explicit transfer pairs, source-linked totals | Mapping profiles; recurring/refund/merchant classification; account-flow graphs; paginated analytical snapshots |
| Identity | General entity and observation authoring, namespaces/leading zeros, validated source anchors/excerpts, reviewed-value comparison, keep-separate/defer decisions, merges/reversals and correction history | Calibrated Splink linkage, grouped identity views, bulk review and advanced comparison tools |
| Local discovery | Selected-host direct HTTPS collection, robots rules, DNS pinning, redirect checks, request/hop/time bounds, source retention, distinct policy/transport/quota/no-content outcomes | Durable frontier/resume, cancellation, source-specific adapters, browser capture, full broad-web coverage |
| Corpus search | Real Lucene engine and Rust supervisor; Boolean, phrase, proximity, fuzzy and fielded engine tests | Cross-platform packaged runtime and isolated worker verification; large manifests and pagination |
| Geography | Local MapLibre coordinate view; geodesic/uncertainty domain rules | Address packs, branch review UI, historical candidate authoring, regional basemaps |
| Assessment | Hypotheses/alternatives in demo, cited findings, immutable self-contained HTML snapshots, JSON transaction export | DOCX, CSV/Parquet/graph exports; report assembly and exhibit controls |
| Java engines | Java 21 adapter builds; Tika text parse and Lucene query verification; hostile Seatbelt development probe | Confined PDF/OCR compatibility, signed helpers, Windows AppContainer, resource exhaustion tests |
| Python engines | DuckDB exact totals with Parquet drillthrough, NetworkX reviewed paths, spaCy phrase candidates | Packaged Python/native libraries; Spatial extension; calibrated Splink; app integration and confinement |
| Distribution | All three native targets compile in source CI; Apple Silicon development app with Java/Lucene staging | Complete runtime inventory on all targets; Windows Fixed WebView2; offline clean-machine tests; signing/notarization |
| Performance | Small synthetic tests only | Required 16 GB / 100,000 transactions / 10,000 pages benchmark and measured p95 |

## Scope clarification

The implementation request was clarified to prohibit external search providers. Direct public website collection and application-owned local indexing are authorised. There is no provider-selection dependency or API-key setup. The broader open-web coverage requirement is still unproven; a small collected corpus does not satisfy it.

## Gate policy

The release checker fails while any required gate is unpassed. A successful source build, local engine test or development sandbox probe must not be relabelled as a complete approved release. Development `.app` builds are not signed/notarized product releases.
