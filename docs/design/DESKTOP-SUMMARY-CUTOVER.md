# Desktop summary consumer migration

This implementation plan follows the consumer inventory at `4efa177`. It supplements the historical [pagination proposal](TRANSACTION-PAGINATION-PROPOSAL.md); it does not activate summary responses or claim completed pagination design. The coordinating implementation task accepted this phased approach, date-ascending ledger order with an explicit direction control, and the existing broad transfer-candidate eligibility as routine implementation choices.

The subsequent [desktop summary implementation](DESKTOP-SUMMARY.md) now carries out this plan. Its verification record distinguishes actual browser/core checks from native integration and the still-unverified editable design extensions. The plan below remains the original consumer inventory and rationale.

## Current consumers and required replacements

| Consumer | Present full-array dependency | Required replacement |
| --- | --- | --- |
| `main.tsx` ledger | Filters, renders and exports `workspace.transactions`; tests selected-row visibility against that array | Revision-bound literal search pages; complete counts; full-scope export; precise selected-row state |
| `main.tsx` transaction review | Builds accepted other-account transfer options from every transaction | Target/version-bound transfer candidate pages; final matching stays canonical |
| `main.tsx` currency controls | Distinct currency values from every transaction | Bounded whole-ledger facet selector |
| `TransactionPatterns.tsx`, `TransactionComparison.tsx` | Distinct account/currency controls from every transaction | The same facet selector, preserving exact selected values between facet pages |
| Overview, sidebar, priority and total cards | Pending/discrepancy counts and analysis membership-vector lengths | Explicit summary counts; no financial calculation in the UI |
| `StatementImport.tsx` | Direct command response sets application data without passing through the main `run` wrapper | Summary-aware response/prop types and explicit shape validation |
| `review-history-types.ts` | Type-only `Workspace["decisions"][number]` alias | Standalone decision type, preserving the existing wire fields |

Assessment citation selection and finding review history already use their dedicated readers. Pattern/comparison drillthrough already uses bounded source reads. Their explicit analysis-result transaction IDs, excluded-transfer IDs and source-version vectors remain necessary; summary mode must not remove them from those separate results.

## Response boundary

Use distinct `DesktopWorkspace`, `DesktopSummaryResponse` and `LedgerSummary` TypeScript types. Preserve historical `Workspace`, analysis and response types for existing commands, fixtures and compatibility tests. Do not make omitted arrays look like successfully loaded empty arrays.

The implemented summary shape is `{schema_version: 1, workspace, analysis}`. It omits full `transactions` and generic `decisions`, adds `review_decision_count`, and replaces default analysis vectors with:

- `transaction_count` and accepted/pending/rejected/deferred `review_counts`;
- `duplicate_candidate_row_count`, `balance_check_count` and `balance_discrepancy_count`;
- per-currency exact credit, debit and net strings, `included_count` and `excluded_transfer_count`.

Existing total charts need only currency/credit/debit/net. Narrow their props; retain the current approximate chart geometry and exact text/table values. Use explicit counts where the current interface takes vector lengths. Current overview cards and chart do not offer aggregate membership drillthrough, so adding that capability is a separate release feature rather than a blocker for preserving existing behavior during this cutover.

Summary still includes complete evidence rows, retained text/acquisition history, observations, entities, statement-import transaction IDs and report metadata. Its implementation still allocates the full canonical view internally. This is a payload projection, not evidence of bounded core memory, indexed queries or a sub-two-second application.

## Ledger and review integration

1. Introduce the shared facet selector using command v15, at most 100 values per request and the existing 256 KiB value budget. Keep the exact selected account/currency visible even when it is not on the current facet page. Do not fetch all pages or silently truncate choices. Facets describe the whole ledger, including every review state; they are not narrowed by another control.
2. Introduce a ledger component using command v16 `SearchTransactions`, including its empty-query path. Keep draft controls separate from applied query/filter/order/revision. Use the reported literal matching profile, complete scope/review/selected denominators, actual returned row offsets and byte-short page semantics. Keep at most one active and one replaceable pending read per component lifetime. Retain only the current payload and bounded cursor history.
3. Read command v18 balance summaries for the visible IDs/versions, at most 200 in one complete ordered set. Render `no_balance`, `no_prior_balance` and `checked` distinctly; an unavailable batch is not an unreconciled balance. Reconciliation continues to use canonical source order and all relevant rows, not displayed date order or page-only rows.
4. Pin transaction review to the source row's ID, version and revision, including pattern/comparison handoffs. The existing main action wrapper can otherwise apply the latest workspace revision to a held older row. Preserve reason/correction drafts on refresh, disable stale mutations, and require deliberate verified refresh or reopening before continuing.
5. Replace the transfer dropdown with command v19 candidate pages. Default candidates remain accepted rows with another ID and exact account. Unequal amounts, other currencies, previously matched rows and repeated purchases remain visible; this reader is not an automatic transfer classifier. Canonical `match_transfer` remains the final validator.
6. Replace local complete-array JSON export with the complete export reader planned for command v20. Pin applied scope, literal query, direction and revision; permit one export at a time, validate the returned count/digest/byte identity and wait for native download completion. Do not relabel current-page output as a full matching export. A separately labelled page export is an optional additional control.

The existing ledger and JSON export use canonical insertion order. The accepted new default is explicitly labelled ascending transaction date, with canonical sequence ascending for equal dates in either direction. This is a visible ordering change; it does not alter balance reconciliation. Preserve App-level currency/review selection across section navigation; current query state resets on navigation. A chart currency pivot should restart the requested currency scope without applying unrelated draft controls.

Absence from the displayed page does not establish exclusion by a filter. Exact structured date/account/currency/review mismatches can retain the current “outside current filters” warning. Otherwise state that the selected row is not on the current page and that text-query membership is unverified. Do not duplicate Rust Unicode matching to infer membership or add a new read API solely to decorate this warning.

The balance response includes previous-row identity/version and contributing-row count, not every contributing transaction. Full reconciliation contributor and aggregate membership drillthrough remain separate release capabilities. Neither should be silently inferred from a visible page.

## Activation and verification order

Keep the current presentation active while facets, ledger/review, transfer selection and export migrate. Narrow retained-only props in assessment, entity, collection, graph and map components; do not force them to depend on historical full workspace types. Move the standalone history type without changing saved history semantics.

Activate the summary boundary together in native desktop dispatch, an additive explicit `ew-dev --summary` mode and the local development bridge. Keep default CLI and existing presentation modes unchanged. Dedicated reader/job responses retain their existing shapes. Validate summary and backup responses explicitly; a result missing `workspace` is not automatically a successful backup.

Before activation, require real-core regressions covering:

- empty, pending-only, populated and byte-short pages; full denominators, literal search and returned matching profile;
- exact facet values beyond the first page and retained selections after revision changes;
- correction/review of an off-page row, changed review membership, structured-filter exclusion and deliberately moved focus;
- concurrent writer, stale source versions/cursors, navigation and late responses;
- accepted same/other-currency, unequal, matched and repeated transfer candidates, with final canonical rejection where appropriate;
- full matching export beyond one page, exact order/count, stale/original-corruption refusal and actual native completion;
- statement import, source inspection, pattern/comparison drillthrough, finding citations/history, report snapshots and backup;
- old command/CLI schemas, summary shape, default transport and compact keyboard/axe behavior.

Measure actual summary payload size and reader timings separately from the retained baseline. Do not overwrite earlier observations or claim the required 16 GB document/map/graph benchmark passed.

## Design status

Reuse the editable industrial ledger frame `6:2`, patterns frame `34:766` and source frame `34:833`, comparison frame `38:1013`, and assessment frames `25:361`/`25:398` in the existing Entity Workbench Figma file. The shared graphite/amber typography, hard edges, labels, warnings and source-review modal are the approved foundation.

New ledger pagination, facet continuation, transfer paging and stale/read-failure extension frames are still uncreated or unverified. The original unsynced Figma tab remains preserved. Existing SVG proposals and browser screenshots do not establish native editable authoring, remote sync or visual approval of those new states. This backend/consumer plan does not substitute for the remaining design-tool work.
