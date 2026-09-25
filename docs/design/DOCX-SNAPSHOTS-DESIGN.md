# Editable DOCX catalogue and capture outcomes

The editable [development design](https://www.figma.com/design/CphN4aTS8IXSVlKX7zDJtg) now contains two DOCX instruments, authored and verified in Figma on 25 September 2026. The working copy is named **Entity Workbench — development design**. The original historical design file and its sharing settings were not changed.

| Specimen | Editable frame | Figma export |
|---|---|---|
| Catalogue, frozen identities and local-save receipt | [2002:2 — DOCX snapshot catalogue](https://www.figma.com/design/CphN4aTS8IXSVlKX7zDJtg?node-id=2002-2), 1120 × 1300 | [Catalogue PNG](review/docx-snapshots/figma-catalogue.png) |
| Uncertain capture, same/later revision recovery, partial success, empty and unavailable states | [2002:58 — DOCX capture outcomes](https://www.figma.com/design/CphN4aTS8IXSVlKX7zDJtg?node-id=2002-58), 1120 × 1480 | [Outcome PNG](review/docx-snapshots/figma-capture-outcomes.png) |

The frames use native editable Figma text and vector layers, imported through the editor and positioned alongside the existing instrument frames. They are not bitmap screenshots on the canvas. Selecting the outcome status text at node `2002:96` exposed the native Text and Typography controls, including Inter Bold at 16 px, position, dimensions and fill. Existing reusable components and historical frames remain in the development copy; the new specimens reuse their visual foundations and control patterns, rather than claiming to be linked component instances.

The design follows the graphite/amber, hard-edge instruments described in [assessment review](ASSESSMENT-REVIEW.md) and the existing ledger frame `57:2`. It uses Inter prose, JetBrains Mono instrument labels, thin borders, 36–40 px actions and restrained amber/error status surfaces. There are no decorative illustration or gradient layers.

## Contract represented

The [implemented DOCX workflow contract](DOCX-SNAPSHOTS-UI.md) remains authoritative. The catalogue is a separate assessment section from historical HTML reports. It shows the captured source revision, retained timestamp, snapshot identity, full document and DOCX digests, byte count, template and generator versions. Its metadata notice explicitly distinguishes listing from original/artifact verification. The catalogue specimen shows two expanded example records representing a paginated list, not a claim that two rows constitute the entire displayed twenty-record page.

The outcome frame collects **independent states**, not one simultaneous application screen:

- An unconfirmed acknowledgement retains the same request and captured revision. Retry and explicit outcome lookup are separate actions.
- Absence at the same revision permits only the same request to be retried; the old request could still publish.
- Absence at a strictly later revision permits an explicit new capture after the visible workspace catches up. The acknowledged old outcome remains in recent history.
- A saved snapshot with a failed refresh is presented as saved, with refresh as the recovery action.
- Confirmed empty catalogue and unavailable metadata remain different states. Corruption does not become an inferred absence.

The save receipt is illustrative and synthetic. Native saving verifies the frozen document and DOCX, returns the app-managed location, and does not regenerate from current workspace state. These design values are not real exported-artifact hashes or a new native-export test result.

## Verification and remaining work

Both frames were inspected in the Figma editor, exported through Figma, and examined at their full exported resolution for clipping, overlap and readable hierarchy. A normal reload then retained the renamed file, both named frames, their dimensions and positions. The initial export of each frame was **byte-identical** to its initial post-reload export. The initial hashes remain in the [verification record](review/docx-snapshots/checksums.json).

The comparison follow-up corrected two native text layers: outcome guidance now says **workspace refresh**, limited by the following open-application sentence, and the illustrative export basename now uses the actual `assessment-` prefix. Both corrected frames were exported after a fresh load and inspected. The corrected catalogue also has matching pre/post-reload exports; the attempted corrected outcome export before reload did not complete, so no equivalent pre/post claim is made for that revision. The current PNGs are these corrected exports.

The [implementation comparison](DOCX-SNAPSHOTS-COMPARISON.md) records the subsequent bounded UI repair and real-core browser evidence. The original design-only handoff changed no application code. Editable-tool evidence and the comparison do not establish owner sign-off, pixel equality, a compact Figma specimen, 200% zoom, screen-reader behaviour or complete report assembly/exhibits. Responsive specimens, linked component conversion, auto-layout and interactive prototype wiring remain open.
