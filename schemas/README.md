# Versioned public data shapes

The schema generator is the Rust development CLI: `cargo run -p workbench-core --bin ew-dev -- schemas`. Canonical validation remains in Rust; JSON Schema describes the public shape but cannot replace contextual checks such as source-anchor validity, exact decimal arithmetic, workspace revision or preview binding.

The current workspace and command interfaces are `workspace.v2.schema.json` and `command.v2.schema.json`. Version 2 adds statement profile/import records and inspect/preview/import commands. Original v1 schemas remain as historical contracts and are not rewritten by the current generator. Workspace SQLite compatibility is also version 2, with the guarded v1 upgrade described in `docs/OPERATIONS.md`.

Statement mapping, sample and preview output each have a first-version schema. Worker-request, analysis-manifest, identity-comparison and source-excerpt contracts retain their existing versions. A statement import is canonical Rust work; no worker can bypass its validation or write these records directly.
