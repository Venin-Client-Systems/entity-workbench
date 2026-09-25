# Industrial statement import — design handoff

The statement import flow extends the owner-selected industrial/technical direction. Its two editable Figma frames were created from the existing instrument palette and typography, imported as native text and vector layers, refined in Figma Design and exported there. They are editable design specimens, not screenshots placed on a canvas. Auto-layout component conversion and interactive prototype wiring remain open.

- [01 Map columns — frame 13:239](https://www.figma.com/design/O50ISV0LGG8nKDcFORNMbO?node-id=13-239), 920×1370 full-content specimen.
- [02 Preview — frame 13:163](https://www.figma.com/design/O50ISV0LGG8nKDcFORNMbO?node-id=13-163), 920×980 full-content specimen.
- [Industrial transaction review foundation — frame 6:2](https://www.figma.com/design/O50ISV0LGG8nKDcFORNMbO?node-id=6-2).

Both new frames were inspected in the actual editor. The layer tree exposes editable statement labels and amounts, with separate geometry. The preview's mapping-name field was refined to remove an inappropriate dropdown chevron. Both PNG files below are exports downloaded from Figma.

## Components and behaviour

| Element | Specification |
|---|---|
| Modal surface | Maximum width 920 px; 90vh maximum height; inner vertical scrolling; native dialog semantics |
| Palette | Surface `#F4F5F3`, graphite text `#202729`, secondary text `#4F5C60`, border `#BCC3C0`, amber action `#A64814` |
| Typography | Bundled Inter for prose/controls; JetBrains Mono for stage labels, amounts, identifiers and source hashes |
| Geometry | Hard edges, 1 px dividers, 36 px controls; two-column field groups at tested desktop sizes |
| Source sample | Collapsible, independently scrollable table; first five logical records; 240-character displayed cell limit |
| Interpretation | Explicit date, decimal, row-order and debit/credit selections precede column mapping |
| Preview instruments | Joined strip: source rows, invalid rows, available-balance mismatches; amount cells and interpreted values remain separate |
| Review state | Import creates pending transactions; invalid rows or an existing original disable import; balance warnings remain reviewable |
| Keyboard | Preview focuses its heading; modal traps Tab; Escape closes to the opener; closing is prevented while canonical import runs |
| Async edits | Mapping edits invalidate in-flight preview responses; commit checks file, mapping and workspace revision again |

The mapping specimen includes layout/keyboard annotations below the product content; those annotations are not application text. The full-height Figma specimens show all content at once. The implementation deliberately scrolls that content inside the window-height modal, including at 960×640. The screenshot at the compact import action is scrolled down, so the heading is above the viewport. Table widths and wrapping follow actual content rather than fixed Figma positions.

## Rendered comparisons

| Stage | Figma export | Working application |
|---|---|---|
| Map columns | [920 px full content](review/statements/figma-mapping.png) | [1440×1000](review/statements/implementation-mapping-1440.png), [960×640, scrolled](review/statements/implementation-mapping-960.png) |
| Preview | [920 px full content](review/statements/figma-preview.png) | [1440×1000](review/statements/implementation-preview-1440.png), [960×640, scrolled](review/statements/implementation-preview-960.png) |

Visual inspection confirms the same hierarchy, square fieldsets, joined counts, original/interpreted value separation and restrained amber action. These are manual design comparisons, not pixel-difference tests or final owner approval. Image hashes are in [checksums.json](review/statements/checksums.json).

The real Rust-backed browser flows pass automated axe checks for both stages at 1440×1000 and 960×640, verify no horizontal document overflow, retain keyboard reachability of the action and test source-dialog focus return. [Accessibility readout](review/statements/accessibility.json) records zero violations and retains counts of checks needing manual review. The native Apple Silicon app separately passed mapping, preview, import and logical-cell source inspection through Tauri/WebKit. Windows/macOS Intel native interaction, screen readers, 200% zoom and longer error/column content remain further design acceptance work.
