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
