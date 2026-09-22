# Editable design and implementation comparison

Review date: 2026-09-22. This is the first applied design pass, not final visual or accessibility sign-off. All case content is synthetic.

## Design source

The native Figma file contains real frames, auto layout, colour/text styles and reusable component sets. The authoring session verified frame dimensions and exported pixels through Figma itself. The separate Make experiment is not the application and its generated business logic is not used.

| Reference | Frame | Observed dimensions |
|---|---|---|
| [Foundations and components](https://www.figma.com/design/O50ISV0LGG8nKDcFORNMbO?node-id=2-790) | `2:790` | 1440 × 1777 |
| [Transaction review](https://www.figma.com/design/O50ISV0LGG8nKDcFORNMbO?node-id=2-6) | `2:6` | 1440 × 1000 |
| [Compact identity comparison](https://www.figma.com/design/O50ISV0LGG8nKDcFORNMbO?node-id=2-576) | `2:576` | 960 × 640 |
| [Collection jobs](https://www.figma.com/design/O50ISV0LGG8nKDcFORNMbO?node-id=2-255) | `2:255` | 1440 × 1000 |

The canvas also contains desktop identity, compact transaction/collection variants and the reusable-component board. Native components include buttons, inputs, review badges, citations and job-state pills. The file remains editable in Figma; links do not change its sharing permissions.

## Applied foundation

| Token | Applied value |
|---|---|
| Canvas / surface | `#FAF8F5` / `#FFFFFF` |
| Navigation / selected navigation | `#0F172A` / `#1E293B` |
| Text / secondary text | `#18181B` / `#515159` |
| Action and focus / selected row | `#0F766E` / `#F0FDFA` |
| Accepted text / background | `#166534` / `#DCFCE7` |
| Pending text / background | `#92400E` / `#FEF3C7` |
| Rejected text / background | `#B91C1C` / `#FEE2E2` |
| Navigation width / header height | 224 px / 64 px |
| Body and table / secondary labels | 14 px / 12 px |
| Main controls | 40 px minimum height |
| Typography | Bundled Inter 4.1; local system monospace for identifiers and source values |

The original generated status colours are too light for some small-text combinations. The application darkens status text rather than copying a generated claim of conformance. The native foundation's unverified “PASS” and conformance labels were manually replaced with a contrast target and a verification requirement. Token definitions are in `ui/src/tokens.css`; third-party font licensing ships in `ui/public/fonts/Inter-LICENSE.txt` and `NOTICE`. The font is unmodified from [the official Inter 4.1 release](https://github.com/rsms/inter/releases/tag/v4.1); there is no font service request at runtime.

## Working transaction review

At 1280 px and above, a persistent review panel keeps the transaction ledger available. Smaller windows use a named native dialog. Source inspection remains a separate modal. The review draft survives crossing the layout breakpoint. Filters remain usable alongside desktop review, and an explicit message appears when the selected transaction falls outside the current filters.

The selected row is highlighted. The review panel retrieves the original anchored value from Rust, while the editable amount remains a separate value. The source dialog now receives that anchor, so it opens the actual cited row/cell. Decisions still require a reason; corrections preserve evidence and reopen review. All totals, source validation and writes remain in Rust.

The native WebKit inspection exposed focus-return behaviour differing from Chromium. Buttons now establish their focus before opening a dialog, and dialog cleanup explicitly restores the connected opener. The native build is checked separately from browser tests.

## Rendered comparison evidence

- [Native transaction design export](review/figma-transaction-1440.png).
- [Working desktop review, 1440 × 1000](review/implementation-transaction-1440.png).
- [Working compact review, 960 × 640](review/implementation-transaction-960.png).
- [Native compact identity design export](review/figma-identity-960.png).
- [Native collection design export](review/figma-collection-1440.png).

These are inspectable references, not pixel-difference acceptance tests. The native design has illustrative values and incomplete labels. The application screenshots use the actual repository fixtures and real Rust commands; their amounts and dates intentionally differ.

## Verification and open discrepancies

- Browser workflows check transaction correction/acceptance, source anchors, draft retention through resize, filtered selections, keyboard restoration, compact modal containment, identity decisions and persistence. Application requests stay local in these tests.
- Automated axe checks cover the eight main section states, authored identities and both transaction review layouts. Measured token pairs are recorded in `contrast-results.json`. Manual results remain distinct from automated checks.
- The full-size native transaction frame was resized to the specified height and its crowded balance/status header corrected through Figma. The application's table uses independent checks and review columns, exact currencies, posting dates and a named horizontal scrolling region.
- Generated identity badges conflate conflicts with review status. The application retains separate domain signals and the real pending/accepted/rejected/deferred review states. The Make experiment also labels an identical identifier as conflicting; it is not accepted as a logic specification.
- The generated collection frames contain illustrative source names, incomplete disclosure wording and controls for jobs not yet implemented. They do not prove provider-free broad coverage or cancellation support. The application keeps its actual broker disclosure, supported HTTPS scope and distinct job outcomes.
- At compact sizes, generated header metadata clips. The application uses a reachable review dialog and tested controls instead of reproducing that clipping. The complete set of 1280 px frames, 200% zoom inspection and all eight workflow designs remains open.
- Monospace typography uses OS fonts rather than the generated JetBrains Mono choice. The numbered navigation remains; a reviewed icon family is still pending.
- Further native keyboard/screen-reader checks, complex empty/error/long-content states, final frame corrections and visual approval remain open. An automated zero-violation result does not establish WCAG conformance.
