# Preserved-source excerpt integrity

`InspectSource` continues to accept the historical typed text or CSV-cell anchor
and return the same `SourceExcerpt` schema. The read now checks that the retained
evidence body identifies the requested canonical key and verifies the original
file's safe path, byte length and SHA-256 before returning the quoted derivative.
An altered or missing original is an explicit error even if cached text is still
available in the workspace. Restoring the exact original bytes permits a fresh
read; the operation does not change records, versions or decisions.

Evidence lookup, anchor validation and the response revision share one SQLite
snapshot. Text line and logical CSV row/column validation retain their existing
semantics. Quotes remain limited to 8000 characters with a truncation indicator;
exact decimal text, newlines and source locations are preserved. This change does
not accept OCR/page/message/capture anchors without their required verified
manifests.

The command still returns the current snapshot revision; it has no caller-supplied
expected revision. It still reads the existing retained evidence body and original
file, so the quote limit is not a whole-process allocation limit. Whole-source
views that display cached text without an anchor do not run this command and do
not acquire an integrity attestation through this repair. A source-catalogue
metadata response likewise is not a substitute for source inspection or review.

Three core regressions cover same-length digest corruption, missing originals,
restoration, mismatched evidence identity, exact CSV decimal excerpts and unchanged
read-only dispatch. A real-core browser regression first loads the interface's
cached derivative, then alters the original without changing its length. Both
the direct transaction excerpt and nested anchor excerpt report the checksum
failure without a quoted value or validation message. Closing and reopening
after byte restoration returns the exact value. The test restores the fixture
and confirms the entire canonical workspace is unchanged.

The repair passes 212 ordinary core tests with 21 native/runtime exclusions,
strict core Clippy and nine targeted browser workflows. The interface layout and
historical schemas are unchanged. A broader combined browser campaign follows
the next integration; this increment passes no complete-release gate.
