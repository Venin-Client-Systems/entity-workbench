# Whole-ledger account and currency suggestions

Command v15 adds `PageTransactionFacets { request, expected_revision }`.
The request selects the closed `account` or `currency` enum, a page size from
1 to 100 and an optional opaque cursor. The response reports schema version 1,
workspace revision, query digest, selected facet, total transaction count,
distinct-value count, exact values with their transaction counts, and a possible
continuation cursor. Both canonical and desktop dispatch return this dedicated
shape without loading a workspace view.

The catalogue covers every transaction regardless of review state, date or any
currently applied ledger filter. Values use exact SQLite BINARY text ordering:
leading zeros, case, Unicode normalization differences and literal wildcard or
SQL-looking text remain distinct. Nothing is trimmed, case-folded or interpreted
as a pattern. All SQL values are bound parameters. This is selector metadata;
its counts are not accepted financial totals.

Revision, counts, cursor validation and values share one SQLite snapshot.
Cursors bind the facet, page size and revision. They identify the first canonical
sequence for the preceding distinct value, so long account text does not inflate
the cursor. A forged nonrepresentative or absent sequence is rejected. A changed
revision requires a fresh catalogue and new cursor chain.

Before copying a value, the reader checks every selected field's stored type and
UTF-8 byte length inside SQLite. Missing, nontext, empty or over-4000-byte values
fail explicitly; they are never silently removed from the denominator. The
complete requested page must fit 256 KiB of raw value text, checked before its
value copies. Overflow fails the whole page and asks for fewer rows. The 4000-byte
limit matches the existing analysis-filter limit and accommodates retained legacy
values; current CSV import uses a smaller 300-byte account limit.

Returned accounts must be nonblank and contain no control characters, matching
the ledger filter's supported account input. Returned currencies must contain
three uppercase ASCII letters. These semantic checks apply to returned values;
they do not certify every later page. A legacy control-containing account is
reported as unsupported, not normalized or offered as an unusable filter.

No full transaction bodies or evidence files are returned or reverified. Invalid
amounts or changed originals can therefore coexist with successful metadata
reads; row/source inspection remains responsible for their integrity checks.
The limits bound copied value text, not escaped JSON size, SQLite scanning/grouping
memory, transaction cardinality or application-wide memory. This reader adds no
index, timing claim or default-response reduction.

Six regressions exercise canonical imports with exact Unicode, leading-zero,
case and hostile-looking account values; review-independent counts and traversal;
changed/query-mismatched/forged cursors; invalid and oversized metadata;
aggregate overflow with simulated legacy values; large nonfacet bodies; empty
and unchanged dispatch; and a real concurrent import during the pinned snapshot.
The first fixture attempt used 4000-byte accounts through the current CSV importer
and correctly failed that import. The retained final fixture explicitly simulates
legacy records without relaxing production validation. Bounded peer review found
the control-character/filter mismatch; the repair includes a real quoted-CSV
regression. The selector UI migration remains separate work.
