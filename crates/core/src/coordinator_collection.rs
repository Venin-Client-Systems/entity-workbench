//! Private synthetic lane owned by JobCoordinator. No public activation or live transport injection.
#![allow(dead_code)]
use super::*;
use crate::{
    collection_execution::{CollectionDriver, PendingSettlement},
    collection_jobs::{
        CollectionInput, CollectionState, CollectionTicket, DurableCollectionJob, RequestTicket,
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
    retry_requested: bool,
    requires_recovery: bool,
}
pub(super) struct CollectionLane {
    state: Mutex<LaneState>,
    wake: Condvar,
    executor: Arc<CollectionExecutor>,
}
impl CollectionLane {
    pub(super) fn new(executor: Arc<CollectionExecutor>) -> Self {
        Self {
            executor,
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
                retry_requested: false,
                requires_recovery: false,
            }),
            wake: Condvar::new(),
        }
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
        Self::with_execution(workspace, 1, processing, Some(collection))
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
        let result = workspace.queue_collection_transport(input, request_key, now())?;
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
            job.schema_version == 2
                && job.synthetic
                && job.checkpoint.state == CollectionState::Interrupted,
            "Only an interrupted synthetic v2 run can resume",
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
        let mut state = lane
            .state
            .lock()
            .map_err(|_| Error::Blocked("Collection lane is unavailable".into()))?;
        let execution = match state.admitted.take() {
            Some(ticket) => Some(ticket),
            None => workspace.claim_collection_transport(&shared.ownership, now())?,
        };
        let Some(execution) = execution else {
            drop(state);
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
        let mut driver = CollectionDriver::attach(&workspace, &shared.ownership, execution)?;
        drop(state);
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
        state.status.phase = LanePhase::Idle;
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
        loop {
            if shared.stopping.load(Ordering::Acquire) {
                drop(state);
                // Exactly one final attempt; failure leaves the canonical reservation charged.
                return Ok(settle(shared, pending).ok());
            }
            if state.retry_requested {
                state.retry_requested = false;
                break;
            }
            state = lane
                .wake
                .wait(state)
                .map_err(|_| Error::Blocked("Collection lane is unavailable".into()))?;
        }
        drop(state);
    }
}

#[cfg(test)]
#[path = "coordinator_collection_tests.rs"]
mod tests;
