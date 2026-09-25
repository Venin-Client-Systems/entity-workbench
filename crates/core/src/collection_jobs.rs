//! Private durable collection protocol. No command, executor or network is activated.
// This foundation is deliberately not reachable from production dispatch yet.
#![allow(dead_code)]
use crate::{collection::validate_seeds, require, Result};
use serde::{Deserialize, Serialize};

pub(crate) const MAX_EVENTS: usize = 256;
pub(crate) const MAX_RECORD_BYTES: usize = 4 * 1024 * 1024;
pub(crate) const MAX_FRONTIER: usize = 500;

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(deny_unknown_fields)]
pub(crate) struct CollectionInput {
    pub urls: Vec<String>,
    pub max_hops: u32,
    pub max_requests: u32,
    pub max_seconds: u64,
}
impl CollectionInput {
    pub fn normalized(mut self) -> Result<Self> {
        self.urls = validate_seeds(&self.urls)?
            .into_iter()
            .map(|url| url.to_string())
            .collect();
        require(
            self.max_hops <= 2
                && (1..=50).contains(&self.max_requests)
                && (1..=600).contains(&self.max_seconds),
            "Collection limits exceed policy",
        )?;
        Ok(self)
    }
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub(crate) enum CollectionState {
    Queued,
    Running,
    Interrupted,
    RecoveryRequired,
    Cancelled,
    Blocked,
    QuotaExhausted,
    Failed,
    Partial,
    Successful,
    SuccessfulNoResults,
}
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub(crate) enum Purpose {
    Robots,
    Seed,
    Link,
    Redirect,
}
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(deny_unknown_fields)]
pub(crate) struct FrontierEntry {
    pub url: String,
    pub hop: u32,
    pub redirects: u32,
    pub purpose: Purpose,
    pub parent: Option<u32>,
}
#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub(crate) enum TransportFailure {
    Network,
    Policy,
    Quota,
    Interrupted,
}
/// Sanitized acquisition facts; raw headers and incomplete body bytes are absent.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(tag = "kind", rename_all = "snake_case", deny_unknown_fields)]
pub(crate) enum FetchRecord {
    Complete {
        status: u16,
        media_type: Option<String>,
        redirect_url: Option<String>,
        sha256: String,
        bytes: u64,
    },
    Incomplete {
        status: u16,
        media_type: Option<String>,
    },
    Failed {
        reason: TransportFailure,
    },
}
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(tag = "state", rename_all = "snake_case", deny_unknown_fields)]
pub(crate) enum RequestProgress {
    Reserved,
    Settled {
        ended_at_ms: i64,
        result: FetchRecord,
    },
    Observed {
        receipt: crate::collection_settlement::TransportReceipt,
    },
    InterruptedUnknown {
        recovered_at_ms: i64,
    },
}
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(deny_unknown_fields)]
pub(crate) struct ChargedRequest {
    pub sequence: u32,
    pub generation: u32,
    pub lease: String,
    pub entry: FrontierEntry,
    pub reserved_at_ms: i64,
    pub progress: RequestProgress,
}
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(deny_unknown_fields)]
pub(crate) struct RobotsCheckpoint {
    pub host: String,
    pub request_sequence: u32,
    /// Only a fully retained 404 or supported 200 permits evaluating a page.
    pub usable: bool,
}
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(deny_unknown_fields)]
pub(crate) struct CollectionCheckpoint {
    pub state: CollectionState,
    pub generation: u32,
    pub lease: Option<String>,
    pub first_started_at_ms: Option<i64>,
    pub deadline_at_ms: Option<i64>,
    pub updated_at_ms: i64,
    pub cancellation_requested: bool,
    pub frontier: Vec<FrontierEntry>,
    pub visited: Vec<String>,
    pub robots: Vec<RobotsCheckpoint>,
    pub requests: Vec<ChargedRequest>,
    pub pages_retained: u32,
    pub saw_blocked: bool,
    pub saw_failed: bool,
    pub saw_quota: bool,
    pub saw_unknown: bool,
}
impl CollectionCheckpoint {
    pub fn requests_used(&self) -> u32 {
        self.requests.len() as u32
    }
}
/// Events are replayed against verified originals. The stored checkpoint is a
/// cache checked for exact equality, never trusted as a supplied frontier.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(tag = "event", rename_all = "snake_case", deny_unknown_fields)]
pub(crate) enum CollectionEvent {
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
    Complete {
        at_ms: i64,
        sequence: u32,
        result: FetchRecord,
    },
    TransportObserved {
        clock_anchor_ms: i64,
        sequence: u32,
        receipt: crate::collection_settlement::TransportReceipt,
    },
    Cancel {
        at_ms: i64,
    },
    StopAcknowledged {
        at_ms: i64,
    },
    Recover {
        at_ms: i64,
    },
}
impl CollectionEvent {
    pub fn at_ms(&self) -> i64 {
        match self {
            Self::TransportObserved {
                clock_anchor_ms, ..
            } => *clock_anchor_ms,
            Self::Start { at_ms, .. }
            | Self::Resume { at_ms, .. }
            | Self::Advance { at_ms }
            | Self::Complete { at_ms, .. }
            | Self::Cancel { at_ms }
            | Self::StopAcknowledged { at_ms }
            | Self::Recover { at_ms } => *at_ms,
        }
    }
}
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(deny_unknown_fields)]
pub(crate) struct DurableCollectionJob {
    pub schema_version: u32,
    pub id: String,
    pub request_key: String,
    pub collector_policy: String,
    /// This unactivated foundation can only publish synthetic specimens.
    pub synthetic: bool,
    pub input: CollectionInput,
    pub created_at_ms: i64,
    pub events: Vec<CollectionEvent>,
    pub checkpoint: CollectionCheckpoint,
}
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct CollectionTicket {
    pub job_id: String,
    pub generation: u32,
    pub lease: String,
}
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct RequestTicket {
    pub run: CollectionTicket,
    pub sequence: u32,
    pub url: String,
}
/// Private transport seam only. Never deserializable from frontend/worker JSON.
#[derive(Debug)]
pub(crate) enum CollectionResponse {
    Complete {
        status: u16,
        content_type: String,
        location: Option<String>,
        body: Vec<u8>,
    },
    Incomplete {
        status: u16,
        content_type: String,
    },
    Failed(TransportFailure),
}
pub(crate) fn canonical_uuid(value: &str) -> bool {
    uuid::Uuid::parse_str(value).is_ok_and(|id| id.to_string() == value)
}
