# Immutable DOCX snapshots

This bounded backend slice adds explicit Rust APIs for canonical editable reports.
It does not expose a desktop command, change the interface, add native DOCX saving,
or complete report assembly and exhibits. The existing HTML `SaveReport` path and
all published command, report and OCR schemas remain unchanged.

## Publication and identity

`Workspace::save_docx_snapshot(request_id, expected_revision)` requires a canonical
lowercase UUID and captures one consistent workspace view at the requested revision.
It omits earlier HTML report bodies. The existing [frozen document and renderer](DOCX.md)
validate retained records, citations and calculations; every captured original is
also verified against its exact canonical evidence identity and bytes. Totals and
balance checks continue to use `analytics::analyse`.

The new `DocxSnapshotRecord` format 1 contains the request/report ID, source revision,
creation time, template/generator versions, and two typed `ReportArtifactRef` values:
`report_document_json_v1` and `report_docx_v1`, each with a SHA-256 and byte length.
These types are separate from historical HTML reports and OCR derivative references.
Their JSON formats currently have Rust API consumers only.

Complete frozen JSON and DOCX objects are retained in the private content-addressed
file store before a single revision-guarded canonical transaction adds the two
catalog references and immutable report record. Publication advances the workspace
revision once and does not reopen findings. A reused request UUID with the same
source revision returns the same verified record without writing again, even after
later workspace changes. Reusing it for a different revision or an HTML report ID
fails. A stale first request fails rather than producing a report from new data.

Filesystem retention and the database commit are **not one atomic transaction**.
A failed or interrupted publication may leave complete unreferenced immutable files.
These files are not considered reports, are excluded from backups and are never
removed as part of publication rollback. Existing deduplicated files are not deleted
or overwritten. No automatic orphan collection or aggregate disk quota is claimed.

## Verified frozen reads

`inspect_docx_snapshot(id, expected_document_sha256, expected_docx_sha256)` returns
small metadata and the explicitly requested frozen model. `read_docx_snapshot` takes
the same identity and returns the DOCX bytes. Both pin a SQLite read snapshot,
validate bounded metadata and catalog references, rehash both files, verify original
identity and bytes, validate the frozen model, and compare the retained package with
a deterministic render of **that frozen model**. Later transaction corrections,
findings or evidence imports cannot alter the saved artifacts. No current analytical
rows are substituted into an old report.

An HTML-only record cannot acquire DOCX by rendering the current workspace.
Corruption, a missing original, a different expected digest or inconsistent model/
package binding fails explicitly. Original evidence names and text in a frozen
model are historical values; current records only establish original identity and
length. These checks are integrity checks, not signed authenticity or protection
against an actor who can consistently rewrite the entire workspace.

The current renderer/model versions are exact, deliberately narrow identifiers.
Future renderer, analytical or retained-domain changes must explicitly support or
reject historical versions; they must not silently reinterpret or regenerate them.

## File and recovery boundaries

Report objects use the same hardened private store as retained OCR files: fixed
hash-derived names, ordinary single-link files, private read-only permissions,
bounded no-follow/nonblocking reads, opened-file and final named-file identity
checks, and no-clobber retention. Windows uses deny-write/delete read sharing and
full handle IDs. These controls do not claim protection from every equivalent-user
ancestor-directory race, malware or modification after verification completes.
DOCX content remains escaped inert OOXML; it is not executed by this application.

Workspace schema **5** marks the additional report references so older schema-4
readers refuse the database. Opening schemas 1–4 first creates a consistent backup
of the prior schema and its originals/derivatives. The transactional migration
checks its version postcondition. Schema-3/4 upgrades preserve finding review state
and historical HTML byte-for-byte; the existing schema-1/2 review invalidation stays.
A newer unsupported schema is refused.

A schema-5 backup has completion manifest format **3**. Its references come from the
saved SQLite snapshot, including both retained OCR and DOCX records, regardless of
later live writes. Catalog closure must exactly match the union of those references;
identical digests deduplicate with equal byte lengths. Backups verify and copy every
referenced original and object, exclude unreferenced objects, and verify the copy.
Restore validates the schema, manifest, catalog and all referenced bytes before
publishing the canonical database name. Schema-4 completion format 2 and actual legacy schema-1–3 manifests
without a format field remain readable. Format 2 recovery points for schemas 1–3
also require completion and exact derivative references. Explicit unknown versions,
including on older schemas, are refused. A failed migration retains its prior
recovery point. A partial backup without a valid completion manifest is not usable.

## Limits and verification

The foundation limits remain explicit: 5,000 transactions, 10,000 represented
records/references, 1 MiB per text field, 16 MiB frozen JSON and 32 MiB DOCX. Exceeding
a limit fails; no rows or report sections are silently truncated. Snapshot metadata
is limited to 4 KiB before deserialization. Canonical capture still allocates the
workspace view; verification buffers bounded JSON/package bytes and rendering.
These are output/content limits, not a total RAM, disk, page-count or throughput
claim. No native renderer or new dependency is needed for application publication.

Run `cargo test --locked -p workbench-core` for publication/replay, exact money and
anchors, stale writers, catalog rollback, prepared-file/original tampering, frozen
model/package mismatch, mixed OCR/DOCX recovery, concurrent backup publication,
migration failure/recovery, bounds and historical HTML preservation. The shared
file tests cover replacement, FIFO, hard-link and platform-specific sharing limits.
No new source anchors are accepted by these APIs.

`cargo run --locked -p workbench-core --example report_docx_snapshot -- NEW_DIRECTORY`
creates only a fixed synthetic workspace, publishes through the real API, backs up,
restores and compares the exact retained package. It writes an inspection JSON,
record JSON and DOCX for developer render QA; it refuses an existing output directory.
Use the documents skill's managed renderer for every-page visual inspection. Its
actual LibreOfficeDev identity and manifest mismatch remain documented in [DOCX.md](DOCX.md).
That renderer is a development tool, not a shipped dependency or Microsoft Word pass.
