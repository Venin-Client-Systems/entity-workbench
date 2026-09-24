# Opt-in desktop summary response

`Workspace::desktop_summary()` returns a strict `DesktopSummaryResponse` v1.
`Workspace::dispatch_summary` and `JobCoordinator::dispatch_summary` use that
response for `View` and the workspace refresh following a successful mutation.
They share the existing command implementation, coordinator mutex, worker wakeup
and cancellation ownership. No command variant, command version, storage schema,
desktop transport or UI default changes in this slice. The application and the
development bridge continue using presentation mode.

The response has `schema_version: 1`, `workspace` and `analysis`. Its workspace
contains the same fields as `WorkspaceView<ReportMetadata>`, except that it omits
`transactions` and generic `decisions` and adds `review_decision_count`. These
omissions are part of a distinct type; absent data is not represented as an empty
canonical array. Identity decisions, merge records and collection jobs remain.
Processing jobs still use their existing explicit reader responses.

`analysis` contains:

| Field | Meaning |
| --- | --- |
| `transaction_count` | Every canonical transaction in the captured workspace. |
| `review_counts` | Accepted, pending, rejected and deferred row counts, including transfers and duplicate candidates. |
| `duplicate_candidate_row_count` | Rows with nonempty duplicate candidates; not distinct pairs or confirmed duplicates. |
| `balance_check_count` | Source-order balance windows checked by the existing calculator. |
| `balance_discrepancy_count` | Those checks whose exact difference is nonzero. |
| `totals` | Existing ordered per-currency decimal strings and included/excluded counts. |

Each total retains `currency`, `credits`, `debits` and `net` verbatim from
`analytics::analyse`. `included_count` replaces that result's transaction ID
vector; `excluded_transfer_count` replaces its excluded transfer ID vector.
Only accepted transactions enter totals. As in the existing calculator, an
accepted transaction with a transfer peer is excluded. This projection does not
introduce the richer transfer classifications from the separate pattern analysis.
Balance checks continue to use all review states, retain original statement order
and separate account/currency/original combinations. No floating-point money,
new aggregation, currency conversion or second calculation engine is introduced.

## Consistency and failure behavior

The implementation captures `presentation()` once. That method pins the revision
and every table to one SQLite read snapshot. It computes the existing analysis
from those captured transaction values, counts those same review states and moves
retained fields into the new response. It does not fetch a later revision or
counts from a second query. Another connection may commit while the snapshot is
read; none of its later values enter the response.

All direct command results remain unchanged: source/report readers, page readers,
analytical results, job queue/inspection/cancellation and backup acknowledgement.
They are not wrapped in a summary. Mutations are still performed once. As with
legacy refresh dispatch, an error constructing a post-mutation response does not
roll back a mutation that already committed; callers must retain existing request
identity/revision handling rather than blindly retrying writes.

Invalid transactions or unrepresentable exact calculations return errors through
the same calculator as full and presentation responses. Summary generation is
read-only and does not create decisions, findings, source anchors or accepted
facts. It does not add original-file integrity checks: explicit source readers
retain that responsibility.

## Limits and the remaining desktop cutover

This first version reduces serialized response content only. It still loads all
transactions and generic decisions internally and constructs the legacy analysis,
including its ID and balance-check vectors, before projecting them. It does not
bound whole-workspace memory, SQLite scanning or response time. Evidence text and
statement-import transaction ID lists remain in the returned workspace; this is
not a universally bounded response. No performance improvement is inferred from
the API shape.

The desktop must stay on presentation until its consumers use explicit readers.
Transaction paging, literal search, facets, source batches and target decision
history already have separate contracts. Remaining callers need revision-bound
visible-row balance annotations, reconciliation contribution drillthrough,
currency-total membership, transfer candidate selection and full-scope export.
Citation selection is being moved separately. No visible ledger page may silently
replace the complete filtered export, and no omitted vector may mean no results.

## Verification

Synthetic tests compare every retained workspace field and exact total against
legacy presentation/calculation output. Cases include all review states,
eight-place amounts, repeated purchases, paired transfers, multiple currencies,
intervening unbalanced rows and a second source for the same account. Other tests
cover empty versus pending-only scopes, calculation/decode failures and snapshot
release, a real concurrent canonical writer after revision capture, single-write
mutation behavior, unchanged direct/legacy results, queue replay and actual
coordinator cancellation signaling. Schema regeneration must leave every prior
schema byte unchanged; only the two summary schemas are added.

## Reproducible synthetic payload diagnostic

The development-only example `desktop_summary_payload` checks the real full
presentation response against the real summary response using the exact defined
projection. It removes only the two specified workspace arrays and replaces only
the legacy analysis vectors/counters; all other fields and values must compare
equal. A capped streaming serializer measures actual UTF-8 JSON bytes and SHA-256
without allocating another entire serialized response buffer.

```sh
python3 scripts/desktop_summary_payload.py --source /path/to/retained/synthetic/case
```

The runner requires the frozen 100,000-row corpus and complete single-original
set. It reads the retained corpus, copies it, prepares schema migration only in
that copy, then compares responses in a second copy. It records exact source,
configuration, executable, original and database hashes, verifies closed database
and original bytes unchanged around comparison, and rechecks the retained source
and prior campaign evidence. Reports use UUID directories so failed runs remain
independent even under an identical clock value. Metadata failures are recorded
before compilation or corpus access. The shared runner's raw child logs may
contain resource counters, but this is a payload diagnostic, not an IPC/UI
latency or memory measurement. Internal allocation remains unchanged.
