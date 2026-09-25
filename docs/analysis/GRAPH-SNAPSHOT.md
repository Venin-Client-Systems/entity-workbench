# Internal graph capture and result validation

This EW-17 increment adds a source-only Rust seam for one fixed operation,
`shortest_connection_path_v1`. It does not launch Python, add a desktop command,
write analytical results or accepted facts, change a public schema, or migrate a
workspace. Existing v1 manifests, historical exports and worker receipts remain
unchanged. The separately verified synthetic NetworkX recipe is not this API's
native integration test.

## Ownership and identity

`Workspace::capture_graph_path(expected_revision, source_id, target_id)` reads a
pinned SQLite transaction. Rust chooses the canonical records and retains a
private `CapturedGraph` handle. The handle cannot be cloned or deserialized; it
contains the actual selected record fingerprints, source provenance, graph,
workspace revision and a fresh request nonce. The caller can obtain bounded
worker-input bytes, but cannot construct a capture from worker JSON.

A fresh private UUID belongs to each open `Workspace` instance. Validation checks
that identity, so another workspace, a reopened copy of the same workspace and a
restored workspace cannot adopt the handle. This conservative process-local
identity does not claim to be a durable workspace identifier. Temporary recovery
readers also get fresh identities; no identity is stored in SQLite or backups.

`Workspace::validate_graph_path(handle, bytes)` consumes the handle, including on
failure. In a new pinned read transaction it checks the revision, rereads the
canonical selection and originals, and compares a SHA-256 over the actual
canonical JSON bodies and graph policy. The fingerprints use existing bytes;
entities, assertions and observations do not acquire invented numeric versions.
Even an out-of-band body change without a revision update is refused. A digest
supplied by a worker never selects or authenticates the canonical snapshot.

The result must echo the fixed recipe, policy, schema version, request nonce,
workspace revision, snapshot digest, NetworkX version `3.6.1` and reviewed runtime
manifest identity `4dc6fd171e842d1f9254be7fc5cb16e2e01203896403dcd9839a8aec69dad822`.
That manifest identifies the existing macOS Apple Silicon development prefix;
it does not imply a Windows or Intel Mac Python runtime. Those fields reject
accidental or malicious result substitution. Echoed fields
alone do not prove that an executable or confinement policy ran; a future native
adapter must independently enforce runtime and process identity.

The validated value is a read-only interpretation **at the captured revision**,
not publication authority. This slice performs no durable publication, so it
cannot publish stale results. A future writer must validate and write in one
owned transaction and account explicitly for any revision changes caused by
queue/claim operations. Holding this value does not authorize a later write.

## Selection and provenance

The closed connectivity policy is
`accepted_undirected_all_retained_time_v1`. Every active entity is a node. Every
accepted assertion with valid endpoints and complete accepted provenance is an
edge. Pending, rejected and deferred assertions do not create edges; their
separate counts and canonical fingerprints remain in the private snapshot.
Merged endpoints and missing/unaccepted provenance cause refusal.

Connectivity is **undirected**, as in `nx.Graph`. Both source assertion directions
are preserved privately, including every parallel assertion. A hop reports all
canonical assertion IDs for its undirected pair, with their individual predicates,
confidence, validity dates, observation IDs, source anchors, extraction quality
and source-origin groups available from the private provenance. The worker
receives only identifiers and topology, never evidence text, account contents,
source paths, SQLite access or authority to add provenance.

The initial scope supports **text anchors only**. Any accepted edge referencing
page, cell, message or capture provenance refuses the entire capture with an
explicit unsupported reason. It does not silently discard an edge, report a
partial graph as complete, or infer that unsupported provenance is invalid.
Existing canonical text-line validation is reused. Each distinct referenced
original is verified with the shared no-follow, single-link, metadata/size and
hash-bound original reader at capture and result validation.

Validity dates are checked and preserved but do not filter connectivity. Every
validated result carries this limitation:

> Undirected connectivity across accepted assertions from all retained time periods. A path may combine disjoint historical periods and does not establish a contemporaneous, directed or causal relationship.

Source independence, extraction quality and analyst confidence stay distinct.
No path computes a combined confidence score or asserts that its sources are
independent. A self-relationship can remain in the retained graph but cannot help
a nonrepeating path between the required distinct endpoints.

## Bounded protocol and semantic checks

| Input or operation | Bound / rule |
| --- | --- |
| Canonical entities / assertions | 1,000 / 5,000 rows, including unselected rows |
| Referenced observations / evidence | 10,000 / 1,000 distinct records |
| Canonical identifier | 1–128 UTF-8 bytes, trimmed, no controls |
| Canonical record / aggregate | 1 MiB / 16 MiB JSON bytes before owned deserialization |
| Distinct referenced originals | 64 MiB aggregate declared bytes, each independently verified |
| Observations per accepted assertion | 1–50 unique IDs |
| Worker request / result | 1 MiB / 128 KiB JSON bytes |
| Worker path | 2–1,000 known, nonrepeating nodes with exact endpoints |

Fixed queries count entity/assertion rows before decoding, borrow SQLite values
for byte preflight, and enforce cumulative bounds before owned record decoding.
A capped serializer also bounds JSON escaping expansion. These are finite
foundation limits, not a capacity or latency claim for a full investigation.
Original filesystem verification is outside SQLite's transaction isolation;
this reader checks each original when reading it and does not claim to prevent a
separate privileged process from altering files after validation.

The result uses closed typed JSON: unknown fields, duplicate struct fields,
malformed data, identity mismatches and oversized replies fail. The worker may
return only a path node sequence or `unreachable`. Rust independently computes
the shortest distance with bounded breadth-first search, requires each returned
hop to exist and derives all assertion provenance itself. Equally short valid
paths are allowed; a longer path, cycle, nonexistent edge or false unreachable
claim is rejected. No worker-supplied assertion, source or confidence field is
accepted.

## Verification and remaining integration

Ordinary tests use real temporary synthetic workspaces. They exercise parallel
and reversed assertions, disjoint validity intervals, rejected shortcuts,
independent unreachable checks, forged paths/identities, unsupported and broken
provenance, canonical lookup-key substitution, changed originals, stale revisions,
same-revision body changes, workspace-instance replay, byte/count limits and
read-only record/history/event invariance. A deterministic second canonical
writer commits a source review after the first connection pins its revision;
tests prove capture and validation do not mix old/new topology or provenance,
that later validation refuses the stale handle, and that recapture sees the
complete correction. Backup/restore preserves records without transferring
capture authority. These tests do not execute NetworkX or test native confinement.

Next integration requires a separately reviewed fixed Python adapter that reads
this exact input and produces the closed result, an owned cancellable analytical
job lifecycle, a revision-safe writer (if persistence is introduced), and actual
confined native tests against captured canonical inputs. Unsupported anchor types,
durable workspace/job identity, historical-time filtering, report linkage and
larger graph capacity remain separate work. No application activation or release
gate changes are implied by this source seam.

Source verification for this increment:

- `cargo test --offline --locked -p workbench-core graph_analysis --lib`: 15 passed.
- `cargo test --offline --locked -p workbench-core`: 437 library tests and 79
  integration tests passed; 29 native library tests remained ignored.
- `cargo clippy --offline --locked -p workbench-core --all-targets -- -D warnings`
  and `cargo fmt --all -- --check`: passed.

These results cover the source at this change and its synthetic fixtures on the
local development host. They are not a Windows native, bundled-installation,
NetworkX execution, performance or release-acceptance result.
