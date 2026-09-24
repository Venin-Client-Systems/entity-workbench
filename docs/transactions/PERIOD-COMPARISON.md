# Transaction period comparison v1

This bounded EW-18 slice implements a read-only Rust calculation for two explicitly selected periods. It supplies a typed domain result through `Workspace::compare_transaction_periods(request, expected_revision)` and public command v8 `compare_transaction_periods { request, expected_revision }`. Versioned request/result v1 schemas and the Transactions period-comparison surface use that same canonical operation. Historical command schemas remain unchanged. The [editable design and UI handoff](../design/TRANSACTION-COMPARISON.md) records visual and browser verification. It does not complete account-flow graphs, statement coverage, reconciliation, reporting integration or performance acceptance.

## Period and scope contract

`TransactionComparisonRequest` contains `baseline` and `comparison` objects with `from` and `through` dates, optional exact `account` and `currency` filters, and an explicit `transfers` choice (`include` or `exclude_reviewed_pairs`). Both period endpoints are inclusive. Dates require canonical ASCII `YYYY-MM-DD` and a valid calendar date. Start must not follow end. The periods must not overlap, including at a shared endpoint. Adjacent periods and single-day periods are valid. No period is inferred from imported data.

The analyst's selected direction remains authoritative: every delta is **comparison minus baseline**, even if the baseline is chronologically later. Filtering uses transaction dates, never posting dates. Accounts retain their exact strings, including leading zeros. Currency codes are exactly three uppercase ASCII letters. There is no exchange conversion or cross-currency total.

The result repeats the request and provides inclusive `baseline_days`, `comparison_days`, `unequal_duration` and `gap_days`. The gap is the number of calendar days strictly between the two periods, whichever was selected first. Amounts are observed totals; no division by days, annualization, equal-duration adjustment or missing-period imputation occurs. A consumer must display the selected periods and unequal-duration warning with any comparison.

## Exact amounts and relative change

Each account/currency group has separate baseline and comparison credits, debit magnitudes and net. Credits and debits are nonnegative; net is credits minus debits. Every amount and difference is an exact decimal string. The shared exact arithmetic rejects a result that cannot be represented without losing precision, including a small fraction beside a very large amount. Failure refuses the whole calculation instead of publishing partial or rounded totals.

Each `AmountChange` contains `baseline`, `comparison`, `delta` and a tagged `relative_change`:

| State | Meaning |
| --- | --- |
| `defined` | Positive baseline. `numerator` is the exact delta and `denominator` is the exact baseline. The percentage is `numerator / denominator × 100`; the core never divides or rounds it. |
| `zero_baseline` | Percentage is undefined. `comparison_is_zero` distinguishes two zero accepted totals from a nonzero comparison. Neither case proves complete statement coverage. |
| `negative_baseline` | No percentage is supplied for a negative net baseline, because a conventional growth percentage can misrepresent the direction. Exact delta remains available. |

For example, credits of `3.00` and `4.00` produce delta `1.00` and the exact ratio `1.00 / 3.00`, without claiming a finite exact decimal percentage. A future presentation can label and round a display value while retaining this ratio. It must never coerce decimal strings to binary floating-point for authoritative totals. A net change from `-20.50` to `-11.00` is `9.50`, with `negative_baseline`, rather than a fabricated positive growth rate.

## Review denominator and drillthrough

Each `PeriodAccountTotal` has a complete, mutually exclusive partition of its `scope_count`:

- `total.transaction_ids`: accepted rows included in money totals.
- `pending_ids`, `rejected_ids` and `deferred_ids`: source transactions excluded by review state.
- `excluded_transfer_ids`: accepted rows excluded only by the explicit transfer choice and verified reciprocal pairing.

The parallel `rows` list retains the existing `AnalysisRow` annotations and source transaction versions for every member of that partition. Duplicate candidates and legitimate repeated purchases remain counted. Existing cash/refund rule annotations are hints; comparison creates no accepted classifications. Every amount drillthrough must use the group's period and contributing IDs, and require the recorded workspace revision and transaction version before presenting them as the current calculation.

Groups are the union of account/currency pairs with source rows in either period, sorted by exact account and currency. A group found on only one side gets an explicit empty scope and zero accepted totals on the other. A period with only pending transactions has a nonempty scope and zero accepted totals. If no row is in either period, the group list is empty; no synthetic account or currency group is invented.

Global denominators are also complete:

```text
workspace_transaction_count
  = outside_account_currency_count + account_currency_transaction_count

account_currency_transaction_count
  = baseline_transaction_count + comparison_transaction_count
    + outside_period_rows.length
```

`outside_period_rows` retains IDs and versions for account/currency-matching records before, between or after the selected periods. It is a ledger metadata denominator, not a claim that their original evidence was inspected by this operation. Absent statements, omitted imports and unobserved transactions cannot be inferred from any empty date interval or zero total.

Transfer behavior is reused from `transaction_patterns_v1`: exclusion requires both sides accepted, reciprocal links, distinct accounts, the same currency, nonzero equal-opposite amounts. The counterpart may be outside both periods or the account filter. Invalid, dangling or unreviewed links never create an exclusion. `verified_transfer_peers` retains the IDs and versions of counterparts supporting annotations, including those outside scope. Consumer drillthrough must keep these supporting peers separate from contributing transactions.

## Snapshot and evidence boundary

The workspace operation begins one SQLite read transaction, verifies `expected_revision`, loads the bounded ledger, verifies retained original hashes for both selected periods and all their recorded transfer counterparts, then calculates both sides against that same snapshot. It uses the shared original-verification path with transaction-pattern analysis. It changes no canonical records, history, review decisions, findings or report snapshots. A stale expected revision returns a conflict; corrupted or missing supporting originals fail the operation.

The pure `transaction_comparison::compare` function validates the supplied ledger and applies domain rules, but it cannot establish the supplied revision's authenticity or verify filesystem evidence. Application callers must use the workspace method. A correction makes prior results stale, returns the affected transaction to pending review and increments its version; recalculation respects that state. Prior report snapshots remain unchanged.

Comparison reuses the existing analysis implementation twice, once for each explicit period, and reuses the exact accumulator for per-account partitioning. It therefore retains the 100,000-row, 32 MiB description and exact currency-aggregate bounds of that implementation. Extra pattern results are not published as part of comparison. This is a bounded correctness implementation, not a measured execution or memory budget; EW-17 still needs the large-workspace benchmark and any required execution redesign.

## Verification and remaining work

Synthetic Rust tests cover inclusive endpoints, year transitions, leap day, adjacent/single-day periods, gaps, reversed selected order, unequal duration, posting-date exclusion, canonical-date rejection, invalid filters, nested unknown fields, independent accounts/currencies, every review partition, duplicates, zero and negative baselines, exact rational output, unrepresentable arithmetic, inherited ledger limits, reviewed transfer peers outside scope and inconsistent links. Canonical workspace tests cover unchanged state, stale revisions, correction/re-review, original source drillthrough, report snapshot preservation and corrupt originals in either period or a filtered-out transfer counterpart. Existing transaction-pattern tests also exercise the refactored shared evidence-verification path.

Local verification on this slice: `cargo test -p workbench-core` passed 135 ordinary tests, including 10 new comparison tests and 11 existing pattern tests; 12 native-runtime tests were ignored and are not claimed as verified by this run. `cargo clippy -p workbench-core --all-targets -- -D warnings` passed. The staged public audit used the private exclusion list outside the repository. These Rust checks do not establish native release acceptance. The UI handoff separately records the 42 passing real-core browser workflows, including seven comparison workflows.

The UI shows periods, duration differences, observed-versus-missing denominators and transfer treatment; retains exact ratios; and disables stale result drillthrough. Draft period/filter edits do not apply until an explicit successful calculation. Native installed-artifact interaction and cross-platform accessibility remain separate checks. Account-flow graphs remain a separate slice. No native packaging, release, performance or complete EW-18 acceptance is claimed here.
