# Industrial transaction patterns — design handoff

The Transactions area now calculates a separate revision-bound scope for description totals, cash/refund candidates and recurring debit candidates. All arithmetic and classification rules run in Rust through command v6. Source review reuses the canonical transaction decision and preserved-source inspection workflow. No accepted merchant identity, payment purpose or recurring-payment classification is created.

## Editable design

Two independent root-level frames were authored and visually inspected in the existing Figma Design file, with native editable text and vector layers:

| Frame | Purpose | Dimensions / position |
|---|---|---|
| [11 / Instrument — transaction patterns, 34:766](https://www.figma.com/design/O50ISV0LGG8nKDcFORNMbO?node-id=34-766) | Scope rail, currency instruments, description totals and cadence candidates | 1120×980 / 21100, −500 |
| [12 / Instrument — recurring source review, 34:833](https://www.figma.com/design/O50ISV0LGG8nKDcFORNMbO?node-id=34-833) | Cadence explanation, anchored dates, retained source rows and stale state | 920×900 / 22500, −500 |

These are editable Figma specimens exported through the application, not flattened screenshots pasted into a design canvas. The direction retains graphite and amber, square controls, fine rules, compact measurement cells, Inter and JetBrains Mono. The design shows a condensed subset of synthetic groups and rows; the working surface shows the full synthetic result with list pagination. Revision numbers and source versions differ because application captures use canonical imports and review decisions.

The implementation adds explicit controls for transfer treatment and all recurrence tolerances, all review-state denominators, exact source versions, errors, pagination and applied-versus-draft scope. The source dialog shows every contributing row rather than only the first specimen. The specimen's stale-state banner is a conditional state in the app, not a permanent warning. There is no interactive Figma prototype, reusable component/auto-layout conversion or owner design acceptance claim.

## Behaviour and component specification

| Element | Behaviour |
|---|---|
| Scope rail | Inclusive transaction dates, exact account and currency. Ledger filters do not affect this separate scope. Defaults: all rows, transfers included, three occurrences, two-day date tolerance, zero amount spread. Draft edits never change displayed results until calculation succeeds; applied parameters stay visible. |
| Currency instruments | Exact credits, debit magnitudes and net, separately for each currency. Included, pending, rejected, deferred and explicitly excluded transfer partitions all open their source rows. Zero-row controls are disabled with the denominator visible. Cash/refund instruments are subsets and must not be added again to totals. |
| Description groups | Original descriptions, ASCII case and whitespace normalization only. Currency and contributing accounts remain visible. Merchant identity, branch and channel are unconfirmed. Credits, debits and net all share the group's source-ID drillthrough. |
| Recurrence | One account, currency and description group per candidate. Review shows weekly/fortnightly/monthly cadence, exact total/range, applied limits, expected/actual dates and signed deviations. All eligible and unmatched debit rows remain accessible. Same-day repeated purchases are counted and can suppress candidates. |
| Source drillthrough | Native 920 px maximum-width dialog, escaped data, canonical transaction ID/version, review partition, duplicate hints, verified or invalid transfer links and rule names. Pages contain at most 25 rows. “Inspect source and review row” closes the result dialog and opens the existing transaction review with preserved-source excerpt and canonical decision controls. |
| Freshness | Calculations carry the displayed expected workspace revision; Rust rejects a stale request. Known revision changes and failed revalidation mark prior results stale and disable drillthrough. Recalculation is explicit. External changes become visible through Refresh workspace or another canonical view refresh; this surface does not poll the entire workspace or claim continuous live freshness. |
| Errors and late responses | Errors retain prior results with clear stale/unavailable status. Generation and unmount guards discard obsolete replies. No mock, optimistic calculation or browser decimal summation is used. |
| Focus and compact layout | Escape returns to the available opener. Source handoff focuses transaction review. Below 1150 px controls and instruments reflow; below 750 px they use two columns and cadence actions stack. Wide data tables retain a labelled keyboard-focusable horizontal scroll region. |

The [calculation contract](../transactions/PATTERNS.md) specifies the heuristic limitations, exact parsing/arithmetic, canonical dates, transfer verification, source verification and hard bounds. Lists render 25 items at a time; the backend result is currently returned in full. This is not the large-workspace pagination/performance gate.

## Rendered comparison evidence

| Surface | Editable design export | Working application |
|---|---|---|
| Pattern instrument | [Figma instrument](review/transaction-patterns/transaction-patterns-figma.png) | [Complete surface](review/transaction-patterns/patterns-full-surface.png), [desktop viewport](review/transaction-patterns/patterns-desktop.png), [compact viewport](review/transaction-patterns/patterns-compact.png) |
| Cadence and source review | [Figma source review](review/transaction-patterns/recurring-source-review-figma.png) | [Desktop dialog](review/transaction-patterns/cadence-desktop.png), [compact dialog](review/transaction-patterns/cadence-compact.png) |

Ordinary desktop captures use 1440×1000 and compact captures 720×900. The full instrument uses a 1440×3200 comparison viewport so the complete panel can be inspected without sticky-header overlap. Ordinary screenshots show the actual viewport and surrounding app controls; dialogs scroll at their normal height limit. Manual comparison caught and repaired heading hierarchy, table header treatment, top accent specificity and a false unapplied-scope notice caused by differing JSON key order. Captures are visual evidence, not pixel-equality tests.

Four axe checks scoped to the new panel/dialog under WCAG 2 A/AA and 2.1 AA returned zero violations: [desktop instrument](review/transaction-patterns/accessibility-patterns-desktop.json), [compact instrument](review/transaction-patterns/accessibility-patterns-compact.json), [desktop source review](review/transaction-patterns/accessibility-cadence-desktop.json), [compact source review](review/transaction-patterns/accessibility-cadence-compact.json). [Checksums](review/transaction-patterns/checksums.json) bind all retained images and accessibility results.

## Verification and remaining gates

On macOS Apple Silicon, the production UI build, strict core Clippy and all 109 ordinary Rust tests pass; eight native-runtime tests remain intentionally ignored in the ordinary run. All 30 real-core browser workflows pass, including seven new transaction workflows. The new workflows cover exact separate totals, all review denominators, source inspection and focus, duplicates, cash/refund rules, anchored cadence, explicit transfer exclusion, applied/draft scope, empty results, correction propagation, stale backend refusal, delayed replies after unmount, stale source-dialog refusal with fallback focus, pagination and hostile-looking escaped descriptions. The transaction workflow records zero external browser requests. Browser tests import and review fixed synthetic CSV rows through the actual development Rust executable; no calculation responses are synthesized.

Peer review identified and repaired exact-decimal rounding and loose-date validation in the shared core. Regression coverage includes enormous amounts plus/minus a cent, parser rounding refusal, exact opposite transfers, reconciliation accumulation and canonical date refusal. The final full Rust/browser runs use these repairs. The existing frontend chunk-size warning remains; this fixture is not a throughput benchmark.

This increment is a bounded part of EW-18. Period comparisons, account-flow exploration, reviewed merchant identity, performance budgets, native Windows/macOS Intel interaction, installed-artifact offline acceptance and signed release remain separate work. Automated axe checks do not establish screen-reader, 200% zoom or native WebKit/WebView2 acceptance. The design has not been described as owner-approved or release-complete.
