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
    scheduling_open: bool,
) -> Result<CollectionRunInspection> {
    let result = workspace.inspect_collection_run(id)?;
    let available = availability(shared, workspace, lane, state)?;
    decorate_inspection(shared, lane, state, result, available, scheduling_open)
}
fn decorate_inspection(
    shared: &Shared,
    lane: Option<&CollectionLane>,
    state: Option<&LaneState>,
    mut result: CollectionRunInspection,
    available: CollectionAvailability,
    scheduling_open: bool,
) -> Result<CollectionRunInspection> {
    let id = result.run.id.as_str();
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
    result.controls.can_resume = scheduling_open
        && matching
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
        if let Command::CancelCollection {
            job_id,
            expected_generation,
        } = command
        {
            return self.dispatch_prepared_collection_cancel(&job_id, expected_generation);
        }
        let mut workspace = self
            .shared
            .workspace
            .lock()
            .map_err(|_| Error::Blocked("Workspace coordinator is unavailable".into()))?;
        let activity = self
            .shared
            .active
            .lock()
            .map_err(|_| Error::Blocked("Processing activity unavailable".into()))?;
        let scheduling_open = activity.allows_resume();
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
                scheduling_open,
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
                    Command::ResumeCollection {
                        job_id,
                        expected_generation,
                    } => {
                        let current = inspection(
                            &self.shared,
                            &workspace,
                            lane,
                            state.as_deref(),
                            &job_id,
                            scheduling_open,
                        )?;
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
                        let current = inspection(
                            &self.shared,
                            &workspace,
                            lane,
                            state.as_deref(),
                            &job_id,
                            scheduling_open,
                        )?;
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
                let response = inspection(
                    &self.shared,
                    &workspace,
                    lane,
                    state.as_deref(),
                    &id,
                    scheduling_open,
                )?;
                drop(state);
                drop(activity);
                drop(workspace);
                if let Some(lane) = lane {
                    lane.wake.notify_all();
                }
                self.shared.wake.notify_all();
                Ok(serde_json::to_value(response)?)
            }
        }
    }
    fn dispatch_prepared_collection_cancel(&self, job_id: &str, generation: u32) -> Result<Value> {
        let lane = self
            .shared
            .collection
            .as_deref()
            .ok_or_else(|| Error::Blocked("Collection cancellation is unavailable".into()))?;
        let capture = {
            let workspace = self
                .shared
                .workspace
                .lock()
                .map_err(|_| Error::Blocked("Workspace coordinator is unavailable".into()))?;
            let _activity = self
                .shared
                .active
                .lock()
                .map_err(|_| Error::Blocked("Processing activity unavailable".into()))?;
            let state = lane
                .state
                .lock()
                .map_err(|_| Error::Blocked("Collection lane is unavailable".into()))?;
            crate::require(
                executable(availability(
                    &self.shared,
                    &workspace,
                    Some(lane),
                    Some(&state),
                )?),
                "Collection cancellation is unavailable",
            )?;
            workspace.capture_collection_cancellation(
                job_id,
                generation,
                &self.shared.ownership,
                lane.protocol,
            )?
        };
        #[cfg(test)]
        cancel_hooks::before_prepare();
        // No workspace, lane lock or SQLite transaction survives into this replay.
        let prepared = capture.prepare()?;
        let mut workspace = self
            .shared
            .workspace
            .lock()
            .map_err(|_| Error::Blocked("Workspace coordinator is unavailable".into()))?;
        let activity = self
            .shared
            .active
            .lock()
            .map_err(|_| Error::Blocked("Processing activity unavailable".into()))?;
        let state = lane
            .state
            .lock()
            .map_err(|_| Error::Blocked("Collection lane is unavailable".into()))?;
        crate::require(
            executable(availability(
                &self.shared,
                &workspace,
                Some(lane),
                Some(&state),
            )?),
            "Collection cancellation is unavailable",
        )?;
        let ready =
            workspace.ready_collection_cancellation(prepared, &self.shared.ownership, now())?;
        let available = availability(&self.shared, &workspace, Some(lane), Some(&state))?;
        crate::require(
            executable(available),
            "Collection cancellation is unavailable",
        )?;
        let response = ready.inspection()?;
        #[cfg(test)]
        let response = cancel_hooks::acknowledgement(response);
        let response = decorate_inspection(
            &self.shared,
            Some(lane),
            Some(&state),
            response,
            available,
            activity.allows_resume(),
        )?;
        // All fallible response work precedes the canonical write. A successful
        // write is followed only by current-token signalling, wakes and return.
        let value = serde_json::to_value(response)?;
        crate::require(
            !self.shared.stopping.load(Ordering::Acquire),
            "Collection coordinator is stopping",
        )?;
        workspace.commit_collection_cancellation(ready, &self.shared.ownership)?;
        if state.status.job_id.as_deref() == Some(job_id)
            && state.status.generation == Some(generation)
        {
            if let Some(token) = &state.active {
                token.cancel();
            }
        }
        drop(state);
        drop(activity);
        drop(workspace);
        lane.wake.notify_all();
        self.shared.wake.notify_all();
        Ok(value)
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

#[cfg(test)]
mod cancel_hooks {
    use super::*;
    use std::cell::RefCell;
    type AckHook = dyn Fn(&mut CollectionRunInspection);
    thread_local! {
        pub(super) static PREPARE: RefCell<Option<Box<dyn Fn()>>> = RefCell::new(None);
        pub(super) static ACK: RefCell<Option<Box<AckHook>>> = RefCell::new(None);
    }
    pub(super) fn before_prepare() {
        PREPARE.with(|hook| {
            if let Some(hook) = hook.borrow_mut().take() {
                hook();
            }
        });
    }
    pub(super) fn acknowledgement(mut result: CollectionRunInspection) -> CollectionRunInspection {
        ACK.with(|hook| {
            if let Some(hook) = hook.borrow_mut().take() {
                hook(&mut result);
            }
        });
        result
    }
}
