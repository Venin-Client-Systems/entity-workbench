# Selected-target review history

Command v14 adds `PageReviewDecisions { request, expected_revision }`, available
through canonical and desktop dispatch without a whole-workspace response.
Existing commands, review-decision records, schemas and full workspace views are
unchanged. This reader does not change review state or remove the default
workspace's decision array.

## Request and response

The strict request contains `target_id`, `page_size` and an optional `cursor`.
The target is an opaque identifier containing 1–256 UTF-8 bytes without control
characters. It is neither a path nor necessarily a UUID. Page size is 1–50 and a
supplied cursor is nonempty and at most 2,048 bytes. Unknown fields are rejected.
These are domain limits after command decoding, not a transport-wide input cap.

The response contains:

| Field | Meaning |
| --- | --- |
| `schema_version` | Response version 1. |
| `workspace_revision` | The exact required revision read by this snapshot. |
| `target_id` | The selected canonical identifier. |
| `resolved_target_kind` | One of the supported current target kinds below. |
| `scope_count` | All generic review-decision records matching this target, before pagination. |
| `query_sha256` | Continuation identity for target, resolved kind, revision and page size. |
| `rows` | Complete existing `ReviewDecision` records in ascending canonical sequence. |
| `next_cursor` | Continuation after the last returned row, or null when finished. |

Each row retains `id`, `target_id`, `state`, `reason` and `at`. States remain
`pending`, `accepted`, `rejected` and `deferred`. Reasons and dates are verbatim
legacy strings: this read neither normalizes them nor applies today's writer
limits or timestamp parsing. Consumers must render them as inert text. Insertion
sequence determines order even when timestamps are equal or nonstandard.

## Supported targets and integrity

Resolution covers precisely the current generic decision writers:
`entity`, `observation`, `hypothesis`, `finding`, `transaction`, `processing_job`
and `merge`. A reversed merge's decision targets its merge record; a processing
retry targets its job. Other decision families retain their separate contracts.
An evidence citation alone is not a supported generic review target.

The identifier must resolve to exactly one supported current record. Missing,
unsupported, ambiguously shared identifiers and a mismatched canonical target
body identity fail explicitly. SQLite checks the target's body identity without
copying a potentially large target body into Rust. This does not validate every
business field in that target. A valid target with no history returns count zero,
an empty row array and no continuation.

Every returned decision undergoes strict typed decoding and must have a nonempty
body ID matching its canonical key and a target ID matching the request. A bad
returned row fails the entire page. The count includes all matching decision
records; it is not an assertion that unreturned history has been fully validated.
This endpoint does not rehash original evidence or validate source anchors.

## Snapshot, cursor and size boundaries

One SQLite read transaction covers revision, target resolution, scope count,
cursor position, size metadata and retained bodies. A changed expected revision
returns a conflict; refresh and restart history. Concurrent canonical writes do
not mix a new count or new rows with the old response revision.

The strict versioned cursor is hex-encoded JSON binding canonical sequence to
the target, resolved kind, revision and page size. Its position must identify a
real decision in the selected scope. It provides continuation consistency, not
authentication or a tamper-proof token. Reusing it for another query or revision
fails. There is no arbitrary sort expression or supplied SQL.

Before decoding any requested row, the reader checks the complete requested
page's stored JSON-body lengths against a total 1 MiB budget. An individual
oversized body or an oversized aggregate rejects the whole page; no partial
prefix, truncation or skipped row is returned. A smaller page size can retrieve
large legacy records that individually fit. The next row is used only to detect
continuation and is validated when requested on the next page.

This bounds retained bodies, not the complete serialized response or all process
memory. SQLite may scan larger history and parse JSON inside its own engine;
this change adds no database index or performance claim. The full default
workspace still contains its decision array until a separate projection change.

## Verification

Eight Rust regressions cover all seven actual writer target kinds, empty history,
57-row pagination across several page sizes with timestamp ties, verbatim legacy
strings, unsupported and ambiguous targets, corrupt identities and typed rows,
stale and cross-query cursors, complete size preflight and single-row overflow,
hostile requests and unchanged read-only dispatch, and a real second canonical
writer committing between revision and subsequent reads while the snapshot stays
consistent. Historical schema files must remain byte-identical when v14 and the
two new version-1 page schemas are generated. This backend slice does not pass a
platform, installation, security or complete-release gate.
