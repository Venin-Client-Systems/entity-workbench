# Versioned public data shapes

The schema generator is the Rust development CLI: `cargo run -p workbench-core --bin ew-dev -- schemas`. Canonical validation remains in Rust; JSON Schema describes the public shape but cannot replace contextual checks such as source-anchor validity, exact decimal arithmetic, workspace revision or preview binding.

The current workspace and command interfaces are `workspace.v3.schema.json` and `command.v3.schema.json`. Version 2 added statement profile/import records and inspect/preview/import commands. Version 3 adds question authoring, linked questions on findings, finding edits and explicit review. Original v1/v2 schemas remain historical contracts and are not rewritten by the current generator. Workspace SQLite compatibility is also version 3, with the guarded v1/v2 upgrade described in `docs/OPERATIONS.md`.

Statement mapping, sample and preview output each have a first-version schema. Worker-request, analysis-manifest, identity-comparison and source-excerpt contracts retain their existing versions. A statement import is canonical Rust work; no worker can bypass its validation or write these records directly.

Release-tooling contracts are maintained separately from the Rust generator: `release-ledger.v2.schema.json`, `release-evidence.v1.schema.json`, `release-review.v1.schema.json` and the runtime inventory contract. The offline Python tools own their contextual validation. See [release evidence](../docs/release-evidence/README.md) for artifact/hash bindings, required OS versions, review receipts and the distinction between source CI and installed-artifact acceptance.
