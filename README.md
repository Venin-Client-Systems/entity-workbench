# Entity Workbench

An independently branded desktop investigation workbench. Preserve sources, review observations, distinguish identities and produce cited findings with an inspectable history.

**Status: active development, not an approved investigative release.** The current macOS development application supports local text/CSV/TSV evidence, reusable statement mappings and preview, transaction review, general entity/observation authoring, reviewed identity comparison and merge/reversal, graph and coordinate views, direct website collection, local Lucene search, exact transaction patterns/period comparisons and HTML assessment snapshots. App-local development runtimes support durable document/image processing and explicit selected-page scan PDF OCR with immutable provenance review. The complete release gates have **not** passed. See [implementation status](docs/STATUS.md) and the [security review entry point](SECURITY.md).

The [three-month delivery programme](docs/delivery/THREE-MONTH-PLAN.md) runs from 23 September to a conditional 23 December 2026 target. Follow [GitHub programme #4](https://github.com/Venin-Client-Systems/entity-workbench/issues/4), the [dependency-linked issue index](docs/delivery/ISSUES.md) and the [runtime inventory contract](packaging/README.md).

No hosted search service, account, API key or generative model is used. The application collects analyst-selected public websites directly and searches its own local index. Coverage consists of collected and imported sources. It does not claim a global web index or comprehensive open-web results.

![Synthetic investigation overview](docs/overview.png)

## Run the development application

Development prerequisites are different from end-user requirements. Building from source currently requires Rust, Node/npm and native platform build tools. Building specialist engines additionally requires a JDK and Python. An approved end-user distribution must bundle all required runtimes; that distribution is still in development.

```sh
npm ci
cargo test -p workbench-core
cargo build -p workbench-core --bin ew-dev
npm run build
npm run desktop
```

For the browser-based synthetic UI harness, run `npm run dev` and open `http://127.0.0.1:1420`. This development-only loopback bridge executes the real Rust core against `artifacts/synthetic-ui-workspace`. It refuses direct web collection and cross-origin requests. It is not included in production UI assets or the desktop runtime.

For the base macOS development bundle with app-local Java and Lucene:

```sh
python3 scripts/bootstrap_maven.py
artifacts/tools/apache-maven-3.9.11/bin/mvn -f workers/java/pom.xml package
python3 scripts/bootstrap_java.py
python3 scripts/test_java_workers.py
python3 scripts/test_macos_confinement.py
python3 scripts/stage_macos_engines.py
cd desktop
../node_modules/.bin/tauri build --bundles app
```

The base commands above do not stage every currently implemented adapter. See the explicit local staging and native-test instructions for [document processing](docs/processing/JOBS.md), [image decoding/OCR](docs/security/IMAGE-WORKERS.md), [PDF rendering](docs/security/PDF-RENDER-WORKERS.md) and [English OCR](docs/security/OCR-WORKERS.md). Missing components fail visibly; there are no first-launch downloads.

These bootstrap scripts run only on the developer's machine. They are not first-run dependency installers. The current macOS prototype uses a restricted Seatbelt profile with Apple's private `dyld-support.sb` bootstrap rules. Signed helper validation and the supported OS matrix remain release requirements.

## Start an investigation

1. Load the fictional North Quay investigation in an empty workspace.
2. Open Transactions and inspect the café debit against its source row. Correct `-180.00` to `-18.00` with a reason, then accept the corrected row.
3. Review both transfer rows and explicitly match the counterpart transactions. Matching is never inferred solely from equal amounts.
4. Create or edit entities in Entities, preserving reference namespaces and leading zeros. Add observations anchored to retained text lines or CSV cells, inspect the exact excerpt and review each observation. Compare any two records; keep them separate, defer the decision or record a reversible merge. Comparison signals use accepted exact values and do not represent identity probabilities.
5. In Discovery, preview selected HTTPS seed URLs, then collect their public pages. Only selected hosts are in scope. Robots requests and redirects count against the request limit.
6. Search the imported/collected corpus in Evidence. The macOS bundle supports Boolean, phrase, proximity, fuzzy and fielded Lucene queries.
7. Save an assessment snapshot. Subsequent corrections flag current findings without changing previous exports.

All repository fixtures, screenshots and automated workflow tests are synthetic. Live network smoke tests use harmless public documentation pages and are excluded from CI.

## Statement mapping and preview

Use **Import evidence** with a UTF-8 CSV or TSV file. Choose a separator, map source columns, and explicitly select date, decimal and debit/credit interpretation. Preview validates every row before import; any invalid row blocks the whole file. Save a named mapping for later statements. All imported transactions start pending review. See [statement import formats, limits and provenance](docs/STATEMENT-IMPORT.md).

The standard example can use the suggested column mapping:

```csv
account,date,posting_date,description,amount,currency,balance
DEMO-001,2025-03-01,2025-03-02,Fictional purchase,-12.30,AUD,87.70
```

Required: `account`, `date`, `description`, `amount`, `currency`. Optional: `posting_date`, `balance`. Dates use `YYYY-MM-DD`. Amounts are exact decimal strings; negative means debit and positive means credit. Currency is a three-letter uppercase code. No currency conversion occurs. Original row descriptions and source anchors are retained. CSV and TSV mapping are implemented; XLSX and PDF/OCR statement extraction remain future work.

## Architecture and review

- React/TypeScript and Tauri provide the desktop interface.
- Rust owns the SQLite workspace, evidence store, validation, decisions, calculations and HTTP broker.
- Java parsing, Lucene search, image decoding and PDF rendering run in separate process roles. The Mac development app uses bounded experimental confinement and an app-local English OCR runtime; signed helpers and installed-platform security acceptance remain open.
- Python adapters demonstrate DuckDB/Parquet totals, NetworkX paths and spaCy phrase candidates. Splink remains blocked until a calibrated model is supplied; no untrained probability is presented as an identity decision.
- Cytoscape, ECharts and MapLibre present relationships, totals and local coordinates. There are no external map tiles.

Read [architecture](docs/ARCHITECTURE.md), [versioned schemas](schemas/), [verification](docs/VERIFICATION.md), [release gates](docs/release-gates.json) and [build/recovery guidance](docs/OPERATIONS.md).

Original project code is licensed under [Apache-2.0](LICENSE). Bundled dependencies retain their own licences. See [NOTICE](NOTICE) and [dependency inventories](sbom/).

## Product design status

The current interface is a development prototype with an industrial/technical design applied from an [editable Figma revision](https://www.figma.com/design/O50ISV0LGG8nKDcFORNMbO?node-id=6-2). See the [design handoff and rendered comparisons](docs/design/HANDOFF.md), [design brief](docs/design/BRIEF.md) and [measured accessibility results](docs/design/accessibility-results.json). Final visual approval remains open. Automated checks do not establish full accessibility conformance.
