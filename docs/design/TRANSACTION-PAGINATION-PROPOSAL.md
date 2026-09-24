# Transaction pages and desktop summary — implementation proposal

This is a design and migration proposal against source revision `3536859`.
It does not change the running interface, command v12, historical workspace
responses or canonical financial rules. The coordinating implementation task accepted the bounded source-batch reader
as the first implementation choice and owns contract integration. The broader
summary switch remains gated on the dependency migrations below.

The existing editable industrial transaction foundation is [frame 6:2](https://www.figma.com/design/O50ISV0LGG8nKDcFORNMbO?node-id=6-2).
The intended addition retains its graphite/amber palette, square controls,
compact ledger and source-review handoff. Native pagination-frame creation and
export are still pending the Figma control/sync recovery described below.
Local SVG preparation is not evidence of a native editable Figma frame.

## Verified current contract

[PageTransactions](../transactions/PAGINATION.md) reads a single SQLite snapshot
at an explicit expected revision. Command v12 accepts inclusive date bounds,
exact account, exact currency, optional review state, date direction, page size
1–200 and an opaque continuation cursor. The body limit is 2 MiB of transaction
JSON, so a page can contain fewer rows than requested while still having a next
cursor. Equal dates retain ascending canonical sequence in either date order.

The result has complete date/account/currency scope and all four review counts,
then a selected count after review filtering. It returns exact canonical rows,
versions and source anchors, verifies originals referenced by those returned
rows, and performs no monetary aggregation. Its counts do not mean that every
original in the scope was reverified. No query text, arbitrary ID lookup,
transfer-candidate predicate or decision-history reader exists in this command.

The current desktop presentation omits historical report HTML but still calls
`view_with_reports`, loading all transaction and review-decision records. It
then calls `analytics::analyse`, returning every included/excluded transaction
ID and balance-check contributor list. Changing only the ledger table to pages
would not remove those costs.

## Ledger interaction design

- Keep draft controls separate from the applied query. Provide From/Through
  date, exact Account, Currency, Review, Date order and Rows per request. Blank
  account/currency/date means unrestricted; leading account zeros survive.
  Apply loads the first page and commits the draft only on a successful read.
  Clear resets the draft; it does not silently replace the displayed scope.
- Display the applied inclusive dates, account, currency, review and sort above
  the table, together with the pinned revision. Always show scope count and
  accepted/pending/rejected/deferred denominators before review selection.
  Label selected count separately. A nonempty pending-only scope selected as
  accepted is not an empty workspace.
- Display an actual returned-row range, for example `101–200 of 326 selected`.
  Maintain the starting cursor and actual starting ordinal for each visited
  page. First uses a null cursor; Next uses the returned cursor; Back re-reads
  the prior page's saved cursor at the same revision. Keep cursor strings
  opaque. Do not calculate a last page or total page count from page size,
  because byte-limited pages may be shorter.
- Request up to 25, 50, 100 or 200 rows; default 100. Applying another size,
  filter or order resets cursor history. Retain only the current row payload,
  selected review row and bounded cursor history, not every visited page's
  transaction objects. If history reaches a chosen bound, disclose that Back
  can reach only the retained history; First always remains available.
- A known revision change invalidates cursor history, row actions and export.
  Keep the old rows visibly marked stale until the analyst explicitly refreshes
  the workspace and restarts at the first page. A backend revision conflict
  has the same behavior even when the UI had not observed the write. A bad
  cursor or read/integrity error is an unavailable page, never zero results.
- While reading, retain the current page with a loading state and disable page
  actions. A generation/revision/unmount guard discards late replies. Filter
  drafts remain editable; a reply commits the captured request, not newer
  draft values. Announce page range after success without moving keyboard
  focus unexpectedly. First/Back/Next restore sensible focus after failures.
- `Export these N rows` exports the displayed page only. Its nearby disclosure
  states page row count, full selected count, applied filters and revision.
  Disable export for stale, loading or unavailable pages. Use a clearly
  versioned page-export envelope containing scope/order/revision/query digest
  and exact rows, not a filename or success message implying a full ledger.
  Await the existing native completion/error path. The historical full export
  remains a separate capability and must not be silently relabelled.
- Dates/accounts/descriptions remain escaped; amount strings remain strings.
  Duplicate and transfer hints never hide rows or change counts. Ledger
  pagination does not alter the independent analysis/comparison scopes.
- Compact layout stacks field pairs and denominators, preserves the labelled
  horizontal table region, and keeps First/Back/Next and the current range
  visible together. Source review retains a usable fallback focus target when
  its original row has moved to another page.

## Full-ledger dependency inventory

| Consumer at `3536859` | Present dependency | Required replacement before array removal |
| --- | --- | --- |
| `main.tsx` ledger and JSON export | `workspace.transactions` is filtered locally by review, currency and a case-insensitive substring of description/account/date | Page v12 for supported exact filters; bounded backend whole-ledger text search before claiming the current search workflow is preserved; explicit current-page export |
| Currency/account selectors in ledger, patterns and comparison | Distinct values are derived from every row | Revision-bound, bounded facet reader, or exact input with a bounded suggestion reader; never derive available values from only the current page |
| Main transaction review | Selected object comes from the full array; an absent current row is reported as outside filters | Canonical row inspection by ID/version/revision; distinguish outside current page, outside applied filter and unavailable. Returning from analysis may legitimately select a row outside the ledger scope |
| Transfer counterpart select | All accepted rows in another account are rendered into one select | Separate bounded accepted-row picker with full selected denominator, exact currency, account and date/search controls; canonical `match_transfer` remains final authority. No nearest/heuristic auto-selection |
| Pattern/comparison source dialogs | A full transaction map resolves IDs emitted by calculations | Bounded ID/version batch read for each visible source page, expected calculation revision, all requested IDs accounted for and explicit failure on missing/mismatched rows; original verification preserved |
| Analysis overview/cards/chart | Full total ID vectors and all balance checks arrive on every mutation/view | Compact exact summary with included/excluded counts and discrepancy counts; on-demand bounded contributing-row and balance-window readers using shared Rust rules |
| Ledger balance-mismatch badges | Scans the full `analysis.balance_checks` array for every row | Current-page balance status from a revision-bound reader or bounded per-ID decoration; preserve source-row-order reconciliation across page boundaries, never recompute from the page alone |
| Finding editor and reviewer | `citations(workspace)` builds every transaction citation; selected IDs and source-origin groups resolve through it | Bounded citation search plus exact selected-ID resolution, both at the displayed revision. Existing selected citations must remain visible even outside current search/page |
| Finding review history | All generic decisions are filtered by finding ID; empty array means no decisions | Target-bound paged review history with total count and explicit loading/error/empty states |
| Entity decisions | Separate `identity_decisions` and `merges` arrays are displayed | Keep these in this bounded migration, or migrate them through separately reviewed history readers; do not remove them as incidental cleanup |
| Save report/backup/legacy CLI | Canonical full view and report rendering need complete data | Preserve the historical methods and schemas. Desktop omission is a response projection, not deletion or omission from saved reports/backups |

The current substring search is broader than v12. Renaming it “search this
page” would change the existing workflow and is not the proposed default.
An additive transaction-search request version must specify literal matching,
field composition and case-fold semantics explicitly, include the search in
cursor/query identity and count before paging. SQL parameters stay bound and
wildcard-looking input stays literal. Existing JavaScript `toLowerCase`
behavior needs named Unicode regression fixtures before claiming equivalent
Rust matching; SQLite ASCII `NOCASE` alone is insufficient.

## Additive contract proposal

Names below are proposed, not implemented or published schemas.

1. **Desktop summary v2.** Use a distinct response/workspace type, not a
   historical `WorkspaceView` filled with empty arrays. Return revision,
   explicit transaction/review totals, remaining non-ledger records and report
   metadata. Omit `transactions` and generic `decisions` entirely. Historical
   `View` and the existing presentation remain intact. Mutation dispatch in the
   new desktop mode returns the same new summary shape. Read operations that
   already return a dedicated result retain that shape.
2. **Transaction source batch v1.** The minimal agreed proposal is
   `ReadTransactionSources { request: { rows: [{ id, expected_version }] },
   expected_revision }`, returning `{ schema_version: 1, workspace_revision,
   rows: Transaction[] }` in request order. Require 1–25 unique opaque IDs,
   1–256 UTF-8 bytes without controls, and a positive `u32` supplied version.
   A null version deliberately means current-at-expected-revision lookup for
   saved finding IDs; patterns/comparisons always supply the recorded version.
   Reject unknown fields. Check a 2 MiB retained-body budget in SQLite before
   allocating body copies. Return exact canonical rows in one snapshot,
   validate key/body identity and transaction values, and verify each referenced
   original once. Missing, mismatched, oversized or corrupt items fail the whole
   batch; never shrink the source denominator. A one-item request covers direct
   source review. The existing `InspectSource` continues to verify/render the
   retained original excerpt. Root is implementing this as the next additive
   command; this design document does not claim its implementation or tests.
3. **Ledger search/facets.** Preserve v12 unchanged. Add a versioned search
   request that reuses the same typed scope/cursor/row reader and reviewed
   matching function, not a second financial engine. Bound query length,
   page size and response bytes. Facets must be bounded and paged if necessary;
   an overflow must not silently remove an account/currency option. Exact
   free-text account entry is a safe alternative when suggestions are partial.
4. **Target review history.** Valid target kind and ID, expected revision,
   bounded page, deterministic canonical sequence and total count. No arbitrary
   database kind or JSON-path query is exposed. Finding review uses this reader
   on opening; transaction review can use the same typed family later.
5. **Summary and reconciliation drillthrough.** Extract a common accumulation
   path from current exact Rust analysis; retain established transfer and
   reconciliation semantics and test summary equality against the legacy
   output. Counts replace default ID lists. Selected balance windows and source
   groups are read on demand. A first payload-only implementation may still
   load temporary Rust rows; document that honestly and measure before claiming
   bounded core memory or sub-two-second performance.
6. **Citation search and selected-ID inspection.** Keep observation/evidence
   citation types, review states and source-origin identity. Search the complete
   eligible catalogue with bounded results and a full count. Resolve saved or
   draft selected IDs independently of search results, with explicit missing
   states. Saving still uses canonical finding validation and expected revision.

All readers must bind count, rows, versions and reported revision to one
snapshot. Avoid nested transactions when sharing lower-level query helpers.
No reader accepts arbitrary SQL, file paths, scripts or downloaded code.

## Implementation sequence and switch gate

1. Implement the bounded ordered source-batch reader first. Migrate visible
   pattern/comparison source pages and direct transaction review with recorded
   version/revision checks. The current desktop presentation remains intact.
2. Add the standalone ledger page component against unchanged v12, exercising
   supported exact filters, short pages, cursor backtracking, source handoff
   and current-page exports. Add further readers only as each remaining
   consumer migration requires them: finding history, whole-ledger search,
   facet suggestions, citation search/selected-ID resolution and transfer
   selection. Verify before/after workflows against the same canonical
   synthetic state. Pagination alone is not a reduced default response.
3. Introduce the typed summary response and compact financial projection.
   Change the desktop and local bridge together only once TypeScript has no
   hidden full-array dependency. Never silently fall back to full `View` on
   a reader error; show that reader's unavailable state.
4. Retain old response/schema compatibility tests. Measure serialized payloads
   on the retained 100,000-row benchmark workspace copy, and record actual
   warm/cold page/summary timings separately from earlier benchmark evidence.
   Do not overwrite prior timings or imply required 16 GB/full-document/map
   benchmark completion.

Switch acceptance must cover all review/correction/transfer workflows;
pattern/comparison versioned source drillthrough; finding citation creation,
edit, saved-ID resolution and review history; original source inspection;
report generation/export and backup; old command/CLI shapes; and clean empty
workspace behavior. Tests must include an external writer between reads,
correction of a row on another page, review-state changes that remove the
selected row, late replies after navigation, rejected cursors, byte-limited
short pages, source corruption and export refusal while stale. Check desktop
and compact keyboard/focus/axe and native download completion. Synthetic real
core responses only; no substituted success payloads.

## Figma recovery observation

On 2026-09-25 the original tab remained open with editable frame `45:1399`,
1120×1200 at 31420, −500, and its individual text/vector layers visible in the
native accessibility tree. Figma explicitly reported “Some changes won't be
synced until Figma is able to reconnect.” The browser-extension selection of
both the original and existing secondary tab timed out. Native export actions
produced no confirmed download; coordinate interaction returned
`noWindowsAvailable`. The original tab was not reloaded or closed. Remote
readback, the missing states PNG and native pagination additions therefore
remain unverified until the control/connection problem is resolved.


After the first recovery attempt, Chrome's native Window menu successfully
selected each existing window. The original Figma page still did not respond
to file actions or action-search shortcuts. The already-existing secondary
Figma tab displayed a blank grey document, including after reloading only that
secondary tab and retrying the canonical file URL. This does not establish a
server outage or loss of the original; it establishes that remote readback was
not obtained in this session. No browser/profile reset, original reload or
credential change was attempted.

## Prepared vector specimens — native authoring pending

[Ledger source](review/transaction-pagination/source-ledger.svg) is a
1120×1110 specimen with exact applied filters, all four review denominators,
First/Back/Next, actual row ranges and explicit page-only export.
[State source](review/transaction-pagination/source-states.svg) is a 920×1160
specimen for stale revision, nonempty scope with zero selected rows,
byte-limited short pages and unavailable reads. They use the existing Inter /
JetBrains Mono, graphite / amber and square-control conventions. Values are
illustrative synthetic content, not measured query output.

These files are prepared editable text/vector sources only. They have not been
pasted into native Figma frames, exported from Figma or visually approved.
[Checksums](review/transaction-pagination/checksums.json) identify that exact
status. Native authoring, remote readback, exports and compact comparison
remain outstanding; these sources must not be presented as completion of the
required design-tool pass.
