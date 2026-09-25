//! Private synthetic access-prefix format. No production claimant or public DTO.
#![allow(dead_code)]
use crate::{
    collection_jobs::{RequestProgress, MAX_EVENTS, MAX_RECORD_BYTES},
    collection_profile::{PublisherAccessInput, ValidatedPublisherAccessPlan},
    collection_settlement::TransportReceipt,
    require, Result,
};
use serde::{Deserialize, Serialize};

pub(crate) const KIND: &str = "collection_access_experiment_v5";
pub(crate) const KEY_KIND: &str = "collection_access_experiment_key_v5";
pub(crate) const POLICY: &str = "selected-publisher-access-synthetic-prefix-v5";

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub(crate) enum State {
    Queued,
    Running,
    AwaitingDecision,
    ReadyContent,
    Interrupted,
    Cancelled,
    Blocked,
    Failed,
    QuotaExhausted,
    RecoveryRequired,
    ContentBoundary,
}
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub(crate) enum Phase {
    Access,
    Content,
}
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case", deny_unknown_fields)]
pub(crate) enum Purpose {
    Robots { host: String },
    SelectedAccess { index: u32 },
    Seed {},
}
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct Request {
    pub sequence: u32,
    pub generation: u32,
    pub lease: String,
    pub url: String,
    pub purpose: Purpose,
    /// Access dependencies never become content ancestors. This prefix has no links.
    pub content_parent: Option<u32>,
    pub robots_sequence: Option<u32>,
    pub reserved_at_ms: i64,
    #[serde(deserialize_with = "closed_progress")]
    pub progress: RequestProgress,
}
// Do not widen legacy RequestProgress's decoder. In this new format the empty
// reserved object must reject extra fields, and old non-lossless Settled is invalid.
fn closed_progress<'de, D: serde::Deserializer<'de>>(
    deserializer: D,
) -> std::result::Result<RequestProgress, D::Error> {
    #[derive(Deserialize)]
    #[serde(tag = "state", rename_all = "snake_case", deny_unknown_fields)]
    enum Closed {
        Reserved {},
        Observed { receipt: Box<TransportReceipt> },
        InterruptedUnknown { recovered_at_ms: i64 },
    }
    Ok(match Closed::deserialize(deserializer)? {
        Closed::Reserved {} => RequestProgress::Reserved,
        Closed::Observed { receipt } => RequestProgress::Observed { receipt: *receipt },
        Closed::InterruptedUnknown { recovered_at_ms } => {
            RequestProgress::InterruptedUnknown { recovered_at_ms }
        }
    })
}
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct Robots {
    pub host: String,
    pub sequence: u32,
    pub usable: bool,
}
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub(crate) enum DecisionOutcome {
    Allow,
    Deny,
    Unknown,
}
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct Decision {
    pub request_key: String,
    pub review_sha256: String,
    pub outcome: DecisionOutcome,
    pub reason: String,
}
impl Decision {
    pub(crate) fn validate(&self) -> Result<()> {
        require(
            crate::collection_jobs::canonical_uuid(&self.request_key)
                && digest(&self.review_sha256)
                && self.reason.len() <= 4000,
            "Malformed synthetic access decision",
        )
    }
}
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct Checkpoint {
    pub state: State,
    pub phase: Phase,
    pub generation: u32,
    pub lease: Option<String>,
    pub first_started_at_ms: Option<i64>,
    pub deadline_at_ms: Option<i64>,
    pub updated_at_ms: i64,
    pub cancellation_requested: bool,
    pub access_cursor: u32,
    pub robots: Vec<Robots>,
    pub requests: Vec<Request>,
    pub decision: Option<Decision>,
    pub content_request: Option<u32>,
    pub promoted: bool,
}
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "event", rename_all = "snake_case", deny_unknown_fields)]
pub(crate) enum Event {
    Start {
        at_ms: i64,
        lease: String,
    },
    Resume {
        at_ms: i64,
        lease: String,
    },
    Advance {
        at_ms: i64,
    },
    Observed {
        clock_anchor_ms: i64,
        sequence: u32,
        receipt: TransportReceipt,
    },
    Decide {
        at_ms: i64,
        decision: Decision,
    },
    Cancel {
        at_ms: i64,
    },
    Recover {
        at_ms: i64,
    },
}
impl Event {
    pub(crate) fn at_ms(&self) -> i64 {
        match self {
            Self::Observed {
                clock_anchor_ms, ..
            } => *clock_anchor_ms,
            Self::Start { at_ms, .. }
            | Self::Resume { at_ms, .. }
            | Self::Advance { at_ms }
            | Self::Decide { at_ms, .. }
            | Self::Cancel { at_ms }
            | Self::Recover { at_ms } => *at_ms,
        }
    }
}
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct Job {
    pub schema_version: u32,
    pub id: String,
    pub request_key: String,
    pub collector_policy: String,
    pub synthetic: bool,
    pub plan_input: PublisherAccessInput,
    pub plan_sha256: String,
    pub created_at_ms: i64,
    pub events: Vec<Event>,
    pub checkpoint: Checkpoint,
}
impl Job {
    pub(crate) fn profile(&self) -> Result<ValidatedPublisherAccessPlan> {
        require(
            self.schema_version == 5
                && self.synthetic
                && self.collector_policy == POLICY
                && crate::collection_jobs::canonical_uuid(&self.id)
                && crate::collection_jobs::canonical_uuid(&self.request_key)
                && self.events.len() <= MAX_EVENTS,
            "Unsupported experimental access record",
        )?;
        let profile = ValidatedPublisherAccessPlan::validate(self.plan_input.clone())?;
        require(
            profile.plan_sha256() == self.plan_sha256,
            "Stored profile hash differs",
        )?;
        Ok(profile)
    }
    pub(crate) fn bounded(&self) -> Result<()> {
        require(
            serde_json::to_vec(self)?.len() <= MAX_RECORD_BYTES,
            "Experimental access record exceeds 4 MiB",
        )
    }
}
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct Execution {
    pub run_id: String,
    pub generation: u32,
    pub lease: String,
    pub ownership_lifetime: String,
}
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct Ticket {
    pub ownership_lifetime: String,
    pub run_id: String,
    pub generation: u32,
    pub lease: String,
    pub sequence: u32,
    pub url: String,
    pub purpose: Purpose,
    pub plan_sha256: String,
}
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub(crate) struct AccessItem {
    pub selected_index: Option<u32>,
    pub sequence: u32,
    pub url: String,
    pub purpose: Purpose,
    pub receipt_sha256: String,
    pub original_sha256: String,
    pub original_bytes: u64,
    pub observed_wall_ms: i64,
}
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub(crate) struct Review {
    pub run_id: String,
    pub generation: u32,
    pub plan_sha256: String,
    pub journal_sha256: String,
    pub deadline_at_ms: i64,
    pub requests_used: u32,
    pub inventory: Vec<AccessItem>,
    pub review_sha256: String,
}
pub(crate) fn review(job: &Job) -> Result<Review> {
    require(
        job.checkpoint.state == State::AwaitingDecision,
        "Access review is not ready",
    )?;
    let mut inventory = Vec::new();
    for request in &job.checkpoint.requests {
        let RequestProgress::Observed { receipt } = &request.progress else {
            return Err(crate::Error::Validation(
                "Access review contains an unresolved request".into(),
            ));
        };
        let crate::collection_jobs::FetchRecord::Complete { sha256, bytes, .. } =
            receipt.fetch_record()
        else {
            return Err(crate::Error::Validation(
                "Access review lacks complete originals".into(),
            ));
        };
        inventory.push(AccessItem {
            selected_index: job
                .plan_input
                .access_urls
                .iter()
                .position(|url| url == &request.url)
                .map(|index| index as u32),
            sequence: request.sequence,
            url: request.url.clone(),
            purpose: request.purpose.clone(),
            receipt_sha256: crate::store::hash(&serde_json::to_vec(receipt)?),
            original_sha256: sha256,
            original_bytes: bytes,
            observed_wall_ms: receipt.observed_wall_ms,
        });
    }
    let mut result = Review {
        run_id: job.id.clone(),
        generation: job.checkpoint.generation,
        plan_sha256: job.plan_sha256.clone(),
        journal_sha256: crate::store::hash(&serde_json::to_vec(job)?),
        deadline_at_ms: job.checkpoint.deadline_at_ms.expect("started access"),
        requests_used: job.checkpoint.requests.len() as u32,
        inventory,
        review_sha256: String::new(),
    };
    // Private v5 encoding: compact struct order with the digest slot empty.
    result.review_sha256 = crate::store::hash(&serde_json::to_vec(&result)?);
    Ok(result)
}
fn digest(value: &str) -> bool {
    value.len() == 64
        && value
            .bytes()
            .all(|c| c.is_ascii_digit() || (b'a'..=b'f').contains(&c))
}
