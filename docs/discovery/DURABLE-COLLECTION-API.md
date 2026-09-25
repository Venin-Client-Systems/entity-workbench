# Durable collection command contract (EW-22 / #25)

This cutover adds public preview, bounded durable-run reads and coordinator-owned controls. **Native execution remains disabled** by the compiled `NATIVE_COLLECTION_ENABLED = false` gate; no command, environment variable or input mode enables it. The normal desktop coordinator reports `native_disabled`. Standalone `ew-dev` can preview/read, but refuses controls requiring a persistent owner. The test-only coordinator runs the same commands, canonical store and joined lane with a clearly labelled synthetic executor. It does not activate a production resolver or issue public requests.

The old synchronous `CollectWeb` entry now refuses before creating a job. Its blocking broker and detached resolver thread are removed. Historical `InspectCollection`/`ExportCollection` remain unchanged for old receipt IDs. This source must be integrated with the matching Discovery UI cutover; it is not a separate working live collection release.

## Wire and disclosure

The additive command schema is `schemas/command.v23.schema.json`. All previous command and result schemas retain their bytes. Rust result types live in `collection_api.rs`.

| Action | Input | Result |
|---|---|---|
| `preview_collection` | `input` | `collection-preview.v1` |
| `queue_collection` | `input`, `preview_sha256`, canonical UUID `request_key` | `collection-run-inspection.v1` |
| `page_collection_runs` | `request: {page_size, cursor}`, `expected_revision: null \| u64` | `collection-run-page.v1` |
| `inspect_collection_run` | `job_id` | `collection-run-inspection.v1` |
| `cancel_collection` | `job_id`, `expected_generation` | `collection-run-inspection.v1` |
| `resume_collection` | `job_id`, `expected_generation` | `collection-run-inspection.v1` |
| `retry_collection_settlement` | `job_id`, `expected_generation`, `request_sequence` | `collection-run-inspection.v1` |

`input` has `urls`, `max_hops`, `max_requests`, `max_seconds`. Existing bounds remain: one to ten credential-free HTTPS URLs on port 443, at most two hops, one to 50 charged attempts and one to 600 seconds. Input and seed order are normalized through the existing URL validator. Preview performs no DNS lookup or network request. A syntactically valid preview is not a destination/access approval: actual execution must still reject unsafe observed addresses, redirects and every out-of-selection host.

`preview_sha256` hashes a closed serialized payload containing schema version, collector policy, normalized input, first-occurrence selected-host order, each host's robots URL and the exact disclosure object. The disclosure records DNS hostnames, connection metadata and selected/followed URLs; `automatic_case_contents` is false and `followed_hosts` is `selected_hosts_only`. Changing a query, seed order, limits or policy requires a new preview. No current source-access, relevance or independence approval is inferred from confirming it. The request key binds normalized input **and trusted record version/policy/mode**; a changed binding fails instead of creating a second job. There is no frontend-supplied mode or executor. An exact request-key replay may recover its already committed acknowledgement under still-held publication ownership, including quarantine, without creating work or changing the revision. Missing, mismatched, historical or corrupt bindings cannot fall through into a new queue.

## Stored versions and bounded reads

V3 keeps the existing generic `collection_run` / `collection_run_key` storage families and uses policy `direct-https-durable-v3`. `synthetic` is still explicit in the canonical record. The only accepted tuples are the historical v1/foundation/synthetic, v2/transport/synthetic, and v3/current-policy synthetic or native modes. Native v3 rejects a synthetic resolver receipt on settlement and replay. Native v3 is defined for the future reviewed activation; no production constructor admits it in this increment.

There is no storage migration. New runs do not create shadow legacy `job` records or change old job identities. The new page reader covers only durable runs, including labelled read-only v1/v2 history. Existing legacy receipt/job lists remain separate. Public controls require a matching configured **v3** lane; they cannot reinterpret or resume a v1/v2 specimen as live work. Existing private synthetic v2 regressions continue using their distinct test constructor.

Pages order by ascending canonical record sequence. Page size is 1–25. The lowercase-hex cursor is bounded to 1,024 bytes, decoded strictly and bound to the fixed kind/order, actual workspace revision, page size, last sequence and actual canonical ID. It is a consistency token, not authorization. A new revision requires restarting at the first page, even if the caller supplied a null expected revision. The whole response is limited to 2 MiB, with no truncated or partial prefix.

Each selected record is preflighted at 4 MiB before its SQLite body is copied. Inspection replays the canonical journal against verified retained original buffers and checks the cached checkpoint, request-key mapping, evidence identity and acquisition provenance. This is not an indexed-performance claim: replay may read many originals, and returned JSON size does not bound total replay I/O. Counts include the complete durable-run catalogue, while only the requested page is decoded/replayed. Summary exposes the canonical request UUID as `request_key` so a queue acknowledgement can be bound to the exact pending request as well as normalized input, collector policy and record version. It does not expose the journal, lease, raw response bytes or raw headers. Inspection exposes bounded per-request ancestry, retained lossless progress and `{evidence_id,sha256,bytes}` for complete bodies.

Current storage schema 5 remains unchanged. The existing backup snapshots the complete SQLite database and copies every canonical Evidence original, independent of collection version. Publication writes that Evidence reference in the same SQLite transaction as the run settlement. The v3 regression restores a snapshot and verifies the exact run, receipts and complete response bytes. Unreferenced files left by failed publication remain excluded. No old binary is claimed to execute or understand v3; a pre-v3 state-machine reader must reject the unknown version. Historical generic schema-compatible backup preserves these records and Evidence objects without interpreting their policy; no historical job recovery code scans `collection_run`. The earlier binary compatibility observation remains historical evidence, not a new v3 execution test.

## Ownership, controls and recovery

Public commands acquire workspace then lane locks. Canonical inspection and ephemeral status are sampled while both are held. A lane status applies only to its exact run ID and generation; a stale retained status cannot enable another run's controls. Every mutation independently checks the current state again, so displayed flags are advisory and never authorize a later stale action.

`availability` is one of `native_disabled`, `standalone_unavailable`, `synthetic_fixture`, `ready`, `recovery_required`, `execution_unavailable`, `stopping`. `native_execution_enabled` remains false, including synthetic tests. Production `ready` is unreachable while the compiled gate is false. A contained failed/stopped lane reports `execution_unavailable`, so it cannot admit work without an executor. `run.state` is the durable state; `execution.phase` separately describes ephemeral work or publication. A failed publication may leave durable `running` plus `settlement_pending` and an irrevocably charged reserved request.

`can_cancel` / `can_resume` require the current enabled v3 lane. `can_retry_settlement` is different: it permits **publication only** for the exact owned pending response under the same still-locked coordinator lifetime. It can remain true under quarantine, but cannot start a resolver or repeat an HTTP attempt. The three explicit retry ceiling remains; no hot retry was added. Released ownership, changed ID/generation/sequence, already settled data or exhausted retries refuse the action. Stopping/drop joins both lanes before releasing ordinary ownership; unknown completion preserves quarantine and lock retention.

Queue publishes before any claim. Claim and every charged reservation happen under the workspace mutex; transport runs outside it. First start fixes the deadline. Crash recovery preserves counts, first deadline and unresolved charge as `interrupted_unknown`; it never invents a response or retries an uncertain robots access. An explicit matching-generation resume increments the lease generation and visits only remaining eligible frontier entries. Cancellation after a complete body retains the original but does not promote text. Windows provider uncertainty remains `recovery_required`; it is not a successful cancellation or proof the shared OS provider stopped. Offline reads remain possible.

The existing raw wall-time versus journal-anchor rules remain: observed backward timestamps are preserved, not clamped. Monotonic time is private to one process segment and is not continued across restart. Destination validation, observed-candidate pinning, no proxy/automatic redirects/retries, verified HTTPS, incremental body limits and same-host robots/redirect rules continue in the shared transport/state machine.

## Source meaning and next activation step

Complete response originals are retained locally and content-addressed. Static text is unreviewed and creates no accepted observations or page/word anchors. Identical byte bodies share Evidence metadata; a later supported media interpretation can replace that Evidence's current text. This increment deliberately does **not** call that mutable text an immutable web derivative. Original response bytes and each acquisition remain authoritative. Copied sources do not become independent evidence merely because URLs or job IDs differ.

Native activation still needs the matching industrial UI, explicit root review of the joined cancellation/recovery path, a reviewed harmless HTTPS source observation and cross-platform execution evidence. This increment executes synthetic loopback TLS only. Earlier Mac/Windows DNS observations are separate, source-bound evidence and do not prove a live HTTPS campaign or a Windows 11 installed application. No discovery benchmark or complete-release gate passes here.

## Verification record

The Mac development host passed **451 ordinary Rust tests** (375 library, 76 integration), with 23 specialized/native cases ignored. Thirteen new focused API tests cover disclosure binding, unchanged historical reads, cursor revisions/order/size, malformed records, standalone and old-live refusal, actual loopback TLS retention/backup/restore, uncharged queued cancellation, complete-response cancellation, fixed-deadline crash resume, exact acknowledgement recovery under quarantine, publication-only retry and refusal after a contained executor exit. Existing document/collection shutdown, unknown-completion, stale-owner and receipt tests also pass. Strict debug and release Clippy pass for all core targets; the previous 85 schema files (84 JSON) remain byte-identical, with five additive files.

[The source and log digest record](verification/durable-collection-api.json) preserves the initial zero-selection filters and two failed test assumptions separately from the final passing runs. The underlying failed/successful logs remain in ignored local artifacts. This is synthetic source-level evidence, not native activation, an installed-platform result or a release pass.
