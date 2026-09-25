//! Immutable editable-report metadata. This format is separate from historical HTML reports and OCR refs.
use crate::{report_document, report_docx, require, Result};
use schemars::JsonSchema;
use serde::{Deserialize, Serialize};
#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum ReportArtifactKind {
    ReportDocumentJsonV1,
    ReportDocxV1,
}
#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema, PartialEq, Eq)]
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
#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema, PartialEq, Eq)]
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
#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct DocxSnapshotInspection {
    pub snapshot: DocxSnapshotRecord,
    /// Explicit bounded read; not part of a default workspace refresh.
    pub document: report_document::ReportDocument,
}

/// Explicit request recovery, separate from the metadata-only catalogue.
#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema, PartialEq, Eq)]
#[serde(deny_unknown_fields)]
pub struct DocxCaptureResolution {
    pub schema_version: u32,
    pub request_id: String,
    pub captured_revision: u64,
    pub workspace_revision: u64,
    pub outcome: DocxCaptureOutcome,
}
#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema, PartialEq, Eq)]
#[serde(tag = "state", rename_all = "snake_case", deny_unknown_fields)]
pub enum DocxCaptureOutcome {
    Saved {
        snapshot: Box<DocxSnapshotRecord>,
    },
    /// Absence at this read snapshot. At the same revision, a previously sent
    /// capture may still publish; only a strictly later revision precludes it.
    NotRecorded,
}

pub const MAX_DOCX_CATALOGUE_ROWS: u32 = 50;
pub const MAX_DOCX_CATALOGUE_BYTES: u64 = 256 * 1024;
pub const MAX_DOCX_CURSOR_BYTES: usize = 2048;

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct DocxSnapshotPageRequest {
    pub page_size: u32,
    pub cursor: Option<String>,
}
impl DocxSnapshotPageRequest {
    pub fn validate(&self) -> Result<()> {
        require(
            (1..=MAX_DOCX_CATALOGUE_ROWS).contains(&self.page_size),
            "DOCX catalogue page size must be between 1 and 50",
        )?;
        require(
            self.cursor
                .as_ref()
                .is_none_or(|c| !c.is_empty() && c.len() <= MAX_DOCX_CURSOR_BYTES),
            "DOCX catalogue cursor is empty or exceeds its bound",
        )
    }
}
#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct DocxSnapshotPage {
    pub schema_version: u32,
    pub workspace_revision: u64,
    pub total_count: u64,
    pub query_sha256: String,
    /// Newest publication first. Metadata and catalog lengths are checked; artifact bytes are not read.
    pub rows: Vec<DocxSnapshotRecord>,
    pub next_cursor: Option<String>,
}
