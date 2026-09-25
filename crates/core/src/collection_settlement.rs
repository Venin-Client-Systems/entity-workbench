//! Lossless private transport receipts. No frontend/worker can submit these for publication.
#![allow(dead_code)]
use crate::{
    collection_jobs::{CollectionInput, FetchRecord, TransportFailure},
    collection_machine::validate_fetch,
    collection_transport::{
        CallerContextState, Observation, Outcome, Phase, ResponseHead, StopReason,
    },
    policy, require, Result,
};
use serde::{Deserialize, Serialize};
use std::net::SocketAddr;

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(deny_unknown_fields)]
pub(crate) struct CandidateSnapshot {
    pub addresses: Vec<SocketAddr>,
    pub method: String,
    pub authoritative_complete_set: bool,
}
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(deny_unknown_fields)]
pub(crate) struct ResolverUncertainty {
    pub method: String,
    pub caller_context: CallerContextState,
}
#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub(crate) enum HttpDelivery {
    DefinitivelyBeforeHttp,
    MayHaveBeenSent,
}
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(tag = "kind", rename_all = "snake_case", deny_unknown_fields)]
pub(crate) enum ReceiptOutcome {
    Complete {
        head: ResponseHead,
        sha256: String,
        bytes: u64,
    },
    Stopped {
        reason: StopReason,
        head: Option<ResponseHead>,
    },
}
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(deny_unknown_fields)]
pub(crate) struct TransportReceipt {
    pub schema_version: u32,
    pub outcome: ReceiptOutcome,
    pub phase: Phase,
    pub http_delivery: HttpDelivery,
    pub elapsed_milliseconds: u64,
    pub observed_wall_ms: i64,
    pub resolved: Option<CandidateSnapshot>,
    pub resolver_uncertainty: Option<ResolverUncertainty>,
    pub stop_observed: Option<StopReason>,
    pub locally_quiescent: bool,
}
fn delivery(phase: Phase) -> HttpDelivery {
    match phase {
        Phase::BeforeRequest | Phase::Pacing | Phase::Dns => HttpDelivery::DefinitivelyBeforeHttp,
        Phase::ConnectTlsHeaders | Phase::Body => HttpDelivery::MayHaveBeenSent,
    }
}
impl TransportReceipt {
    pub fn from_observation(observation: &Observation) -> Self {
        Self {
            schema_version: 1,
            outcome: match &observation.outcome {
                Outcome::Complete { head, body } => ReceiptOutcome::Complete {
                    head: head.clone(),
                    sha256: crate::store::hash(body),
                    bytes: body.len() as u64,
                },
                Outcome::Stopped { reason, head } => ReceiptOutcome::Stopped {
                    reason: *reason,
                    head: head.clone(),
                },
            },
            phase: observation.phase,
            http_delivery: delivery(observation.phase),
            elapsed_milliseconds: observation.elapsed_milliseconds,
            observed_wall_ms: observation.observed_wall_ms,
            resolved: observation
                .resolved
                .as_ref()
                .map(|value| CandidateSnapshot {
                    addresses: value.addresses.clone(),
                    method: value.method.into(),
                    authoritative_complete_set: value.authoritative_complete_set,
                }),
            resolver_uncertainty: observation.resolver_uncertainty.map(|value| {
                ResolverUncertainty {
                    method: value.method.into(),
                    caller_context: value.caller_context,
                }
            }),
            stop_observed: observation.stop_observed,
            locally_quiescent: observation.locally_quiescent,
        }
    }
    pub fn stopped_for(&self, reason: StopReason) -> bool {
        self.stop_observed == Some(reason)
            || matches!(self.outcome, ReceiptOutcome::Stopped { reason: actual, .. } if actual == reason)
    }
    pub fn fetch_record(&self) -> FetchRecord {
        match &self.outcome {
            ReceiptOutcome::Complete {
                head,
                sha256,
                bytes,
            } => FetchRecord::Complete {
                status: head.status,
                media_type: head.media_type.clone(),
                redirect_url: if [301, 302, 303, 307, 308].contains(&head.status) {
                    head.redirect_url.clone()
                } else {
                    None
                },
                sha256: sha256.clone(),
                bytes: *bytes,
            },
            ReceiptOutcome::Stopped {
                head: Some(head), ..
            } => FetchRecord::Incomplete {
                status: head.status,
                media_type: head.media_type.clone(),
            },
            ReceiptOutcome::Stopped { reason, .. } => FetchRecord::Failed {
                reason: match reason {
                    StopReason::Deadline | StopReason::BodyLimit => TransportFailure::Quota,
                    StopReason::Policy | StopReason::ResolverUnavailable | StopReason::Busy => {
                        TransportFailure::Policy
                    }
                    StopReason::Cancelled
                    | StopReason::ClockChanged
                    | StopReason::QuiescenceUnverified
                    | StopReason::RecoveryRequired => TransportFailure::Interrupted,
                    StopReason::Timeout | StopReason::Network => TransportFailure::Network,
                },
            },
        }
    }
    pub fn head(&self) -> Option<&ResponseHead> {
        match &self.outcome {
            ReceiptOutcome::Complete { head, .. } => Some(head),
            ReceiptOutcome::Stopped { head, .. } => head.as_ref(),
        }
    }
    pub fn validate(&self, url: &str, input: &CollectionInput, body: Option<&[u8]>) -> Result<()> {
        require(
            self.schema_version == 1 && self.http_delivery == delivery(self.phase),
            "Invalid transport receipt version or delivery knowledge",
        )?;
        require(
            self.stop_observed.is_none_or(|reason| {
                matches!(
                    reason,
                    StopReason::Cancelled | StopReason::Deadline | StopReason::ClockChanged
                )
            }),
            "Invalid final transport stop observation",
        )?;
        require(
            chrono::DateTime::from_timestamp_millis(self.observed_wall_ms).is_some(),
            "Transport clock sample is not representable",
        )?;
        validate_fetch(&self.fetch_record(), body)?;
        if let Some(head) = self.head() {
            require(
                self.phase == Phase::Body && (100..=599).contains(&head.status),
                "Response head has invalid phase or status",
            )?;
            if let Some(target) = &head.redirect_url {
                require(
                    policy::validate_https_url(target)?.as_str() == target,
                    "Invalid transport redirect",
                )?;
            }
        }
        require(
            !matches!(self.outcome, ReceiptOutcome::Complete { .. })
                || (self.phase == Phase::Body && self.locally_quiescent),
            "Complete body lacks local completion",
        )?;
        let uncertain = self.stopped_for(StopReason::QuiescenceUnverified)
            || self.stopped_for(StopReason::RecoveryRequired);
        require(
            self.locally_quiescent != uncertain,
            "Transport quiescence contradicts outcome",
        )?;
        require(
            self.resolver_uncertainty.is_some()
                == self.stopped_for(StopReason::QuiescenceUnverified),
            "Resolver uncertainty is missing or unexpected",
        )?;
        if let Some(uncertainty) = &self.resolver_uncertainty {
            require(
                self.phase == Phase::Dns
                    && uncertainty.method == "windows_overlapped_dns"
                    && self.resolved.is_none(),
                "Invalid resolver uncertainty provenance",
            )?;
        }
        if let Some(resolved) = &self.resolved {
            require(
                !resolved.authoritative_complete_set
                    && !resolved.addresses.is_empty()
                    && resolved.addresses.len() <= 64
                    && resolved
                        .addresses
                        .iter()
                        .all(|address| address.port() == 443)
                    && matches!(
                        resolved.method.as_str(),
                        "literal_ip"
                            | "macos_dns_service_observed_batch"
                            | "windows_completed_system_candidates"
                            | "synthetic_fixed_candidates"
                    ),
                "Invalid observed candidate snapshot",
            )?;
            let seeds = input
                .urls
                .iter()
                .map(|raw| policy::validate_https_url(raw))
                .collect::<Result<Vec<_>>>()?;
            let hosts = seeds
                .iter()
                .filter_map(|seed| seed.host_str())
                .collect::<Vec<_>>();
            policy::validate_destination(
                url,
                &resolved
                    .addresses
                    .iter()
                    .map(|address| address.ip())
                    .collect::<Vec<_>>(),
                &hosts,
            )?;
            require(
                matches!(
                    self.phase,
                    Phase::Dns | Phase::ConnectTlsHeaders | Phase::Body
                ),
                "Resolved candidates precede DNS",
            )?;
        }
        require(
            !matches!(self.phase, Phase::ConnectTlsHeaders | Phase::Body)
                || self.resolved.is_some(),
            "HTTP phase lacks pinned candidate snapshot",
        )?;
        Ok(())
    }
}
