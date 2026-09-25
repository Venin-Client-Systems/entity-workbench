//! Versioned acquisition facts. These records do not assert source relevance or access approval.
use crate::{domain::JobState, require, Result};
use schemars::JsonSchema;
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema, PartialEq)]
#[serde(rename_all = "snake_case")]
pub enum AcquisitionMode {
    Live,
    Synthetic,
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize, JsonSchema, PartialEq)]
#[serde(rename_all = "snake_case")]
pub enum RequestPurpose {
    AccessReview,
    Seed,
    Link,
    Redirect,
}

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema, PartialEq)]
#[serde(rename_all = "snake_case")]
pub enum FetchOutcome {
    Fetched,
    Blocked,
    Failed,
    Incomplete,
}

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct RequestReceipt {
    pub sequence: u32,
    pub url: String,
    pub method: String,
    pub purpose: RequestPurpose,
    pub parent_request: Option<u32>,
    pub hop: u32,
    pub started_at: String,
    pub ended_at: String,
    pub outcome: FetchOutcome,
    pub http_status: Option<u16>,
    /// Parsed, normalized media type; not the complete response header block.
    pub media_type: Option<String>,
    /// Resolved credential-free HTTPS destination, including out-of-scope hosts.
    /// Invalid/credential-bearing Location values are omitted, never echoed.
    pub redirect_url: Option<String>,
    pub body_sha256: Option<String>,
    pub body_bytes: Option<u64>,
    pub original_evidence_id: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct CollectionReceipt {
    pub schema_version: u32,
    pub job_id: String,
    pub mode: AcquisitionMode,
    pub application_version: String,
    pub collector_policy: String,
    pub selected_urls: Vec<String>,
    pub max_hops: u32,
    pub max_requests: u32,
    pub max_seconds: u64,
    pub requests_used: u32,
    pub started_at: String,
    pub ended_at: String,
    pub elapsed_milliseconds: u64,
    pub time_limit_exceeded: bool,
    pub start_revision: u64,
    pub retained_revision: u64,
    pub state: JobState,
    pub retention_complete: bool,
    pub requests: Vec<RequestReceipt>,
    pub notes: Vec<String>,
}

fn timestamp(raw: &str) -> Result<chrono::DateTime<chrono::FixedOffset>> {
    require(
        raw.len() == 20 && raw.ends_with('Z'),
        "Receipt timestamp must be UTC to seconds",
    )?;
    chrono::DateTime::parse_from_rfc3339(raw)
        .map_err(|_| crate::Error::Validation("Invalid receipt timestamp".into()))
}

fn sha256(raw: &str) -> bool {
    raw.len() == 64
        && raw
            .bytes()
            .all(|b| b.is_ascii_digit() || (b'a'..=b'f').contains(&b))
}

impl CollectionReceipt {
    /// Validate structure and bindings before publishing/reading. Original bytes are checked by Rust storage.
    pub fn validate(&self) -> Result<()> {
        require(self.schema_version == 1, "Unsupported collection receipt")?;
        require(
            uuid::Uuid::parse_str(&self.job_id).is_ok(),
            "Invalid receipt job identifier",
        )?;
        require(
            self.collector_policy == "direct-https-v1" && self.application_version.len() <= 80,
            "Unknown collector policy",
        )?;
        let seeds = crate::collection::validate_seeds(&self.selected_urls)?;
        require(
            self.max_hops <= 2
                && self.max_requests > 0
                && self.max_requests <= 50
                && self.max_seconds > 0
                && self.max_seconds <= 600,
            "Invalid receipt limits",
        )?;
        require(
            self.requests_used as usize == self.requests.len()
                && self.requests_used <= self.max_requests,
            "Receipt accounting mismatch",
        )?;
        require(
            self.start_revision < self.retained_revision,
            "Invalid receipt revision range",
        )?;
        require(
            !matches!(self.state, JobState::Queued | JobState::Running),
            "Receipt must describe a finished attempt",
        )?;
        require(
            self.retention_complete || matches!(self.state, JobState::Failed),
            "Incomplete retention must be a failed job",
        )?;
        let start = timestamp(&self.started_at)?;
        let end = timestamp(&self.ended_at)?;
        require(
            start <= end && end <= chrono::Utc::now() + chrono::Duration::seconds(5),
            "Invalid or future receipt chronology",
        )?;
        let wall_milliseconds = (end - start).num_milliseconds() as u64;
        require(
            self.elapsed_milliseconds.abs_diff(wall_milliseconds) < 1000,
            "Receipt duration differs from timestamps",
        )?;
        require(
            self.time_limit_exceeded == (self.elapsed_milliseconds > self.max_seconds * 1000),
            "Receipt time-limit accounting mismatch",
        )?;
        require(
            !self.time_limit_exceeded
                || matches!(self.state, JobState::QuotaExhausted | JobState::Failed),
            "Exceeded time limit must be explicit in terminal state",
        )?;
        require(
            self.notes.len() <= 600 && self.notes.iter().all(|n| n.len() <= 4096),
            "Receipt notes exceed bound",
        )?;
        let mut previous_end = start;
        for (index, request) in self.requests.iter().enumerate() {
            require(
                request.sequence as usize == index
                    && request.method == "GET"
                    && request.hop <= self.max_hops,
                "Invalid receipt request",
            )?;
            let url = crate::policy::validate_https_url(&request.url)?;
            require(
                url.as_str() == request.url && seeds.iter().any(|s| s.host_str() == url.host_str()),
                "Request is outside selected hosts",
            )?;
            let request_start = timestamp(&request.started_at)?;
            let request_end = timestamp(&request.ended_at)?;
            require(
                previous_end <= request_start && request_start <= request_end && request_end <= end,
                "Invalid request chronology",
            )?;
            previous_end = request_end;
            match request.purpose {
                RequestPurpose::AccessReview | RequestPurpose::Seed => {
                    require(
                        request.parent_request.is_none() && request.hop == 0,
                        "Initial request cannot have a parent",
                    )?;
                    if request.purpose == RequestPurpose::Seed {
                        require(
                            seeds.contains(&url),
                            "Seed request differs from selected seed",
                        )?;
                    } else {
                        require(
                            url.path() == "/robots.txt" && url.query().is_none(),
                            "Unexpected access-review request",
                        )?;
                    }
                }
                RequestPurpose::Link | RequestPurpose::Redirect => {
                    let parent = request
                        .parent_request
                        .and_then(|p| ((p as usize) < index).then_some(p as usize))
                        .ok_or_else(|| crate::Error::Validation("Invalid request parent".into()))?;
                    let source = &self.requests[parent];
                    require(
                        source.outcome == FetchOutcome::Fetched
                            && source.purpose != RequestPurpose::AccessReview,
                        "Request parent was not acquired content",
                    )?;
                    if request.purpose == RequestPurpose::Link {
                        require(
                            source.http_status == Some(200) && request.hop == source.hop + 1,
                            "Invalid followed-link ancestry",
                        )?;
                    } else {
                        require(
                            source
                                .http_status
                                .is_some_and(|s| [301, 302, 303, 307, 308].contains(&s))
                                && source.redirect_url.as_deref() == Some(&request.url)
                                && request.hop == source.hop,
                            "Invalid redirect ancestry",
                        )?;
                    }
                }
            }
            if request.outcome == FetchOutcome::Fetched {
                require(
                    request
                        .http_status
                        .is_some_and(|s| (100..=599).contains(&s))
                        && request.body_sha256.as_deref().is_some_and(sha256)
                        && request.body_bytes.is_some_and(|b| b <= 2 * 1024 * 1024),
                    "Invalid fetched response metadata",
                )?;
                require(
                    request.original_evidence_id.is_none()
                        || request.original_evidence_id == request.body_sha256,
                    "Original binding differs from response hash",
                )?;
                require(
                    !self.retention_complete || request.original_evidence_id.is_some(),
                    "Complete receipt lacks original",
                )?;
            } else if request.outcome == FetchOutcome::Incomplete {
                require(
                    request
                        .http_status
                        .is_some_and(|s| (100..=599).contains(&s))
                        && request.body_sha256.is_none()
                        && request.body_bytes.is_none()
                        && request.original_evidence_id.is_none(),
                    "Incomplete body cannot claim a complete original",
                )?;
            } else {
                require(
                    request.http_status.is_none()
                        && request.body_sha256.is_none()
                        && request.body_bytes.is_none()
                        && request.original_evidence_id.is_none()
                        && request.media_type.is_none()
                        && request.redirect_url.is_none(),
                    "Unfetched attempt cannot claim a response",
                )?;
            }
            if let Some(media) = &request.media_type {
                require(
                    media.len() <= 128 && media.is_ascii() && !media.chars().any(char::is_control),
                    "Invalid response media type",
                )?;
            }
            if let Some(redirect) = &request.redirect_url {
                require(
                    crate::policy::validate_https_url(redirect)?.as_str() == redirect
                        && request
                            .http_status
                            .is_some_and(|s| [301, 302, 303, 307, 308].contains(&s)),
                    "Invalid retained redirect",
                )?;
            }
        }
        Ok(())
    }
}
