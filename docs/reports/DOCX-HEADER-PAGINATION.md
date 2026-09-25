# DOCX table-header pagination

The native Word review of generator 2 showed a repeated transaction-table
header alone at the bottom of page 1, with its first data row on page 2.
New captures use `ooxml-foundation-3`, which adds `w:keepNext` to the
`TableHeader` paragraph style. Body-row paragraphs keep their existing styles
and remain free to paginate. This is a layout preference when the following
content can fit; it is not a promise that an arbitrarily long row fits on one
page. Microsoft's [KeepNext reference](https://learn.microsoft.com/en-us/dotnet/api/documentformat.openxml.wordprocessing.keepnext?view=openxml-3.0.1)
describes keeping successive paragraphs together where possible.

Generators 1 and 2 are still accepted and rendered with their exact original
styles. Neither saved reports nor frozen source models are rewritten. Historical
golden artifacts retain SHA-256 values:

| Generator | DOCX SHA-256 |
| --- | --- |
| `ooxml-foundation-1` | `c839120c7646195012e57119be34dcca7ad27e32a032ad734ca47680cfc1c75d` |
| `ooxml-foundation-2` | `0b9f69392b57dd1190dea7d45c7fcc16bad97ab9fcf2b9dec3b8350b6fc74fd9` |

The existing historical publication regression now exercises both versions:
inspect, idempotent capture retry, exact native-export content and complete
backup/restore after a generator-3 capture. The renderer suite also verifies
that historical bytes remain identical and that only the new header style
keeps its next paragraph. Unsupported later versions still fail validation.
No storage or public schema migration is introduced.

Managed LibreOffice rendered the fixed ordinary specimen to three pages and
the long-literal specimen to four. All seven page PNGs were inspected: each
table header shares page 2 with its first row, and no clipping or overlap was
observed. Full IDs, leading zeros, exact amounts, source text and long literals
remain present. This observation is distinct from native Word, whose
generator-3 pagination and edit/save checks remain pending. The earlier native
generator-2 observations and failed long-literal picker interaction are retained.

The [review record](verification/header-pagination.json) binds the source files,
managed renderer, fixtures, output hashes and page review. Its source-file
binding is explicit; it does not claim a clean signed release or installed-app
acceptance. Report assembly, full template design and complete-release gates
remain open.

Two subsequent bounded native Word app selections returned
`timeoutReached -10005`; an intervening inventory still listed Word running.
No generator-3 document was opened, edited or saved, and the existing app state
was preserved. The [operator attempt record](verification/native-word-header-attempt.json)
retains this unavailable verification separately from the successful managed
rendering. It does not replace the earlier generator-2 Word observations.
