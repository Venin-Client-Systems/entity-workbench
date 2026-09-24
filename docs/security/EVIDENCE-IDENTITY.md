# Canonical evidence lookup identity

An intact original file does not prove that its metadata belongs to the requested source. A hostile database could place source B's complete, valid body under source A's SQLite key. Hashing the file named by B's body would succeed while an observation, job or calculation still referred to A.

The Evidence-specific readers in `store/evidence.rs` retain the storage key and require `records.id == Evidence.id == Evidence.sha256`. They reuse the existing original-reference validator for lowercase SHA-256 shape and the import-policy byte ceiling. A complete empty HTTP response remains valid. `find_evidence` returns `None` only for an absent row; invalid JSON structure, identity, or reference metadata is an error. Generic `get` and `all` remain unchanged for other record kinds.

These are metadata checks. They do not hash files, verify source truth, establish source independence, accept an observation or repair a corrupt database. Existing original-byte verification remains separate. The metadata-only positive control still reads a correctly bound record after its original file is removed; a subsequent byte-verification request fails.

## Changed read paths

| Path | Binding and retained behaviour |
|---|---|
| Transaction patterns and period comparisons | Each in-scope source and recorded transfer counterpart's source is resolved through the typed reader before original verification. Out-of-scope counterpart provenance remains required. Existing revision snapshot, row bound, exact calculations and transfer semantics are unchanged. |
| Observation anchors and finding review | Text/cell anchor validation refuses a different body beneath the anchor key. Observation add/correct/review and finding review keep their existing transactions; failure preserves revision, current records, history and decisions. Whole-source findings resolve their cited key before hashing. |
| Job queueing | All four existing document/image/PDF operation routes validate the requested Evidence key before deriving an immutable processing input. Existing preparation/publication checks still validate that input and its original bytes. |
| Views, presentation, desktop summary and new reports | The evidence list validates every SQLite key in canonical sequence order. The existing view snapshot is unchanged. New report generation consequently refuses an inconsistent evidence list; previous report records and exported bytes remain unchanged. |
| Corpus search input and identity comparisons | The full evidence read retains its key. Search still receives the same ordered metadata/text; comparison still uses explicit origin groups without claiming independence. Runtime availability and existing source-snapshot limits are unchanged. |
| Backup and restore | The snapshot evidence enumeration validates keys before deriving the referenced-original list. A substituted snapshot cannot be blessed through a matching body-only manifest. No completed backup manifest or restored canonical database is published on this failure; the existing partial-directory lifecycle is retained. |
| Import and collection | Import distinguishes missing evidence from invalid existing metadata rather than swallowing every error as absence. A corrupt existing record cannot be silently overwritten or returned as a duplicate. Existing collection response lookup, page promotion, receipt verification and export lookup use the same typed reader. Acquisition/history, original retention, receipt validation and export staging semantics remain unchanged. |

## Inventory of specialised paths

The remaining production generic `get<Evidence>` reads are already explicitly bound to their requested key: source inspection, transaction source batches, page/search/transfer page provenance, complete transaction export, and processing-input verification. They also retain their existing digest or shared original-reader verification. Their context-specific errors and read budgets were preserved.

The durable synthetic collection foundation uses a separate 4 MiB preflighted metadata reader. Both callers check lookup key, body ID, digest and expected length before use; that bound and replay contract were preserved. The citation catalogue projects metadata without allocating source text, checks joined storage-key/body identity and then body-ID/digest equality, and retains its own row/page bounds. These readers were reviewed rather than replaced with the full-body helper.

Statement duplicate detection and initial finding-citation resolution query existence/counts only. They do not deserialize or return a trusted Evidence body; statement import refuses an existing key, and reviewed findings subsequently use the validated Evidence reader. The new helpers do not change other record kinds' key/body rules. This slice is not a general audit of observation, transaction, entity, derivative or receipt identities.

## Regression evidence and limits

Synthetic regressions replace A's metadata with the complete valid body of B while preserving intact originals. They cover anchor and finding review, patterns and comparisons, a filtered-out transfer peer, all four job queue methods, views/comparisons, backup/restore, page promotion and import deduplication. The restore adversary also removes B's separate canonical row and rewrites the manifest so an unrelated duplicate destination check cannot mask the key-binding problem. State comparisons include exact current records, history, events and revision; saved HTML and original bytes are checked independently.

Positive controls cover normal imports and deduplication, absent records, sequence order, correctly bound metadata without an original file, and existing successful calculations/reviews. Separate malformed-body and body/digest substitution cases fail closed. The verification record retains the before-fix false successes, the refined restore reproduction, a repaired test-only compile error, and final test/Clippy results.

No public command, schema, dependency, storage migration, native export transport, UI or live collection activation changed. Full evidence reads still decode all source metadata/text and are not newly bounded or indexed; this patch makes no performance claim. Local macOS source tests do not establish Windows or Intel runtime behaviour, clean installation, complete sandboxing, signed distribution or a passed release gate.
