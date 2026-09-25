//! Public graph routing. Lock order remains workspace -> activity; no runtime attachment.
use super::*;
use crate::{graph_api::*, processing::ProcessingState};

pub(super) fn is_public_command(command: &Command) -> bool {
    matches!(
        command,
        Command::QueueGraphPath { .. }
            | Command::PageGraphJobs { .. }
            | Command::InspectGraphJob { .. }
            | Command::CancelGraphJob { .. }
            | Command::RetryGraphPublication { .. }
            | Command::InspectGraphAnalysis { .. }
    )
}
fn availability(shared: &Shared, workspace: &Workspace) -> Result<GraphAvailability> {
    if !shared.ownership.held() || workspace.processing_execution_suspended()? {
        return Ok(GraphAvailability::RecoveryRequired);
    }
    Ok(match shared.graph_executor {
        graph::GraphExecution::Unavailable => GraphAvailability::RuntimeUnavailable,
        graph::GraphExecution::Configured(_) => GraphAvailability::Ready,
        #[cfg(test)]
        graph::GraphExecution::Synthetic(_) => GraphAvailability::SyntheticFixture,
    })
}
fn inspection(shared: &Shared, workspace: &Workspace, id: &str) -> Result<GraphJobInspection> {
    let mut response = workspace.inspect_graph_job(id)?;
    response.availability = availability(shared, workspace)?;
    response.controls.can_cancel &= shared.ownership.held();
    let activity = shared
        .active
        .lock()
        .map_err(|_| Error::Blocked("Graph activity unavailable".into()))?;
    if let Some(status) = activity.status().filter(|s| {
        s.job_id == id && s.attempt == response.job.attempt && s.lease == response.job.lease
    }) {
        let phase = match status.phase {
            graph::GraphPhase::Draining => GraphExecutionPhase::Draining,
            graph::GraphPhase::Running => GraphExecutionPhase::Running,
            graph::GraphPhase::Publishing => GraphExecutionPhase::Publishing,
            graph::GraphPhase::PublicationPending => GraphExecutionPhase::PublicationPending,
            graph::GraphPhase::RecoveryRequired => GraphExecutionPhase::RecoveryRequired,
            graph::GraphPhase::UnpublishedKnownStopped => {
                GraphExecutionPhase::UnpublishedKnownStopped
            }
        };
        response.controls.can_retry_publication = status.can_retry
            && shared.ownership.publication_held()
            && response.job.state == ProcessingState::Running;
        response.execution = Some(GraphExecutionStatus {
            phase,
            attempt: status.attempt,
            host_attempt_lease: status.lease,
            request_sha256: status.request_sha256,
            publication_retries: status.retries,
            publication_retry_limit: 3,
        });
    }
    Ok(response)
}
impl JobCoordinator {
    pub(super) fn dispatch_graph_command(&self, command: Command) -> Result<Value> {
        let mut workspace = self
            .shared
            .workspace
            .lock()
            .map_err(|_| Error::Blocked("Workspace coordinator unavailable".into()))?;
        crate::require(
            !self.shared.stopping.load(Ordering::Acquire),
            "Workspace coordinator is stopping",
        )?;
        let result = match command {
            Command::PageGraphJobs {
                request,
                expected_revision,
            } => {
                let mut page = workspace.page_graph_jobs(&request, expected_revision)?;
                page.availability = availability(&self.shared, &workspace)?;
                serde_json::to_value(page)?
            }
            Command::InspectGraphAnalysis {
                id,
                expected_request_sha256,
                expected_result_sha256,
            } => serde_json::to_value(workspace.inspect_graph_result(
                &id,
                &expected_request_sha256,
                &expected_result_sha256,
            )?)?,
            Command::InspectGraphJob { job_id } => {
                serde_json::to_value(inspection(&self.shared, &workspace, &job_id)?)?
            }
            Command::QueueGraphPath {
                expected_revision,
                source_id,
                target_id,
                request_key,
            } => {
                // Exact known-key acknowledgement recovery is a read, even under quarantine.
                if workspace.graph_request_job(&request_key)?.is_none() {
                    crate::require(
                        self.shared.ownership.held(),
                        "Graph execution ownership unavailable",
                    )?;
                }
                let response = workspace.queue_graph_job(
                    expected_revision,
                    &source_id,
                    &target_id,
                    &request_key,
                )?;
                serde_json::to_value(inspection(&self.shared, &workspace, &response.job.id)?)?
            }
            Command::CancelGraphJob {
                job_id,
                expected_attempt,
            } => {
                crate::require(
                    self.shared.ownership.held(),
                    "Graph cancellation ownership unavailable",
                )?;
                workspace.cancel_graph_processing_job(&job_id, expected_attempt)?;
                // Canonical cancellation and signaling share the claim lock order.
                if let Some(token) = self
                    .shared
                    .active
                    .lock()
                    .map_err(|_| Error::Blocked("Graph cancellation unavailable".into()))?
                    .tokens
                    .get(&job_id)
                {
                    token.cancel();
                }
                // A corrupt referenced result may make the response unavailable after commit.
                // Signal first and wake retained publication even when projection returns Err.
                self.shared.graph_wake.notify_all();
                self.shared.wake.notify_all();
                serde_json::to_value(inspection(&self.shared, &workspace, &job_id)?)?
            }
            Command::RetryGraphPublication {
                job_id,
                expected_attempt,
                host_attempt_lease,
                request_sha256,
            } => serde_json::to_value(retry(
                self,
                &workspace,
                &job_id,
                expected_attempt,
                &host_attempt_lease,
                &request_sha256,
            )?)?,
            _ => return Err(Error::Validation("Not a graph command".into())),
        };
        drop(workspace);
        self.shared.wake.notify_all();
        self.shared.graph_wake.notify_all();
        Ok(result)
    }
}

// Caller holds workspace: shared by dispatch and the deterministic transition regression.
pub(super) fn retry(
    c: &JobCoordinator,
    workspace: &Workspace,
    job_id: &str,
    expected_attempt: u32,
    host_attempt_lease: &str,
    request_sha256: &str,
) -> Result<GraphJobInspection> {
    crate::require(
        uuid(job_id) && uuid(host_attempt_lease) && digest(request_sha256) && expected_attempt > 0,
        "Invalid graph publication identity",
    )?;
    let current = inspection(&c.shared, workspace, job_id)?;
    // An exact duplicate is harmless while one retry remains requested, before consumption.
    crate::require(
        current.job.state == ProcessingState::Running
            && current.job.attempt == expected_attempt
            && current.job.lease.as_deref() == Some(host_attempt_lease)
            && current.execution.as_ref().is_some_and(|s| {
                s.phase == GraphExecutionPhase::PublicationPending
                    && s.request_sha256.as_deref() == Some(request_sha256)
            }),
        "Pending graph publication changed",
    )?;
    c.retry_graph_publication(job_id, expected_attempt, host_attempt_lease, request_sha256)?;
    inspection(&c.shared, workspace, job_id)
}
