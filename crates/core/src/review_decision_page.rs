//! Bounded reads of the existing ID-targeted review history; no new decision semantics.
use crate::{domain::ReviewDecision, require, Result};
use schemars::JsonSchema;
use serde::{Deserialize, Serialize};

pub const MAX_DECISION_PAGE_ROWS: u32 = 50;
pub const MAX_DECISION_PAGE_BODY_BYTES: u64 = 1024 * 1024;
pub const MAX_DECISION_CURSOR_BYTES: usize = 2048;

#[derive(Debug, Clone, Copy, Serialize, Deserialize, JsonSchema, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum ReviewDecisionTargetKind {
    Entity,
    Observation,
    Hypothesis,
    Finding,
    Transaction,
    ProcessingJob,
    Merge,
}

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct ReviewDecisionPageRequest {
    pub target_id: String,
    pub page_size: u32,
    pub cursor: Option<String>,
}
impl ReviewDecisionPageRequest {
    pub fn validate(&self) -> Result<()> {
        require(
            !self.target_id.is_empty()
                && self.target_id.len() <= 256
                && !self.target_id.chars().any(char::is_control),
            "Review target identifier must contain 1 to 256 bytes without controls",
        )?;
        require(
            (1..=MAX_DECISION_PAGE_ROWS).contains(&self.page_size),
            "Review decision page size must be between 1 and 50",
        )?;
        require(
            self.cursor.as_ref().is_none_or(|cursor| {
                !cursor.is_empty() && cursor.len() <= MAX_DECISION_CURSOR_BYTES
            }),
            "Review decision cursor is empty or exceeds its bound",
        )
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct ReviewDecisionPage {
    pub schema_version: u32,
    pub workspace_revision: u64,
    pub target_id: String,
    /// Current unique supported target; legacy ReviewDecision has no kind field.
    pub resolved_target_kind: ReviewDecisionTargetKind,
    pub scope_count: u64,
    pub query_sha256: String,
    /// Complete canonical records in insertion order, never sorted by timestamp.
    pub rows: Vec<ReviewDecision>,
    pub next_cursor: Option<String>,
}
