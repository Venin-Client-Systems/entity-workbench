# Editable DOCX foundation

This is an engine-only foundation for EW-29 / issue 33. `report_document::capture`
freezes typed content from one already captured `WorkspaceView`; `report_docx::render`
turns that document into editable WordprocessingML. Neither function writes the
workspace, saves a report, exposes a command or changes existing HTML generation.
It does not complete report assembly, DOCX publication, exhibits or the release gate.

## Frozen contract and calculations

`ReportDocument` format 1 records the supplied report UUID, workspace revision,
creation time, template `assessment-foundation-1` and generator `ooxml-foundation-2`
for new captures. The reader and renderer retain generator `ooxml-foundation-1`
support so historical artifacts still verify against their original bytes.
Its content retains questions and alternatives, findings and both citation lists,
transactions, entities, observations, evidence metadata/text/acquisitions, identity
and merge decisions, and review history. Input vector order is retained. The frozen
JSON preserves exact strings, decimals, reference-number namespaces and leading
zeros, transaction versions, origin groups, source anchors and review states.

Totals, transfer exclusions, included row IDs and balance checks come from the
existing `analytics::analyse`; this module does not calculate independent monetary
results. Separate accepted/pending/rejected/deferred counts describe the entire
retained ledger. Deserialization and rendering validate the result against that
same engine. Any future analytical or retained-domain shape change must version
this format/generator deliberately; format 1 is not a published command/schema
or a promise that future software can silently reinterpret an older calculation.

The adapter checks unique typed record identities, evidence ID/digest equality,
reference existence, bounded anchor structure and text-line ranges. **It does not
open or rehash originals.** The separate [canonical publisher](DOCX-SNAPSHOTS.md) captures a consistent
view, verifies its original files, uses a revision conflict guard and publishes validated
artifacts with recoverable backup references. It must not rebuild a historical
DOCX from a newer workspace. Existing HTML-only snapshots remain HTML-only; their
saved HTML bytes and hashes are unchanged by this foundation.

There is no new source-anchor acceptance. A page/cell/message/capture anchor is a
retained input location, not a position verified by this renderer. Document page
numbers are not evidence page numbers. Plain location labels refer to the adjacent
source bookmark; the exact typed anchor remains in the frozen JSON.

## Limits and inert content

| Bound | Failure behavior |
|---|---|
| 5,000 transaction rows | Reject, never select the first rows silently |
| 10,000 total represented records | Reject |
| 10,000 nested list entries/references | Reject |
| 1 MiB UTF-8 per text field | Reject before capture clones fields |
| 16 MiB serialized frozen JSON | Capped preflight before clone, capped output/input |
| Original metadata within the existing import size policy | Reject unsupported size; no original read |
| 32 MiB per generated XML part and entire DOCX | Capped writes and ZIP seeks; no partial success |

The caller's `WorkspaceView` is already allocated. These are content/output bounds,
not a total process-memory or layout-page-count guarantee. Deserializing bounded
JSON still allocates its typed records. Rendering buffers XML and then the package.
No import/OCR runtime or network request is involved.

XML 1.0-invalid characters, dangling citations, ambiguous IDs, invalid money,
non-finite quality/region values and mismatched calculation results fail explicitly.
Literal HTML, URLs, selectors and formula-looking text become escaped text. The
writer creates only six fixed package parts and internal relationships. There are
no macros, external relationships, linked templates, fields, `altChunk`, embedded
originals, remote images or font downloads. IDs produce checked, deterministic
internal bookmarks; no input becomes a ZIP path or relationship destination.

## Deterministic packaging and layout

The Rust writer uses pinned `quick-xml` 0.42.0 (MIT) and `zip` 8.6.0 (MIT), with ZIP
default features disabled. ZIP entries are stored without compression, in fixed
order, with fixed 1980 metadata, Unix permissions and no wall-clock or host author
metadata. The only new transitive package is `typed-path` 0.12.3 (MIT OR Apache-2.0).
Exact locked checksums and licence copies are in [the dependency record](../../third_party/docx/README.md).

Native paragraphs/runs/tables and bookmarks remain editable. The fixed review
layout uses Letter pages, one-inch margins, 11-point body text, black headings,
9-point table/metadata text, visible table borders and repeated table headers.
Modest rows and location paragraphs are kept together; long records can flow over
pages. Generator 2 enables word-level wrapping for ordinary prose without inserting
characters into literal text. Generator 1 retains its original character-level
wrapping for byte-identical historical verification. See the
[Word wrapping repair](DOCX-WORD-WRAPPING.md) for the native observation and limits.
Arial is requested by name; no font is embedded. Font fallback and layout depend
on the reader. This is not an approved or complete product font pack, reusable
analyst template system, accessibility certification or universal Word-processor
compatibility claim.

Primary format references: [WordprocessingML document structure](https://learn.microsoft.com/en-us/office/open-xml/word/structure-of-a-wordprocessingml-document),
[ECMA-376](https://ecma-international.org/publications-and-standards/standards/ecma-376/),
and [character-level wrapping](https://learn.microsoft.com/en-us/dotnet/api/documentformat.openxml.wordprocessing.wordwrap?view=openxml-3.0.1).
ZIP options follow the [pinned library API](https://docs.rs/zip/8.6.0/zip/write/struct.FileOptions.html).

## Verification and reproduction

`cargo test --locked -p workbench-core --test report_docx` covers fixed editable
package structure, deterministic output, bookmark resolution, shared difficult
money/balance semantics, original HTML and frozen DOCX stability after a canonical
correction, source retargeting/dangling references, XML injection, exact whitespace,
all retained anchor variants and input/JSON/XML/output limit failures. The private
archive test covers size-bound seek and overwrite behavior.

`cargo run --locked -p workbench-core --example report_docx -- NEW_DIRECTORY` writes
one fixed synthetic frozen JSON and DOCX, refusing an existing output directory.
The example is a developer fixture, not a canonical report publication operation.

For actual render evidence, run `scripts/test_docx_report.py` with the documents
skill's bundled Python and `--runtime-root` pointing at its managed runtime. The
runner generates twice, compares byte hashes, checks package structure with Python
and `python-docx`, and invokes that runtime's `render_docx.py` for page PNGs/PDF.
It records source/executable/artifact/runtime hashes and the actual renderer
version. No system LibreOffice fallback is used. This QA runtime is a developer
tool, not shipped application functionality. Every rendered page must then be
visually inspected; a successful subprocess alone leaves the observation at
`rendered_awaiting_visual_inspection`. Evidence history uses exclusive UUID names
so repeated clocks cannot overwrite earlier observations. Four portable harness
regressions cover failed preflight, subprocess timeout, history preservation and
external-relationship rejection.

On the development Mac, the managed manifest declares `25.2-headless-codex.1`, but
the actual executable identifies as **LibreOfficeDev 26.8.0.0.alpha0**
(commit `2c87e51eeaa2b413ff4ae097b2705eea1995d8e5`). Evidence must identify that
actual alpha renderer; it is not a stable LibreOffice or Microsoft Word pass.
Initial layout observations and failed compiler runs remain retained locally;
final visual/native evidence must identify its exact source and artifact hashes.
Microsoft Word native opening/editability, other platforms, maximum-size document
layout, approved fonts, assembly/exhibits and desktop/native DOCX integration remain separate acceptance work.
Canonical publication/migration and backup/restore now have a separate bounded
[Rust API implementation](DOCX-SNAPSHOTS.md).
