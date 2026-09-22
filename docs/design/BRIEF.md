# Entity Workbench product design brief

Status: editable design study and first implementation comparison completed; full visual sign-off remains open. The signed-in Figma session contains native frames, shared colour/text styles and five reusable component sets. Transaction review, identity comparison and collection each have desktop and compact variants on the same canvas. Generated content was inspected and is not an approved product specification.

- [Native Figma Design: design system and workflows](https://www.figma.com/design/O50ISV0LGG8nKDcFORNMbO) — the editable design reference.
- [Figma Make: interaction experiment](https://www.figma.com/make/wxFT4C8udxU0zW860eFBcz) — separate, synthetic prototype; known comparison-label errors remain.
- [Implementation comparison and open discrepancies](HANDOFF.md) — exact frame IDs, applied tokens, screenshots and verification scope.

Figma is a development design tool. The distributed application has no Figma, hosted font or generative-AI dependency. Rust remains authoritative for evidence and decisions; generated prototype calculations and fixtures were not imported into the application.

## Design deliverables

Produce an editable design file with pages for foundations, components, analyst workflows and handoff. Record the file and frame links here when they exist. Use synthetic content only. Preserve local SVG/icon/font assets with their licences; do not introduce remote asset requests into the application.

Design the three demanding workflows first: source-linked transaction review, namesake comparison and collection scope preview. Use the overview to expose work that needs attention, rather than make it a collection of decorative charts.

| Frame | Content and required states |
|---|---|
| Workspace overview | Case question, outstanding decisions, evidence register, revision; empty workspace and synthetic demonstration clearly distinct |
| Evidence review | Original/derivative distinction; acquisition URL/time; page/row anchor; unsupported and partial extraction; local search with stale/no-results/error states |
| Statement review | Persistent source context beside selected row, exact amount/currency, balance calculation with contributing rows, duplicate candidates and explicit decisions |
| Identity comparison | Two entities side by side; namespaces and leading zeros; supporting and conflicting observations; reasoned merge and reversal |
| Relationships | Selection and evidence panel; reviewed/unreviewed assertions; direction, dates and path explanation conveyed beyond colour |
| Locations | Coverage boundary; historical address period; merchant/branch/channel separately; uncertainty and unresolved denominator |
| Direct collection | Analyst-selected URLs, disclosed request information, request/hop/time limits; running, cancelled, blocked, quota, error and no-results states |
| Assessment | Findings with support, contradiction and limitations; invalidation after corrections; immutable snapshot identity and export |

Create frames for 1440×1000, 1280×800 and the minimum native 960×640 viewport. Also inspect browser zoom at 200%. Dense tables may scroll inside a labelled region; controls and source context must remain reachable. A narrow layout must preserve navigation labels or accessible names.

## Component specification

Use shared components and semantic tokens for text, surfaces, borders, focus, selection and review status. Do not assign colour to statistical confidence or source independence as though those were one judgement.

| Component | Variants and behaviour |
|---|---|
| Navigation | Current section, hover, keyboard focus, count with explicit meaning; accessible name remains at reduced width |
| Buttons | Primary, secondary, text, destructive; idle, hover, focus, disabled, working; one clear primary action per task |
| Inputs | Persistent labels, hint, validation message, error and disabled states; exact decimal values remain strings |
| Review status | Pending, accepted, rejected, deferred, invalidated; explicit text plus optional icon, colour supplementary |
| Data table | Stable identifier, selected row, sort/filter state, empty/filter-empty/error, source link, exact numbers aligned by column |
| Evidence reference | Source title, anchor, extraction state, original hash; acquisition history without treating copied sources as independent |
| Review panel/dialog | Named heading, context, reason, decision; keyboard containment, Escape, return to opener, nested source inspection |
| Job progress | Used/allowed limits, elapsed time and stop reason; no indefinite spinner after an interrupted operation |
| Report snapshot | Revision, timestamp, hash, citations and export; current findings and historical snapshot visually distinguishable |

## Measurable acceptance criteria

- Normal text contrast at least 4.5:1; qualifying large text at least 3:1. Measure rendered foreground/background values, not visual impression.
- Meaningful controls and focus indicators distinguishable against adjacent colours. Review status and graph meaning never rely on colour alone.
- Minimum target-size/spacing checks for controls, with comfortable 36–40 px primary controls as a project design target.
- Prefer 14 px body/table text and 12 px secondary labels as initial project targets; verify density with realistic rows. These sizes are design targets, not claimed WCAG requirements.
- Use system fonts or packaged licensed fonts. No font CDN, hosted icon library or map tiles.
- Tab and Shift+Tab remain inside the topmost modal; Escape closes it; focus returns to its opener. Source review can open above transaction review without exposing background actions.
- No clipped primary controls at the supported minimum viewport. Text zoom, long filenames, leading-zero identifiers, large decimal amounts and long error messages must remain usable.
- Run automated accessibility checks and keyboard tests, then inspect rendered native screens. Automated checks alone do not establish accessibility conformance.
- Compare final implementation screenshots to named design frames. Keep a discrepancy list and resolve it before visual sign-off.

## Evidence and pending work

- Prototype screenshots: `artifacts/ui-overview.png`, `artifacts/ui-locations.png` (generated locally; not design approval).
- Native modal component: `ui/src/Dialog.tsx`; keyboard verification belongs in the existing real-workspace UI test.
- Editable Figma files: linked above; inspected exports and implementation comparisons are recorded in HANDOFF.md.
- First palette, typography and layout pass: applied from native Figma foundations; status text darkened after measured contrast review. Broader component refinement remains open.
- Source publication: explicitly labelled a development prototype. Final visual sign-off remains pending resolution of the handoff discrepancies.

References: [W3C modal dialog pattern](https://www.w3.org/WAI/ARIA/apg/patterns/dialog-modal/), [text contrast](https://www.w3.org/WAI/WCAG22/Understanding/contrast-minimum.html), [target size](https://www.w3.org/WAI/WCAG22/Understanding/target-size-minimum.html).

## Prototype inspection notes

The native macOS app was launched and its corpus phrase query returned the expected synthetic source. The 960 px browser capture retains the toolbar but wraps statement dates; table density and fixed-width date/amount columns need to be resolved in the editable design. Automated measurements found 40 overview contrast failures before targeted corrections and zero violations in the eight subsequently tested section states. Incomplete rules still require manual checks. No final visual sign-off is recorded.

The next implementation pass replaced demonstration-specific identity actions with general entity/observation authoring and comparison using the existing provisional components. Namespaces, leading-zero references, cited excerpts, review states and decision history are now represented in the functional workflow. This is functional work, not a completed Figma design.

## Current owner-selected direction — industrial / technical

The first warm/teal pass was superseded by the industrial direction on 2026-09-22. The owner selected compact instruments, hard edges and utilitarian controls. The current [editable transaction frame 6:2](https://www.figma.com/design/O50ISV0LGG8nKDcFORNMbO?node-id=6-2) and [handoff](HANDOFF.md) define the revised graphite/amber palette, square 36 px controls, joined metric cells, 208 px rail, 56 px header, 360 px inspector and bundled Inter/JetBrains Mono typography. Earlier design observations above are historical. Final visual approval is still open.
