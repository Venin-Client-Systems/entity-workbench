# Ledger scope and native save design

[Editable Figma frame 57:2](https://www.figma.com/design/O50ISV0LGG8nKDcFORNMbO?node-id=57-2), **20 / Instrument — Ledger scope and native save**, extends the industrial transaction foundation. The native frame is 1120×1140 at 33000, −500. Its text and vectors remain individually editable. Native layer, fill, font, position and export controls were inspected on 25 September 2026; five metric labels were corrected to `#BFC8C8` against `#232A2C` in Figma before export.

[The native PNG](review/ledger-scope/figma-ledger.png) was exported through Figma and inspected at its actual dimensions. It has no observed clipping or overlapping labels. The original unsynced tabs remain untouched. A separate saved-frame view timed out, so independent remote persistence verification remains open. The exported file and editable-layer observations are real authoring evidence; neither implies that the remote readback passed.

## Components and behavior

The specimen separates draft controls, successfully applied scope, matching counts, selected-row range, ledger rows and complete export. It uses square 36 px controls, Inter for ordinary labels, JetBrains Mono for source values and scope details, a 7 px burnt-amber top rule, joined graphite metric cells and fine neutral table rules. Amounts retain explicit currency and account references retain leading zeros.

The four visible rows illustrate repeated purchases and a refund; they are an excerpt, not the displayed page’s entire 100-row payload. Counts are illustrative synthetic values: 1,248 before review filtering, with 880 accepted, 326 pending, 18 rejected and 24 deferred. Pagination reads actual returned rows. Export identifies all 326 matching pending rows at revision 84, including rows outside the current page. A native save receipt gives its location and confirms reuse of an identical existing file.

The applied query remains distinct from edits to the draft. Account/currency choices have independent continuation and retain the selected exact value. Page failures must stay unavailable, rather than appear as zero results; a known revision change restarts the applied scope at its first page in the implemented interface. Complete export remains unavailable during an uncommitted draft or invalidated read. Source review and transfer decisions retain their separately validated versions and provenance.

## Comparison with the running implementation

The existing real-core [1440 px ledger](review/desktop-summary/ledger-1440.png) and [960 px ledger](review/desktop-summary/ledger-960.png) were inspected alongside the native design export. They agree on the industrial palette, square controls, explicit filters, independent facet paging, complete denominators, visible revision and complete-scope export. The compact implementation retains a focusable horizontally scrollable table rather than shrinking source values beyond legibility.

The current interface places controls in a responsive grid, expresses counts in text and keeps Export JSON beside Apply/Clear. The new specimen proposes joined count instruments and a separate export panel. Those presentation changes are not implemented by this design-only commit. The prior 110-case browser suite and actual native export proof remain tied to their recorded source; this frame does not retroactively establish those screenshots as pixel-equivalent designs.

Compact frame variants, stale/unavailable/empty-state frames, native remote readback, transfer-selection and citation extensions, keyboard/manual accessibility and owner visual approval remain open. [Artifact hashes](review/ledger-scope/checksums.json) bind the inspected source and comparison images. The earlier [pagination proposal](TRANSACTION-PAGINATION-PROPOSAL.md) remains history; its page-only export idea was superseded by the implemented complete-scope export.


## Recovered remote readback — 25 September 2026

After sign-in, a fresh view of original frame 57:2 showed the expected 1120×1140 layout, industrial controls, five count instruments and export receipt. This resolves the earlier remote-frame readback timeout. It does not prove every old unsynced edit survived. The original is view-only for the current account; [the recovery record](DESIGN-WORKING-COPY.md) identifies the editable Drafts copy and the subsequent browser-control limitation. No implementation layout or design-approval claim changes.
