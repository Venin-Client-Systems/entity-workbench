//! Public control routing under workspace -> lane locks. No native executor is enabled.
use super::*;
use crate::collection_api::*;

pub(in crate::coordinator) fn is_public_command(command: &Command) -> bool {
    matches!(
        command,
        Command::PreviewCollection { .. }
            | Command::QueueCollection { .. }
            | Command::PageCollectionRuns { .. }
            | Command::InspectCollectionRun { .. }
            | Command::CancelCollection { .. }
            | Command::ResumeCollection { .. }
            | Command::RetryCollectionSettlement { .. }
    )
}
fn availability(
    shared: &Shared,
    workspace: &Workspace,
    lane: Option<&CollectionLane>,
    state: Option<&LaneState>,
) -> Result<CollectionAvailability> {
    if shared.stopping.load(Ordering::Acquire) {
        return Ok(CollectionAvailability::Stopping);
    }
    if !shared.ownership.held()
        || state.is_some_and(|s| s.requires_recovery)
        || workspace.processing_execution_suspended()?
        || workspace.collection_transport_available().is_err()
    {
        return Ok(CollectionAvailability::RecoveryRequired);
    }
    if state.is_some_and(|s| {
        matches!(
            s.status.phase,
            LanePhase::Faulted | LanePhase::Unpublished | LanePhase::Stopped
        )
    }) {
        return Ok(CollectionAvailability::ExecutionUnavailable);
    }
    Ok(match lane {
        Some(lane) if lane.protocol == CollectionProtocol::SyntheticV4 => {
            CollectionAvailability::SyntheticFixture
        }
        Some(lane)
            if lane.protocol == CollectionProtocol::NativeV4 && NATIVE_COLLECTION_ENABLED =>
        {
            CollectionAvailability::Ready
        }
        _ => CollectionAvailability::NativeDisabled,
    })
}
fn executable(value: CollectionAvailability) -> bool {
    matches!(
        value,
        CollectionAvailability::SyntheticFixture | CollectionAvailability::Ready
    )
}
fn phase(value: LanePhase) -> CollectionExecutionPhase {
    match value {
        LanePhase::Idle => CollectionExecutionPhase::Idle,
        LanePhase::Running => CollectionExecutionPhase::Running,
        LanePhase::Settling => CollectionExecutionPhase::Settling,
        LanePhase::SettlementPending => CollectionExecutionPhase::SettlementPending,
        LanePhase::Faulted => CollectionExecutionPhase::Faulted,
        LanePhase::RecoveryRequired => CollectionExecutionPhase::RecoveryRequired,
        LanePhase::Stopped => CollectionExecutionPhase::Stopped,
        LanePhase::Unpublished => CollectionExecutionPhase::Unpublished,
    }
}
fn matching_run(run: &CollectionRunSummary, lane: Option<&CollectionLane>) -> bool {
    run.record_version == 4
        && run.collector_policy == COLLECTOR_POLICY
        && lane.is_some_and(|l| {
            l.protocol.version() == 4
                && l.protocol.synthetic()
                    == (run.mode == crate::collection_receipt::AcquisitionMode::Synthetic)
        })
}
fn inspection(
    shared: &Shared,
    workspace: &Workspace,
    lane: Option<&CollectionLane>,
    state: Option<&LaneState>,
    id: &str,
) -> Result<CollectionRunInspection> {
    let mut result = workspace.inspect_collection_run(id)?;
    let available = availability(shared, workspace, lane, state)?;
    result.availability = available;
    let matching = matching_run(&result.run, lane);
    let current = state.filter(|s| {
        s.status.job_id.as_deref() == Some(id) && s.status.generation == Some(result.run.generation)
    });
    if matching {
        if let Some(state) = current {
            result.execution = CollectionExecutionStatus {
                phase: phase(state.status.phase),
                request_sequence: state.status.sequence,
                publication_retries: state.status.publication_retries,
                publication_retry_limit: MAX_PUBLICATION_RETRIES,
            };
        } else if executable(available) {
            result.execution.phase = CollectionExecutionPhase::Idle;
        }
    }
    result.controls.can_cancel = matching
        && executable(available)
        && !result.run.cancellation_requested
        && matches!(
            result.run.state,
            CollectionState::Queued | CollectionState::Running | CollectionState::Interrupted
        );
    result.controls.can_resume = matching
        && executable(available)
        && !result.run.cancellation_requested
        && result.run.state == CollectionState::Interrupted
        && state.is_some_and(|s| s.status.phase == LanePhase::Idle && s.admitted.is_none());
    // Publication-only recovery remains safe with held quarantined ownership.
    result.controls.can_retry_settlement = matching
        && !shared.stopping.load(Ordering::Acquire)
        && shared.ownership.publication_held()
        && current.is_some_and(|s| {
            s.status.phase == LanePhase::SettlementPending
                && !s.retry_requested
                && s.status.publication_retries < MAX_PUBLICATION_RETRIES
                && s.status.sequence.is_some_and(|n| {
                    result.requests.get(n as usize).is_some_and(|r| {
                        r.generation == result.run.generation
                            && matches!(
                                r.progress,
                                crate::collection_jobs::RequestProgress::Reserved
                            )
                    })
                })
        });
    crate::require(
        serde_json::to_vec(&result)?.len() <= RESPONSE_BYTES,
        "Collection inspection exceeds response bound",
    )?;
    Ok(result)
}
impl JobCoordinator {
    pub(in crate::coordinator) fn dispatch_collection_command(
        &self,
        command: Command,
    ) -> Result<Value> {
        if let Command::PreviewCollection { input } = command {
            return Ok(serde_json::to_value(preview(input)?)?);
        }
        let mut workspace = self
            .shared
            .workspace
            .lock()
            .map_err(|_| Error::Blocked("Workspace coordinator is unavailable".into()))?;
        let lane = self.shared.collection.as_deref();
        let mut state = lane
            .map(|l| {
                l.state
                    .lock()
                    .map_err(|_| Error::Blocked("Collection lane is unavailable".into()))
            })
            .transpose()?;
        let available = availability(&self.shared, &workspace, lane, state.as_deref())?;
        match command {
            Command::PageCollectionRuns {
                request,
                expected_revision,
            } => {
                let mut page = workspace.page_collection_runs(&request, expected_revision)?;
                page.availability = available;
                Ok(serde_json::to_value(page)?)
            }
            Command::InspectCollectionRun { job_id } => Ok(serde_json::to_value(inspection(
                &self.shared,
                &workspace,
                lane,
                state.as_deref(),
                &job_id,
            )?)?),
            command => {
                let id = match command {
                    Command::QueueCollection {
                        input,
                        preview_sha256,
                        request_key,
                    } => {
                        let input = confirmed(input, &preview_sha256)?;
                        let lane = lane.filter(|l| l.protocol.version() == 4).ok_or_else(|| {
                            Error::Blocked("Native collection execution is disabled".into())
                        })?;
                        if self.shared.stopping.load(Ordering::Acquire)
                            || !self.shared.ownership.publication_held()
                        {
                            return Err(Error::Blocked(
                                "Collection publication ownership is unavailable".into(),
                            ));
                        }
                        if let Some(existing) = workspace.existing_collection_request(
                            &input,
                            &request_key,
                            lane.protocol,
                        )? {
                            existing.id
                        } else {
                            if !executable(available) {
                                return Err(Error::Blocked(
                                    "Native collection execution is disabled or requires recovery"
                                        .into(),
                                ));
                            }
                            workspace
                                .queue_collection_protocol(
                                    input,
                                    &request_key,
                                    now(),
                                    lane.protocol,
                                )?
                                .id
                        }
                    }
                    Command::CancelCollection {
                        job_id,
                        expected_generation,
                    } => {
                        let current =
                            inspection(&self.shared, &workspace, lane, state.as_deref(), &job_id)?;
                        crate::require(
                            current.run.generation == expected_generation,
                            "Collection generation changed",
                        )?;
                        // Repeated acknowledgement is safe only on this enabled v4 lane.
                        crate::require(
                            matching_run(&current.run, lane)
                                && executable(available)
                                && (current.controls.can_cancel
                                    || current.run.cancellation_requested),
                            "Collection cancellation is unavailable",
                        )?;
                        workspace.cancel_durable_collection(&job_id, expected_generation, now())?;
                        if let Some(state) = state.as_deref() {
                            if state.status.job_id.as_deref() == Some(&job_id)
                                && state.status.generation == Some(expected_generation)
                            {
                                if let Some(token) = &state.active {
                                    token.cancel();
                                }
                            }
                        }
                        job_id
                    }
                    Command::ResumeCollection {
                        job_id,
                        expected_generation,
                    } => {
                        let current =
                            inspection(&self.shared, &workspace, lane, state.as_deref(), &job_id)?;
                        crate::require(
                            current.controls.can_resume
                                && current.run.generation == expected_generation,
                            "Collection resume is unavailable or generation changed",
                        )?;
                        state.as_deref_mut().expect("resumable lane").admitted = workspace
                            .start_durable_collection(
                                &job_id,
                                expected_generation,
                                &self.shared.ownership,
                                now(),
                            )?;
                        job_id
                    }
                    Command::RetryCollectionSettlement {
                        job_id,
                        expected_generation,
                        request_sequence,
                    } => {
                        let current =
                            inspection(&self.shared, &workspace, lane, state.as_deref(), &job_id)?;
                        crate::require(
                            current.controls.can_retry_settlement
                                && current.run.generation == expected_generation
                                && current.execution.request_sequence == Some(request_sequence),
                            "Exact pending settlement is unavailable or changed",
                        )?;
                        let state = state.as_deref_mut().expect("pending publication lane");
                        state.retry_requested = true;
                        state.status.publication_retries += 1;
                        job_id
                    }
                    _ => return Err(Error::Validation("Not a collection control".into())),
                };
                let response = inspection(&self.shared, &workspace, lane, state.as_deref(), &id)?;
                drop(state);
                drop(workspace);
                if let Some(lane) = lane {
                    lane.wake.notify_all();
                }
                self.shared.wake.notify_all();
                Ok(serde_json::to_value(response)?)
            }
        }
    }
    #[cfg(test)]
    fn with_public_collection_executor(
        workspace: Workspace,
        processing: Arc<Executor>,
        collection: Arc<CollectionExecutor>,
    ) -> Result<Self> {
        Self::with_execution(
            workspace,
            1,
            processing,
            Some((collection, CollectionProtocol::SyntheticV4)),
        )
    }
}
#[cfg(test)]
#[path = "coordinator_collection_api_tests.rs"]
mod tests;
