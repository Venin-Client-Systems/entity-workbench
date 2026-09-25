# Bounded static HTML interpretation

This EW-22 / #25 source increment replaces a repeated ancestor walk in static HTML interpretation and versions the resulting refusal semantics. It does not enable normal native collection. `NATIVE_COLLECTION_ENABLED` remains false, normal coordinator startup has no collection executor, and the synchronous collector stays disabled. No public-network campaign is part of this change.

## Exact extraction and admission limits

The pinned parser remains html5ever 0.35.0 with scraper 0.24.0's unmodified tree sink. Tree-builder options remain unchanged. The tokenizer's repeated per-feed BOM stripping is replaced by explicit initial/script-resumption checks matching the old one-buffer driver, so newly introduced chunk boundaries cannot erase interior U+FEFF. The dependency was already in Cargo.lock; core now names that same locked version directly. The adapter does not implement another HTML grammar or tree builder.

Input is fed on UTF-8 boundaries in chunks of at most 512 bytes. A refusal discards the entire parser result. After a refusal, no later token is forwarded to the tree builder; the tokenizer may finish consuming its current chunk, but no next chunk or EOF repair is admitted. Script suspension is handled exactly as the pinned scraper driver handles it: scripts are never executed.

| Checked quantity | Bound and exact observation point |
| --- | --- |
| Raw UTF-8 HTML input | 2 MiB, checked before parser construction |
| Token callbacks | 65,536, including parser-error callbacks, checked before forwarding each token |
| Attributes | 128 on each emitted tag token, checked before forwarding; tokenizer attribute accumulation happens earlier |
| Pending token input | At most 16 KiB of admitted chunks since the last completed non-error token; the entire last chunk is counted conservatively. Check is before the next feed. Error callbacks do not reset this counter. |
| Weighted tree admission | 32 × 1,024 × 1,024 units; each forwarded token costs `1 + current allocated node count + emitted attribute count`. Check precedes forwarding. This is an admission metric, not an instruction count. |
| Allocated tree nodes | Refuse when the count after a tree-builder token exceeds 8,192, including orphan nodes. One token can create more than one node, so this is a post-call check, not an exact allocation ceiling. |
| Connected extraction depth | 128, including the root HTML element and text nodes, checked on entering each node after parsing |
| Normalized text | 512 KiB UTF-8, checked before each word/separator is appended |
| Selected valid links | 512 KiB of joined URL strings, checked before each link is appended |

The extraction walk visits each connected node at most twice using first-child, next-sibling and parent pointers. A hidden-element depth counter replaces scanning every text node's ancestors. Words are appended directly, avoiding both intermediate collected vectors and the full joined-text allocation.

For admitted documents, output follows the old algorithm exactly: Unicode `split_whitespace` normalization with one separating space; text beneath `script`, `style`, `svg`, `noscript` and `template` is omitted. Link selection still uses the same scraper `a[href]` selector in its allocation order, including hidden anchors and duplicates. It takes the first **1,000 anchor candidates before** URL joining and public-HTTPS policy filtering. It does not substitute the first 1,000 valid URLs or DOM traversal order. Invalid UTF-8 and unsupported media remain unsupported; `text/plain` behavior is unchanged.

The tests compare exact text/URL vectors against the frozen old algorithm for bounded ordinary and malformed specimens, including table foster parenting, formatting repair, templates, hidden links, duplicate anchors, UTF-8/entity/script chunk boundaries, and the first-1,000 rule. Separate tests exercise deep/wide/long/formatting-heavy inputs, repeated attribute errors, and exact output boundaries. Counter tests prove that refusal stops further tree-builder admission, including EOF. They do not rely on a timing threshold.

## Versioned settlement and historical replay

New current-policy runs use record version 4 and policy `direct-https-durable-html-bounded-v4`. The event, checkpoint and public DTO shapes are unchanged. The policy participates in the existing preview digest and request-key identity. An old v3 preview cannot authorize a v4 queue, and an old key cannot be reused as a current-policy run. Current queue acknowledgements require version 4 and the exact policy/input/key. V1–v3 identities and policy strings remain recognized for historical reads. Public execution controls do not resume them as v4.

On a v4 interpretation limit, the truthful complete HTTP receipt is retained, the request charge remains consumed, the complete original and its acquisition are published atomically with the journal checkpoint, and the run records a quota condition. No prefix text or links are promoted, no successful page is counted, and no follow-up frontier is taken from a rejected page. The normal state machine completes as quota-exhausted (or partial if it already retained another page). This is an interpretation refusal, not an incomplete network body or an invented transport cancellation. Unsupported media retains its existing failed interpretation behavior.

For v1–v3, admitted HTML is replayed exactly with the same text/link semantics. Over-limit historical HTML produces an explicit **read-only historical interpretation refusal**. It is never reclassified under v4, rewritten, normalized in storage, or partially accepted. A catalogue page containing such a run fails visibly rather than silently omitting it. Its canonical record, existing evidence text, receipt/acquisition history and original remain unchanged. Generic backup/restore preserves those records and referenced originals even if detailed collection replay is refused. The historical pathological test is an explicitly constructed synthetic old-policy specimen, not a claim that an old native collector was executed in this campaign.

Storage schema and commands are unchanged; no migration runs. Public JSON schemas retain their exact bytes because the existing version/policy fields are integer/string projections with unchanged shape. Older binaries are not claimed to understand or execute v4; they must reject its unknown policy/version. Generic schema-compatible backup still copies records and every referenced Evidence original without interpreting collection policy.

Evidence remains content-addressed and shared. If the same bytes already have a prior accepted static interpretation, a refused new interpretation does not erase or relabel that earlier text. This increment creates no immutable web-text derivative or accepted observation, source independence claim, quote validation or source-region anchor.

The fixed native HTTPS and cancellation proof profiles remain v3. Their trusted test-only preview/confirmation explicitly bind that historical policy, rather than treating a current v4 preview as permission for an old receipt. Their source-bound prior reports remain unchanged. The debug review fixture uses v4 for its two exact-acknowledgement specimens; its other 28 specimens intentionally remain historical v3.

## Remaining execution limits

Coordinator v4 settlement now uses the private [prepared-settlement seam](OFF-LOCK-HTML-SETTLEMENT.md) to replay and interpret outside the workspace mutex. General inspection, cancellation, start and advance still replay historical HTML under that mutex; prepared publication still validates original files and performs canonical I/O under it. Tokenizer and tree-builder calls, parser-internal scans, allocation, and one token's node creation are not preempted by these counters. The adapter bounds the specified input/admission quantities and post-call checks; it does not prove total CPU instructions, RSS, elapsed time or a cooperative cancellation deadline. Tree depth is checked after construction. A cancellation can still wait for another read path's parser work, and in-progress off-lock interpretation may finish before its cancelled result is suppressed. General lock responsiveness or a reviewed isolation boundary remains separate activation work.

Likewise, fixed successful HTTPS and one response-cancellation sample do not establish redirects, all cancellation phases, cross-run per-host pacing, Windows provider quiescence or general publisher access. Native activation and every release gate remain unchanged.
