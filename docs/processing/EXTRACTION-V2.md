# Parse extraction v2

New canonical document-parser publications use `extraction.v2.schema.json` and `schema_version: 2`. The worker request/result protocol remains **1**. This increment does not activate Windows document execution, change the ordinary macOS parser recipe, enable OCR, or create accepted observations and source anchors.

## Shared result validation

The exact new PDF parser identity is `pdfbox-3.0.8-local-fonts-v1`, with media type `application/pdf`. It identifies the separately reviewed local-font extraction policy; it is not a promise of font coverage, layout fidelity or complete extraction.

| Result | Validation |
|---|---|
| Local-font PDF partial output | Keeps `no_source_anchors`, `ocr_not_performed` and `embedded_documents_excluded`; no error. Existing text, metadata and page limits still apply. |
| Substitution disclosure | `font_substituted` and `font_coverage_unverified` occur together and only under the exact local-font identity. Both are absent when the worker made no substitution claim. |
| Failed extraction after a substitution attempt | The same paired disclosures may remain, but failed output retains neither text nor metadata. A typed error is mandatory. |
| Font asset unavailable | `font_asset_unavailable` is allowed only under the exact local-font PDF identity with `failed` status and empty text/metadata. |
| Complete claim | The new PDF identity cannot claim `complete`. That existing status remains restricted to the established UTF-8 contract. |
| Legacy PDF identity | `pdfbox-3.0.8` retains its previous rules and cannot adopt either new limitation or the new asset failure. |

Only the new identity permits up to eight distinct limitations; other identities keep the prior six-field ceiling and their existing allowed subsets. Common limits, source hash/length checks and strict unknown-field decoding remain in the shared Rust parser validator. Canonical publication and inspection reuse it; no separate Java font policy or duplicate extraction algorithm is introduced here.

Raw JSON decoding rejects duplicate metadata keys, including spellings that become the same key after JSON escape decoding. It applies to both versions before canonical publication or inspection can accept the result; a duplicate cannot silently replace an earlier value even when the final value would match the stored digest. Distinct case-sensitive keys remain valid. Worker adapters must decode their bounded raw JSON directly into `ParseResult`, because a prior generic JSON-object decode could already have discarded duplicates. The shared field-count and string-size checks still apply after decoding.

## Immutable versions and inspection

The saved v1 schema is unchanged. Inspection accepts versions 1 and 2, but version 1 cannot carry the new parser identity or its semantics. The v2 schema constrains new publications to version 2. It does not replace the historical v1 schema, and neither schema replaces Rust's identity-dependent semantic validation.

Inspection binds one SQLite read snapshot across the derivative, owning job and evidence metadata. It checks the requested key against the record ID, the ID derived from job/attempt, the owning canonical UUID, a bounded RFC3339 publication timestamp, immutable input identity, and result digest. Both the derivative and owning-job bodies are size-checked before decoding, with a 2 MiB retained-body ceiling. Source-byte verification uses the existing bounded original reader. Missing, replaced or altered originals fail visibly; inspection does not repair or rewrite a record.

An earlier attempt remains inspectable after a retry: its immutable input must match the owner, its attempt may be less than the current reserved attempt, and its ID must remain in the owner's retained result list. It need not be the newest result. The worker request UUID is independent of the canonical document-job UUID.

The read path reserializes the typed result in memory to check its established digest. It does not re-encode stored record bodies or migrate v1 data. Generic canonical records and existing backup/restore already retain both versions and their original files, so this format increment requires no storage migration. Older binaries are not promised to understand the new font semantics; only current-reader compatibility with valid v1 records is established.

The fixed [v1 fixture](../../fixtures/processing/extraction-v1.json) was captured through the previous development canonical publisher, using its explicit synthetic processing seed. No worker ran. It retains both decoded fields and the exact original JSON bodies. Tests bind its SHA-256 and the historical schema hash, then preserve those bodies through manual retry, reopen, backup and restore alongside a new v2 derivative. Test-only database substitution cases are corruption/recovery fixtures, not production write APIs.

## Review presentation and evidence

The existing industrial [extraction review design](../design/DOCUMENT-JOBS.md) is reused without layout or interaction changes. Font substitution and unverified coverage are separate, visible limitation lines; unavailable font assets explain why no text or metadata was retained. Text/metadata remain inert and unreviewed, and failed output cannot enable Copy text.

The retained [full review](../design/review/extraction-v2/extraction-full-surface.png) and [compact review](../design/review/extraction-v2/extraction-compact.png) show the real Rust synthetic fixture. Visual inspection found the added lines readable, without overlap or horizontal overflow. Existing keyboard, focus, clipboard and accessibility checks remain in the document-job workflow suite. These are implementation checks against the existing design foundation. No new editable Figma extension frame, remote synchronization, native Windows UI proof or owner design acceptance is claimed for the added wording.

The debug-only fixed processing seed retains eleven jobs and canonical publication. Its partial PDF specimen now exercises both font disclosures; its failed PDF specimen exercises the unavailable-asset error. They explicitly remain synthetic states with no worker execution. The saved v1 fixture is separate and is never overwritten by that seed.

See [the verification record](verification/extraction-v2.json) for exact source/fixture/artifact hashes, test counts and retained failures. Shared validation includes unknown identities, unpaired/duplicate limitations, false complete/failed claims, media mismatch and output limits. Canonical tests cover rollback, idempotent replay, historical retry/recovery, full valid-record substitution, owner/source binding, original loss, oversized records and a real concurrent SQLite writer. No complete-release or Windows activation gate is passed by this contract increment.
