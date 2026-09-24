# Ordered transaction source reads

Command v13 adds `ReadTransactionSources { request, expected_revision }` for small
source selections. It is available through both canonical and desktop dispatch
without a whole-workspace response. Existing commands and schemas are unchanged.

The request contains `rows: [{ id, expected_version }]`, from one to 25 unique
canonical transaction IDs in the desired display order. IDs are opaque text,
not UUIDs or paths: 1–256 UTF-8 bytes without control characters. A supplied
version must be positive. Unknown fields are rejected. The domain layer enforces
these limits after decoding the command; this is not a transport-wide input cap.

Analysis and period-comparison drillthrough must supply the version recorded by
the originating analysis. A null or omitted version explicitly selects the
current row at the required workspace revision, for example when resolving a
saved finding's current citation. Neither mode reads historical transaction
versions. A stale workspace revision or supplied row version returns a conflict
and requires refreshing the originating result.

One SQLite snapshot covers the revision, size metadata, complete row lookup and
source metadata. Before copying any retained row body into Rust, the complete
selection must exist and fit within a total 2 MiB UTF-8 JSON-body budget. The
response preserves the requested order, exact decimal strings, canonical
versions, review states and original source anchors. Identity and conventional
transaction validation run before publication; every distinct referenced
original is checked once per batch. No decision, report or canonical row changes.

An absent row, bad identity, invalid value, changed original, version conflict or
oversized selection fails the whole request. There is no partial success,
truncation or silent omission. Successful responses contain schema version 1,
the pinned workspace revision and the selected rows. Source anchors are retained
as recorded; this read does not accept extraction or establish recognition accuracy.

The budget bounds retained transaction bodies, not the complete serialized
envelope or process memory. SQLite may read larger storage pages, and original
verification also reads evidence metadata and source bytes. This is a foundation
for migrating current source-review screens; the current interface still uses
full transaction/review arrays and the default analysis still carries large ID
and reconciliation vectors. This API alone does not improve default refresh size.

Six regressions cover exact order/opaque IDs/shared originals/dispatch, hostile
and missing selections, stale revisions and versions, complete preflight before
body decoding, original/canonical corruption, and an actual concurrent writer
committing while the read snapshot remains pinned. At its initial integration,
194 ordinary core tests and strict Clippy pass; 21 native/runtime tests are
explicitly excluded. Independent bounded review found no actionable defect in
this source-read scope. No release gate changes.
