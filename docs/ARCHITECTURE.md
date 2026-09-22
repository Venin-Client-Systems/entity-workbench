# Architecture and data ownership

```mermaid
flowchart LR
  Analyst --> UI[Bundled React interface]
  UI -->|Typed Tauri commands| Rust[Rust domain and coordinator]
  Rust --> DB[(Canonical SQLite)]
  Rust --> Originals[Content-addressed originals]
  Rust --> Snapshots[Immutable report snapshots]
  Rust -->|Explicit selected URLs| Broker[Rust HTTPS broker]
  Broker -->|Pinned public IP + verified TLS| Web[Selected public websites]
  Rust -->|Derived text only| Search[Separate Java Lucene process]
  Search --> Index[Rebuildable revision-labelled index]
  Rust -. gated .-> Parser[Disposable Java parsing process]
  Rust -. gated .-> Analysis[Packaged Python analysis process]
  Analysis -.-> Parquet[Rebuildable Arrow / Parquet / DuckDB]
```

Only Rust writes canonical records. Every mutation runs in an immediate SQLite transaction with an incremented workspace revision and an event record. Previous record bodies are retained in history. Analyst corrections require a reason and an expected workspace revision. A stale decision is rejected instead of overwriting newer work.

Originals are named by SHA-256. Imported display names never become filesystem paths. Existing content is deduplicated without deleting repeated transaction rows. Corrections modify canonical transaction records, preserve the original bytes, invalidate current findings and clear affected transfer matches. Snapshot HTML and its digest are retained independently of future corrections.

The current schema uses typed JSON records in a constrained SQLite table, with explicit Rust validation at mutation boundaries. SQL is fixed application code and never accepted from the interface or a recipe. This is an initial schema; normalized analytical access, migrations, pagination and high-volume snapshots remain work items.

Local Lucene indexes are disposable, contain derived text and identify their source workspace revision. The Rust supervisor validates returned IDs and revision before showing results. The native application selects the runtime from bundled resources; the UI cannot choose an executable or classpath.

Workers receive one versioned JSON request and files in a job area. The parser and search adapter are separate Java entry points. The app enables only the macOS experimental search supervisor; other worker integration remains gated. A developer executing a worker directly is running an engine test, not proving isolation.

The UI never renders collected HTML. React escapes source text. Graph labels and map popups use text APIs. The application CSP denies remote scripts, frames, objects and external webview connections. The Rust broker is the only application HTTP path. MapLibre's worker is bundled locally and remote tile requests are refused.
