//! Public projections and disclosure binding. No resolver, worker or write authority.
use crate::{collection_receipt::AcquisitionMode, require, Result};
use schemars::JsonSchema;
use serde::{Deserialize, Serialize};
use url::Url;

pub use crate::collection_jobs::{
    CollectionInput, CollectionState, FrontierEntry, RequestProgress,
};

pub const COLLECTOR_POLICY: &str = "direct-https-durable-v3";
/// Deliberate reviewed-source gate, not an environment or frontend override.
pub const NATIVE_COLLECTION_ENABLED: bool = false;
pub(crate) const PAGE_LIMIT: u32 = 25;
pub(crate) const RESPONSE_BYTES: usize = 2 * 1024 * 1024;

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema, PartialEq, Eq)]
#[serde(deny_unknown_fields)]
pub struct CollectionDisclosure {
    pub dns_hostnames: bool,
    pub connection_metadata: bool,
    pub selected_and_followed_urls: bool,
    pub automatic_case_contents: bool,
    pub followed_hosts: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema, PartialEq, Eq)]
#[serde(deny_unknown_fields)]
pub struct CollectionPreview {
    pub schema_version: u32,
    pub collector_policy: String,
    pub input: CollectionInput,
    pub selected_hosts: Vec<String>,
    pub robots_urls: Vec<String>,
    pub disclosure: CollectionDisclosure,
    pub preview_sha256: String,
}

/// A digest binds this closed payload, not a UI checkbox, remote-access approval or DNS result.
#[derive(Serialize)]
struct PreviewPayload<'a> {
    schema_version: u32,
    collector_policy: &'a str,
    input: &'a CollectionInput,
    selected_hosts: &'a [String],
    robots_urls: &'a [String],
    disclosure: &'a CollectionDisclosure,
}

pub fn preview(input: CollectionInput) -> Result<CollectionPreview> {
    let input = input.normalized()?;
    let mut selected_hosts = Vec::new();
    let mut robots_urls = Vec::new();
    for raw in &input.urls {
        let mut url = Url::parse(raw).expect("normalized collection seed");
        let host = url.host_str().expect("normalized host").to_owned();
        if !selected_hosts.contains(&host) {
            selected_hosts.push(host);
            url.set_path("/robots.txt");
            url.set_query(None);
            robots_urls.push(url.to_string());
        }
    }
    let disclosure = CollectionDisclosure {
        dns_hostnames: true,
        connection_metadata: true,
        selected_and_followed_urls: true,
        automatic_case_contents: false,
        followed_hosts: "selected_hosts_only".into(),
    };
    let preview_sha256 = crate::store::hash(&serde_json::to_vec(&PreviewPayload {
        schema_version: 1,
        collector_policy: COLLECTOR_POLICY,
        input: &input,
        selected_hosts: &selected_hosts,
        robots_urls: &robots_urls,
        disclosure: &disclosure,
    })?);
    Ok(CollectionPreview {
        schema_version: 1,
        collector_policy: COLLECTOR_POLICY.into(),
        input,
        selected_hosts,
        robots_urls,
        disclosure,
        preview_sha256,
    })
}

pub(crate) fn confirmed(input: CollectionInput, digest: &str) -> Result<CollectionInput> {
    require(
        digest.len() == 64,
        "Collection disclosure digest is invalid",
    )?;
    let preview = preview(input)?;
    require(
        preview.preview_sha256 == digest,
        "Collection scope or policy changed; review the disclosure again",
    )?;
    Ok(preview.input)
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize, JsonSchema, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum CollectionAvailability {
    NativeDisabled,
    StandaloneUnavailable,
    SyntheticFixture,
    Ready,
    RecoveryRequired,
    ExecutionUnavailable,
    Stopping,
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize, JsonSchema, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum CollectionExecutionPhase {
    Unavailable,
    Idle,
    Running,
    Settling,
    SettlementPending,
    Faulted,
    RecoveryRequired,
    Stopped,
    Unpublished,
}

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema, PartialEq, Eq)]
#[serde(deny_unknown_fields)]
pub struct CollectionExecutionStatus {
    pub phase: CollectionExecutionPhase,
    pub request_sequence: Option<u32>,
    pub publication_retries: u32,
    pub publication_retry_limit: u32,
}

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema, PartialEq)]
#[serde(deny_unknown_fields)]
pub struct CollectionRunSummary {
    pub id: String,
    pub request_key: String,
    pub record_version: u32,
    pub mode: AcquisitionMode,
    pub collector_policy: String,
    pub input: CollectionInput,
    pub created_at_ms: i64,
    pub updated_at_ms: i64,
    pub state: CollectionState,
    pub generation: u32,
    pub first_started_at_ms: Option<i64>,
    pub deadline_at_ms: Option<i64>,
    pub cancellation_requested: bool,
    pub requests_used: u32,
    pub pages_retained: u32,
    pub frontier_remaining: u32,
}

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema, PartialEq, Eq)]
#[serde(deny_unknown_fields)]
pub struct CollectionOriginalRef {
    pub evidence_id: String,
    pub sha256: String,
    pub bytes: u64,
}

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema, PartialEq, Eq)]
#[serde(deny_unknown_fields)]
pub struct CollectionRequestView {
    pub sequence: u32,
    pub generation: u32,
    pub entry: FrontierEntry,
    pub reserved_at_ms: i64,
    pub progress: RequestProgress,
    pub original: Option<CollectionOriginalRef>,
}

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema, PartialEq, Eq)]
#[serde(deny_unknown_fields)]
pub struct CollectionControls {
    pub can_cancel: bool,
    pub can_resume: bool,
    pub can_retry_settlement: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema, PartialEq)]
#[serde(deny_unknown_fields)]
pub struct CollectionRunInspection {
    pub schema_version: u32,
    pub workspace_revision: u64,
    pub availability: CollectionAvailability,
    pub native_execution_enabled: bool,
    pub run: CollectionRunSummary,
    pub execution: CollectionExecutionStatus,
    pub controls: CollectionControls,
    pub requests: Vec<CollectionRequestView>,
    pub limitations: Vec<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct CollectionRunPageRequest {
    pub page_size: u32,
    pub cursor: Option<String>,
}
impl CollectionRunPageRequest {
    pub(crate) fn validate(&self) -> Result<()> {
        require(
            (1..=PAGE_LIMIT).contains(&self.page_size),
            "Collection page size must be 1 to 25",
        )?;
        require(
            self.cursor.as_ref().is_none_or(|c| c.len() <= 1024),
            "Collection cursor is too large",
        )
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema, PartialEq)]
#[serde(deny_unknown_fields)]
pub struct CollectionRunPage {
    pub schema_version: u32,
    pub workspace_revision: u64,
    pub availability: CollectionAvailability,
    pub native_execution_enabled: bool,
    pub scope_count: u64,
    pub rows: Vec<CollectionRunSummary>,
    pub next_cursor: Option<String>,
}
