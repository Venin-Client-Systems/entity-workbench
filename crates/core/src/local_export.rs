//! Native-only typed export lifecycle. These receipts never contain exported document bytes.
use crate::{literal_search::LiteralMatching, transaction_export::TransactionExportRequest};
use schemars::JsonSchema;
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
#[serde(tag = "kind", rename_all = "snake_case", deny_unknown_fields)]
pub enum NativeExportRequest {
    DocxReport {
        report_id: String,
        expected_document_sha256: String,
        expected_docx_sha256: String,
    },
    Transactions {
        request: TransactionExportRequest,
        expected_revision: u64,
        expected_row_count: u64,
        expected_matching: LiteralMatching,
    },
    HtmlReport {
        report_id: String,
        expected_sha256: String,
    },
}

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
#[serde(tag = "kind", rename_all = "snake_case", deny_unknown_fields)]
pub enum ExportArtifact {
    DocxReport {
        report_id: String,
        workspace_revision: u64,
        document_sha256: String,
        bytes: u64,
        sha256: String,
    },
    Transactions {
        workspace_revision: u64,
        request: TransactionExportRequest,
        matching: LiteralMatching,
        query_sha256: String,
        row_count: u64,
        bytes: u64,
        sha256: String,
    },
    HtmlReport {
        report_id: String,
        workspace_revision: u64,
        bytes: u64,
        sha256: String,
    },
}
impl ExportArtifact {
    pub fn bytes(&self) -> u64 {
        match self {
            Self::Transactions { bytes, .. }
            | Self::HtmlReport { bytes, .. }
            | Self::DocxReport { bytes, .. } => *bytes,
        }
    }
    pub fn sha256(&self) -> &str {
        match self {
            Self::Transactions { sha256, .. }
            | Self::HtmlReport { sha256, .. }
            | Self::DocxReport { sha256, .. } => sha256,
        }
    }
    pub(crate) fn maximum_bytes(&self) -> usize {
        match self {
            Self::DocxReport { .. } => crate::report_docx::MAX_DOCX_BYTES,
            Self::Transactions { .. } | Self::HtmlReport { .. } => {
                crate::transaction_export::MAX_EXPORT_JSON_BYTES
            }
        }
    }
    pub(crate) fn filename(&self) -> String {
        match self {
            Self::DocxReport {
                report_id, sha256, ..
            } => format!("assessment-{report_id}-{sha256}.docx"),
            Self::Transactions {
                workspace_revision,
                sha256,
                ..
            } => format!("transactions-r{workspace_revision}-{sha256}.json"),
            Self::HtmlReport {
                report_id, sha256, ..
            } => format!("assessment-{report_id}-{sha256}.html"),
        }
    }
}
#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct PreparedExport {
    pub schema_version: u32,
    pub ticket: String,
    pub expires_after_seconds: u64,
    pub artifact: ExportArtifact,
}
#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct SavedExportReceipt {
    pub schema_version: u32,
    pub ticket: String,
    pub artifact: ExportArtifact,
    pub filename: String,
    /// Display-only local location. Callers cannot supply it to a writer or opener.
    pub location: String,
}
#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
#[serde(tag = "state", rename_all = "snake_case", deny_unknown_fields)]
pub enum DiscardedExport {
    Discarded,
    Saved { receipt: Box<SavedExportReceipt> },
}
