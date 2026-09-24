# Revision-bound literal transaction search

Command v16 adds `SearchTransactions { request, expected_revision }`. The strict
request is `{ query, page }`, where `page` is the existing `TransactionPageRequest`
with its exact account, currency, date and review filters, date order, page size
and optional cursor. The response is version 1:

```json
{
  "schema_version": 1,
  "matching": {
    "algorithm": "unicode_default_lowercase_literal_v1",
    "unicode_version": [16, 0, 0]
  },
  "page": {
    "schema_version": 1,
    "workspace_revision": 42,
    "query_sha256": "example-query-identity",
    "scope_count": 0,
    "review_counts": { "accepted": 0, "pending": 0, "rejected": 0, "deferred": 0 },
    "selected_count": 0,
    "rows": [],
    "next_cursor": null
  }
}
```

The Unicode version above is illustrative of the tested Rust 1.90.0 build. The
application reports the actual compiled standard library's version; it is not a
fixed value in the schema. Both dispatch modes return this response directly,
without a full workspace envelope. There are no writes, worker jobs or network
requests. Existing `PageTransactions`, its request/page schemas and older command
schemas are unchanged.

## Exact matching behavior

For each transaction, concatenate `description`, one ASCII space, `account`, one
ASCII space and `date`, in that order. Lowercase this complete string and the
query, then test literal substring membership. Query whitespace is retained;
spaces between fields can participate in a match. There is no tokenization,
regular expression, wildcard expansion, locale selection, accent removal or
Unicode normalization. `%`, `_`, `*`, quotes and SQL-looking text are literal.

The shared `literal_search` helper uses Rust's whole-string lowercase operation,
including contextual Greek sigma and locale-independent expansion mappings.
Lowercasing individual characters would not preserve that behavior. JavaScript's
existing `toLowerCase()` operation also specifies Unicode default case mapping.
The two runtimes may use different Unicode data versions. These contracts are
described by [Rust `str::to_lowercase`](https://doc.rust-lang.org/std/primitive.str.html#method.to_lowercase),
[Rust's Unicode version](https://doc.rust-lang.org/std/char/constant.UNICODE_VERSION.html),
and [ECMAScript `String.prototype.toLowerCase`](https://tc39.es/ecma262/multipage/text-processing.html#sec-string.prototype.tolowercase).

The fixed 32-case synthetic fixture is checked by both canonical Rust search and
the actual JavaScript expression in `scripts/test_transaction_search_case.mjs`.
It covers sigma context, dotted I, composed/decomposed accents, supplementary
Deseret letters, sharp S, literal punctuation/control characters and cross-field
spaces. The observed local builds are Rust 1.90.0 / Unicode 16.0.0 and Node
26.7.0 / Unicode 17.0 / ICU 78.3. This is specific regression evidence, not a
claim of equivalence across every Unicode version or platform WebView. Canonical
Rust strings are valid Unicode scalar text; lone UTF-16 surrogate queries are
not accepted strings in this command contract.

## Scope, continuation and provenance

Text is an additional scope dimension applied after the existing exact
date/account/currency scope and before review selection. `scope_count` and all
four review denominators count text matches; `selected_count` then applies the
optional review state. An empty text scope and matching pending-only records
therefore remain distinguishable.

Search uses the same paging implementation as `PageTransactions`: deterministic
date order with ascending canonical sequence for ties, a maximum of 200 rows,
the existing 2 MiB retained-body page budget, strict returned-row validation,
exact decimal strings and original anchors, and one original verification per
distinct returned evidence ID. A body-budget-shortened page resumes after its
last returned row without skipping the unreturned row.

The shared verification now requires the evidence lookup key, evidence body ID
and content digest to agree before hashing the original. This also hardens the
existing page reader: corrupted metadata cannot substitute a different intact
original for the one named by a transaction's anchor. Valid v12 behavior and its
wire contract are preserved.

One SQLite snapshot pins revision, matching/counts, cursor validation, row-size
metadata, rows and evidence metadata. Original file verification remains the
existing filesystem check. A stale expected revision fails. The continuation
hash binds the existing page query, revision, normalized lowercase query and
matching profile/Unicode version. Different raw query casing can share a cursor
only when it lowers to the same string. Whitespace changes remain significant.
Cursors provide continuation consistency, not authentication.

An empty query bypasses the new text bounds and matcher entirely, preserving v12
row/count/order/source semantics, including older long text that fits its page
body bound. Search still uses a separate cursor family, so ordinary page and
search cursors cannot be interchanged even for an empty query.

## Bounds and invalid records

The query is at most 1,024 UTF-8 bytes, checked before lowercasing. A nonempty
search requires borrowed description and account fields to be text of 1–4,000
bytes each and nonblank, and a valid 10-byte ISO date. All fields are checked
before concatenating or lowercasing a row. The internally supplied lowered query
also has a 4,096-byte defensive bound. Whole-string case mapping can expand text;
the concatenated input itself is at most 8,012 bytes.

A fixed deterministic Rust SQLite function performs matching. Its arguments are
borrowed and validated before Rust string copies. A fixed `CASE` expression
limits evaluation to the base date/account/currency scope. Invalid or oversized
fields in that scope fail the search even if they would not match the query or
would be excluded by review state. Invalid text outside that scope is not checked.
Unknown or malformed search fields are not silently interpreted as no match.
The function is registered with `SQLITE_DIRECTONLY` to prevent persisted schema
views/triggers from invoking it; this is not a barrier against application-created
TEMP views or a general database sandbox. See [SQLite CASE evaluation](https://www.sqlite.org/lang_expr.html#the_case_expression)
and [function flags](https://www.sqlite.org/c3ref/c_deterministic.html).

The bounds apply to domain inputs and copied matching text, not command-transport
allocation, complete database memory or total execution time. SQLite still reads
and parses retained JSON and scans the scoped ledger. Matching does not validate
every other field of unreturned rows or verify their originals. There is no index,
throughput claim, UI cutover or change to the default large workspace response in
this slice.

## Verification scope

Nine Rust test groups cover the shared 32-case fixture through real imports and
canonical search, exact scope/review/order/tie behavior, empty-query v12 parity,
stale and cross-query cursor rejection, malformed/oversized hidden text, hostile
requests and persisted-function restrictions, the shared row-byte budget and
original corruption, evidence retargeting to another intact original, and a real concurrent canonical import during a pinned
snapshot. The JavaScript fixture test records its actual runtime versions and
fixture hash. Older schemas must remain byte-identical after generating command
v16 and the two new version-1 search schemas. No release gate changes.
