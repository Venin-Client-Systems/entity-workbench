use super::*;
use crate::{
    collection_jobs::RequestProgress,
    collection_transport::{
        tests::canonical_tls, Outcome, Phase, ResolvedCandidates, ResponseHead,
    },
};
use std::sync::atomic::AtomicUsize;

fn input() -> CollectionInput {
    CollectionInput {
        urls: vec!["https://collection.invalid/start".into()],
        max_hops: 2,
        max_requests: 8,
        max_seconds: 60,
    }
}
fn fixture() -> (tempfile::TempDir, Workspace) {
    let temp = tempfile::tempdir_in(std::env::temp_dir().canonicalize().unwrap()).unwrap();
    let workspace = Workspace::open(temp.path().join("case")).unwrap();
    (temp, workspace)
}
fn no_documents() -> Arc<Executor> {
    Arc::new(|_, _, _, _, _| panic!("No document job in this fixture"))
}
fn key() -> String {
    uuid::Uuid::new_v4().to_string()
}
fn until(mut ready: impl FnMut() -> bool) {
    let deadline = Instant::now() + Duration::from_secs(15);
    while !ready() {
        assert!(
            Instant::now() < deadline,
            "Synthetic coordinator deadline exceeded"
        );
        thread::sleep(Duration::from_millis(5));
    }
}
fn complete(ticket: &RequestTicket) -> Observation {
    Observation {
        outcome: Outcome::Complete {
            head: ResponseHead {
                status: if ticket.sequence == 0 { 404 } else { 200 },
                media_type: Some("text/html".into()),
                redirect_url: None,
                identity_encoding: true,
            },
            body: format!("<p>Synthetic canonical sequence {}</p>", ticket.sequence).into_bytes(),
        },
        phase: Phase::Body,
        elapsed_milliseconds: 1,
        observed_wall_ms: now(),
        resolved: Some(ResolvedCandidates {
            addresses: vec!["1.1.1.1:443".parse().unwrap()],
            method: "synthetic_fixed_candidates",
            authoritative_complete_set: false,
        }),
        resolver_uncertainty: None,
        stop_observed: None,
        locally_quiescent: true,
    }
}
fn queue(coordinator: &JobCoordinator) -> DurableCollectionJob {
    coordinator.queue_collection(input(), &key()).unwrap()
}
fn sql(temp: &tempfile::TempDir, sql: &str) {
    rusqlite::Connection::open(temp.path().join("case/workspace.db"))
        .unwrap()
        .execute_batch(sql)
        .unwrap();
}

#[test]
fn ordinary_coordinator_keeps_collection_unactivated_and_does_not_recover_private_runs() {
    let (_temp, mut workspace) = fixture();
    let owner = workspace.collection_ownership().unwrap();
    let job = workspace
        .queue_collection_transport(input(), &key(), now())
        .unwrap();
    let ticket = workspace
        .start_durable_collection(&job.id, 1, &owner, now())
        .unwrap()
        .unwrap();
    workspace
        .advance_durable_collection(&ticket, &owner, now())
        .unwrap()
        .unwrap();
    let before = workspace.inspect_durable_collection(&job.id).unwrap();
    drop(owner);
    let coordinator = JobCoordinator::start(workspace, 1).unwrap();
    assert!(coordinator.queue_collection(input(), &key()).is_err());
    assert_eq!(
        coordinator
            .shared
            .workspace
            .lock()
            .unwrap()
            .inspect_durable_collection(&job.id)
            .unwrap(),
        before
    );
    coordinator.shutdown().unwrap();
    assert_eq!(
        coordinator
            .shared
            .workspace
            .lock()
            .unwrap()
            .inspect_durable_collection(&job.id)
            .unwrap(),
        before
    );
}

#[test]
fn synthetic_coordinator_runs_one_tls_lane_with_canonical_receipts_and_no_held_workspace_lock() {
    let (_temp, workspace) = fixture();
    let calls = Arc::new(AtomicUsize::new(0));
    let active = Arc::new(AtomicUsize::new(0));
    let running = active.clone();
    let requests = calls.clone();
    let coordinator = JobCoordinator::with_collection_executor(
        workspace,
        no_documents(),
        Arc::new(move |ticket, input, window, pacing, token| {
            assert_eq!(running.fetch_add(1, Ordering::AcqRel), 0);
            requests.fetch_add(1, Ordering::AcqRel);
            let response = if ticket.sequence == 0 {
                b"HTTP/1.1 404 Not Found\r\nContent-Length: 0\r\n\r\n".to_vec()
            } else {
                b"HTTP/1.1 200 OK\r\nContent-Type: text/plain\r\nContent-Length: 9\r\n\r\nSynthetic"
                    .to_vec()
            };
            let observation = canonical_tls(ticket, input, window, pacing, token, response, false);
            running.fetch_sub(1, Ordering::AcqRel);
            observation
        }),
    )
    .unwrap();
    let first = queue(&coordinator);
    let second = queue(&coordinator);
    until(|| {
        coordinator
            .inspect_collection_job(&first.id)
            .unwrap()
            .checkpoint
            .state
            == CollectionState::Successful
            && coordinator
                .inspect_collection_job(&second.id)
                .unwrap()
                .checkpoint
                .state
                == CollectionState::Successful
    });
    assert_eq!(calls.load(Ordering::Acquire), 4);
    let workspace = coordinator.shared.workspace.lock().unwrap();
    let view = workspace.view().unwrap();
    assert_eq!(view.evidence.len(), 2);
    assert_eq!(
        view.evidence
            .iter()
            .map(|e| e.acquisitions.len())
            .sum::<usize>(),
        4
    );
    assert!(view.observations.is_empty());
    drop(workspace);
    coordinator.shutdown().unwrap();
    assert_eq!(active.load(Ordering::Acquire), 0);
}

#[test]
fn active_cancellation_is_responsive_and_complete_race_keeps_bytes_without_promotion() {
    let (_temp, workspace) = fixture();
    let entered = Arc::new(AtomicBool::new(false));
    let stopped = Arc::new(AtomicBool::new(false));
    let release = Arc::new(AtomicBool::new(false));
    let (running, cancelled, released) = (entered.clone(), stopped.clone(), release.clone());
    let coordinator = JobCoordinator::with_collection_executor(
        workspace,
        no_documents(),
        Arc::new(move |ticket, _, _, _, token| {
            if ticket.sequence > 0 {
                running.store(true, Ordering::Release);
                until(|| token.is_cancelled());
                cancelled.store(true, Ordering::Release);
                until(|| released.load(Ordering::Acquire));
            }
            complete(ticket)
        }),
    )
    .unwrap();
    let job = queue(&coordinator);
    until(|| entered.load(Ordering::Acquire));
    let response = coordinator.cancel_collection(&job.id, 1).unwrap();
    assert_eq!(response.checkpoint.state, CollectionState::Running);
    assert!(response.checkpoint.cancellation_requested);
    until(|| stopped.load(Ordering::Acquire));
    assert_eq!(
        coordinator
            .inspect_collection_job(&job.id)
            .unwrap()
            .checkpoint
            .state,
        CollectionState::Running
    );
    coordinator.dispatch(Command::View {}).unwrap();
    release.store(true, Ordering::Release);
    until(|| {
        coordinator
            .inspect_collection_job(&job.id)
            .unwrap()
            .checkpoint
            .state
            == CollectionState::Cancelled
    });
    let finished = coordinator.inspect_collection_job(&job.id).unwrap();
    assert_eq!(finished.checkpoint.requests_used(), 2);
    assert_eq!(finished.checkpoint.pages_retained, 0);
    let view = coordinator.shared.workspace.lock().unwrap().view().unwrap();
    assert_eq!(view.evidence.len(), 2);
    assert!(view.evidence.iter().all(|e| e.text.is_none()));
}

#[test]
fn publication_failure_waits_for_exact_explicit_retry_without_another_transport() {
    let (temp, workspace) = fixture();
    sql(&temp, "CREATE TRIGGER reject_collection_evidence BEFORE INSERT ON records WHEN NEW.kind='evidence' BEGIN SELECT RAISE(ABORT,'synthetic publication fault'); END;");
    let calls = Arc::new(AtomicUsize::new(0));
    let requests = calls.clone();
    let coordinator = JobCoordinator::with_collection_executor(
        workspace,
        no_documents(),
        Arc::new(move |ticket, _, _, _, _| {
            requests.fetch_add(1, Ordering::AcqRel);
            complete(ticket)
        }),
    )
    .unwrap();
    let job = queue(&coordinator);
    until(|| coordinator.collection_status().unwrap().phase == LanePhase::SettlementPending);
    assert_eq!(calls.load(Ordering::Acquire), 1);
    assert!(matches!(
        coordinator
            .inspect_collection_job(&job.id)
            .unwrap()
            .checkpoint
            .requests[0]
            .progress,
        RequestProgress::Reserved
    ));
    assert!(coordinator
        .retry_collection_settlement(&job.id, 2, 0)
        .is_err());
    assert!(coordinator
        .retry_collection_settlement(&job.id, 1, 1)
        .is_err());
    sql(&temp, "DROP TRIGGER reject_collection_evidence;");
    for _ in 0..10 {
        coordinator
            .shared
            .collection
            .as_ref()
            .unwrap()
            .wake
            .notify_all();
    }
    assert_eq!(
        coordinator.collection_status().unwrap().phase,
        LanePhase::SettlementPending
    );
    assert_eq!(
        coordinator.collection_status().unwrap().publication_retries,
        0
    );
    coordinator
        .retry_collection_settlement(&job.id, 1, 0)
        .unwrap();
    until(|| {
        coordinator
            .inspect_collection_job(&job.id)
            .unwrap()
            .checkpoint
            .state
            == CollectionState::Successful
    });
    assert_eq!(calls.load(Ordering::Acquire), 2);
    assert_eq!(
        coordinator
            .inspect_collection_job(&job.id)
            .unwrap()
            .checkpoint
            .requests_used(),
        2
    );
    assert_eq!(
        coordinator
            .shared
            .workspace
            .lock()
            .unwrap()
            .view()
            .unwrap()
            .evidence
            .iter()
            .map(|e| e.acquisitions.len())
            .sum::<usize>(),
        2
    );
}

#[test]
fn explicit_publication_retry_limit_and_failed_shutdown_leave_one_charged_unknown() {
    let (temp, workspace) = fixture();
    sql(&temp, "CREATE TRIGGER reject_collection_evidence BEFORE INSERT ON records WHEN NEW.kind='evidence' BEGIN SELECT RAISE(ABORT,'synthetic publication fault'); END;");
    let calls = Arc::new(AtomicUsize::new(0));
    let requests = calls.clone();
    let coordinator = JobCoordinator::with_collection_executor(
        workspace,
        no_documents(),
        Arc::new(move |ticket, _, _, _, _| {
            requests.fetch_add(1, Ordering::AcqRel);
            complete(ticket)
        }),
    )
    .unwrap();
    let job = queue(&coordinator);
    until(|| coordinator.collection_status().unwrap().phase == LanePhase::SettlementPending);
    for count in 1..=MAX_PUBLICATION_RETRIES {
        coordinator
            .retry_collection_settlement(&job.id, 1, 0)
            .unwrap();
        until(|| {
            let state = coordinator
                .shared
                .collection
                .as_ref()
                .unwrap()
                .state
                .lock()
                .unwrap();
            state.status.phase == LanePhase::SettlementPending
                && state.status.publication_retries == count
                && !state.retry_requested
        });
    }
    assert!(coordinator
        .retry_collection_settlement(&job.id, 1, 0)
        .is_err());
    coordinator.shutdown().unwrap();
    assert_eq!(calls.load(Ordering::Acquire), 1);
    assert_eq!(
        coordinator.collection_status().unwrap().phase,
        LanePhase::Unpublished
    );
    let before = coordinator
        .shared
        .workspace
        .lock()
        .unwrap()
        .inspect_durable_collection(&job.id)
        .unwrap();
    assert_eq!(before.checkpoint.requests_used(), 1);
    assert!(matches!(
        before.checkpoint.requests[0].progress,
        RequestProgress::Reserved
    ));
    assert!(before.checkpoint.cancellation_requested);
    drop(coordinator);
    sql(&temp, "DROP TRIGGER reject_collection_evidence;");
    let reopened = Workspace::open(temp.path().join("case")).unwrap();
    let recovered = JobCoordinator::with_collection_executor(
        reopened,
        no_documents(),
        Arc::new(|_, _, _, _, _| panic!("recovered cancellation must not retry transport")),
    )
    .unwrap();
    let after = recovered.inspect_collection_job(&job.id).unwrap();
    assert_eq!(after.checkpoint.state, CollectionState::Cancelled);
    assert!(matches!(
        after.checkpoint.requests[0].progress,
        RequestProgress::InterruptedUnknown { .. }
    ));
    assert!(recovered
        .shared
        .workspace
        .lock()
        .unwrap()
        .view()
        .unwrap()
        .evidence
        .is_empty());
}

#[test]
fn joined_shutdown_holds_shared_ownership_until_both_document_and_collection_lanes_exit() {
    let (temp, mut workspace) = fixture();
    let source = workspace
        .import("synthetic.txt", b"Synthetic processing input")
        .unwrap();
    workspace.queue_document_parse(&source, &key()).unwrap();
    let job = workspace
        .queue_collection_transport(input(), &key(), now())
        .unwrap();
    let entered = Arc::new(AtomicUsize::new(0));
    let stopped = Arc::new(AtomicUsize::new(0));
    let release = Arc::new(AtomicBool::new(false));
    let (doc_entered, doc_stopped, doc_release) =
        (entered.clone(), stopped.clone(), release.clone());
    let (net_entered, net_stopped, net_release) =
        (entered.clone(), stopped.clone(), release.clone());
    let coordinator = Arc::new(
        JobCoordinator::with_collection_executor(
            workspace,
            Arc::new(move |_, _, _, _, token| {
                doc_entered.fetch_add(1, Ordering::AcqRel);
                until(|| token.is_cancelled());
                doc_stopped.fetch_add(1, Ordering::AcqRel);
                until(|| doc_release.load(Ordering::Acquire));
                Err(Error::Interrupted("Synthetic joined stop".into()))
            }),
            Arc::new(move |ticket, _, _, _, token| {
                net_entered.fetch_add(1, Ordering::AcqRel);
                until(|| token.is_cancelled());
                net_stopped.fetch_add(1, Ordering::AcqRel);
                until(|| net_release.load(Ordering::Acquire));
                complete(ticket)
            }),
        )
        .unwrap(),
    );
    until(|| entered.load(Ordering::Acquire) == 2);
    let stale_owner = coordinator.shared.ownership.clone();
    let stopping = coordinator.clone();
    let handle = thread::spawn(move || stopping.shutdown());
    until(|| stopped.load(Ordering::Acquire) == 2);
    assert!(stale_owner.held());
    assert!(JobCoordinator::start(Workspace::open(temp.path().join("case")).unwrap(), 1).is_err());
    release.store(true, Ordering::Release);
    handle.join().unwrap().unwrap();
    assert!(!stale_owner.held());
    let mut workspace = coordinator.shared.workspace.lock().unwrap();
    assert!(workspace
        .start_durable_collection(&job.id, 1, &stale_owner, now())
        .is_err());
    assert_eq!(
        workspace
            .inspect_durable_collection(&job.id)
            .unwrap()
            .checkpoint
            .state,
        CollectionState::Cancelled
    );
    drop(workspace);
    let next =
        JobCoordinator::start(Workspace::open(temp.path().join("case")).unwrap(), 1).unwrap();
    coordinator.shutdown().unwrap();
    assert!(JobCoordinator::start(Workspace::open(temp.path().join("case")).unwrap(), 1).is_err());
    next.shutdown().unwrap();
}

#[test]
fn exclusive_start_recovers_charged_unknown_and_requires_explicit_resume_with_original_deadline() {
    let (_temp, mut workspace) = fixture();
    let owner = workspace.collection_ownership().unwrap();
    let job = workspace
        .queue_collection_transport(input(), &key(), now())
        .unwrap();
    let execution = workspace
        .start_durable_collection(&job.id, 1, &owner, now())
        .unwrap()
        .unwrap();
    workspace
        .advance_durable_collection(&execution, &owner, now())
        .unwrap()
        .unwrap();
    let deadline = workspace
        .inspect_durable_collection(&job.id)
        .unwrap()
        .checkpoint
        .deadline_at_ms;
    drop(owner);
    let coordinator = JobCoordinator::with_collection_executor(
        workspace,
        no_documents(),
        Arc::new(|_, _, _, _, _| panic!("unknown robots cannot retry")),
    )
    .unwrap();
    assert_eq!(
        coordinator
            .inspect_collection_job(&job.id)
            .unwrap()
            .checkpoint
            .state,
        CollectionState::Interrupted
    );
    assert!(coordinator.resume_collection(&job.id, 2).is_err());
    coordinator.resume_collection(&job.id, 1).unwrap();
    until(|| {
        coordinator
            .inspect_collection_job(&job.id)
            .unwrap()
            .checkpoint
            .state
            == CollectionState::Failed
    });
    let result = coordinator.inspect_collection_job(&job.id).unwrap();
    assert_eq!(result.checkpoint.requests_used(), 1);
    assert_eq!(result.checkpoint.deadline_at_ms, deadline);
    assert_eq!(result.checkpoint.generation, 2);
}

#[test]
fn panicked_collection_adapter_keeps_the_shared_lock_quarantined_without_false_settlement() {
    let (temp, workspace) = fixture();
    let coordinator = JobCoordinator::with_collection_executor(
        workspace,
        no_documents(),
        Arc::new(|_, _, _, _, _| panic!("Synthetic unverified executor")),
    )
    .unwrap();
    let job = queue(&coordinator);
    until(|| coordinator.collection_status().unwrap().phase == LanePhase::RecoveryRequired);
    assert!(coordinator.shutdown().is_err());
    assert!(coordinator.shutdown().is_err());
    let result = coordinator.inspect_collection_job(&job.id).unwrap();
    assert_eq!(result.checkpoint.requests_used(), 1);
    assert!(matches!(
        result.checkpoint.requests[0].progress,
        RequestProgress::Reserved
    ));
    assert!(coordinator
        .shared
        .workspace
        .lock()
        .unwrap()
        .view()
        .unwrap()
        .evidence
        .is_empty());
    assert!(JobCoordinator::start(Workspace::open(temp.path().join("case")).unwrap(), 1).is_err());
    drop(coordinator);
    assert!(JobCoordinator::start(Workspace::open(temp.path().join("case")).unwrap(), 1).is_err());
}

#[test]
fn panicked_join_never_releases_shared_ownership_on_repeated_shutdown() {
    let (temp, workspace) = fixture();
    let coordinator = JobCoordinator::start(workspace, 1).unwrap();
    coordinator
        .workers
        .lock()
        .unwrap()
        .push(thread::spawn(|| panic!("Synthetic thread failure")));
    assert!(coordinator.shutdown().is_err());
    assert!(coordinator.shutdown().is_err());
    assert!(JobCoordinator::start(Workspace::open(temp.path().join("case")).unwrap(), 1).is_err());
    drop(coordinator);
    assert!(JobCoordinator::start(Workspace::open(temp.path().join("case")).unwrap(), 1).is_err());
}

#[test]
fn unknown_operation_retains_canonical_quarantine_and_never_starts_the_next_run() {
    use crate::collection_transport::{CallerContextState, ResolverUncertainty, StopReason};
    let (temp, mut workspace) = fixture();
    let first = workspace
        .queue_collection_transport(input(), &key(), now())
        .unwrap();
    let second = workspace
        .queue_collection_transport(input(), &key(), now())
        .unwrap();
    let calls = Arc::new(AtomicUsize::new(0));
    let requests = calls.clone();
    let coordinator = JobCoordinator::with_collection_executor(
        workspace,
        no_documents(),
        Arc::new(move |_, _, _, _, _| {
            requests.fetch_add(1, Ordering::AcqRel);
            Observation {
                outcome: Outcome::Stopped {
                    reason: StopReason::QuiescenceUnverified,
                    head: None,
                },
                phase: Phase::Dns,
                elapsed_milliseconds: 1,
                observed_wall_ms: now(),
                resolved: None,
                resolver_uncertainty: Some(ResolverUncertainty {
                    method: "windows_overlapped_dns",
                    caller_context: CallerContextState::RetainedPendingCompletion,
                }),
                stop_observed: None,
                locally_quiescent: false,
            }
        }),
    )
    .unwrap();
    until(|| coordinator.collection_status().unwrap().phase == LanePhase::RecoveryRequired);
    assert_eq!(
        coordinator
            .inspect_collection_job(&first.id)
            .unwrap()
            .checkpoint
            .state,
        CollectionState::RecoveryRequired
    );
    assert_eq!(
        coordinator
            .inspect_collection_job(&second.id)
            .unwrap()
            .checkpoint
            .state,
        CollectionState::Queued
    );
    assert!(coordinator.queue_collection(input(), &key()).is_err());
    assert!(coordinator.shutdown().is_err());
    assert_eq!(calls.load(Ordering::Acquire), 1);
    assert!(JobCoordinator::start(Workspace::open(temp.path().join("case")).unwrap(), 1).is_err());
    let reopened = Workspace::open(temp.path().join("case")).unwrap();
    assert_eq!(
        reopened
            .inspect_durable_collection(&first.id)
            .unwrap()
            .checkpoint
            .state,
        CollectionState::RecoveryRequired
    );
    assert!(reopened.view().unwrap().evidence.is_empty());
}

#[test]
fn synthetic_lane_recovery_does_not_rewrite_historical_v1_running_records() {
    let (_temp, mut workspace) = fixture();
    let owner = workspace.collection_ownership().unwrap();
    let job = workspace
        .queue_durable_collection(input(), &key(), now())
        .unwrap();
    workspace
        .start_durable_collection(&job.id, 1, &owner, now())
        .unwrap()
        .unwrap();
    let before = workspace.inspect_durable_collection(&job.id).unwrap();
    let revision = workspace.revision().unwrap();
    drop(owner);
    let coordinator = JobCoordinator::with_collection_executor(
        workspace,
        no_documents(),
        Arc::new(|_, _, _, _, _| panic!("historical v1 is not activated")),
    )
    .unwrap();
    assert_eq!(coordinator.inspect_collection_job(&job.id).unwrap(), before);
    assert_eq!(
        coordinator
            .shared
            .workspace
            .lock()
            .unwrap()
            .revision()
            .unwrap(),
        revision
    );
    coordinator.shutdown().unwrap();
    assert_eq!(
        coordinator
            .shared
            .workspace
            .lock()
            .unwrap()
            .inspect_durable_collection(&job.id)
            .unwrap(),
        before
    );
}

#[test]
fn unknown_document_exit_cancels_active_collection_preserves_its_bytes_and_blocks_queued_work() {
    use crate::processing::ProcessingFailure;
    let (temp, mut workspace) = fixture();
    let source = workspace
        .import("synthetic.txt", b"Synthetic document")
        .unwrap();
    let document = workspace.queue_document_parse(&source, &key()).unwrap();
    let first = workspace
        .queue_collection_transport(input(), &key(), now())
        .unwrap();
    let second = workspace
        .queue_collection_transport(input(), &key(), now())
        .unwrap();
    let entered = Arc::new(AtomicBool::new(false));
    let stopped = Arc::new(AtomicBool::new(false));
    let requests = Arc::new(AtomicUsize::new(0));
    let (doc_entered, net_entered, net_stopped, net_requests) = (
        entered.clone(),
        entered.clone(),
        stopped.clone(),
        requests.clone(),
    );
    let coordinator = JobCoordinator::with_collection_executor(
        workspace,
        Arc::new(move |_, _, _, _, _| {
            until(|| doc_entered.load(Ordering::Acquire));
            Err(Error::TerminationUnverified(
                "Synthetic document exit is unknown".into(),
            ))
        }),
        Arc::new(move |ticket, _, _, _, token| {
            net_requests.fetch_add(1, Ordering::AcqRel);
            if ticket.sequence == 1 {
                net_entered.store(true, Ordering::Release);
                until(|| token.is_cancelled());
                net_stopped.store(true, Ordering::Release);
            }
            complete(ticket)
        }),
    )
    .unwrap();
    until(|| stopped.load(Ordering::Acquire));
    until(|| {
        coordinator
            .shared
            .workspace
            .lock()
            .unwrap()
            .processing_job(&document.id)
            .unwrap()
            .failure
            == Some(ProcessingFailure::WorkerExitUnverified)
    });
    until(|| {
        coordinator
            .inspect_collection_job(&first.id)
            .unwrap()
            .checkpoint
            .state
            == CollectionState::Cancelled
    });
    assert_eq!(requests.load(Ordering::Acquire), 2);
    assert_eq!(
        coordinator
            .inspect_collection_job(&second.id)
            .unwrap()
            .checkpoint
            .state,
        CollectionState::Queued
    );
    let view = coordinator.shared.workspace.lock().unwrap().view().unwrap();
    let acquired: Vec<_> = view
        .evidence
        .iter()
        .filter(|e| !e.acquisitions.is_empty())
        .collect();
    assert_eq!(acquired.len(), 2);
    assert!(acquired.iter().all(|e| e.text.is_none()));
    assert!(acquired
        .iter()
        .any(|e| e.sha256 == crate::store::hash(b"<p>Synthetic canonical sequence 1</p>")));
    assert!(coordinator
        .shared
        .workspace
        .lock()
        .unwrap()
        .processing_job(&document.id)
        .unwrap()
        .result_ids
        .is_empty());
    // Reading the typed failure stays available; new execution is quarantined.
    coordinator
        .dispatch(Command::InspectProcessingJob {
            job_id: document.id.clone(),
        })
        .unwrap();
    assert!(coordinator.queue_collection(input(), &key()).is_err());
    assert!(coordinator.shutdown().is_err());
    assert!(coordinator.shutdown().is_err());
    drop(coordinator);
    assert!(JobCoordinator::start(Workspace::open(temp.path().join("case")).unwrap(), 1).is_err());
}

#[test]
fn persisted_document_uncertainty_prevents_collection_start_after_exclusive_recovery() {
    use crate::processing::ProcessingFailure;
    let (_temp, mut workspace) = fixture();
    let source = workspace
        .import("synthetic.txt", b"Interrupted synthetic document")
        .unwrap();
    let document = workspace.queue_document_parse(&source, &key()).unwrap();
    workspace.claim_processing_job().unwrap().unwrap();
    let collection = workspace
        .queue_collection_transport(input(), &key(), now())
        .unwrap();
    let coordinator = JobCoordinator::with_collection_executor(
        workspace,
        no_documents(),
        Arc::new(|_, _, _, _, _| panic!("document recovery quarantine forbids collection")),
    )
    .unwrap();
    until(|| coordinator.collection_status().unwrap().phase == LanePhase::RecoveryRequired);
    assert_eq!(
        coordinator
            .shared
            .workspace
            .lock()
            .unwrap()
            .processing_job(&document.id)
            .unwrap()
            .failure,
        Some(ProcessingFailure::WorkerExitUnverified)
    );
    assert_eq!(
        coordinator
            .inspect_collection_job(&collection.id)
            .unwrap()
            .checkpoint
            .state,
        CollectionState::Queued
    );
    assert!(coordinator.shutdown().is_err());
}

#[test]
fn ordinary_document_cleanup_failure_does_not_quarantine_collection_or_joined_release() {
    use crate::processing::ProcessingFailure;
    let (temp, mut workspace) = fixture();
    let source = workspace
        .import("synthetic.txt", b"Synthetic cleanup case")
        .unwrap();
    let document = workspace.queue_document_parse(&source, &key()).unwrap();
    let coordinator = JobCoordinator::with_collection_executor(
        workspace,
        Arc::new(|_, _, _, _, _| {
            Err(Error::Cleanup(
                "Synthetic confirmed-stop cleanup failure".into(),
            ))
        }),
        Arc::new(|ticket, _, _, _, _| complete(ticket)),
    )
    .unwrap();
    let collection = queue(&coordinator);
    until(|| {
        coordinator
            .inspect_collection_job(&collection.id)
            .unwrap()
            .checkpoint
            .state
            == CollectionState::Successful
    });
    until(|| {
        coordinator
            .shared
            .workspace
            .lock()
            .unwrap()
            .processing_job(&document.id)
            .unwrap()
            .failure
            == Some(ProcessingFailure::CleanupFailed)
    });
    coordinator.shutdown().unwrap();
    let next =
        JobCoordinator::start(Workspace::open(temp.path().join("case")).unwrap(), 1).unwrap();
    next.shutdown().unwrap();
}

#[test]
fn unknown_collection_quarantines_before_failed_publication_and_retains_explicit_retry() {
    for late_success in [false, true] {
        unknown_collection_publication_case(late_success);
    }
}

fn unknown_collection_publication_case(late_success: bool) {
    use crate::{
        collection_jobs::CollectionEvent,
        collection_transport::{CallerContextState, ResolverUncertainty, StopReason},
        engines::parser::{ParseLimitation, ParseResult, ParseStatus},
        processing::{ProcessingFailure, ProcessingState},
    };
    let (temp, mut workspace) = fixture();
    let source = workspace
        .import("synthetic.txt", b"Synthetic pending publication")
        .unwrap();
    let active_document = workspace.queue_document_parse(&source, &key()).unwrap();
    let queued_document = workspace.queue_document_parse(&source, &key()).unwrap();
    let collection = workspace
        .queue_collection_transport(input(), &key(), now())
        .unwrap();
    sql(&temp, "CREATE TRIGGER reject_unknown_receipt BEFORE UPDATE ON records WHEN NEW.kind='collection_run' AND json_extract(NEW.body,'$.events[#-1].event')='transport_observed' BEGIN SELECT RAISE(ABORT,'synthetic receipt publication fault'); END;");
    let entered = Arc::new(AtomicBool::new(false));
    let cancelled = Arc::new(AtomicBool::new(false));
    let document_calls = Arc::new(AtomicUsize::new(0));
    let transport_calls = Arc::new(AtomicUsize::new(0));
    let (doc_entered, doc_cancelled, doc_calls) =
        (entered.clone(), cancelled.clone(), document_calls.clone());
    let (net_entered, net_calls) = (entered.clone(), transport_calls.clone());
    let coordinator = JobCoordinator::with_collection_executor(
        workspace,
        Arc::new(move |_, _, _, bytes, token| {
            assert_eq!(
                doc_calls.fetch_add(1, Ordering::AcqRel),
                0,
                "Queued document must never launch"
            );
            doc_entered.store(true, Ordering::Release);
            until(|| token.is_cancelled());
            doc_cancelled.store(true, Ordering::Release);
            if late_success {
                Ok(ProcessingOutput::Document(ParseResult {
                    protocol_version: 1,
                    job_id: key(),
                    content_sha256: crate::store::hash(bytes),
                    source_bytes: bytes.len() as u64,
                    parser: "utf8-v1".into(),
                    media_type: "text/plain".into(),
                    status: ParseStatus::Complete,
                    text: "Synthetic late result must not publish".into(),
                    metadata: BTreeMap::new(),
                    limitations: vec![ParseLimitation::NoSourceAnchors],
                    error: None,
                }))
            } else {
                Err(Error::Interrupted(
                    "Synthetic cancellation before collection publication".into(),
                ))
            }
        }),
        Arc::new(move |_, _, _, _, _| {
            until(|| net_entered.load(Ordering::Acquire));
            net_calls.fetch_add(1, Ordering::AcqRel);
            Observation {
                outcome: Outcome::Stopped {
                    reason: StopReason::QuiescenceUnverified,
                    head: None,
                },
                phase: Phase::Dns,
                elapsed_milliseconds: 1,
                observed_wall_ms: now(),
                resolved: None,
                resolver_uncertainty: Some(ResolverUncertainty {
                    method: "windows_overlapped_dns",
                    caller_context: CallerContextState::RetainedPendingCompletion,
                }),
                stop_observed: None,
                locally_quiescent: false,
            }
        }),
    )
    .unwrap();
    until(|| coordinator.collection_status().unwrap().phase == LanePhase::SettlementPending);
    // This is before retry or shutdown. A blocked canonical write cannot delay quarantine.
    assert!(!coordinator.shared.ownership.held());
    until(|| cancelled.load(Ordering::Acquire));
    until(|| {
        coordinator
            .shared
            .workspace
            .lock()
            .unwrap()
            .processing_job(&active_document.id)
            .unwrap()
            .state
            != ProcessingState::Running
    });
    let stopped_document = coordinator
        .shared
        .workspace
        .lock()
        .unwrap()
        .processing_job(&active_document.id)
        .unwrap();
    assert_eq!(
        stopped_document.failure,
        Some(ProcessingFailure::Interrupted)
    );
    assert!(stopped_document.result_ids.is_empty());
    assert_eq!(
        coordinator
            .shared
            .workspace
            .lock()
            .unwrap()
            .processing_job(&queued_document.id)
            .unwrap()
            .state,
        ProcessingState::Queued
    );
    assert_eq!(document_calls.load(Ordering::Acquire), 1);
    assert_eq!(transport_calls.load(Ordering::Acquire), 1);
    let before = coordinator.inspect_collection_job(&collection.id).unwrap();
    assert!(matches!(
        before.checkpoint.requests[0].progress,
        RequestProgress::Reserved
    ));
    assert!(!before
        .events
        .iter()
        .any(|event| matches!(event, CollectionEvent::TransportObserved { .. })));
    assert!(coordinator.queue_collection(input(), &key()).is_err());
    assert!(coordinator
        .retry_collection_settlement(&collection.id, 2, 0)
        .is_err());
    assert!(coordinator
        .retry_collection_settlement(&collection.id, 1, 1)
        .is_err());
    sql(&temp, "DROP TRIGGER reject_unknown_receipt;");
    coordinator
        .retry_collection_settlement(&collection.id, 1, 0)
        .unwrap();
    until(|| coordinator.collection_status().unwrap().phase == LanePhase::RecoveryRequired);
    let after = coordinator.inspect_collection_job(&collection.id).unwrap();
    assert_eq!(after.checkpoint.state, CollectionState::RecoveryRequired);
    assert_eq!(
        after
            .events
            .iter()
            .filter(|event| matches!(event, CollectionEvent::TransportObserved { .. }))
            .count(),
        1
    );
    assert_eq!(after.checkpoint.requests_used(), 1);
    assert_eq!(transport_calls.load(Ordering::Acquire), 1);
    assert_eq!(document_calls.load(Ordering::Acquire), 1);
    assert!(coordinator
        .retry_collection_settlement(&collection.id, 1, 0)
        .is_err());
    assert!(coordinator.shutdown().is_err());
    assert!(coordinator.shutdown().is_err());
    drop(coordinator);
    assert!(JobCoordinator::start(Workspace::open(temp.path().join("case")).unwrap(), 1).is_err());
}
