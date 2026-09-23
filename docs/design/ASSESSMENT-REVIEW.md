# Industrial assessment review — design handoff

The assessment increment follows the owner-selected industrial direction. The [editable finding-review frame 25:361](https://www.figma.com/design/O50ISV0LGG8nKDcFORNMbO?node-id=25-361) was created in the existing Figma Design file as native text and vector layers, named and positioned alongside the statement instruments, inspected in the editor and exported from Figma. It is a 920×980 full-content specimen. The [editable finding-editor frame 25:398](https://www.figma.com/design/O50ISV0LGG8nKDcFORNMbO?node-id=25-398) adds a 920×1180 specimen using the same native primitives, inspected and exported in Figma. The native layer tree exposes individual labels and geometry; the frame is not a screenshot pasted onto the canvas.

## Components and behaviour

| Element | Specification |
|---|---|
| Surface | 920 px maximum-width native dialog, 90vh maximum height, inner vertical scrolling |
| Foundations | Existing graphite/amber palette; bundled Inter prose and JetBrains Mono instrument labels; hard edges and 36 px controls |
| Review hierarchy | Title and explicit review state, assessment, limitations, linked questions, opposing citations, decision, history |
| Citations | Separate supporting/contradictory sections; record value and review status, retained source name and anchor, source-inspection control |
| Source origin | Count distinct origin groups without claiming independence; whole-source citations are labelled separately from accepted extracted observations |
| Authoring | Questions, alternatives and gaps; editable finding content, multi-record citation roles and linked questions; selected citations persist when filtering the available list |
| Decision | Required reason; Rust verifies accepted observation/transaction citations, anchors, original hashes and workspace revision before recording review |
| Correction | Edits require a reason and reopen review. Existing evidence changes conservatively reopen all current findings; report snapshots remain unchanged |
| Keyboard | Native modal focus containment; nested source closes back to its opener; forms retain failed drafts; writes disable form edits and closing |

## Rendered comparisons

| View | Design source | Working application |
|---|---|---|
| Finding review | [Figma export](review/assessments/figma-finding-review.png) | [1440×1000](review/assessments/implementation-finding-review-1440.png), [960×1000](review/assessments/implementation-finding-review-960.png) |
| Finding editor | [Figma export](review/assessments/figma-finding-editor.png) | [1440×1000](review/assessments/implementation-finding-editor-1440.png), [960×1000](review/assessments/implementation-finding-editor-960.png) |

The application uses content-driven row heights and scrolls within the dialog. Its source buttons have explicit control borders and the operational review guidance is longer than the specimen. Supporting and contradictory evidence retain the same hierarchy, with amber reserved for the review action and pending status. Screenshots are manual comparisons, not pixel-equality tests or owner sign-off. [Image hashes](review/assessments/checksums.json) identify the saved evidence.

[The accessibility readout](review/assessments/accessibility.json) records zero automated axe violations in all four assessment states and retains manual-review rule names. Source-dialog focus restoration, viewport resizing without lost review text, horizontal modal overflow and failed-write draft retention are covered by the Rust-backed browser workflows. A dedicated question Figma frame, auto-layout component conversion, interactive prototype wiring, screen-reader checks, 200% zoom and Windows/Intel native interaction remain open.
