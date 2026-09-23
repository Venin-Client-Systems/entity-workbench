# Editable design and implementation comparison

Review date: 2026-09-22. Current direction: **industrial / technical**, selected by the owner. This is a revised development interface; final visual and accessibility sign-off remains open. Case content is synthetic.

## Current design source

[Instrument — transaction review, Figma frame 6:2](https://www.figma.com/design/O50ISV0LGG8nKDcFORNMbO?node-id=6-2) is the current editable reference, 1440 × 1000. It was duplicated from the first transaction frame and edited through Figma's native layer, auto-layout, colour and dimension controls. The original frames remain intact for comparison. This revision is a native frame with editable text and controls, not a flattened screenshot or a generated application imported as the design.

The revised frame has a 208 px graphite navigation rail, 56 px header, 360 px inspector, joined metric cells, 16 px panel padding and square 36 px action/search controls. The ledger gutters and navigation spacing are tighter. Burnt amber marks selection and actions. Status text was darkened in Figma as well as the app.

The application applies this visual system across its eight sections. The overview uses joined metric cells and ruled section headings. Transaction review keeps the ledger beside the source and decision controls. Graph nodes, chart bars and map markers use the same graphite/amber family. Plain section names replace the overview's promotional headline.

## Applied foundation

| Token | Applied value |
|---|---|
| Canvas / surface | `#E7EAE8` / `#F4F5F3` |
| Navigation / selected navigation | `#232A2C` / `#394347` |
| Text / secondary text | `#202729` / `#4F5C60` |
| Action and focus / selected row | `#A64814` / `#F5E5C5` |
| Panel rule / control outline | `#BCC3C0` / `#747F7C` |
| Accepted text / background | `#166534` / `#DCFCE7` |
| Pending text / background | `#92400E` / `#FEF3C7` |
| Rejected text / background | `#B91C1C` / `#FEE2E2` |
| Navigation / header / desktop inspector | 208 / 56 / 360 px |
| Body and table / secondary labels | 13 / 12 px |
| Main controls / navigation row | 36 / 38 px minimum height |
| Panel padding / ledger cell padding | 16 px / 6 px vertically, 10 px horizontally |
| Corners | Square |
| Typography | Bundled Inter 4.1; bundled JetBrains Mono 2.304 for references, amounts and instrument labels |

Both typefaces ship locally with their SIL Open Font License files. The unmodified fonts come from the official [Inter release](https://github.com/rsms/inter/releases/tag/v4.1) and [JetBrains Mono release](https://github.com/JetBrains/JetBrainsMono/releases/tag/v2.304). Exact asset hashes and sources are in `sbom/bundled-assets.json`. No font service or first-run font download is used. Source typography disables ligatures so literal identifiers remain legible.

## Working review behaviour

At 1280 px and above, a persistent inspector leaves the ledger available. Smaller windows use a named native dialog. Review drafts survive crossing the breakpoint. Source inspection is a separate modal, opening the actual anchored row/cell. Escape restores focus to its opener. Filtering out the selected transaction displays an explicit message.

Original source values and editable values remain separate. Decisions require a reason; corrections preserve originals and reopen review. All validation, calculations and writes remain in Rust. The design revision adds no analytical decision logic.

The native macOS check exposed default select controls shrinking despite minimum-height styling. Explicit select appearance now preserves the native option menu while applying the square 36 px control box; forced-colour mode restores system appearance. Graph action groups and the local map now have explicit semantic roles for their accessible names.

## Rendered comparison evidence

- [Native Figma industrial export, 1440 × 1000](review/industrial/figma-transaction-1440.png).
- [Working desktop review, 1440 × 1000](review/industrial/implementation-transaction-1440.png).
- [Working compact review, 960 × 640](review/industrial/implementation-transaction-960.png).
- [Working overview, 1440 × 1000](review/industrial/implementation-overview-1440.png).
- [SHA-256 image inventory](review/industrial/checksums.json).

These are inspectable design comparisons, not pixel-difference acceptance tests. Figma contains illustrative amounts and dates; application images show repository fixtures and real Rust commands. The app retains separate checks/review columns, posting dates, original currencies, transfer controls and its eight-section navigation rather than inheriting missing or conflated concepts from the original generated specimen.

## Verification and remaining work

- All four browser workflows pass against the real Rust core, including correction/acceptance, source anchors, resize drafts, filtered selection, compact modal keyboard containment, identity decisions and persistence. Tests assert no external browser requests or uncaught page errors.
- Automated axe checks cover the eight main section states, authored identities and both transaction review layouts. Ten declared text/background pairs pass 4.5:1, with a lowest measured ratio of 5.30:1. See `contrast-results.json` and `accessibility-results.json`. These checks do not establish full WCAG conformance.
- The rebuilt Apple Silicon development app displays the revised interface and bundled fonts. Native source inspection and Escape/focus return are checked separately from Chromium.
- The current industrial Figma revisions cover the desktop transaction workflow and statement mapping/preview. Compact and remaining workflow frames still need the same design treatment. All eight application sections use the revised styles, but this is not a claim that eight industrial design frames have been completed.
- Full 1280 px design coverage, 200% zoom, assistive-technology checks, complex empty/error/long-content states, final icon refinement and owner visual approval remain open. The synthetic ten-row ledger is not a large-data performance benchmark.
- Complete bundled installation, signing, platform confinement and broad-web coverage remain separate unpassed release gates.

## Previous direction

The first warm/teal direction is retained as design history, not the current target: [foundations 2:790](https://www.figma.com/design/O50ISV0LGG8nKDcFORNMbO?node-id=2-790), [transaction review 2:6](https://www.figma.com/design/O50ISV0LGG8nKDcFORNMbO?node-id=2-6), [compact identity 2:576](https://www.figma.com/design/O50ISV0LGG8nKDcFORNMbO?node-id=2-576), and [collection jobs 2:255](https://www.figma.com/design/O50ISV0LGG8nKDcFORNMbO?node-id=2-255). Earlier exports remain in `review/`.

The separate Figma Make experiment is not the application. Its generated identity and collection logic is not authoritative; known illustrative comparison errors must not be copied into domain rules.

## Statement import extension

The two-stage local statement workflow now has [editable Figma frames, component specifications and rendered comparisons](STATEMENT-IMPORT.md). It preserves the industrial visual language while separating source mapping, full-row validation and pending import.

The next assessment increment adds an [editable finding-review frame and working authoring/review flow](ASSESSMENT-REVIEW.md). Its source export, 1440/960 comparisons, accessibility observations and remaining frame scope are retained there.
