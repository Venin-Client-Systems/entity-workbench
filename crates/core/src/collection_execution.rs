//! One committed request at a time. Private and synthetic-only; no application caller.
#![allow(dead_code)]
use crate::{
    collection_jobs::{CollectionInput, CollectionTicket, DurableCollectionJob, RequestTicket},
    collection_transport::{self, ExecutionWindow, Observation},
    engines::CancellationToken,
    store::{CollectionOwnership, Workspace},
    Error, Result,
};
use std::{
    sync::Mutex,
    time::{Duration, Instant},
};

/// A process segment owns one monotonic deadline. Restart creates a new segment
/// bounded by the unchanged canonical wall deadline, never historical elapsed.
pub(crate) struct CollectionDriver {
    execution: CollectionTicket,
    ownership_lifetime: String,
    input: CollectionInput,
    wall_deadline_ms: i64,
    monotonic_deadline: Instant,
    not_before: Instant,
}
/// The one response stays owned across publication failures. Retrying settlement
/// cannot call the network. The canonical ticket and exact receipt are checked on every retry.
pub(crate) struct PendingSettlement {
    request: RequestTicket,
    observation: Observation,
    ownership_lifetime: String,
}
impl PendingSettlement {
    pub(crate) fn settle(
        &self,
        workspace: &mut Workspace,
        owner: &CollectionOwnership,
    ) -> Result<DurableCollectionJob> {
        crate::require(
            owner.lifetime() == self.ownership_lifetime,
            "Collection ownership changed before settlement",
        )?;
        workspace.settle_collection_transport(&self.request, &self.observation, owner)
    }
}
impl CollectionDriver {
    pub(crate) fn attach(
        workspace: &Workspace,
        owner: &CollectionOwnership,
        execution: CollectionTicket,
    ) -> Result<Self> {
        let job = workspace.collection_execution_snapshot(&execution, owner)?;
        let deadline = job
            .checkpoint
            .deadline_at_ms
            .ok_or_else(|| Error::Validation("Collection deadline is missing".into()))?;
        let remaining = deadline
            .saturating_sub(chrono::Utc::now().timestamp_millis())
            .max(0) as u64;
        let remaining =
            Duration::from_millis(remaining).min(Duration::from_secs(job.input.max_seconds));
        Ok(Self {
            execution,
            ownership_lifetime: owner.lifetime().into(),
            input: job.input,
            wall_deadline_ms: deadline,
            monotonic_deadline: Instant::now() + remaining,
            not_before: Instant::now()
                + if job.checkpoint.requests.is_empty() {
                    Duration::ZERO
                } else {
                    Duration::from_secs(1)
                },
        })
    }
    pub(crate) fn next(
        &mut self,
        workspace: &Mutex<Workspace>,
        owner: &CollectionOwnership,
        cancellation: &CancellationToken,
    ) -> Result<Option<PendingSettlement>> {
        self.next_with(workspace, owner, cancellation, collection_transport::fetch)
    }
    fn next_with(
        &mut self,
        workspace: &Mutex<Workspace>,
        owner: &CollectionOwnership,
        cancellation: &CancellationToken,
        fetch: impl FnOnce(
            &RequestTicket,
            &CollectionInput,
            ExecutionWindow,
            Instant,
            &CancellationToken,
        ) -> Observation,
    ) -> Result<Option<PendingSettlement>> {
        crate::require(
            owner.lifetime() == self.ownership_lifetime,
            "Collection execution ownership changed",
        )?;
        let (request, previous_wall) = {
            let mut workspace = workspace
                .lock()
                .map_err(|_| Error::Blocked("Collection workspace lock failed".into()))?;
            workspace.collection_execution_snapshot(&self.execution, owner)?;
            let now = chrono::Utc::now().timestamp_millis();
            if cancellation.is_cancelled() {
                workspace.cancel_durable_collection(
                    &self.execution.job_id,
                    self.execution.generation,
                    now,
                )?;
            }
            let Some(request) =
                workspace.advance_durable_collection(&self.execution, owner, now)?
            else {
                return Ok(None);
            };
            // The reservation transaction has committed before this scope releases
            // the workspace lock and before a resolver/socket can run.
            (request, now)
        };
        let window = ExecutionWindow::new(
            self.wall_deadline_ms,
            previous_wall,
            self.monotonic_deadline
                .saturating_duration_since(Instant::now()),
        )
        .map_err(|_| Error::Validation("Invalid collection execution window".into()))?;
        let observation = fetch(&request, &self.input, window, self.not_before, cancellation);
        self.not_before = Instant::now() + Duration::from_secs(1);
        Ok(Some(PendingSettlement {
            request,
            observation,
            ownership_lifetime: self.ownership_lifetime.clone(),
        }))
    }
}

#[cfg(test)]
#[path = "collection_execution_tests.rs"]
mod tests;
