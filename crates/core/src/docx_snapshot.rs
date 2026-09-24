//! Immutable editable-report metadata. This format is separate from historical HTML reports and OCR refs.
use crate::{report_document, report_docx, require, Result};
use serde::{Deserialize, Serialize};
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum ReportArtifactKind {
    ReportDocumentJsonV1,
    ReportDocxV1,
}
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(deny_unknown_fields)]
pub struct ReportArtifactRef {
    pub kind: ReportArtifactKind,
    pub sha256: String,
    pub bytes: u64,
}
impl ReportArtifactRef {
    pub(crate) fn validate(&self) -> Result<()> {
        let maximum = match self.kind {
            ReportArtifactKind::ReportDocumentJsonV1 => report_document::MAX_DOCUMENT_BYTES,
            ReportArtifactKind::ReportDocxV1 => report_docx::MAX_DOCX_BYTES,
        };
        require(
            self.sha256.len() == 64
                && self
                    .sha256
                    .bytes()
                    .all(|b| b.is_ascii_digit() || (b'a'..=b'f').contains(&b)),
            "Invalid report artifact digest",
        )?;
        require(
            self.bytes > 0 && self.bytes <= maximum as u64,
            "Report artifact exceeds its type bound",
        )
    }
}
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(deny_unknown_fields)]
pub struct DocxSnapshotRecord {
    pub schema_version: u32,
    /// Also the caller's stable request identity. Reuse binds the same source revision.
    pub id: String,
    pub workspace_revision: u64,
    pub created_at: String,
    pub template_version: String,
    pub generator_version: String,
    pub document: ReportArtifactRef,
    pub docx: ReportArtifactRef,
}
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct DocxSnapshotInspection {
    pub snapshot: DocxSnapshotRecord,
    /// Explicit bounded read; not part of a default workspace refresh.
    pub document: report_document::ReportDocument,
}
