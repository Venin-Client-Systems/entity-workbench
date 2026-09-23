# Build, local data and recovery

The application manages its workspace under the operating system's app-local data directory for `org.entityworkbench.desktop`, in `workspaces/default`. The directory contains `workspace.db`, `originals/`, `indexes/`, `scratch/`, `backups/` and `exports/`. The app does not encrypt these files. OS/storage encryption must also cover backups, exports, caches and swap.

Use **Back up** to make a consistent database copy and copy every referenced original after checksum verification. The Rust `Workspace::restore` API restores into a new destination, checks evidence hashes and refuses an existing destination or unsupported schema. The restore UI is not yet implemented. Keep the original backup unchanged and test recovery before relying on it.

Newer schemas are refused. Opening a version 1 or 2 workspace creates a consistent backup of the database and every referenced original, then transactionally advances it to version 3 and records a revision/event. Version 2 introduced mapped CSV dialects; version 3 adds finding/question links and explicit finding-review semantics. Existing findings reopen for review during upgrade, because earlier creation did not require a separate review decision. Previously saved report HTML remains byte-identical.

If backup verification or the migration fails, opening fails and the migration transaction rolls back. Failure-injection tests verify version/revision and finding-status rollback, with referenced evidence retained in the pre-upgrade backup. The backup manifest records the actual source schema. Restoring a version 1 or 2 backup into a new destination applies the same guarded upgrade. Keep backups if an older application version is needed. Abrupt process interruption and further migration paths remain open.

## Dependencies and build commands

- Rust dependencies: `cargo test -p workbench-core --locked`; `cargo clippy -p workbench-core --all-targets -- -D warnings`.
- UI: `npm ci`; `npm run build`; `npx playwright install chromium`; `npm run test:ui`.
- Java: bootstrap Maven and Java using the README commands, build the pinned POM, run `scripts/test_java_workers.py` and the macOS confinement probe.
- Python: `uv sync --project workers/python --frozen`; `uv run --project workers/python pytest workers/python -q`.
- Schemas: `cargo run -p workbench-core --bin ew-dev -- schemas`.
- Inventory: `python3 scripts/generate_sbom.py` after resolving all dependency locks and building Java.
- Release policy consistency: `python3 scripts/release_gate.py --check-policy`.
- Actual release gate: `python3 scripts/release_gate.py` (currently fails by design).

The developer bootstraps obtain upstream components with checksums. Normal app launch does not execute them. The staged macOS bundle currently includes Java and Lucene only; Python, OCR, Chromium, Spatial and regional data are not complete release components yet.

A supported release must provide a signed Windows installer and signed/notarized macOS packages, complete checksums, upstream licence/notice files, SBOMs and tests against the actual downloadable binaries on clean offline machines. No signing credential is stored in this repository. Development app bundles must remain labelled development.

## Update procedure

There are no background update checks. A future supported update is an explicit installer action: verify the release signature/checksum, back up the workspace and referenced evidence, install the matching platform artifact, and run post-upgrade integrity checks. Roll back using the consistent backup if a migration fails. Windows Fixed WebView2 and bundled Chromium/Java/Python updates are the release maintainer's responsibility.

## Current lifecycle limits

Originals, decisions and report snapshots are retained; no automatic evidence deletion is implemented. Rebuildable index/cache directories may be removed only while the app is closed. Failed jobs and partial originals remain inspectable. Full durable job resume and scratch garbage collection remain work items; do not describe an interrupted collection as complete.

Source verification pins Rust 1.90.0 in CI, matching the locally verified compiler. Updating that pin requires rerunning the supported source-build matrix; it does not change the end-user requirement to install no developer tools.
