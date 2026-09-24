# Canonical collection acquisition receipts v1

The Rust direct HTTPS broker now produces an ordered acquisition trace. Rust storage retains complete response bodies and publishes the final job and its validated receipt in one SQLite transaction. This is a foundation for EW-05, not a completed discovery benchmark or an approval to collect a website.

## What the receipt establishes

Each charged GET attempt records its sequence, URL, request purpose, parent request, hop, UTC start/end timestamps and outcome. Complete responses record status, normalized media type, safe resolved redirect destination, exact body size and SHA-256, and the canonical evidence identifier. The receipt identifies the job, selected URLs, collector policy, application version, configured bounds, start/retained workspace revisions and synthetic/live mode. It records monotonic elapsed milliseconds and explicitly flags an observed time-limit overrun; an overrun cannot claim a successful terminal state. The broker still stops new requests at its request/hop/time limits.

The mode is assigned by the actual transport. The production broker always uses `live`; scripted unit transports use `synthetic`. A request refused before reservation produces no phantom charged attempt. DNS, TLS and policy failures after reservation consume the budget but claim no HTTP response. Request timestamps use a UTC wall-clock anchor advanced by monotonic time, at second resolution. Validation rejects future timestamps beyond five seconds of local clock tolerance and inconsistent chronology/duration/accounting.

Complete empty responses, robots rules, redirects, unsupported formats, invalid UTF-8 and HTTP errors are retained as content-addressed originals. They do not automatically become searchable documents. Only accepted HTTP 200 UTF-8 HTML/plain-text content gets a static text derivative; source markup is never executed. Identical bytes share one original and origin group, while each request retains its own acquisition context. Retaining an error or robots response never erases an existing legitimate text derivative. A later explicit document import can process an acquisition-only original without losing its acquisition history.

Interrupted, oversized or failed body reads retain known status/type metadata where available, with `incomplete` outcome. They receive no complete-original hash, size or evidence binding. The original bytes of such partial streams are not accepted. `retention_complete` means all complete received bodies were retained, not that every attempted download succeeded. An exported receipt can describe a failed or incomplete download without claiming its partial bytes as an original. Storage failure makes retention incomplete and the job failed; earlier verified originals remain available, and complete-bundle export is refused. If final database publication fails, the job remains unfinished and no completed receipt is published.

## Local inspection and export

The additive [command v4](../../schemas/command.v4.schema.json) operations use the same Rust dispatcher as the desktop application. Example requests for the development harness:

```json
{"action":"inspect_collection","job_id":"REPLACE_WITH_JOB_UUID"}
```

```json
{"action":"export_collection","job_id":"REPLACE_WITH_JOB_UUID"}
```

Inspection returns the validated [receipt v1](../../schemas/collection-receipt.v1.schema.json), after checking its canonical job and every referenced original. Legacy and interrupted jobs explicitly report that a receipt is unavailable; acquisition history is never used to invent a missing request trace. These commands are ready for the designed collection-review UI; this increment adds no UI controls.

Export creates a new UUID directory under the workspace's `exports` directory. A temporary directory is populated and checked before publication. `manifest.json` embeds the receipt and snapshot revision, plus a path/hash/size manifest for deduplicated `originals/<sha256>.bin` files. Raw bodies remain data and must not be executed. The export record stores a relative manifest path and exact manifest hash. Later workspace changes or another export do not rewrite an earlier snapshot. Failed publication cleans only directories allocated by that attempt. Unix links, dangling links and Windows reparse paths are rejected. As with the workspace, this assumes an application-owned directory without a hostile same-user process racing filesystem operations; it is not cryptographic signing or filesystem immutability against the owner.

Response originals are included in the existing consistent backup/restore path, even when they have no text derivative. Export bundles themselves follow the existing export lifecycle; a workspace backup contains the canonical records and evidence, not copies of previous export directories.

## Compatibility and limits

The canonical SQLite schema and `WorkspaceView` remain version 3. Receipts and export records are additive kinds in the existing generic record/history tables. No database migration runs. Historical command v3 and workspace v3 schema bytes are preserved; only command v4 adds the two operations. The receipt schema is separately versioned and rejects unknown fields.

The collector policy is still `direct-https-v1`: one to ten explicitly selected credential-free HTTPS seeds, exact selected hosts, normal HTTPS port, up to two hops, 50 charged attempts, ten minutes and 2 MiB per complete response. Selected URL query strings are preserved; fragments are removed by URL validation. Cookies, authorization headers and raw response header blocks are not recorded. Unsafe/credential-bearing Location values are omitted from parsed redirect metadata, although the original body remains unmodified. Off-host safe redirect destinations can be recorded as denied decisions without being fetched.

This differs from benchmark v1's one frozen query-free seed and publisher scope. No automatic conversion masks those differences. The receipt is not terms approval, cold-corpus proof, local-search/UI evidence, independently reviewed relevance, exhaustive web coverage, a source signature or proof of a particular Git commit. Application version alone does not establish build identity. Benchmark integration must separately bind its source revision and retain those other records. Interrupted-job checkpoints and durable frontier/resume remain EW-22 work; this version finalizes a trace after a bounded collection attempt.
