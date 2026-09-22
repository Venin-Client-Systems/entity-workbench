# Development verification — 2026-09-22

These observations describe the initial implementation, not completion of the full plan.

| Check | Observed result | Scope |
|---|---|---|
| Rust domain/workspace suite | 29 tests passed (27 integration, 2 unit) | Exact decimals/dates; atomic import; duplicate retention; review conflicts; correction propagation; transfer exclusion; currencies; reversible merges; backup/restore; corrupted originals; newer schemas; symlink paths; escaped reports; uncertainty; public-IP policy; worker-message limits; discovery budget; static HTML extraction; missing balances; acquisition provenance; general identity authoring; source-anchor validation/excerpts; identity decision recovery; assertion invalidation |
| Rust static analysis | Clippy passed with warnings denied | Core and core test targets |
| UI production build | TypeScript and Vite passed | Bundled React/Cytoscape/ECharts/MapLibre assets, including map worker |
| Browser workflows | Two passed against real Rust commands | Seed, transaction correction/acceptance, merge/reversal, local map, report snapshot and persistence; no external browser requests; no uncaught page errors. Empty-workspace authoring adds two namesakes, reviews cited observations, records separate/deferred decisions, corrects an observation and checks persistence |
| Accessibility and keyboard | Eight tested section states have zero axe violations; keyboard and minimum-width checks passed | Native modal focus, nested source inspection, Escape, return to opener, 960×640 layout; remaining manual checks in `design/accessibility-results.json` |
| Native desktop build | macOS Apple Silicon executable and development app bundle built | Not notarized; not a complete runtime distribution |
| Java engines | Tika synthetic text parse and five Lucene queries passed on Java 21.0.12.1 | Boolean, phrase, proximity, fuzzy, fielded queries; revision returned |
| Rust → confined Lucene | Phrase and fuzzy query returned expected evidence and revision; native app phrase search returned the expected synthetic source | Staged app-local Java, local index, macOS development profile |
| Hostile Java development probe | Passed after dyld profile repair | Control worker could access sentinels/network; confined worker could not; assigned job I/O still worked |
| Python engines | 3 tests passed | Exact DuckDB/Parquet totals with record IDs; reviewed NetworkX paths; spaCy phrase candidates |
| Direct website collection | One public documentation page retained with original SHA-256 and static text | `https://example.com/`, 2 requests including robots policy; no search provider; not broad-web coverage |
| Complete release gate | Unpassed | Explicit machine-readable gates in `release-gates.json` |

The Java hostile probe exercises disposable synthetic sentinels only. It does not establish confinement for Python, Tesseract, Chromium, all JVM/native libraries, descendant processes or every supported OS version.

The source CI matrix targets Windows x64 and both Mac architectures. CI source compilation is separate from clean offline installation, Windows 11 runtime acceptance, signed helper validation and downloadable artifact testing. Consult the live workflow result for the current commit; this record does not pre-claim CI success.

No performance claim is made. The required 16 GB large-corpus benchmark has not been run. A locally fast synthetic test is not a substitute.
