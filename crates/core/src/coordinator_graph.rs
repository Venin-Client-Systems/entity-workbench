//! Exclusive automatic-write interval. Normal startup has no configured graph executor.
//! Lock order is workspace -> activity -> collection lane.
#![allow(dead_code)] // Private host seams await a separately reviewed runtime attachment.
use super::*;
use crate::processing::{JobTicket, ProcessingJob};
use crate::store::graph_analysis::GraphAttempt;
use std::{collections::BTreeMap, path::PathBuf};

#[cfg(test)]
type SyntheticGraph = dyn Fn(&[u8], &CancellationToken) -> Result<Vec<u8>> + Send + Sync;
pub(super) enum GraphExecution {
    Unavailable,
    Configured(crate::engines::python_graph::VerifiedGraphRuntime),
    #[cfg(test)]
    Synthetic(Arc<SyntheticGraph>),
}
impl GraphExecution {
    fn available(&self) -> bool {
        !matches!(self, Self::Unavailable)
    }
    fn execute(&self, scratch: &Path, input: &[u8], token: &CancellationToken) -> Result<Vec<u8>> {
        match self {
            Self::Unavailable => {
                let _ = (scratch, input, token);
                Err(Error::Blocked(
                    "Confined application-local graph runtime unavailable".into(),
                ))
            }
            Self::Configured(runtime) => runtime.execute(scratch, input, token),
            #[cfg(test)]
            Self::Synthetic(executor) => executor(input, token),
        }
    }
}
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum GraphPhase {
    Draining,
    Running,
    Publishing,
    PublicationPending,
    RecoveryRequired,
    UnpublishedKnownStopped,
}
#[derive(Clone, Debug)]
pub(crate) struct GraphStatus {
    pub job_id: String,
    pub attempt: u32,
    pub lease: Option<String>,
    pub request_sha256: Option<String>,
    pub phase: GraphPhase,
    pub retries: u32,
    pub can_retry: bool,
}
struct Drain {
    job: ProcessingJob,
    // No new claims are admitted after this snapshot. Existing tokens stay registered
    // through terminal publication, so an empty registry is a proven drained set.
    frozen_processing: Vec<String>,
    collection: Option<crate::collection_jobs::CollectionTicket>,
}
struct Owned {
    status: GraphStatus,
    retry_requested: bool,
    cancel_settlement_used: bool,
}
enum Interval {
    Open,
    Draining(Box<Drain>),
    Owned(Owned),
}
pub(super) struct ProcessingActivity {
    pub tokens: BTreeMap<String, CancellationToken>,
    interval: Interval,
    // Retain bounded authority/result after a failed final shutdown settlement.
    retained: Option<Box<Pending>>,
    #[cfg(test)]
    publication_attempts: u32,
}
impl Default for ProcessingActivity {
    fn default() -> Self {
        Self {
            tokens: BTreeMap::new(),
            interval: Interval::Open,
            retained: None,
            #[cfg(test)]
            publication_attempts: 0,
        }
    }
}
impl ProcessingActivity {
    pub(super) fn allows_resume(&self) -> bool {
        matches!(self.interval, Interval::Open)
    }
    fn status(&self) -> Option<GraphStatus> {
        match &self.interval {
            Interval::Open => None,
            Interval::Draining(drain) => Some(GraphStatus {
                job_id: drain.job.id.clone(),
                attempt: drain.job.attempt,
                lease: None,
                request_sha256: None,
                phase: GraphPhase::Draining,
                retries: 0,
                can_retry: false,
            }),
            Interval::Owned(owned) => Some(owned.status.clone()),
        }
    }
}
#[derive(Clone, Copy)]
pub(super) enum Lane {
    Processing,
    Collection,
}
pub(super) enum Admission {
    Open,
    Wait,
    Graph(Box<ProcessingJob>),
}

/// Called with workspace and activity held, before either loop can write a claim.
/// Only the already-admitted collection ticket may start while draining.
pub(super) fn admit(
    shared: &Shared,
    workspace: &mut Workspace,
    activity: &mut ProcessingActivity,
    lane: Lane,
) -> Result<Admission> {
    if let Interval::Draining(drain) = &activity.interval {
        let current = workspace.processing_job(&drain.job.id)?;
        if serde_json::to_vec(&current)? != serde_json::to_vec(&drain.job)? {
            // A queued cancellation/manual transition is not an execution or stop observation.
            activity.interval = Interval::Open;
        }
    }
    if matches!(activity.interval, Interval::Open) {
        let Some((sequence, job)) = workspace.processing_queue_head()? else {
            return Ok(Admission::Open);
        };
        if !matches!(job.input, ProcessingInput::ShortestConnectionPath { .. }) {
            return Ok(Admission::Open);
        }
        if !shared.graph_executor.available() {
            // Known unavailability never pauses other lanes or creates a Running claim.
            if matches!(lane, Lane::Processing) {
                workspace.block_graph_before_claim(&job, "Verified graph executor unavailable; no exclusive interval, capture or worker started")?;
                return Ok(Admission::Wait);
            }
            return Ok(Admission::Open);
        }
        let snapshot = shared
            .collection
            .as_ref()
            .map(|l| l.drain_snapshot())
            .transpose()?;
        if snapshot.as_ref().is_some_and(|s| s.faulted) {
            workspace.block_graph_before_claim(
                &job,
                "Collection lane requires settlement or recovery; graph did not capture or launch",
            )?;
            return Ok(Admission::Wait);
        }
        if let Some(collection) = &shared.collection {
            if workspace
                .collection_queue_sequence(collection.protocol)?
                .is_some_and(|n| n < sequence)
            {
                return Ok(if matches!(lane, Lane::Processing) {
                    Admission::Wait
                } else {
                    Admission::Open
                });
            }
        }
        activity.interval = Interval::Draining(Box::new(Drain {
            job,
            frozen_processing: activity.tokens.keys().cloned().collect(),
            collection: snapshot.and_then(|s| s.ticket),
        }));
        shared.graph_wake.notify_all();
    }
    match &activity.interval {
        Interval::Open => Ok(Admission::Open),
        Interval::Owned(_) => Ok(Admission::Wait),
        Interval::Draining(drain) => {
            crate::require(
                activity
                    .tokens
                    .keys()
                    .all(|key| drain.frozen_processing.contains(key)),
                "Automatic processing admission crossed graph drain",
            )?;
            let snapshot = shared
                .collection
                .as_ref()
                .map(|l| l.drain_snapshot())
                .transpose()?;
            if snapshot.as_ref().is_some_and(|s| s.faulted) {
                workspace.block_graph_before_claim(
                    &drain.job,
                    "Collection lane failed to drain; recovery required before graph retry",
                )?;
                activity.interval = Interval::Open;
                return Ok(Admission::Wait);
            }
            if let Some(snapshot) = &snapshot {
                if let Some(ticket) = &snapshot.ticket {
                    crate::require(
                        drain
                            .collection
                            .as_ref()
                            .is_some_and(|frozen| same_collection(frozen, ticket)),
                        "Collection admission crossed graph drain",
                    )?;
                }
                if matches!(lane, Lane::Collection) {
                    return Ok(if snapshot.admitted && snapshot.ticket.is_some() {
                        Admission::Open
                    } else {
                        Admission::Wait
                    });
                }
            }
            if activity.tokens.is_empty() && snapshot.as_ref().is_none_or(|s| s.idle) {
                Ok(Admission::Graph(Box::new(drain.job.clone())))
            } else {
                Ok(Admission::Wait)
            }
        }
    }
}
fn same_collection(
    a: &crate::collection_jobs::CollectionTicket,
    b: &crate::collection_jobs::CollectionTicket,
) -> bool {
    a.job_id == b.job_id && a.generation == b.generation && a.lease == b.lease
}

pub(super) struct Pending {
    ticket: JobTicket,
    attempt: GraphAttempt,
    token: CancellationToken,
    ownership_lifetime: String,
    // Derived from this workspace while admission and canonical ownership are held.
    // Neither queued JSON nor the executor chooses the assignment parent.
    scratch: PathBuf,
    result: Option<Result<ProcessingOutput>>,
}
/// Caller holds registry before atomic claim, so no committed claim can escape unregistered.
pub(super) fn claim(
    shared: &Shared,
    workspace: &mut Workspace,
    activity: &mut ProcessingActivity,
    job: &ProcessingJob,
) -> Result<Option<Pending>> {
    crate::require(
        matches!(&activity.interval, Interval::Draining(d) if d.job.id==job.id)
            && activity.tokens.is_empty()
            && shared.graph_executor.available(),
        "Exclusive graph claim not reserved",
    )?;
    if shared.stopping.load(Ordering::Acquire) || !shared.ownership.held() {
        return Ok(None);
    }
    let (ticket, attempt) = match workspace.claim_graph_exclusive(job) {
        Ok(value) => value,
        Err(error @ Error::Database(_)) => return Err(error),
        Err(_) => {
            // Atomic capture failure left no Running claim. Storage failure here keeps drain ownership.
            workspace.block_graph_before_claim(
                job,
                "Graph input could not be captured; no process started",
            )?;
            activity.interval = Interval::Open;
            return Ok(None);
        }
    };
    let token = CancellationToken::default();
    if shared.stopping.load(Ordering::Acquire) {
        token.cancel();
    }
    activity.tokens.insert(ticket.job_id.clone(), token.clone());
    activity.interval = Interval::Owned(Owned {
        status: GraphStatus {
            job_id: ticket.job_id.clone(),
            attempt: ticket.attempt,
            lease: Some(ticket.lease.clone()),
            request_sha256: Some(crate::store::hash(attempt.worker_input())),
            phase: GraphPhase::Running,
            retries: 0,
            can_retry: false,
        },
        retry_requested: false,
        cancel_settlement_used: false,
    });
    Ok(Some(Pending {
        ticket,
        attempt,
        token,
        ownership_lifetime: shared.ownership.lifetime().into(),
        scratch: workspace.processing_scratch(),
        result: None,
    }))
}
fn completion(
    result: &Option<Result<ProcessingOutput>>,
    stopped: bool,
) -> Result<&ProcessingOutput> {
    match result.as_ref().expect("executor completed") {
        Err(Error::TerminationUnverified(_)) => Err(Error::TerminationUnverified(
            "Graph worker exit unverified".into(),
        )),
        Err(Error::Cleanup(_)) => Err(Error::Cleanup("Graph worker scratch cleanup failed".into())),
        _ if stopped => Err(Error::Interrupted(
            "Graph worker stopped after cancellation".into(),
        )),
        Ok(value) => Ok(value),
        Err(Error::Interrupted(_)) => Err(Error::Interrupted(
            "Graph worker stopped before completion".into(),
        )),
        Err(Error::Blocked(_)) => Err(Error::Blocked("Graph runtime unavailable".into())),
        Err(Error::QuotaExhausted(_)) => Err(Error::QuotaExhausted("Graph resource limit".into())),
        Err(Error::InvalidWorkerResult(_)) => Err(Error::InvalidWorkerResult(
            "Graph worker result invalid".into(),
        )),
        Err(_) => Err(Error::Validation("Graph worker failed".into())),
    }
}
pub(super) fn execute_and_publish(shared: &Shared, mut pending: Pending) {
    let result = if pending.token.is_cancelled() {
        Err(Error::Interrupted("Graph cancelled before launch".into()))
    } else {
        std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
            shared.graph_executor.execute(
                &pending.scratch,
                pending.attempt.worker_input(),
                &pending.token,
            )
        }))
        .unwrap_or_else(|_| {
            Err(Error::TerminationUnverified(
                "Graph adapter panicked".into(),
            ))
        })
    }
    .and_then(|bytes| {
        if bytes.len() > 128 * 1024 {
            Err(Error::InvalidWorkerResult(
                "Graph result exceeds bound".into(),
            ))
        } else {
            Ok(ProcessingOutput::Graph(bytes))
        }
    });
    let unknown = matches!(result, Err(Error::TerminationUnverified(_)));
    pending.result = Some(result);
    if unknown {
        quarantine_execution(shared);
    }
    let mut final_shutdown_attempt = false;
    loop {
        let finished = {
            let mut workspace = shared
                .workspace
                .lock()
                .unwrap_or_else(std::sync::PoisonError::into_inner);
            let stopping = shared.stopping.load(Ordering::Acquire);
            let cancelled = pending.token.is_cancelled();
            {
                let mut activity = shared
                    .active
                    .lock()
                    .unwrap_or_else(std::sync::PoisonError::into_inner);
                #[cfg(test)]
                {
                    activity.publication_attempts += 1;
                }
                if let Interval::Owned(owned) = &mut activity.interval {
                    owned.status.phase = GraphPhase::Publishing;
                    owned.status.can_retry = false;
                    owned.cancel_settlement_used |= cancelled;
                }
            }
            // If stop is already observed, this attempt itself is the single final settlement.
            if stopping {
                final_shutdown_attempt = true;
            }
            let result = completion(&pending.result, cancelled || stopping);
            // The captured authority cannot move to a new coordinator ownership lifetime.
            if shared.ownership.lifetime() != pending.ownership_lifetime
                || !shared.ownership.publication_held()
            {
                Err(Error::Blocked("Graph publication ownership differs".into()))
            } else {
                workspace.finish_graph_processing_job(&pending.ticket, &mut pending.attempt, result)
            }
        };
        let mut activity = shared
            .active
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        if finished.is_ok() {
            if unknown {
                if let Interval::Owned(owned) = &mut activity.interval {
                    owned.status.phase = GraphPhase::RecoveryRequired;
                }
            } else {
                activity.tokens.remove(&pending.ticket.job_id);
                activity.interval = Interval::Open;
            }
            shared.wake.notify_all();
            shared.graph_wake.notify_all();
            return;
        }
        if let Interval::Owned(owned) = &mut activity.interval {
            owned.status.phase = GraphPhase::PublicationPending;
            owned.status.can_retry =
                owned.status.retries < 3 && !shared.stopping.load(Ordering::Acquire);
        } else {
            shared.ownership.quarantine();
            activity.retained = Some(Box::new(pending));
            return;
        }
        shared.graph_wake.notify_all();
        loop {
            let Interval::Owned(owned) = &mut activity.interval else {
                unreachable!("owned interval remains reserved")
            };
            if shared.stopping.load(Ordering::Acquire) {
                if !final_shutdown_attempt {
                    final_shutdown_attempt = true;
                    break;
                }
                owned.status.phase = if unknown {
                    GraphPhase::RecoveryRequired
                } else {
                    GraphPhase::UnpublishedKnownStopped
                };
                owned.status.can_retry = false;
                shared.ownership.quarantine();
                activity.retained = Some(Box::new(pending));
                return;
            }
            if pending.token.is_cancelled() && !owned.cancel_settlement_used {
                owned.cancel_settlement_used = true;
                break;
            }
            if owned.retry_requested {
                owned.retry_requested = false;
                break;
            }
            activity = shared
                .graph_wake
                .wait(activity)
                .unwrap_or_else(std::sync::PoisonError::into_inner);
        }
        drop(activity);
    }
}
impl JobCoordinator {
    pub(crate) fn graph_status(&self) -> Result<Option<GraphStatus>> {
        Ok(self
            .shared
            .active
            .lock()
            .map_err(|_| Error::Blocked("Processing activity unavailable".into()))?
            .status())
    }
    pub(crate) fn retry_graph_publication(
        &self,
        job_id: &str,
        attempt: u32,
        lease: &str,
        request_sha256: &str,
    ) -> Result<GraphStatus> {
        crate::require(
            !self.shared.stopping.load(Ordering::Acquire)
                && self.shared.ownership.publication_held(),
            "Graph publication ownership unavailable",
        )?;
        let mut activity = self
            .shared
            .active
            .lock()
            .map_err(|_| Error::Blocked("Processing activity unavailable".into()))?;
        let Interval::Owned(owned) = &mut activity.interval else {
            return Err(Error::Conflict("No owned graph publication".into()));
        };
        crate::require(
            owned.status.phase == GraphPhase::PublicationPending
                && owned.status.job_id == job_id
                && owned.status.attempt == attempt
                && owned.status.lease.as_deref() == Some(lease)
                && owned.status.request_sha256.as_deref() == Some(request_sha256),
            "Pending graph publication changed",
        )?;
        if !owned.retry_requested {
            crate::require(
                owned.status.retries < 3,
                "Graph publication retry limit reached",
            )?;
            owned.status.retries += 1;
            owned.retry_requested = true;
        }
        owned.status.can_retry = false;
        let status = owned.status.clone();
        drop(activity);
        self.shared.graph_wake.notify_all();
        Ok(status)
    }
}

#[cfg(test)]
#[path = "coordinator_graph_tests.rs"]
mod tests;

/// Joined teardown can release an unclaimed drain without claiming any worker stopped.
pub(super) fn finish_shutdown(activity: &mut ProcessingActivity) {
    if matches!(activity.interval, Interval::Draining(_)) {
        activity.interval = Interval::Open;
    }
}
