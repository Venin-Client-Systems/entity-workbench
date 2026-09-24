//! Durable, offline document and image jobs. Records are canonical; engine processes never write them.
use schemars::JsonSchema;
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum ProcessingState {
    Queued,
    Running,
    Partial,
    Blocked,
    QuotaExhausted,
    Failed,
    Cancelled,
    Completed,
}

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema, PartialEq, Eq)]
#[serde(tag = "operation", rename_all = "snake_case", deny_unknown_fields)]
pub enum ProcessingInput {
    ParseDocument {
        evidence_id: String,
        sha256: String,
        bytes: u64,
    },
    ImageOcr {
        evidence_id: String,
        sha256: String,
        bytes: u64,
    },
    PdfPageOcr {
        evidence_id: String,
        sha256: String,
        bytes: u64,
        page_number: u32,
        dpi: u32,
    },
}

impl ProcessingInput {
    pub(crate) fn source(&self) -> (&str, &str, u64) {
        match self {
            Self::ParseDocument {
                evidence_id,
                sha256,
                bytes,
            }
            | Self::ImageOcr {
                evidence_id,
                sha256,
                bytes,
            }
            | Self::PdfPageOcr {
                evidence_id,
                sha256,
                bytes,
                ..
            } => (evidence_id, sha256, *bytes),
        }
    }
}

// Preserve the parse-only extraction v1 wire and schema contract.
#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
#[serde(tag = "operation", rename_all = "snake_case", deny_unknown_fields)]
#[schemars(rename = "ProcessingInput")]
pub enum ParseDocumentInput {
    ParseDocument {
        evidence_id: String,
        sha256: String,
        bytes: u64,
    },
}

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
#[serde(tag = "operation", rename_all = "snake_case", deny_unknown_fields)]
pub enum ImageOcrInput {
    ImageOcr {
        evidence_id: String,
        sha256: String,
        bytes: u64,
    },
}

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum ProcessingFailure {
    Interrupted,
    InputUnavailable,
    RuntimeUnavailable,
    WorkerFailed,
    InvalidResult,
    UnsupportedFormat,
    DocumentFailed,
    ImageDecodeFailed,
    EncryptedDocument,
    PdfRenderFailed,
    CancelledByAnalyst,
    CleanupFailed,
    WorkerExitUnverified,
    RecoveryRequired,
}

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct RetryPolicy {
    /// Retries always require an explicit analyst request.
    pub automatic: bool,
    pub max_attempts: u32,
}

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct ProcessingJob {
    pub schema_version: u32,
    pub id: String,
    pub request_key: String,
    pub input: ProcessingInput,
    pub state: ProcessingState,
    pub attempt: u32,
    pub retry: RetryPolicy,
    pub cancellation_requested: bool,
    pub created_at: String,
    pub updated_at: String,
    pub started_at: Option<String>,
    pub finished_at: Option<String>,
    pub failure: Option<ProcessingFailure>,
    pub detail: String,
    pub result_ids: Vec<String>,
    /// Opaque attempt ownership, never supplied by a worker or frontend.
    pub(crate) lease: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct ProcessingJobPage {
    pub jobs: Vec<ProcessingJob>,
    pub total: u64,
    pub limit: u32,
}

/// This capability is constructed only by canonical claim; there is no IPC finish command.
#[derive(Debug, Clone)]
pub(crate) struct JobTicket {
    pub job_id: String,
    pub attempt: u32,
    pub lease: String,
}

pub(crate) struct PreparedProcessingJob {
    pub ticket: JobTicket,
    pub input: ProcessingInput,
    pub bytes: Vec<u8>,
}

/// In-memory only: image rasters are needed for acceptance, never serialized into job records.
pub(crate) enum ProcessingOutput {
    Document(crate::engines::parser::ParseResult),
    Image(Box<crate::engines::image::ImageOcr>),
    Pdf(Box<crate::engines::pdf_render::PdfOcr>),
}

/// An immutable, unreviewed derivative. Parsing does not replace source text or accept facts.
#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct ExtractionRecord {
    pub schema_version: u32,
    pub id: String,
    pub job_id: String,
    pub attempt: u32,
    pub input: ParseDocumentInput,
    pub created_at: String,
    pub result_sha256: String,
    pub result: crate::engines::parser::ParseResult,
}

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct ImageExtractionResult {
    pub decoder: crate::engines::image::ImageDecodeResult,
    pub recognition: Option<crate::engines::ocr::OcrResult>,
    /// Always false in v1: the raster was validated in memory and then discarded.
    pub raster_retained: bool,
}

/// An immutable, unreviewed image derivative. No pixel regions or accepted observations.
#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct ImageExtractionRecord {
    pub schema_version: u32,
    pub id: String,
    pub job_id: String,
    pub attempt: u32,
    pub input: ImageOcrInput,
    pub created_at: String,
    pub result_sha256: String,
    pub result: ImageExtractionResult,
}

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
#[serde(tag = "operation", rename_all = "snake_case", deny_unknown_fields)]
pub enum PdfPageOcrInput {
    PdfPageOcr {
        evidence_id: String,
        sha256: String,
        bytes: u64,
        page_number: u32,
        dpi: u32,
    },
}

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct PdfExtractionResult {
    pub render: crate::engines::pdf_render::PdfRenderResult,
    pub recognition: Option<crate::engines::ocr::OcrResult>,
    /// Always false: the raster was checked during publication, then discarded.
    pub raster_retained: bool,
}

/// Immutable unreviewed page recognition and renderer provenance; no accepted word regions.
#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct PdfExtractionRecord {
    pub schema_version: u32,
    pub id: String,
    pub job_id: String,
    pub attempt: u32,
    pub input: PdfPageOcrInput,
    pub created_at: String,
    pub result_sha256: String,
    pub result: PdfExtractionResult,
}
