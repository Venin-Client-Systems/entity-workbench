# Industrial collection review — design handoff

Collection history now opens the validated acquisition receipt returned by Rust. The analyst can inspect each charged request, follow its recorded parent, review retained text as escaped data and export a hash-bound local acquisition bundle. Job outcome, missing responses and local retention remain separate facts.

The design was authored in the existing Figma Design file using native editable text and vector layers, named and positioned independently beside the earlier workflow frames. The [collection-review frame 27:528](https://www.figma.com/design/O50ISV0LGG8nKDcFORNMbO?node-id=27-528) is a 920×1100 full-content specimen. The [collection-exceptions frame 27:606](https://www.figma.com/design/O50ISV0LGG8nKDcFORNMbO?node-id=27-606) is a 920×630 paired specimen for partial retention and an unavailable legacy/interrupted receipt. Both were inspected in the editor and exported through Figma. They contain editable primitives, not a pasted screenshot. Their fixed example hashes, times and byte counts are illustrative synthetic design content; measured application values come from the Rust fixture.

## Components and behaviour

| Element | Specification |
|---|---|
| Surface | Native dialog, 920 px maximum width, 90vh maximum height and inner scrolling; split ledger/inspector at desktop width, stacked below 760 px |
| Foundations | Existing graphite/amber tokens, bundled Inter and JetBrains Mono, square controls, 1 px rules, amber selection edge and visible keyboard focus |
| Job context | Exact job ID, synthetic/live acquisition mode, explicit terminal status, selected URLs, limits, UTC timestamps and receipt revision range |
| Accounting | Charged requests including access reviews, redirects and failures; elapsed time, distinct retained originals and separate retention completeness; no invented rows for uncharged reservations |
| Request ledger | Outcome filter and individually labelled buttons; URL, purpose, parent, hop and HTTP/outcome labels; 01-based presentation corresponds to zero-based receipt sequence 0 |
| Ancestry | Parent controls select the recorded predecessor and reset the outcome filter. A parent sequence of zero remains valid. Redirect destinations are displayed as text and do not trigger navigation |
| Original inspector | Response media type, complete body byte count, response SHA-256 and retained evidence ID; zero-byte originals are explicit; incomplete, blocked and failed attempts have no invented body hash |
| Source text | Current retained text derivative from the workspace, linked through original evidence ID. React text rendering never executes raw collected HTML, scripts, SVG, images or external links. The source dialog closes back to its opener |
| Missing evidence | Partial retention disables export and explains why. Legacy/interrupted jobs keep their job context and show receipt-unavailable/retry instead of successful-empty results |
| Export | Rust revalidates receipt/originals, creates a new local export and returns a workspace-relative manifest path, SHA-256, snapshot revision and timestamp. UI reports success only after that command succeeds, then refreshes the workspace. Closing is disabled during export |
| Meaning | Retention completeness does not turn an incomplete body into an original. Searchable pages and acquisition facts do not establish relevance, source independence, access approval or republication permission |

## Rendered comparisons

| View | Editable design export | Working application |
|---|---|---|
| Request review | [Figma review](review/collections/figma-collection-review.png) | [1440×1000 desktop viewport](review/collections/collection-review-desktop.png), [complete surface in a 1440×2200 comparison viewport](review/collections/collection-review-full-surface.png), [720×900 compact viewport](review/collections/collection-review-compact.png) |
| Missing evidence | [Figma exceptions](review/collections/figma-collection-exceptions.png) | [Partial retention](review/collections/collection-partial.png), [unavailable receipt](review/collections/collection-unavailable.png) |
| Export feedback | Uses the review frame's local export area | [Saved acquisition bundle](review/collections/collection-export-saved.png) |

Application captures are modal/region crops of the stated viewports. The full-surface capture uses a taller comparison viewport to show the complete instrument; ordinary windows scroll. The implementation adds full URLs, timestamps, an outcome filter, receipt metadata, explanatory notes and real export feedback beyond the compact design specimen. It preserves the instrument hierarchy and selected-request treatment. A visual pass corrected global header-style interference and checked compact wrapping. These are manual design comparisons, not pixel-equality assertions or owner sign-off. [Checksums](review/collections/checksums.json) bind the retained PNGs and accessibility readouts.

## Verification and fixture boundary

`ui/tests/collection-review.spec.ts` contains seven synthetic browser workflows against the actual development Rust executable. No API response stubs, external collection or arbitrary database mutation endpoint are used. They cover redirect/link ancestry (including parent zero), filter behaviour, escaped hostile-looking text with zero external browser requests, source and review focus restoration, zero-byte/unsupported originals, incomplete/blocked/failed outcomes, partial/unavailable receipts, retry, real export hashes and original bytes, immutable earlier exports, export rejection after deliberate original corruption, compact wrapping and header containment.

The [desktop accessibility result](review/collections/accessibility-desktop.json) and [compact result](review/collections/accessibility-compact.json) each record zero automated axe violations under WCAG 2 A/AA and 2.1 AA rules. Automated rules and Chromium keyboard checks do not replace screen-reader and native-platform verification.

The fixed `ew-dev seed-collection-review <fresh-workspace>` helper builds eight explicitly synthetic jobs through the canonical collection start/finish publication paths. It accepts only a workspace path, rejects any prior records or revisions, performs no networking and preserves the existing `seed_demo` command. One deliberate response mismatch exercises real partial-retention publication; a fixed interrupted legacy job has no receipt. This helper and its public Rust method exist only with `debug_assertions`; there is no new public command variant or desktop dispatch entry. A release-profile build check covers its compile-time exclusion. The helper belongs to development tooling, not release data generation.

Validated on macOS Apple Silicon with the repository's sandbox-enabled Chromium Playwright harness: UI build; all 13 browser workflows; all 68 core Rust tests; release-profile core/dev-harness compile check; strict Clippy. The existing large frontend-chunk warning remains. No performance claim follows from these small fixtures.

## Remaining review and delivery gates

This completes the bounded acquisition-review UI increment. It does not establish a completed broad discovery benchmark, relevance labels, source-access review, worker confinement, offline native installation, signed packages or overall EW-05 completion. Live application campaign exports still need the frozen benchmark's independent review and scoring process.

Figma component/auto-layout conversion, interactive prototype wiring, owner acceptance, screen-reader review, 200% zoom, native WebKit/WebView2 interaction and actual Windows/macOS Intel release-artifact checks remain separate verification work. The canonical collector, export format and broader release gates retain their existing limits.
