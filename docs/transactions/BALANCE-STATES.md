# Selected transaction balance states

Command v18 adds `read_transaction_balances` with a `TransactionBalancesRequest` and `expected_revision`. It returns a v1 dedicated response in all three dispatch modes. This is an additive backend for paginated ledger warnings; the UI and existing response modes are unchanged.

The request contains 1–200 unique `{id, expected_version}` pairs. IDs contain 1–256 UTF-8 bytes without controls; versions must be positive. Revision and all selected versions must match. The response contains the exact requested set in request order, with the canonical row ID/version and one of three explicit states:

| State | Meaning |
| --- | --- |
| `no_balance` | This row has no retained available balance. Its amount can still contribute to a later balance window. |
| `no_prior_balance` | This is the first balance for its source/account/currency group. There is no preceding balance to reconcile against. |
| `checked` | Returns the preceding balance row ID/version, contributing-row count, exact decimal difference and reconciliation result. |

Neither unchecked state means a successful reconciliation. A checked result describes arithmetic in retained canonical records; it is not analyst acceptance, source verification, or proof that an entire statement reconciles. Every review state participates. Groups remain separate by exact account, currency and source identity. Canonical insertion order, rather than displayed date order, controls each balance window. All movements between balances contribute even when absent from the selected page. The contributing-row count excludes the preceding balance row and includes the current closing row, including zero-amount rows. Duplicate hints and recorded transfers do not remove movements.

The implementation captures the revision and complete ordered canonical transaction set in one SQLite read snapshot, validates canonical key/body identity and positive versions, resolves the whole request, then calls the existing `analytics::analyse` once. It does not reimplement financial calculations from the visible rows. Missing, stale, malformed or invalid records fail explicitly and return no partial successful batch. The calculator still validates transactions outside the requested set; a corrupt unselected row must not silently erase a warning. Concurrent corrections cannot mix a newer amount/version into the old result.

The returned shape is structurally limited to 200 small states with bounded identifiers and exact decimal results. It excludes full transaction bodies and contributor ID vectors. This is a payload projection: full transaction bodies, all legacy calculation vectors and an ID index are still allocated internally. It does not establish bounded worker memory, indexed-query performance or the target two-second latency. On-demand contributor paging and full discrepancy review remain separate work.

The reader does not load source bodies, rehash originals, validate source anchors, create decisions or mutate canonical data. Source inspection/review retain their independent integrity checks. A consumer must pin the response to the same revision and row versions as its page, reject delayed results, distinguish unavailable status from an unchecked state, and suppress actionable stale warnings until refreshed. Existing workspace/command schemas remain immutable; only command v18 and the two new v1 schemas are added.

Synthetic tests cover exact eight-place arithmetic, repeated purchases, missing balances, interleaved accounts/currencies/sources, nonchronological dates, all review states, legacy-calculator equivalence, malformed identities and unselected values, bounded and maximum-size requests, a 301-contributor window, complete-set failures, exact mode parity, unchanged originals/report snapshots and a real concurrent correction with stale-version rejection. No UI or native release gate is claimed.

The integrated core run passed 247 ordinary tests, with 21 native-engine cases explicitly ignored; strict all-target Clippy and formatting passed. All 64 earlier schemas remain byte-identical, and v18 preserves all 52 v17 variants and 31 definitions. A separate read-only peer review found no acceptance blocker. Retained logs: `artifacts/transaction-balance-{focused,core,clippy,schemas}.log`.
