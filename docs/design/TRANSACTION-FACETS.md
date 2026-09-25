# Bounded transaction account and currency selectors

The pattern and period-comparison forms now read account/currency choices from
the canonical `PageTransactionFacets` reader. Neither form constructs choices
from `workspace.transactions`. Their workspace prop needs only a revision;
source-review handoffs also carry the originating calculation revision.

## Design basis and outstanding design work

The implementation reuses the industrial square controls, typography, palette,
scope fieldsets and pagers in the editable native Figma foundations:

- [Transaction patterns, frame 34:766](https://www.figma.com/design/O50ISV0LGG8nKDcFORNMbO?node-id=34-766).
- [Period comparison, frame 38:1013](https://www.figma.com/design/O50ISV0LGG8nKDcFORNMbO?node-id=38-1013).

**The added facet loading, retained-selection, pagination, empty and failed-read
states have not received their own native Figma design/readback pass.** The
implementation screenshots below verify rendered behavior, not editable design
approval. No new frame is claimed.

The native select keeps its explicit label and an All accounts/currencies
choice. Each option preserves the exact canonical value (including case,
Unicode and leading zeros) and labels its whole-ledger row count. Beneath it,
the control shows the actual range, distinct-value count, workspace revision and
whole-ledger row denominator across every review state. These are choice-list
counts; they are not the applied analysis denominator or reviewed money totals.

## Interaction and bounds

`TransactionFacetSelect` takes `kind`, `revision`, `label`, `value`, `onChange`
and optional `disabled`. Values use `string | null`; null means All values.
The component issues one fixed reader request for at most 100 values at a time.
It never loads every page in the background. First restarts from the beginning;
Back uses recorded cursors and actual row offsets; Next uses the returned
continuation. Shorter pages advance by their real length. History retains at
most 100 prior page positions; when older positions are dropped, an explicit
note explains the Back limit and First remains available. This is not a cap on
forward browsing.

A selected value remains a native option even when it is outside the current
page, or while choices are loading or unavailable. It is visibly marked
“retained selection.” Changing pages never changes the draft or recalculates
results. All values remains available during a read so a selection can be
cleared. An analyst who has moved focus elsewhere is not pulled back when a
delayed reply arrives. A completed pager/Retry action moves focus to its status
only when focus still belongs to that action or has fallen to the document.
A per-read focus marker remembers a deliberate move even if that other control
is subsequently blurred. Body and document-element fallback states are handled
without confusing them with a deliberate move. The listener, marker and pending
animation frame are cleaned up when the keyed selector unmounts.

The existing generic `CitationReadLane<T>` scheduling primitive is reused with
a local name. Each mounted selector has at most one active and one latest
pending reader request. Its lane survives revision changes; keyed page state,
cursor history and query binding do not. Superseded pending requests are
discarded; the active request completes without being rendered into another
revision. Unmount discards the pending read and prevents later publication.
This does not claim transport cancellation or a global limit across independent
selectors or separate mounts.

Loading, an empty canonical ledger and a failed read have different messages.
Failure preserves the exact selection, exposes Retry and directs a stale
revision back to the existing workspace refresh. Rust remains the authority for
value validation, BINARY ordering, counts and continuation membership. UI
checks bind the response to the requested facet, revision and query hash and
check bounded page/count/continuation consistency. No business calculations,
SQL, source verification or financial rules are added to the component.

## Verification

The seven dedicated Playwright cases use the actual Rust development command bridge
and canonical import/review commands with synthetic data: 205 distinct accounts,
125 currencies and accepted, pending, rejected and deferred records. They cover:

- Exact first-page equality with the canonical reader, no automatic later-page
  reads, real First/Back/Next ranges and final-page bounds in both consumers.
- Leading-zero and off-page selections retained through paging, exact applied
  account/currency values, and separate unapplied draft/calculation behavior.
- Actual stale-revision rejection, preserved draft/result state, refresh and
  first-page restart at the new revision.
- A transport failure that is distinct from empty, real Retry and keyboard focus.
- Two canonical revisions behind a held real response: one active and only the
  newest pending request per selector, with no obsolete publication.
- A changed selection while a real continuation is held, with no later reset
  or focus theft after a deliberate move and blur. A separate control verifies
  status focus when the pending action falls to the document element.
- Empty canonical scope and axe checks at desktop/compact widths and in empty
  and error states.

The final focused run passed all 24 cases (seven facet cases and 17 existing
pattern/comparison cases). The production TypeScript/Vite build passed; the
selector axe checks recorded zero violations for desktop, compact, empty and
unavailable states. The existing pattern/comparison suites exercise calculations, source
inspection, escaping, revision failures and keyboard access. Local evidence is
written under ignored `artifacts/transaction-facets-ui/`; the initial failed test
runs remain there. One early assertion expected different stale-error wording;
four existing partial-label selectors were made exact after the new pager
labels made them ambiguous. Neither change relaxes core behavior. Bounded peer
review found the moved-then-blurred focus gap; the marker and both focus
regressions were added before handoff.

Rendered comparisons (synthetic data):

- [Patterns, desktop](review/transaction-facets-patterns-desktop.png).
- [Patterns, compact](review/transaction-facets-patterns-compact.png).
- [Comparison, desktop](review/transaction-facets-comparison-desktop.png).
- [Comparison, compact](review/transaction-facets-comparison-compact.png).
- [Unavailable choices](review/transaction-facets-unavailable.png).

This slice does not switch the desktop response or remove the ledger array from
other consumers. It makes no native-desktop, performance or complete-release
claim. The existing production bundle-size warning remains.
