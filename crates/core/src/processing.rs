//! Durable, offline document jobs. Records are canonical; engine processes never write them.
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
    CancelledByAnalyst,
    CleanupFailed,
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

pub(crate) struct PreparedDocumentJob {
    pub ticket: JobTicket,
    pub bytes: Vec<u8>,
}

/// An immutable, unreviewed derivative. Parsing does not replace source text or accept facts.
#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct ExtractionRecord {
    pub schema_version: u32,
    pub id: String,
    pub job_id: String,
    pub attempt: u32,
    pub input: ProcessingInput,
    pub created_at: String,
    pub result_sha256: String,
    pub result: crate::engines::parser::ParseResult,
}
