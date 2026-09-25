# DOCX catalogue implementation comparison

This bounded comparison uses the editable [catalogue frame 2002:2](https://www.figma.com/design/CphN4aTS8IXSVlKX7zDJtg?node-id=2002-2) and [capture outcomes frame 2002:58](https://www.figma.com/design/CphN4aTS8IXSVlKX7zDJtg?node-id=2002-58), with the native-text corrections recorded in [the design handoff](DOCX-SNAPSHOTS-DESIGN.md). The images below are actual application panels backed by an isolated fictional workspace, captured with the existing Playwright workflow. They are implementation evidence in addition to the editable Figma frames, not substitute design frames or owner approval.

## Findings and changes

| Design requirement | Observed before the repair | Implemented and checked |
|---|---|---|
| Visible verification boundary before using a record | The metadata warning followed every record and the pager. | A separate shaded metadata notice precedes the list. It explicitly distinguishes listing from artifact verification and current-state regeneration. |
| Reachable page controls before a long catalogue | First/previous/next controls appeared below all returned records. | Range and First/Back/Next controls precede the records. A real 43-record test proves placement and complete 20/20/3 traversal, including keyboard activation and focus protection. |
| Captured revision and identities form a readable instrument | A small revision pill, large timestamp and undifferentiated identity paragraphs competed for attention. | A graphite header pairs source revision with the verbatim timestamp. A definition list labels the full snapshot ID and document/DOCX digests. The byte count and retained generator/template remain visible. |
| Uncertain capture remains attached to its original request | Existing retry and recovery semantics were correct, but the unframed explanation used ambiguous “Refresh” wording. | A bordered capture state retains the request/revision and explicitly says “Workspace refresh” and “only while this application remains open.” The same wording constraint was corrected in editable Figma. |
| Verified local save has a distinct outcome and location | The success message and long machine-specific location were one inline string. | A separate success label precedes the verbatim location. The public image masks only the location; the test compares actual saved bytes with the retained DOCX artifact. |

The change reuses existing graphite, amber, border, type and control tokens through narrowly scoped CSS. It preserves the assessment panel heading and right-aligned capture action. The Figma specimen has a larger standalone heading and adjacent capture/refresh actions; those are deliberate composition differences for the embedded section, not pixel-equality claims. Every returned application record uses the same expanded card; the specimen’s second example uses a quieter abbreviated presentation. The current “Save DOCX file” label remains unchanged, with the metadata notice and receipt explaining the destination and verification.

The capture UUID, revision guard, outcome lookup, catalogue read lane, cursor history and native prepare/commit/discard behavior are unchanged. No Rust, command, schema or transport files changed in this slice.

## Retained visual evidence

All nine panel PNGs were visually inspected at their exported resolution. No text clipping, overlap or omitted digest characters was observed. Desktop screenshots use a 1440 × 1000 browser viewport and compact screenshots a 960 × 1000 viewport; the embedded panels are respectively 1200 and 720 pixels wide because the application navigation remains present.

| Application state | Captured panel |
|---|---|
| Empty, confirmed catalogue | [Empty](review/docx-snapshots/application/empty-desktop.png) |
| Captured record, full metadata | [Desktop catalogue](review/docx-snapshots/application/catalogue-desktop.png), [compact catalogue](review/docx-snapshots/application/catalogue-compact.png) |
| Metadata unavailable, separate from empty | [Compact unavailable](review/docx-snapshots/application/unavailable-compact.png) |
| Lost acknowledgement, retained request | [Uncertain capture](review/docx-snapshots/application/uncertain-desktop.png) |
| Saved result followed by failed refresh | [Saved, refresh failed](review/docx-snapshots/application/saved-refresh-failed-desktop.png) |
| Confirmed absence at a later revision, explicit new capture | [Later revision](review/docx-snapshots/application/later-revision-absence-desktop.png) |
| Confirmed absence at the same revision, same-request retry only | [Same revision](review/docx-snapshots/application/same-revision-absence-desktop.png) |
| Verified save through the actual Rust export session | [Saved receipt](review/docx-snapshots/application/native-receipt-desktop.png) |

The screenshots contain generated fictional IDs, timestamps and hashes, not the illustrative identities in Figma. The native-receipt image masks the real machine-specific destination with a neutral rectangle through Playwright’s screenshot mask; it does not replace the tested file path or exported bytes. The native bridge in that browser test is routed to the real Rust `native_export_session` helper. This is not a new macOS WebKit interaction or operating-system save-dialog observation.

## Verification

The unchanged baseline passed all 13 DOCX workflows before the UI repair. After the change and the peer-discovered Back label-in-name correction, the same full file passed **13/13**. A final receipt-only style adjustment was followed by its **1/1** affected workflow and the final production build. The build passed with the existing bundle-size advisory; there was no new build failure. Test source formatting after the full run was whitespace-only.

The workflow uses actual canonical Rust commands for import, capture, catalogue and typed recovery, and actual saved-artifact comparison through the native export helper. Delayed, rejected and lost acknowledgements are injected at the transport boundary to exercise failure handling; successful response bodies and frozen artifacts come from Rust. Global setup rebuilds the CLI and export helper from the checked-out source.

The retained suite covers creation and uncertain acknowledgement reuse, typed same/later revision absence, corrupt metadata/artifact rejection, stale and late responses across navigation, unmount completion, late preparation discard, lost commit acknowledgement and historic frozen bytes after a later workspace change. Added assertions require the warning and pager above the first of 20 actual rows, Next via Enter with status focus, no focus theft after a deliberate move and blur, and explicit open-application recovery wording. The accessible Back name now includes its visible label: “Back to previous DOCX page.”

All nine captured states have zero reported axe violations and no unresolved axe rule results in the tested panel scope. The existing compact horizontal-overflow assertion also passed. These checks do not replace screen-reader or voice-control testing. [The comparison manifest](review/docx-snapshots/comparison.json) binds the exact source files, test/build log hashes, helper binaries, PNGs and accessibility observations; raw logs remain local.

No new native WebKit, Word, 200% zoom, complete keyboard traversal or assistive-technology acceptance is claimed. A dedicated compact editable frame, responsive state specifications, linked component instances, auto-layout and interactive prototype wiring remain open. This remains a reviewed prototype with a bounded implementation comparison, not final product-design approval or a complete release.
