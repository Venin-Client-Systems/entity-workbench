# DOCX generator 2 word wrapping

Generator 1 set `w:wordWrap w:val="off"` in every paragraph style. Actual Word
16.113.2 on macOS opened and edited the synthetic native export, but broke ordinary
English words such as “revision” and “analytical” across lines. Microsoft’s
[WordWrap documentation](https://learn.microsoft.com/en-us/dotnet/api/documentformat.openxml.wordprocessing.wordwrap)
confirms that `off` enables character-level breaking. This is a formatting defect;
the retained text and monetary values were unchanged.

New captures use `ooxml-foundation-2`, which sets the seven fixed paragraph styles
to `wordWrap=on`. Template `assessment-foundation-1`, page geometry, fonts, content,
calculation rules and limits are unchanged. No break characters, shortened values,
external relationships or new dependencies are introduced.

The frozen-document and snapshot validators accept only the two known generator
versions. The renderer dispatches the style setting from the validated frozen
version. Generator 1 retains its original byte sequence, including the old setting.
There is no migration, command change, historical schema rewrite or regeneration
from a current workspace. Older applications that only know generator 1 reject a
new generator 2 artifact rather than silently interpreting it as generator 1.

## Verification

The fixed synthetic generator 1 JSON fixture was retained from the separately
observed native capture. Rendering it must still yield SHA-256
`c839120c7646195012e57119be34dcca7ad27e32a032ad734ca47680cfc1c75d`.
A canonical test installs that historical fixture through the private CAS/catalogue
operations, captures a new generator 2 snapshot, and checks old inspection, UUID
retry, exact native export bytes, and backup/restore of both versions. Unsupported
generators fail validation. The fixture introduces no arbitrary publication API.

The developer-only `report_docx_wrapping` example produces two fixed synthetic
outputs: the native fixture with the new generator, and a variant containing
160-character unbroken literals in a paragraph and a table cell. It does not
publish canonical records or verify original files. It is a layout test, not a new
source observation.

The managed renderer generated three ordinary pages and four long-literal pages.
Every page was inspected: no clipped text, overflowing table, omitted decimal text
or overlap was observed. The bundled executable reported LibreOfficeDev
26.8.0.0.alpha0, source `2c87e51eeaa2b413ff4ae097b2705eea1995d8e5`, while its
managed manifest labels the package `25.2-headless-codex.1`; the actual identity is
retained with the evidence rather than relabelled.

**New native Word layout remains unverified.** The follow-up Word file picker
selected the new DOCX but kept Open disabled, and behaved the same way for the
previously opened disposable control file. Later CUA reads timed out. No OS
permissions were changed, and no process was force-quit. This does not invalidate
the earlier observed generator 1 native capture/save/recovery and Word editability
proof, but that proof does not establish generator 2 layout. The native follow-up
must inspect both ordinary prose and long literals when control is available.

Local ignored evidence is retained under `artifacts/word-wrapping/`; the earlier
native app proof remains in the separate `native-docx-review` worktree. Neither
record is a signed-release, clean-install, Intel/Windows, complete report assembly
or approved product-design claim.


## Later native Word observation — 25 September 2026

After controls became available, the file picker's explicit Open Finder item action opened the unchanged generator-2 ordinary specimen in Word. The native window identified `generator2-ordinary` in Compatibility Mode; its three-page, 481-word document showed generator `ooxml-foundation-2`. A screenshot of the first-page prose showed ordinary words wrapping at word boundaries, with the reviewed decimal calculation intact. The specimen SHA-256 remains `0b9f69392b57dd1190dea7d45c7fcc16bad97ab9fcf2b9dec3b8350b6fc74fd9`.

A subsequent Whole page zoom action and state read timed out. This updates the earlier inability to open generator 2: opening and the visible first-page prose are now observed, but complete-page inspection, long-literal Word layout and generator-2 edit/save remain unverified. No document content was edited or saved, no app was force-quit and no permission changed. The managed seven-page inspection remains separate evidence.

## Three-page native follow-up — 25 September 2026

The same unchanged ordinary specimen was subsequently inspected across all three
native Word pages. Page 2 displays the three transaction rows, exact amounts,
account `001`, review states and full transaction/source identifiers. Page 3
displays review history, the source catalogue, original CSV text and limitations.
No clipped content, overlapping text or loss of exact decimal text was observed.
The table header remains alone at the bottom of page 1 and repeats with its rows
on page 2; keeping the header with the first data row is a remaining layout
refinement. Native accessibility scrolling changed the viewport, but repainting
the following pages required a zoom change. This is an observed control behavior,
not a diagnosis of the document renderer.

The long-literal specimen remained selected in Word's native file picker with
Open disabled. Its secondary open action and Return did not open it, and Cancel
timed out. The final picker state is unconfirmed. No content was edited or saved;
the ordinary source digest above remains unchanged. The retained operator record
has SHA-256 `c5fdaaac420afdf105dd0c4bdf71d8ae05a4482754a2ec2a4e7a3ae3a2d4b2cf`.
Long-literal native layout and generator-2 edit/save remain unverified. Earlier
generator-1 editability and managed-render evidence retain their original scope.
