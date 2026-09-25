//! Immutable local graph analysis records. These are derived snapshots, never accepted facts.
use crate::domain::{Assertion, Entity, Evidence, Observation};
use schemars::JsonSchema;
use serde::{Deserialize, Serialize};

pub const MAX_GRAPH_RECORD_BYTES: usize = 16 * 1024 * 1024;

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct GraphFingerprint {
    pub kind: String,
    pub id: String,
    pub sha256: String,
}
#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct GraphEdge {
    pub source_id: String,
    pub target_id: String,
    pub assertion_ids: Vec<String>,
}
#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
#[serde(tag = "state", rename_all = "snake_case", deny_unknown_fields)]
pub enum GraphOutcome {
    Path {
        nodes: Vec<String>,
        hops: Vec<GraphEdge>,
    },
    Unreachable {},
}
#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct GraphReviewCounts {
    pub accepted: usize,
    pub pending: usize,
    pub rejected: usize,
    pub deferred: usize,
}
#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct FrozenGraph {
    pub assertion_reviews: GraphReviewCounts,
    pub nodes: Vec<String>,
    pub edges: Vec<GraphEdge>,
    pub fingerprints: Vec<GraphFingerprint>,
    pub entities: Vec<Entity>,
    pub assertions: Vec<Assertion>,
    pub observations: Vec<Observation>,
    pub evidence: Vec<Evidence>,
}
#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct GraphAnalysisRecord {
    #[schemars(range(min = 1, max = 1))]
    pub schema_version: u32,
    pub id: String,
    pub job_id: String,
    pub request_key: String,
    pub attempt: u32,
    pub host_attempt_lease: String,
    pub requested_revision: u64,
    pub queued_revision: u64,
    pub recipe: String,
    pub policy: String,
    pub engine: String,
    pub engine_version: String,
    pub runtime_manifest_sha256: String,
    pub source_id: String,
    pub target_id: String,
    pub captured_revision: u64,
    pub published_revision: u64,
    pub capture_nonce: String,
    pub snapshot_sha256: String,
    pub request_sha256: String,
    pub result_sha256: String,
    pub request_json: String,
    pub result_json: String,
    pub frozen: FrozenGraph,
    pub outcome: GraphOutcome,
    pub limitation: String,
}
#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum GraphFreshness {
    CurrentAtPublication,
    WorkspaceAdvanced,
}
#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum GraphOriginalIntegrity {
    Verified,
    Unavailable,
}
#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct GraphAnalysisInspection {
    pub record: GraphAnalysisRecord,
    pub compared_revision: u64,
    pub freshness: Option<GraphFreshness>,
    pub original_integrity: GraphOriginalIntegrity,
}
