# Industrial period comparison — design handoff

The Transactions area now exposes the reviewed period-comparison core through command v8. It compares two analyst-selected inclusive periods without automatic date selection, currency conversion, duration normalization or browser arithmetic. Exact amounts, review denominators and versioned source drillthrough share the canonical Rust model.

## Editable design

[14 / Instrument — period comparison, frame 38:1013](https://www.figma.com/design/O50ISV0LGG8nKDcFORNMbO?node-id=38-1013) is an independent root-level frame in the existing Figma Design file. It is 1120×1160 at position 25400, −500. Native editable text and vector layers were authored, named, positioned and visually inspected in Figma. The retained [full-resolution PNG](review/transaction-comparison/figma-period-comparison.png) was exported using Figma's native **Copy as PNG** action; it is not a screenshot pasted into the design canvas.

The frame continues the graphite and amber instrument direction: square controls, thin rules, compact labels, paired applied-period blocks, a restrained table and visible review partitions. It uses Inter and JetBrains Mono. The specimen presents the primary synthetic account/currency and an alternate zero/negative-baseline state. The live application includes every result group, all transfer controls, source versions, errors, stale states and full denominator partitions. It retains dark row headers for measure names, shared with the existing analysis table styling. Native date inputs localize their entry presentation; the applied date labels and backend contract remain canonical ISO dates.

The existing editable [recurring source-review frame 34:833](https://www.figma.com/design/O50ISV0LGG8nKDcFORNMbO?node-id=34-833) supplies the square 920 px source-dialog foundation. The new comparison dialog reuses its source-row presentation and focus behavior; it substitutes applied period context for recurrence details. A separate duplicated source-dialog design was not needed. No interactive Figma prototype, auto-layout component library, owner acceptance or release approval is claimed.

## Interaction contract

| Element | Behavior |
| --- | --- |
| Draft scope | Four required date inputs define the two inclusive periods. Account, currency and transfer treatment are explicit. Dates start empty. The core rejects overlap or invalid dates. Ledger filters do not apply. Edits never recalculate or alter displayed results automatically. |
| Applied scope | Two dark blocks retain the calculated dates, inclusive day counts and source counts. Delta always means comparison minus baseline. Unequal duration and a chronologically later baseline get explicit warnings. Gap days, account/currency filters and transfer treatment remain visible. |
| Exact amounts | Separate account/currency tables show credits, debit magnitude and net for baseline, comparison and delta. A defined relative change remains the exact `numerator / denominator × 100%` expression. Zero and negative baselines show a reason without a percentage. No money string is converted to a browser number. |
| Denominators | Included, pending, rejected, deferred and opted-out verified transfer rows remain visible for each period. Every nonempty partition opens its source IDs. Zero-row controls remain visibly disabled. Empty periods differ from nonempty periods with no accepted included rows. |
| Supporting and outside rows | Versioned outside-period records and verified transfer counterparts have separate drillthrough controls. The dialog explains that outside-period records are metadata and their originals were not necessarily verified by the calculation. Supporting counterparts are not additional contributions. |
| Source review | The shared `AnalysisSourceRow` renders escaped descriptions, IDs, source versions, review status, duplicate hints and reviewed-transfer annotations. It refuses a missing or mismatched version. The source action hands off to the existing transaction review and preserved-source excerpt. No HTML-looking source string executes. |
| Freshness | Expected revision travels with the command; Rust reads both periods in one snapshot and verifies their originals and recorded peers. Known revision changes, failed calculations and explicit comparison refresh invalidate prior results and disable drillthrough. External changes become visible through a workspace refresh or canonical response; there is no continuous freshness polling claim. |
| In-flight work | Request generations, unmount guards and response/workspace revision checks discard obsolete replies. Navigation away and back cannot populate a new panel with a late old response. Failed calculation retains a visibly stale prior result. |
| Focus | The native modal traps focus. Escape returns to its enabled opener; a stale/disabled opener falls back to Compare selected periods. Handoff focuses canonical transaction review. |
| Compact layout | Paired date controls and account/currency filters use two columns; periods and review partitions stack at compact widths. The wide amount table has a labelled keyboard-focusable horizontal scroll region. Group/source lists page in batches of 25. |

The [domain contract](../transactions/PERIOD-COMPARISON.md) defines exact arithmetic, scope/review/transfer rules, ledger bounds, source verification and limitations. The result is returned in full; visible list pagination is not analytical pagination or a large-workspace performance claim.

## Rendered comparison evidence

| Surface | Retained artifact |
| --- | --- |
| Editable specimen export | [Figma PNG](review/transaction-comparison/figma-period-comparison.png) |
| Complete working panel | [Full surface](review/transaction-comparison/comparison-full-surface.png) |
| Ordinary viewports | [Desktop 1440×1000](review/transaction-comparison/comparison-desktop.png), [compact 720×900](review/transaction-comparison/comparison-compact.png) |
| Source drillthrough | [Desktop](review/transaction-comparison/source-desktop.png), [compact](review/transaction-comparison/source-compact.png) |

The complete-panel capture uses a 1440×3200 viewport so the full synthetic result can be compared with the condensed Figma specimen. Viewport captures show surrounding app controls; dialogs remain at their normal maximum height and scroll. Visual inspection confirmed the applied-period hierarchy, unrounded ratio presentation, compact reflow, readable source wrapping and focus treatment. The live fixture's canonical revision differs from the illustrative Figma revision.

Four scoped axe checks under WCAG 2 A/AA and 2.1 AA returned no violations: [desktop panel](review/transaction-comparison/accessibility-desktop.json), [compact panel](review/transaction-comparison/accessibility-compact.json), [desktop source dialog](review/transaction-comparison/accessibility-source-desktop.json) and [compact source dialog](review/transaction-comparison/accessibility-source-compact.json). [SHA-256 checksums](review/transaction-comparison/checksums.json) bind the retained images and accessibility results.

## Verification and remaining gates

Seven new real-core browser workflows cover exact amounts and partitions, inert original text, source review/focus, draft versus applied scope, reversed periods, empty and pending-only results, explicit transfer exclusion, out-of-scope peers, stale backend refusal, correction freshness, overlap failure, delayed replies after unmount, stale open source dialogs, keyboard containment/restoration and compact accessibility. Every response comes from the actual Rust development executable and fixed synthetic imports/review decisions. The workflows delay real requests for race tests; they do not fabricate command responses. The main source workflow records zero external browser requests. The first targeted run passed 12 of 14 workflows: two exact select-label lookups timed out. Explicit accessible names on the account, currency and transfer selectors repaired those lookups; the subsequent seven-test and full-suite runs passed.

The full real-core browser suite passed all 42 workflows, including these seven new comparison workflows and the seven existing pattern workflows exercising the shared source renderer. The ordinary Rust suite passed 135 tests; 12 native-runtime tests were ignored and are not claimed as verified by that run. Production UI build and strict core Clippy pass. The build retains its existing large-chunk warning. Public schema generation adds only command v8 and comparison request/result v1; every v7 command variant remains present and the historical schema files are unchanged. A canonical Rust dispatch assertion verifies the new command returns the same revision-bound result as the workspace method.

This is the period-comparison UI slice of EW-18. Account-flow graphs, further statement/reconciliation workflows, full performance budgets, installed-artifact offline tests and cross-platform native interaction remain separate work. Automated axe checks do not establish screen-reader, 200% zoom or native WebKit/WebView2 acceptance. Previous report snapshots remain immutable; no report-integration or complete release claim is made here.
