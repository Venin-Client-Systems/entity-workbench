# Bounded finding review history

This increment keeps the industrial finding-review dialog and its existing editable [Figma frame 25:361](https://www.figma.com/design/O50ISV0LGG8nKDcFORNMbO?node-id=25-361), typography, review hierarchy and square controls. The original [assessment handoff](ASSESSMENT-REVIEW.md) remains its design foundation. Figma is currently unavailable for a verified editable pagination extension and remote readback. The new history controls have implementation evidence only; this is an explicit design gap, not a claim that screenshots replace the required editable design pass.

## Read contract and scope

`FindingReviewHistory` calls `page_review_decisions` for the selected finding at the displayed workspace revision. It requests up to 50 canonical decisions, with an opaque continuation cursor and the complete target denominator. The core rejects a requested page exceeding its 1 MiB retained-body budget without returning a partial prefix. Canonical insertion order is retained; timestamps and decision IDs are displayed verbatim. Accepted, pending, rejected and deferred states have distinct labels. Reason text is escaped and preserves line breaks.

Only an actual finding review mounts this reader. Whole-evidence citations retain their source inspector and are not submitted as generic review-history targets. No new history is invented for other citation types. `AssessmentWorkbench` no longer reads `workspace.decisions`; the legacy workspace type and default response still contain that array for compatibility. This change alone does not reduce the default payload size.

First, previous and next controls navigate bounded pages. The last 100 page positions are retained for back navigation; first remains available independently. The counter reports actual row positions and the full scope count, not a guessed page total. A successful zero-count response says “No decision recorded.” A failed read says history is unavailable and makes no empty-history conclusion.

## Revision, selection and focus

A target or known revision change remounts page state. Generation and mounted guards reject delayed replies, including an A → B → A sequence. Continuing after a concurrent canonical write fails at the Rust revision guard, clears visible rows and offers an explicit workspace refresh. Refresh begins again at the first page. It does not silently adopt a new revision for an in-progress review mutation: the draft reason remains, the form becomes disabled, and the dialog asks the analyst to close and reopen before recording a decision.

Pager buttons remain mounted while loading. User-triggered paging and retries return focus to the result status if focus remains on the initiating control or falls back to the document body; an analyst-selected field keeps its focus. The existing modal manages Escape, nested source inspection and focus restoration to the selected finding.

## Verification

The browser tests use the real Rust development command bridge and canonical import, finding creation, edits and review operations. No success payload is mocked. Transport-failure tests abort requests; the delayed-response test holds an actual canonical response and then releases it after selection changes.

The full real-core browser campaign passed **69/69** scenarios, including six new history tests. The final evidence-only history rerun passed **6/6**. `cargo build -p workbench-core --bin ew-dev --locked` and the production UI type/build passed; the existing large JavaScript chunk warning remains. The [machine-readable readout](review/finding-history/verification.json) records scope, host OS/architecture, backend identity and retained log hashes.

The first targeted run passed 7/8 and exposed Chromium dropping focus when the last-page Next button became disabled. An incomplete first repair omitted that callback, so the second run retained the same failure (7/8). The complete repair passed 8/8. A sixth regression then proved that a delayed read does not steal focus from a review draft, before the full 69-test run. These failures are retained separately; none is represented as an initial pass.

| State | Rendered implementation evidence |
|---|---|
| Populated final page, 1440 px viewport | [Desktop view](review/finding-history/history-1440.png) |
| Populated final page, 720 px viewport | [Compact view](review/finding-history/history-720.png) |
| Failed read, without an empty-history claim | [Unavailable history](review/finding-history/history-error.png) |
| Successful zero-decision response | [Empty history](review/finding-history/history-empty.png) |

These are scrolled views of the existing modal, not whole-document renders or a new editable Figma export. The final-page controls, separate decision reasons/times/IDs and scope counts were visually inspected at both widths, with no horizontal modal overflow. [The axe readout](review/finding-history/accessibility.json) records zero automated violations at both widths and retains incomplete checks for manual review. [Checksums](review/finding-history/checksums.json) bind the saved evidence and implementation files.

Native platform interaction, manual screen-reader checks and a new editable Figma pagination state are outside this browser-only verification.


## Subsequent native integration check

Root's actual development Mac app at clean `04d056724c536da16e34b7f695d9ec8603168ddc` displayed all three existing synthetic finding decisions at revision 31. Exact IDs, reasons and timestamps matched the canonical records. Same-revision refresh retained an unsaved review draft; nested source inspection displayed the original CSV row 10 amount, and Escape restored both opener controls. The draft was not submitted. Normal quit left all 55 records byte-identical. Operator record SHA-256: `1c00051299c9a78de660d2deeb5b8c0b275044436b09fc7d1b2c1ac28d1f1e2d`. This adds native Tauri/WebKit interaction evidence for the three-row path; it does not extend the browser pagination/stale-writer scenarios into native or clean-installation claims. No new native Figma export was obtained.
