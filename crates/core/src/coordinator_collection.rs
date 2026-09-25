//! Collection lane owned by JobCoordinator. Native production transport remains disabled.
#![allow(dead_code)]
use super::*;
use crate::{
    collection_execution::{CollectionDriver, PendingSettlement},
    collection_jobs::{
        CollectionInput, CollectionProtocol, CollectionState, CollectionTicket,
        DurableCollectionJob, RequestTicket,
    },
    collection_transport::{ExecutionWindow, Observation},
};
use std::time::Instant;

pub(super) type CollectionExecutor = dyn Fn(
        &RequestTicket,
        &CollectionInput,
        ExecutionWindow,
        Instant,
        &CancellationToken,
    ) -> Observation
    + Send
    + Sync;
const MAX_PUBLICATION_RETRIES: u32 = 3;
#[cfg(test)]
type PreparationHook = dyn Fn(&RequestTicket) + Send + Sync;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum LanePhase {
    Idle,
    Running,
    Settling,
    SettlementPending,
    Faulted,
    RecoveryRequired,
    Stopped,
    Unpublished,
}

#[derive(Clone, Debug)]
pub(crate) struct CollectionLaneStatus {
    pub phase: LanePhase,
    pub job_id: Option<String>,
    pub generation: Option<u32>,
    pub sequence: Option<u32>,
    pub publication_retries: u32,
}
struct LaneState {
    status: CollectionLaneStatus,
    active: Option<CancellationToken>,
    admitted: Option<CollectionTicket>,
    current: Option<CollectionTicket>,
    retry_requested: bool,
    requires_recovery: bool,
}
pub(super) struct CollectionLane {
    state: Mutex<LaneState>,
    wake: Condvar,
    executor: Arc<CollectionExecutor>,
    pub(super) protocol: CollectionProtocol,
    #[cfg(test)]
    preparation_hook: Mutex<Option<Arc<PreparationHook>>>,
    #[cfg(test)]
    publication_retry_hook: Mutex<Option<Arc<PreparationHook>>>,
}
pub(super) struct DrainSnapshot {
    pub ticket: Option<CollectionTicket>,
    pub admitted: bool,
    pub idle: bool,
    pub faulted: bool,
}
impl CollectionLane {
    pub(super) fn new(executor: Arc<CollectionExecutor>, protocol: CollectionProtocol) -> Self {
        Self {
            executor,
            protocol,
            #[cfg(test)]
            preparation_hook: Mutex::new(None),
            #[cfg(test)]
            publication_retry_hook: Mutex::new(None),
            state: Mutex::new(LaneState {
                status: CollectionLaneStatus {
                    phase: LanePhase::Idle,
                    job_id: None,
                    generation: None,
                    sequence: None,
                    publication_retries: 0,
                },
                active: None,
                admitted: None,
                current: None,
                retry_requested: false,
                requires_recovery: false,
            }),
            wake: Condvar::new(),
        }
    }
    #[cfg(test)]
    pub(super) fn scheduling_fixture(
        &self,
        phase: LanePhase,
        ticket: Option<CollectionTicket>,
        admitted: bool,
    ) {
        let mut state = self.state.lock().unwrap();
        state.status.phase = phase;
        state.current = if admitted { None } else { ticket.clone() };
        state.admitted = if admitted { ticket } else { None };
        state.active = state.current.as_ref().map(|_| CancellationToken::default());
    }
    #[cfg(test)]
    pub(super) fn wait_scheduling_phase(&self, phase: LanePhase) {
        let deadline = Instant::now() + Duration::from_secs(10);
        let mut state = self.state.lock().unwrap();
        while state.status.phase != phase {
            let left = deadline
                .checked_duration_since(Instant::now())
                .expect("Synthetic lane deadline");
            let (next, timeout) = self.wake.wait_timeout(state, left).unwrap();
            state = next;
            assert!(
                !timeout.timed_out() || state.status.phase == phase,
                "Synthetic lane deadline"
            );
        }
    }
    pub(super) fn drain_snapshot(&self) -> Result<DrainSnapshot> {
        let state = self
            .state
            .lock()
            .map_err(|_| Error::Blocked("Collection lane unavailable".into()))?;
        Ok(DrainSnapshot {
            ticket: state.current.clone().or_else(|| state.admitted.clone()),
            admitted: state.admitted.is_some(),
            idle: state.status.phase == LanePhase::Idle
                && state.active.is_none()
                && state.current.is_none()
                && state.admitted.is_none()
                && !state.retry_requested
                && !state.requires_recovery,
            faulted: state.requires_recovery
                || matches!(
                    state.status.phase,
                    LanePhase::Faulted
                        | LanePhase::Unpublished
                        | LanePhase::RecoveryRequired
                        | LanePhase::Stopped
                ),
        })
    }
    pub(super) fn cancel_active(&self) {
        let state = self
            .state
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        if let Some(token) = &state.active {
            token.cancel();
        }
        self.wake.notify_all();
    }
    pub(super) fn requires_recovery(&self) -> bool {
        self.state
            .lock()
            .map_or(true, |state| state.requires_recovery)
    }
    fn fault(&self, unknown: bool) {
        let mut state = self
            .state
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        state.requires_recovery |= unknown;
        state.status.phase = if state.requires_recovery {
            LanePhase::RecoveryRequired
        } else {
            LanePhase::Faulted
        };
        state.active = None;
    }
}

impl JobCoordinator {
    /// Test-only constructor: production start cannot configure or execute this lane.
    #[cfg(test)]
    fn with_collection_executor(
        workspace: Workspace,
        processing: Arc<Executor>,
        collection: Arc<CollectionExecutor>,
    ) -> Result<Self> {
        Self::with_execution(
            workspace,
            1,
            processing,
            Some((collection, CollectionProtocol::SyntheticV2)),
        )
    }
    fn collection_lane(&self) -> Result<&CollectionLane> {
        self.shared
            .collection
            .as_deref()
            .ok_or_else(|| Error::Blocked("Durable collection execution is not enabled".into()))
    }
    fn collection_open(&self) -> Result<()> {
        crate::require(
            !self.shared.stopping.load(Ordering::Acquire) && self.shared.ownership.held(),
            "Collection coordinator is stopped or requires recovery",
        )
    }
    pub(crate) fn queue_collection(
        &self,
        input: CollectionInput,
        request_key: &str,
    ) -> Result<DurableCollectionJob> {
        let lane = self.collection_lane()?;
        let mut workspace = self
            .shared
            .workspace
            .lock()
            .map_err(|_| Error::Blocked("Workspace coordinator is unavailable".into()))?;
        self.collection_open()?;
        crate::require(
            !lane.requires_recovery(),
            "Collection lane requires recovery",
        )?;
        let result =
            workspace.queue_collection_protocol(input, request_key, now(), lane.protocol)?;
        drop(workspace);
        self.shared.wake.notify_all();
        Ok(result)
    }
    pub(crate) fn inspect_collection_job(&self, id: &str) -> Result<DurableCollectionJob> {
        self.collection_lane()?;
        self.shared
            .workspace
            .lock()
            .map_err(|_| Error::Blocked("Workspace coordinator is unavailable".into()))?
            .inspect_durable_collection(id)
    }
    pub(crate) fn collection_status(&self) -> Result<CollectionLaneStatus> {
        Ok(self
            .collection_lane()?
            .state
            .lock()
            .map_err(|_| Error::Blocked("Collection lane is unavailable".into()))?
            .status
            .clone())
    }
    pub(crate) fn cancel_collection(
        &self,
        id: &str,
        generation: u32,
    ) -> Result<DurableCollectionJob> {
        let lane = self.collection_lane()?;
        let mut workspace = self
            .shared
            .workspace
            .lock()
            .map_err(|_| Error::Blocked("Workspace coordinator is unavailable".into()))?;
        self.collection_open()?;
        let result = workspace.cancel_durable_collection(id, generation, now())?;
        // Identical workspace -> lane order as claim prevents missed cancellation.
        let state = lane
            .state
            .lock()
            .map_err(|_| Error::Blocked("Collection lane is unavailable".into()))?;
        if state.status.job_id.as_deref() == Some(id) {
            if let Some(token) = &state.active {
                token.cancel();
            }
        }
        drop(state);
        drop(workspace);
        lane.wake.notify_all();
        self.shared.wake.notify_all();
        Ok(result)
    }
    pub(crate) fn resume_collection(
        &self,
        id: &str,
        generation: u32,
    ) -> Result<DurableCollectionJob> {
        let lane = self.collection_lane()?;
        let mut workspace = self
            .shared
            .workspace
            .lock()
            .map_err(|_| Error::Blocked("Workspace coordinator is unavailable".into()))?;
        self.collection_open()?;
        let activity = self
            .shared
            .active
            .lock()
            .map_err(|_| Error::Blocked("Processing activity unavailable".into()))?;
        crate::require(
            activity.allows_resume(),
            "Collection resume is blocked by exclusive graph scheduling",
        )?;
        let mut state = lane
            .state
            .lock()
            .map_err(|_| Error::Blocked("Collection lane is unavailable".into()))?;
        crate::require(
            state.status.phase == LanePhase::Idle && state.admitted.is_none(),
            "Collection lane is not idle",
        )?;
        let job = workspace.inspect_durable_collection(id)?;
        crate::require(
            lane.protocol.matches(&job) && job.checkpoint.state == CollectionState::Interrupted,
            "Only an interrupted matching-policy run can resume",
        )?;
        state.admitted =
            workspace.start_durable_collection(id, generation, &self.shared.ownership, now())?;
        let result = workspace.inspect_durable_collection(id)?;
        drop(state);
        drop(workspace);
        self.shared.wake.notify_all();
        Ok(result)
    }
    pub(crate) fn retry_collection_settlement(
        &self,
        id: &str,
        generation: u32,
        sequence: u32,
    ) -> Result<CollectionLaneStatus> {
        let lane = self.collection_lane()?;
        crate::require(
            !self.shared.stopping.load(Ordering::Acquire)
                && self.shared.ownership.publication_held(),
            "Collection publication ownership is stopped or unavailable",
        )?;
        let mut state = lane
            .state
            .lock()
            .map_err(|_| Error::Blocked("Collection lane is unavailable".into()))?;
        crate::require(
            state.status.phase == LanePhase::SettlementPending
                && state.status.job_id.as_deref() == Some(id)
                && state.status.generation == Some(generation)
                && state.status.sequence == Some(sequence),
            "Pending collection settlement changed",
        )?;
        if !state.retry_requested {
            crate::require(
                state.status.publication_retries < MAX_PUBLICATION_RETRIES,
                "Collection publication retry limit reached",
            )?;
            state.retry_requested = true;
            state.status.publication_retries += 1;
        }
        let result = state.status.clone();
        drop(state);
        lane.wake.notify_all();
        Ok(result)
    }
}

fn now() -> i64 {
    chrono::Utc::now().timestamp_millis()
}

pub(super) fn work(shared: Arc<Shared>) {
    let lane = shared
        .collection
        .as_ref()
        .expect("configured collection lane")
        .clone();
    match std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| run(&shared, &lane))) {
        Ok(Ok(())) => {}
        Ok(Err(_)) => lane.fault(!shared.ownership.held()),
        Err(_) => {
            // No synthetic success or local-stop claim can follow a panicked adapter.
            // Its charged request remains unresolved, and the shared lock stays held.
            lane.fault(true);
            quarantine_execution(&shared);
        }
    }
}

fn run(shared: &Shared, lane: &CollectionLane) -> Result<()> {
    loop {
        let mut workspace = shared
            .workspace
            .lock()
            .map_err(|_| Error::Blocked("Workspace coordinator is unavailable".into()))?;
        if shared.stopping.load(Ordering::Acquire) {
            lane.state
                .lock()
                .map_err(|_| Error::Blocked("Collection lane is unavailable".into()))?
                .status
                .phase = LanePhase::Stopped;
            return Ok(());
        }
        if !shared.ownership.held() {
            lane.fault(true);
            return Ok(());
        }
        let mut activity = shared
            .active
            .lock()
            .map_err(|_| Error::Blocked("Processing activity unavailable".into()))?;
        if !matches!(
            graph::admit(
                shared,
                &mut workspace,
                &mut activity,
                graph::Lane::Collection
            )?,
            graph::Admission::Open
        ) {
            drop(activity);
            let _guard = shared
                .wake
                .wait_timeout(workspace, Duration::from_millis(100));
            continue;
        }
        let mut state = lane
            .state
            .lock()
            .map_err(|_| Error::Blocked("Collection lane is unavailable".into()))?;
        let execution = match state.admitted.take() {
            Some(ticket) => Some(ticket),
            None => workspace.claim_collection_protocol(&shared.ownership, now(), lane.protocol)?,
        };
        let Some(execution) = execution else {
            drop(state);
            drop(activity);
            let _guard = shared
                .wake
                .wait_timeout(workspace, Duration::from_millis(100));
            continue;
        };
        let token = CancellationToken::default();
        if shared.stopping.load(Ordering::Acquire) {
            token.cancel();
        }
        state.active = Some(token.clone());
        state.status = CollectionLaneStatus {
            phase: LanePhase::Running,
            job_id: Some(execution.job_id.clone()),
            generation: Some(execution.generation),
            sequence: None,
            publication_retries: 0,
        };
        state.current = Some(execution.clone());
        let mut driver = CollectionDriver::attach(&workspace, &shared.ownership, execution)?;
        drop(state);
        drop(activity);
        drop(workspace);
        loop {
            if shared.stopping.load(Ordering::Acquire) {
                token.cancel();
            }
            let pending = driver.next_with(
                &shared.workspace,
                &shared.ownership,
                &token,
                |ticket, input, window, pacing, cancel| {
                    (lane.executor)(ticket, input, window, pacing, cancel)
                },
            )?;
            let Some(pending) = pending else { break };
            let unknown = !pending.locally_quiescent();
            {
                let mut state = lane
                    .state
                    .lock()
                    .map_err(|_| Error::Blocked("Collection lane is unavailable".into()))?;
                state.requires_recovery |= unknown;
                state.status.sequence = Some(pending.request().sequence);
                state.status.publication_retries = 0;
                state.retry_requested = false;
            }
            if unknown {
                // Execution must stop even when canonical settlement is blocked.
                // The still-locked ownership lifetime permits publication only.
                quarantine_execution(shared);
            }
            let finished = publish(shared, lane, &pending)?;
            if unknown {
                lane.fault(true);
                return Ok(());
            }
            let Some(job) = finished else {
                lane.state
                    .lock()
                    .map_err(|_| Error::Blocked("Collection lane is unavailable".into()))?
                    .status
                    .phase = LanePhase::Unpublished;
                return Ok(());
            };
            if job.checkpoint.state != CollectionState::Running {
                break;
            }
        }
        let mut state = lane
            .state
            .lock()
            .map_err(|_| Error::Blocked("Collection lane is unavailable".into()))?;
        state.active = None;
        state.current = None;
        state.status.phase = LanePhase::Idle;
        shared.wake.notify_all();
    }
}

fn settle(shared: &Shared, pending: &PendingSettlement) -> Result<DurableCollectionJob> {
    settle_at(shared, pending, now())
}

fn settle_at(
    shared: &Shared,
    pending: &PendingSettlement,
    at_ms: i64,
) -> Result<DurableCollectionJob> {
    let capture = {
        let workspace = shared
            .workspace
            .lock()
            .map_err(|_| Error::Blocked("Workspace coordinator is unavailable".into()))?;
        pending.capture(&workspace, &shared.ownership)?
    };
    if let Some(capture) = capture {
        // Full historical replay and new HTML interpretation happen without
        // either the workspace mutex or a SQLite read transaction held.
        #[cfg(not(test))]
        let prepared = pending.prepare(capture)?;
        #[cfg(test)]
        let prepared = pending.prepare_with_hook(capture, || {
            let hook = shared
                .collection
                .as_ref()
                .and_then(|lane| lane.preparation_hook.lock().unwrap().clone());
            if let Some(hook) = hook {
                hook(pending.request());
            }
        })?;
        let mut workspace = shared
            .workspace
            .lock()
            .map_err(|_| Error::Blocked("Workspace coordinator is unavailable".into()))?;
        return pending.commit_prepared(
            prepared,
            &mut workspace,
            &shared.ownership,
            shared.stopping.load(Ordering::Acquire) || !shared.ownership.held(),
            at_ms,
        );
    }
    let mut workspace = shared
        .workspace
        .lock()
        .map_err(|_| Error::Blocked("Workspace coordinator is unavailable".into()))?;
    if shared.stopping.load(Ordering::Acquire) || !shared.ownership.held() {
        let job = workspace.inspect_durable_collection(&pending.request().run.job_id)?;
        if job.checkpoint.state == CollectionState::Running {
            if at_ms < job.checkpoint.updated_at_ms {
                // The stop intent uses the already validated journal anchor;
                // pending.settle retains the actual observation clock unchanged.
                workspace.cancel_collection_settlement_at_checkpoint(
                    pending.request(),
                    &shared.ownership,
                )?;
            } else {
                workspace.cancel_durable_collection(
                    &pending.request().run.job_id,
                    pending.request().run.generation,
                    at_ms,
                )?;
            }
        }
    }
    pending.settle(&mut workspace, &shared.ownership)
}

fn publish(
    shared: &Shared,
    lane: &CollectionLane,
    pending: &PendingSettlement,
) -> Result<Option<DurableCollectionJob>> {
    loop {
        lane.state
            .lock()
            .map_err(|_| Error::Blocked("Collection lane is unavailable".into()))?
            .status
            .phase = LanePhase::Settling;
        if let Ok(job) = settle(shared, pending) {
            return Ok(Some(job));
        }
        let mut state = lane
            .state
            .lock()
            .map_err(|_| Error::Blocked("Collection lane is unavailable".into()))?;
        state.status.phase = LanePhase::SettlementPending;
        lane.wake.notify_all();
        loop {
            if shared.stopping.load(Ordering::Acquire) {
                drop(state);
                // Exactly one final attempt; failure leaves the canonical reservation charged.
                return Ok(settle(shared, pending).ok());
            }
            if state.retry_requested {
                // Consumption and the in-progress phase are one transition.
                // Otherwise readers can offer/accept another retry before the
                // next iteration acquires this lock to begin publication.
                state.retry_requested = false;
                state.status.phase = LanePhase::Settling;
                break;
            }
            state = lane
                .wake
                .wait(state)
                .map_err(|_| Error::Blocked("Collection lane is unavailable".into()))?;
        }
        drop(state);
        #[cfg(test)]
        {
            let hook = lane.publication_retry_hook.lock().unwrap().take();
            if let Some(hook) = hook {
                hook(pending.request());
            }
        }
    }
}

#[cfg(test)]
#[path = "coordinator_collection_tests.rs"]
mod tests;

#[path = "coordinator_collection_api.rs"]
mod public_api;
pub(super) use public_api::is_public_command;
