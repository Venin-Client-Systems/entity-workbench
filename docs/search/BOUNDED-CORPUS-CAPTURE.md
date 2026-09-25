# Bounded legacy Search corpus capture

Search now captures the revision and complete legacy text corpus in one SQLite read
transaction. The revision read pins the snapshot before preparing the ordered evidence
query. A writer on a second connection cannot substitute later rows under the earlier
revision. The read transaction ends before engine execution. Coordinator Search still
holds its existing workspace mutex and workspace execution ownership throughout capture,
indexing, querying and completion. The Search execution permit is created after successful
capture; an ordinary capture refusal does not imply an unverified worker. Shutdown,
quarantine, persisted intent and cleanup ownership are unchanged.

## Admission and retained memory

The private capture admits at most 100,000 evidence rows, 17 MiB of raw JSON per row,
and 64 MiB of aggregate raw evidence JSON. These are explicit development capture
ceilings, not pagination or permission to omit evidence. Exceeding any ceiling refuses
the entire Search before cache creation, marker mutation or executor entry. Existing
large legacy metadata can therefore be refused even if its eventual text corpus would
be small.

Each SQLite key/body is borrowed through `ValueRef`. Storage types, 64-byte key length,
raw row and checked aggregate sizes, and UTF-8 are checked before owned JSON decoding.
The shared Evidence decoder then verifies the complete typed metadata shape and
canonical lookup key = body ID = lowercase SHA-256, plus the existing 16 MiB original
byte-reference ceiling. At most one admitted full Evidence is decoded at a time.
Acquisition arrays and other metadata are dropped after that row. This is not an
original-file integrity check: capture does not open or rehash original files.

A capped JSON writer refuses each append before growing its vector. It streams the
same `id`, `name`, `text` documents in canonical sequence order, preserving the prior
manifest's object-key order, JSON escaping and numeric revision. The complete encoded
manifest remains limited to 16 MiB and 10,000 documents. `Some("")` remains an indexed
document; `None` does not. All admitted evidence IDs, including metadata-only rows,
remain in the bounded known-ID set, preserving the existing result authorization.
The corpus fields are private, have no deserializer or clone implementation, and a
builder which suffered an admission failure cannot produce a corpus.

The existing public Rust `Runtime::search` entry point uses the same bounded manifest
builder for its already-owned Evidence slice. Canonical store Search passes the opaque
capture directly instead of assembling a second full Evidence array. Queries retain
their existing 1,024 UTF-8-byte ceiling; result bytes, 100-hit limit, finite score checks,
revision validation and ranked order remain unchanged. Java bytes, public Search DTOs,
command/schema versions, index marker format and the per-worker 30-second limits do
not change.

These ceilings bound Rust input copying and manifest construction. They do not bound
SQLite's internal page reads, ordering work or memory, claim constant-cost scanning,
or measure end-to-end memory/performance. One decoded row, SQLite allocations, the
manifest and known-ID set can coexist. An external canonical writer may commit after
capture; the response names its captured revision and does not claim to be the newest
state when delivered. This increment does not add off-lock execution, Search cancellation,
incremental indexing, query receipts or a corpus-fingerprint cache marker.

## Acquisition-purpose limitation

Eligibility deliberately remains the legacy rule: each canonical Evidence with
`text: Some` enters the global corpus once. Evidence is content-addressed and can have
multiple acquisitions. Neither a shared hash nor a global text field proves that an
access-review acquisition is eligible content. A later independently authorized content
acquisition can legitimately populate the same row while access-review ancestry remains
separate. This change does not establish acquisition-safe or run-scoped indexing.
The private fresh-workspace access experiment remains production-inaccessible; future
general activation still requires an explicit acquisition-purpose policy. No new
content-admission records or speculative collection policy are introduced here.

## Source verification

Synthetic tests coordinate an actual second canonical Workspace writer in WAL mode,
pin byte-identical old revision/manifest and require the next capture to see the new
revision. Normal application journal configuration is unchanged. Other tests cover
borrowed invalid storage/UTF-8/JSON, key/body/digest and metadata corruption, exact row,
aggregate, document and encoded-byte ceilings, early query refusal, unchanged existing
cache files, and no executor entry on failure. Whole canonical table/schema/sequence
digests and original bytes are conserved on successful and rejected captures. A fixed
synthetic two-stage callback verifies actual staged bytes against the old manifest and
query encoding without invoking Java. Existing Search ownership, shutdown, uncertainty
and cleanup regressions remain applicable.

The fixed confinement source list includes the new engine corpus module. The coordinator
Search runner already inventories tracked Rust source recursively. Historical native
receipts remain bound to their original sources; this source increment has no new
native/runtime proof or release claim.
