//! Additive graph job controls. Queue acknowledgement is not an execution capability.
use crate::{processing::ProcessingJob, require, Result};
use schemars::JsonSchema;
use serde::{Deserialize, Serialize};

pub const MAX_GRAPH_JOB_PAGE: u32 = 25;
pub(crate) const JOB_BYTES: usize = 64 * 1024;
pub(crate) const PAGE_BYTES: usize = 2 * 1024 * 1024;

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum GraphAvailability {
    StandaloneUnavailable,
    RuntimeUnavailable,
    Ready,
    SyntheticFixture,
    RecoveryRequired,
}
#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum GraphExecutionPhase {
    Draining,
    Running,
    Publishing,
    PublicationPending,
    RecoveryRequired,
    UnpublishedKnownStopped,
}
#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct GraphExecutionStatus {
    pub phase: GraphExecutionPhase,
    pub attempt: u32,
    pub host_attempt_lease: Option<String>,
    pub request_sha256: Option<String>,
    pub publication_retries: u32,
    pub publication_retry_limit: u32,
}
#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct GraphControls {
    pub can_cancel: bool,
    pub can_retry_publication: bool,
}
/// Discoverable metadata only; full inspection verifies the immutable artifact and originals.
#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct GraphResultReference {
    pub id: String,
    pub request_sha256: String,
    pub result_sha256: String,
    pub captured_revision: u64,
    pub published_revision: u64,
}
#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct GraphJobInspection {
    #[schemars(range(min = 1, max = 1))]
    pub schema_version: u32,
    pub workspace_revision: u64,
    pub availability: GraphAvailability,
    pub job: ProcessingJob,
    /// At most three metadata references, from the same snapshot as the canonical job.
    pub results: Vec<GraphResultReference>,
    /// Ephemeral state only when job, attempt and lease match this canonical record.
    pub execution: Option<GraphExecutionStatus>,
    pub controls: GraphControls,
    pub limitations: Vec<String>,
}
#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct GraphJobPageRequest {
    #[schemars(range(min = 1, max = 25))]
    pub page_size: u32,
    pub cursor: Option<String>,
}
impl GraphJobPageRequest {
    pub(crate) fn validate(&self) -> Result<()> {
        require(
            (1..=MAX_GRAPH_JOB_PAGE).contains(&self.page_size),
            "Graph page size must be 1..25",
        )?;
        require(
            self.cursor.as_ref().is_none_or(|c| c.len() <= 1024),
            "Graph cursor exceeds bound",
        )
    }
}
#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct GraphJobEntry {
    pub sequence: u64,
    pub job: ProcessingJob,
}
#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct GraphJobPage {
    #[schemars(range(min = 1, max = 1))]
    pub schema_version: u32,
    pub workspace_revision: u64,
    pub availability: GraphAvailability,
    /// All durable graph jobs, regardless of canonical state or executor availability.
    pub total_count: u64,
    pub rows: Vec<GraphJobEntry>,
    pub next_cursor: Option<String>,
}
pub(crate) fn uuid(value: &str) -> bool {
    value.len() == 36 && uuid::Uuid::parse_str(value).is_ok_and(|id| id.to_string() == value)
}
pub(crate) fn digest(value: &str) -> bool {
    value.len() == 64
        && value
            .bytes()
            .all(|b| b.is_ascii_digit() || (b'a'..=b'f').contains(&b))
}
pub(crate) fn endpoint(value: &str) -> Result<()> {
    require(
        !value.is_empty()
            && value.len() <= 128
            && value.trim() == value
            && !value.chars().any(char::is_control),
        "Invalid graph endpoint",
    )
}
