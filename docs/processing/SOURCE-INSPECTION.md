# Preserved-source excerpt integrity

`InspectSource` continues to accept the historical typed text or CSV-cell anchor
and return the same `SourceExcerpt` schema. The read now checks that the retained
evidence body identifies the requested canonical key and content digest, then verifies the original
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

Four core regressions cover same-length digest corruption, missing originals,
restoration, mismatched evidence identity, digest retargeting to another intact original, exact CSV decimal excerpts and unchanged
read-only dispatch. A real-core browser regression first loads the interface's
cached derivative, then alters the original without changing its length. Both
the direct transaction excerpt and nested anchor excerpt report the checksum
failure without a quoted value or validation message. Closing and reopening
after byte restoration returns the exact value. The test restores the fixture
and confirms the entire canonical workspace is unchanged.

Bounded peer review found that a body-ID check alone could still allow digest
retargeting to another intact original. The repaired invariant binds the requested
key, body ID and content digest. Common original verification also enforces body
ID/digest equality, so other callers cannot treat a substituted original as the
same evidence. The new two-original regression covers that exact failure mode.

The repaired source passes 213 ordinary core tests with 21 native/runtime
exclusions and strict core Clippy. The preceding excerpt repair passed all 70
browser workflows; that run's compiled binary predates the final shared digest
check, which will be rebuilt in the next combined campaign. Nine earlier targeted
workflows also passed. The interface layout and historical schemas are unchanged.
This increment passes no complete-release gate.


The subsequent combined search integration rebuilt the bridge with the final
shared digest check and passed all 70 real-core browser workflows, 222 ordinary
core tests and strict Clippy. No native or installation claim is inferred from
that browser campaign. See the combined source identity in
[verification](../VERIFICATION.md).
