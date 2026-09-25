# Bounded transfer counterpart selection

Command v19 adds `page_transfer_candidates`. It preserves the existing broad
selector: accepted transactions with an ID and exact account different from the
selected target. Unequal amounts, other currencies, already matched rows and
legitimate repeated purchases remain visible. Nothing is ranked as a match,
automatically excluded as a duplicate, accepted or paired. `match_transfer`
continues to enforce reviewed states, equal-and-opposite nonzero amounts, matching
currency, different accounts and absence of an existing pair.

The target may itself be pending, rejected, deferred or already matched. This
preserves the current selector rather than silently changing which targets can
be inspected. Listing a counterpart does not promise that a subsequent matching
operation will succeed.

## Contract

`PageTransferCandidates` receives `request` and `expected_revision`. The strict
request contains:

| Field | Meaning and bound |
| --- | --- |
| `target_id` | Exact canonical ID; 1–256 UTF-8 bytes without controls. |
| `expected_target_version` | Positive canonical target version. |
| `query` | Literal text query; at most 1,024 UTF-8 bytes. Empty means no text restriction. |
| `filter` | Nullable inclusive `date_from`/`date_to`, exact `account` and exact uppercase three-letter `currency`. No review selector. |
| `order` | `date_ascending` or `date_descending`. Equal dates retain canonical sequence ascending in both directions. |
| `page_size` | 1–200 rows. |
| `cursor` | Null for the first page; otherwise the preceding bounded continuation. |

The response is `{schema_version:1,target_id,target_version,matching,page}`.
`matching` uses the existing literal-search algorithm and runtime Unicode metadata.
`page` is the existing `TransactionPage` shape, including its revision, query
identity, exact canonical transaction rows and original anchors, counts and next
cursor. A target is returned only after its actual canonical ID/version validates.
All three dispatch modes return this dedicated response directly.

The page's `scope_count` and accepted/pending/rejected/deferred `review_counts`
cover the other-account scope after optional date/account/currency/text filters,
before review selection. `selected_count` is the accepted counterpart denominator.
A pending-only scope and an empty scope are distinct even when both return no rows.
Counts describe recorded rows in this scope, not verified transactions or matches.

Text matching reuses [the existing literal search contract](LITERAL-SEARCH.md):
whole-string Unicode lowercase of `description + " " + account + " " + date`,
then literal containment. Query whitespace and cross-field boundaries remain
significant. There is no wildcard, SQL expression, transliteration or normalization.
Equivalent query casing has the same continuation semantics. Existing search field
bounds and type checks apply within the transfer filter scope before accepted
selection; oversized or malformed search fields cannot silently count as no match.

## Shared implementation and integrity

A closed internal page context adds only fixed, parameterized target ID/account
exclusions to the existing transaction paging/search engine. The target, revision,
counts, cursor membership and returned rows share one SQLite read snapshot. Target
version or revision changes require refresh; a candidate correction cannot mix a
new state into an old count. No analyst SQL or independent monetary rule is added.

Target lookup checks its retained body size before allocation, then validates
canonical key/body identity, positive/current version and the existing transaction
rules. Its separate retained-body limit is 2 MiB. The target's referenced evidence
must bind canonical key, ID and content hash, and its original must pass the existing
checksum check even when the counterpart result is empty. The shared page code
validates each returned row and hashes each distinct referenced original once per
request, deduplicating with the target's original. Missing or nontext accounts in
the active filter scope fail explicitly instead of disappearing through SQL NULL
comparison. Exact optional filters still define the scope; this is not a global
validation pass over every excluded record.

Candidate pages keep the inherited 2 MiB aggregate retained-body budget. A byte
boundary can shorten a page; its cursor points to the last returned row, so the
next row is not skipped. A single oversized candidate fails explicitly when it is
the next row. A malformed returned body, canonical identity mismatch or failed
original check produces no successful partial page. Originals for unreturned
candidates are not rehashed merely to compute the denominator. Original checksum
verification does not prove that a corrected transaction is accurate or that its
source anchor is sufficient for analyst acceptance.

The cursor is bound to its own transfer family, target ID/version, workspace
revision, exact filters/order/page size, normalized query and matcher profile.
It must identify a real row in the accepted filtered scope. It cannot be reused
for another target, ledger page or query. It is a continuation-consistency token,
not an authorization credential or tamper-proof signature.

The reader is read-only. It makes no bounded-memory or indexed-query claim: SQLite
still scans/parses canonical JSON, and existing original verification can load
source bodies/files. No default desktop response, UI selector, matching command,
canonical schema or previous wire schema changes. Contributor review, automatic
matching and full filtered export are separate capabilities.

## Verification

Synthetic regressions compare the broad denominator to the actual old selector
predicate. They cover both date orders/ties, repeated purchases, other currencies,
unequal amounts, preexisting pairs, every target review state, exact filters,
pending-only versus empty scope, Unicode/literal query behavior, equivalent-casing
continuation, cross-target/query/family cursor rejection and target correction.

Additional tests cover malformed target/candidate identities and money, missing
originals and content-digest retargeting, a corrupt target with no matching
candidates, byte-shortened pages and oversized bodies, request bounds, malformed
account types, hidden oversized pending search text, and an actual second canonical
writer correcting a candidate after the read snapshot is pinned. Direct response
parity and unchanged legacy page/search results are checked in all dispatch modes.
Historical schema regeneration is compared byte-for-byte. No UI or release gate
is passed by this backend slice.
