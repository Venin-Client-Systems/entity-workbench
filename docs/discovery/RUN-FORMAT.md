# Discovery evidence format v1

The executable contract is `scripts/discovery_benchmark.py`. Unknown or missing members, duplicate JSON members and non-finite numbers are rejected. JSON inputs are bounded to 8 MiB. Strings are bounded and contain no control characters; dates use second-resolution UTC `YYYY-MM-DDTHH:MM:SSZ`. IDs use lower-case letters, digits and hyphens, at most 80 characters. SHA-256 values are 64 lower-case hex digits. The fixture generator provides a complete **synthetic-only** example; never relabel that file as a live run.

## Run object

| Member | Required value |
| --- | --- |
| `schema_version` | Integer `1`, not a Boolean. |
| `benchmark_sha256` | Hash of the exact benchmark file bytes used for this campaign. |
| `run_id` | Stable identifier for this campaign. |
| `mode` | `live` for public-reference benchmark or `synthetic` for fictional fixture benchmark; these cannot mix. |
| `started_at`, `ended_at` | Enclose every task; follow the freeze; span at most 14 days. |
| `app_revision` | Exact 40-digit lower-case hexadecimal Git revision. |
| `app_version` | Pinned application build/version description, at most 128 characters. |
| `platform` | `windows-x86_64`, `macos-aarch64` or `macos-x86_64`. One platform per campaign. |
| `runner_id` | Sanitised acquisition operator identifier. |
| `corpus_reset_artifact` | Artifact ID for initial campaign setup and empty-corpus receipt. |
| `artifacts` | At most 1,800 artifact declarations, described below. |
| `results` | At most 30 unique known task results. Missing tasks remain explicitly not run. |

This format contains public benchmark metadata and sanitised operator IDs only. Private investigations cannot be used as benchmark input. A live manifest still needs review before public export; do not include personal machine paths, credentials or account identity.

## Artifact declarations

Each artifact has exactly `id`, `kind`, `path` and `sha256`. `kind` is `original`, `access_review`, `local_search`, `ui_capture` or `corpus_reset`. Paths are relative to the supplied evidence directory, portable, case-distinct and at most 12 components deep. Each path is unique. Absolute/traversal paths, symlinks, reparse points, hardlinks, non-regular files, hash mismatches, files over 64 MiB and more than 2 GiB total evidence are rejected. Evidence directories must remain immutable while scoring.

The scorer verifies file integrity and reference kinds; the separate reviewer verifies contents. The underlying receipts must contain:

| Kind | Contents that the reviewer must inspect |
| --- | --- |
| `corpus_reset` | Actual application/job/corpus IDs, revision, date and proof that no prior documents/index entries/cache were available for this task. Each task has its own reset receipt. |
| `access_review` | The access/disclosure record in the benchmark protocol: robots/terms URLs, dates and hashes, allow/deny decision and reasons, effective limits, retention/display/export conditions and actual sent header configuration. |
| `original` | The retained response bytes to which request hash/source anchors refer. Do not execute them. Their acquisition URL and time are in the manifest. |
| `local_search` | Actual application query, corpus/workspace revision, result IDs/ranks/source anchors and result status, including empty/error/blocked states. The query matches the frozen task and stays local. |
| `ui_capture` | Actual application view or recording showing the task, results and relevant source chain, with revision/job linkage. Sanitise host paths and other incidental private UI before export. |

## Task result

A not-run placeholder has only `task_id` and `status: "not_run"`. An omitted result means the same measurement state. Every attempted task has exactly:

- `task_id`, `status`, `started_at`, `ended_at`, `stop_reason`.
- `effective_limits`: exact keys `max_hops` (0–2), `max_requests` (0–50), `max_seconds` (1–600). Lower publisher quotas and analyst choices apply; no field can exceed the frozen ceiling.
- `corpus_reset_artifact`, `access_review_artifact`, `local_search_artifact`, `ui_capture_artifact`.
- `requests`: all attempted requests in serial chronological order, including access reviews, redirects, retries and failures. No calls are excluded from cost to improve a score.
- `labels`: the independent review object, or `null` while awaiting review.

Attempt status is one of `successful`, `no_results`, `blocked`, `quota_exhausted` or `failed`. A successful attempt may still receive irrelevant labels. Only `successful` can receive a positive relevance label. The receipt and stop reason distinguish blocked access, exhausted limits, technical failure and a completed search without results. The current scorer validates declared states; the reviewer must check that they describe the actual application outcome.

## Request trace

Each request has exactly `url`, `method`, `purpose`, `parent_request`, `hop`, `started_at`, `ended_at`, `outcome`, `http_status` and `original_artifact`.

`url` is a plain HTTPS URL in the task's selected publisher scope. An out-of-scope destination may appear only with outcome `blocked`, retaining a denied redirect without credit or a fetched original. It contains no credentials, alternate port, query or fragment. A rejected candidate containing unsafe/private URL material belongs in a sanitised access-review receipt, not the public manifest. `method` is GET or HEAD. `purpose` and `parent_request` establish a forward-only graph:

- `access_review`: robots/terms checks; parent is null and hop is zero. These never become scored discovery sources.
- `seed`: the exact frozen seed URL; parent is null and hop is zero.
- `link`: parent is the zero-based index of an earlier successful GET response. Hop is parent hop plus one. The acquired parent must actually contain the followed link; the reviewer inspects the source anchor.
- `redirect`: parent is an earlier fetched 3xx response. Hop stays unchanged. Its actual Location header and destination must match the trace; the reviewer checks them.

Request timestamps are serial and lie inside the task. `outcome` is `fetched`, `blocked` or `failed`. A fetched response has an HTTP status (100–599) and an `original` artifact, including non-success responses. Unfetched attempts have a null original; HTTP status may be null. A result source must be a fetched GET with a 2xx status and a content purpose. All actual redirects consume requests; a forbidden redirect is not followed.

## Independent labels

Labels contain exactly `reviewer_id`, `reviewed_at`, `relevant_result`, `useful_expansion`, `rationale`, `source_requests` and `chain`. The reviewer differs from the runner and reviews after collection. The two labels are Booleans. The rationale refers to the frozen criteria and explains support, conflict, irrelevance or failure. `source_requests` is a unique list of zero-based request indexes.

Positive relevance requires at least one successful source. Irrelevant labels have an empty source list. Expansion requires relevance and a `chain` with exactly:

- `identifier`: newly discovered identifier, not supplied in the task input.
- `identifier_anchor`: reproducible location in the original source: text range, element path or equivalent, with enough detail for a reviewer to locate it.
- `source_request`: source request index, included in `source_requests`.
- `lead_request`: later distinct source index, also included in `source_requests`. Its URL differs and its hop is later; its parent chain descends from the identifier source.
- `lead_anchor`: reproducible location showing why the further source is a relevant lead.

Without a useful expansion, `chain` is null. The scorer verifies ordering and references. The independent reviewer verifies that the identifier is new, anchors are accurate, pages are not merely copies and the later lead contributes useful information. Distinct strings and files alone cannot prove independent evidence.

## Output and release use

The report binds its benchmark and run digests, retains a denominator of 30, and distinguishes `not_run`, `incomplete` and `complete`. Incomplete percentages remain null. It reports measured/labelled/relevant/expansion counts, per-status counts, missing task/label IDs and request/time cost. `thresholds_met` requires all 30 tasks to be attempted and independently labelled, at least 18 relevant and at least nine useful expansions.

`live_measurement_eligible` means only that a structurally valid **declared live** campaign meets these numerical conditions. It does not authenticate the application, reviewer or acquisition receipts. `release_gate_decision` is always `not_evaluated`. Any release-evidence integration must inspect actual application results, access decisions, source semantics and this report; neither an exit code nor this eligibility bit independently passes EW-05.
