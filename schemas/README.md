# Versioned public data shapes

The Rust development CLI emits current schemas with `cargo run -p workbench-core --bin ew-dev -- schemas`. Rust remains the canonical validator: JSON Schema cannot replace contextual checks for exact money, source integrity, workspace revisions, selected scope, worker identity or permissions. Historical JSON schemas are retained byte-for-byte when a new version is introduced.

| Interface | Current public shape | Compatibility |
|---|---|---|
| Commands | `command.v25.schema.json` | v23 adds durable collection preview/read/control contracts; v24 adds complete typed-literal CSV export; v25 adds revision-bound account-flow analysis. Earlier versions remain published. |
| Collection | `collection-preview.v1.schema.json`, `collection-run-page.v1.schema.json`, `collection-run-inspection.v1.schema.json` | Disclosure and canonical request identity are explicit; native execution remains disabled pending verification. |
| Transaction CSV | `transaction-csv-request.v1.schema.json`, `transaction-csv-export.v1.schema.json` | Complete revision-bound selection, explicit review policy, reversible typed literals and separate format/artifact identities. |
| Workspace | `workspace.v3.schema.json` | Historical full workspace and presentation responses remain available to their existing callers. |
| Desktop refresh | `desktop-summary-response.v1.schema.json` | The desktop uses this projection plus bounded transaction, citation, report and review readers. |
| Processing jobs | `processing-job.v4.schema.json` | Earlier v1–v3 descriptions remain fixed; the current version includes retained image-region jobs. |
| Document derivatives | `extraction.v2.schema.json` | New parse publications use v2. Verified historical v1 records remain readable without rewriting their stored bytes. |
| Editable reports | `docx-snapshot.v1.schema.json` and associated page/inspection/recovery schemas | Snapshots retain a frozen document model and DOCX; recovery is distinct from metadata-only catalogue reads. |
| Native saves | `native-export-request.v2.schema.json`, `prepared-export.v2.schema.json`, `saved-export-receipt.v2.schema.json`, `discarded-export.v2.schema.json` | CSV prepared/saved envelopes use version 2; earlier artifact kinds retain version 1. Historical schemas remain unchanged. Rust controls exact staging, no-clobber saving and acknowledgement recovery. |

Image, PDF and image-region derivatives, statement mapping, analytical requests/results, source excerpts, identity comparisons and collection receipts have separate versioned shapes. A worker never acquires canonical write authority merely by returning schema-shaped data.

SQLite storage compatibility is **version 5**, separate from these public interface versions. Guarded v1–v4 upgrades create consistent recoverable backups covering the database and referenced originals/derivatives/report artifacts; unsupported newer storage is refused. See [workspace operations](../docs/OPERATIONS.md), [DOCX storage](../docs/reports/DOCX-SNAPSHOTS.md) and [extraction compatibility](../docs/processing/EXTRACTION-V2.md).

Release-tooling contracts are maintained separately from the Rust generator: `release-ledger.v2.schema.json`, `release-evidence.v1.schema.json`, `release-review.v1.schema.json` and `runtime-inventory.v1.json`. The offline release tools own their contextual checks. See [release evidence](../docs/release-evidence/README.md) for exact artifact bindings and the distinction between source CI and installed-artifact acceptance.
