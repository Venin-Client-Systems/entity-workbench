//! Deterministic synthetic lifecycle tests; no packaged interpreter or networking.
use super::*;
use crate::{
    collection_jobs::{CollectionInput, CollectionProtocol, CollectionTicket},
    processing::{ProcessingFailure, ProcessingState},
    store::graph_analysis::probe_fixture::specimen,
};
use std::sync::{atomic::AtomicUsize, mpsc};
use std::time::Instant;

fn queue(w: &mut Workspace) -> ProcessingJob {
    w.queue_graph_path(
        w.revision().unwrap(),
        "a",
        "c",
        &uuid::Uuid::new_v4().to_string(),
    )
    .unwrap()
}
fn response(input: &[u8]) -> Vec<u8> {
    let mut value: Value = serde_json::from_slice(input).unwrap();
    for key in ["nodes", "edges", "source_id", "target_id"] {
        value.as_object_mut().unwrap().remove(key);
    }
    value["outcome"] = serde_json::json!({"state":"path","nodes":["a","b","c"]});
    serde_json::to_vec(&value).unwrap()
}
fn no_documents() -> Arc<Executor> {
    Arc::new(|_, _, _, _, _| panic!("Unexpected document invocation"))
}
fn synthetic() -> GraphExecution {
    GraphExecution::Synthetic(Arc::new(|input, _| Ok(response(input))))
}
fn input() -> CollectionInput {
    CollectionInput {
        urls: vec!["https://collection.invalid/start".into()],
        max_hops: 0,
        max_requests: 2,
        max_seconds: 60,
    }
}
fn manual(
    workspace: Workspace,
    executor: GraphExecution,
    protocol: Option<CollectionProtocol>,
) -> JobCoordinator {
    let ownership = Arc::new(workspace.collection_ownership().unwrap());
    let exports = workspace.start_native_exports().unwrap();
    JobCoordinator {
        exports,
        shared: Arc::new(Shared {
            workspace: Mutex::new(workspace),
            active: Mutex::new(ProcessingActivity::default()),
            stopping: AtomicBool::new(false),
            wake: Condvar::new(),
            graph_wake: Condvar::new(),
            executor: no_documents(),
            graph_executor: executor,
            ownership,
            collection: protocol.map(|p| {
                Arc::new(collection::CollectionLane::new(
                    Arc::new(|_, _, _, _, _| {
                        panic!("No collection execution in admission fixture")
                    }),
                    p,
                ))
            }),
        }),
        workers: Mutex::new(vec![]),
    }
}
fn admit_processing(c: &JobCoordinator) -> Admission {
    let mut w = c.shared.workspace.lock().unwrap();
    let mut a = c.shared.active.lock().unwrap();
    admit(&c.shared, &mut w, &mut a, Lane::Processing).unwrap()
}
fn take(c: &JobCoordinator) -> Pending {
    let mut w = c.shared.workspace.lock().unwrap();
    let mut a = c.shared.active.lock().unwrap();
    let Admission::Graph(job) = admit(&c.shared, &mut w, &mut a, Lane::Processing).unwrap() else {
        panic!("Graph not admitted")
    };
    claim(&c.shared, &mut w, &mut a, &job).unwrap().unwrap()
}
fn run(c: &JobCoordinator, pending: Pending) {
    let shared = c.shared.clone();
    c.workers
        .lock()
        .unwrap()
        .push(thread::spawn(move || execute_and_publish(&shared, pending)));
}
fn wait_activity(c: &JobCoordinator, ready: impl Fn(&ProcessingActivity) -> bool) {
    let until = Instant::now() + Duration::from_secs(10);
    let mut state = c.shared.active.lock().unwrap();
    while !ready(&state) {
        let remaining = until
            .checked_duration_since(Instant::now())
            .expect("Synthetic activity deadline");
        let (next, timeout) = c.shared.graph_wake.wait_timeout(state, remaining).unwrap();
        state = next;
        assert!(
            !timeout.timed_out() || ready(&state),
            "Synthetic activity deadline"
        );
    }
}
fn job(c: &JobCoordinator, id: &str) -> ProcessingJob {
    c.shared
        .workspace
        .lock()
        .unwrap()
        .processing_job(id)
        .unwrap()
}
fn sql(root: &tempfile::TempDir, source: &str) {
    rusqlite::Connection::open(root.path().join("case/workspace.db"))
        .unwrap()
        .execute_batch(source)
        .unwrap();
}
fn reject_finish(root: &tempfile::TempDir) {
    sql(root,"CREATE TRIGGER reject_graph_finish BEFORE UPDATE ON records WHEN OLD.kind='processing_job' AND json_extract(OLD.body,'$.state')='running' AND json_extract(NEW.body,'$.state')!='running' BEGIN SELECT RAISE(ABORT,'synthetic publication refusal'); END;");
}
fn pending(c: &JobCoordinator, count: u32) {
    wait_activity(c, |a| {
        a.publication_attempts == count
            && a.status()
                .is_some_and(|s| s.phase == GraphPhase::PublicationPending)
    });
}
fn retry(c: &JobCoordinator) {
    let status = c.graph_status().unwrap().unwrap();
    c.retry_graph_publication(
        &status.job_id,
        status.attempt,
        status.lease.as_deref().unwrap(),
        status.request_sha256.as_deref().unwrap(),
    )
    .unwrap();
}

#[test]
fn unavailable_blocks_before_drain_and_never_calls_executor() {
    let (_root, mut w, _) = specimen();
    let q = queue(&mut w);
    let c = manual(w, GraphExecution::Unavailable, None);
    c.shared.active.lock().unwrap().tokens.insert(
        "already-running-document".into(),
        CancellationToken::default(),
    );
    assert!(matches!(admit_processing(&c), Admission::Wait));
    assert!(c.graph_status().unwrap().is_none());
    let finished = job(&c, &q.id);
    assert_eq!(
        finished.failure,
        Some(ProcessingFailure::SchedulingOrRuntimeUnavailable)
    );
    assert!(finished.lease.is_none() && finished.started_at.is_none());
    assert!(!c.shared.active.lock().unwrap().tokens["already-running-document"].is_cancelled());
}

#[test]
fn two_real_coordinator_workers_serialize_graphs_without_self_invalidation() {
    let (_root, mut w, _) = specimen();
    let first = queue(&mut w);
    let second = queue(&mut w);
    let (sent, received) = mpsc::channel();
    let (release, gate) = mpsc::channel();
    let gate = Mutex::new(gate);
    let calls = Arc::new(AtomicUsize::new(0));
    let count = calls.clone();
    let c = JobCoordinator::with_graph_execution(
        w,
        2,
        no_documents(),
        None,
        GraphExecution::Synthetic(Arc::new(move |input, _| {
            let n = count.fetch_add(1, Ordering::SeqCst);
            sent.send(n).unwrap();
            if n == 0 {
                gate.lock()
                    .unwrap()
                    .recv_timeout(Duration::from_secs(10))
                    .unwrap();
            }
            Ok(response(input))
        })),
    )
    .unwrap();
    assert_eq!(received.recv_timeout(Duration::from_secs(10)).unwrap(), 0);
    assert_eq!(job(&c, &first.id).state, ProcessingState::Running);
    assert_eq!(job(&c, &second.id).state, ProcessingState::Queued);
    release.send(()).unwrap();
    assert_eq!(received.recv_timeout(Duration::from_secs(10)).unwrap(), 1);
    wait_activity(&c, |a| a.publication_attempts == 2 && a.status().is_none());
    for q in [first, second] {
        assert_eq!(job(&c, &q.id).state, ProcessingState::Completed);
    }
    assert_eq!(calls.load(Ordering::SeqCst), 2);
    c.shutdown().unwrap();
}

#[test]
fn drain_waits_for_registered_document_publication_and_cancel_does_not_stop_it() {
    let (_root, mut w, _) = specimen();
    let q = queue(&mut w);
    let c = manual(w, synthetic(), None);
    let document = CancellationToken::default();
    c.shared
        .active
        .lock()
        .unwrap()
        .tokens
        .insert("owned-document".into(), document.clone());
    assert!(matches!(admit_processing(&c), Admission::Wait));
    assert_eq!(
        c.graph_status().unwrap().unwrap().phase,
        GraphPhase::Draining
    );
    c.dispatch(Command::CancelProcessingJob {
        job_id: q.id.clone(),
        expected_attempt: 1,
    })
    .unwrap();
    assert!(matches!(admit_processing(&c), Admission::Open));
    assert!(!document.is_cancelled());
    assert_eq!(job(&c, &q.id).state, ProcessingState::Cancelled);
    assert!(c.graph_status().unwrap().is_none());
}

#[test]
fn graph_owned_blocks_all_automatic_claims_but_analyst_write_stales_result() {
    let (_root, mut w, source) = specimen();
    let q = queue(&mut w);
    let c = manual(w, synthetic(), None);
    let prepared = take(&c);
    let doc = {
        let mut w = c.shared.workspace.lock().unwrap();
        w.queue_document_parse(&source, &uuid::Uuid::new_v4().to_string())
            .unwrap()
    };
    let before = c.shared.workspace.lock().unwrap().revision().unwrap();
    assert!(matches!(admit_processing(&c), Admission::Wait));
    assert_eq!(
        c.shared.workspace.lock().unwrap().revision().unwrap(),
        before
    );
    assert_eq!(job(&c, &doc.id).state, ProcessingState::Queued);
    execute_and_publish(&c.shared, prepared);
    assert_eq!(
        job(&c, &q.id).failure,
        Some(ProcessingFailure::StaleGraphCapture)
    );
    assert!(c.graph_status().unwrap().is_none());
}

#[test]
fn exact_pending_retries_never_reexecute_and_success_releases_interval() {
    let (root, mut w, _) = specimen();
    let q = queue(&mut w);
    let calls = Arc::new(AtomicUsize::new(0));
    let count = calls.clone();
    let c = manual(
        w,
        GraphExecution::Synthetic(Arc::new(move |input, _| {
            count.fetch_add(1, Ordering::SeqCst);
            Ok(response(input))
        })),
        None,
    );
    let prepared = take(&c);
    let bytes = prepared.attempt.worker_input().to_vec();
    reject_finish(&root);
    run(&c, prepared);
    pending(&c, 1);
    let status = c.graph_status().unwrap().unwrap();
    assert!(c
        .retry_graph_publication(&q.id, 1, "other", status.request_sha256.as_deref().unwrap())
        .is_err());
    retry(&c);
    pending(&c, 2);
    sql(&root, "DROP TRIGGER reject_graph_finish");
    retry(&c);
    wait_activity(&c, |a| a.status().is_none());
    let finished = job(&c, &q.id);
    assert_eq!(finished.state, ProcessingState::Completed);
    let record = c
        .shared
        .workspace
        .lock()
        .unwrap()
        .graph_analysis_record(&finished.result_ids[0])
        .unwrap();
    assert_eq!(record.request_json.as_bytes(), bytes);
    assert_eq!(calls.load(Ordering::SeqCst), 1);
    c.shutdown().unwrap();
}

#[test]
fn exhausted_result_retries_allow_one_new_cancel_settlement() {
    let (root, mut w, _) = specimen();
    let q = queue(&mut w);
    let c = manual(w, synthetic(), None);
    let p = take(&c);
    reject_finish(&root);
    run(&c, p);
    pending(&c, 1);
    for count in 2..=4 {
        retry(&c);
        pending(&c, count);
    }
    let status = c.graph_status().unwrap().unwrap();
    assert!(!status.can_retry);
    assert!(c
        .retry_graph_publication(
            &q.id,
            1,
            status.lease.as_deref().unwrap(),
            status.request_sha256.as_deref().unwrap()
        )
        .is_err());
    sql(&root, "DROP TRIGGER reject_graph_finish");
    c.dispatch(Command::CancelProcessingJob {
        job_id: q.id.clone(),
        expected_attempt: 1,
    })
    .unwrap();
    wait_activity(&c, |a| a.status().is_none());
    assert_eq!(c.shared.active.lock().unwrap().publication_attempts, 5);
    assert_eq!(job(&c, &q.id).state, ProcessingState::Cancelled);
    c.shutdown().unwrap();
}

#[test]
fn pending_shutdown_has_exactly_one_final_settlement_and_retains_known_stopped_authority() {
    let (root, mut w, _) = specimen();
    let q = queue(&mut w);
    let c = manual(w, synthetic(), None);
    let p = take(&c);
    reject_finish(&root);
    run(&c, p);
    pending(&c, 1);
    for count in 2..=4 {
        retry(&c);
        pending(&c, count);
    }
    assert!(c.shutdown().is_err());
    let a = c.shared.active.lock().unwrap();
    assert_eq!(a.publication_attempts, 5);
    assert_eq!(
        a.status().unwrap().phase,
        GraphPhase::UnpublishedKnownStopped
    );
    assert!(a.retained.as_ref().unwrap().attempt.worker_input().len() > 100);
    assert_eq!(job(&c, &q.id).state, ProcessingState::Running);
    assert!(!c.shared.ownership.held());
}

#[test]
fn stop_observed_before_first_publication_gets_no_extra_final_retry() {
    let (root, mut w, _) = specimen();
    let _q = queue(&mut w);
    let c = manual(w, synthetic(), None);
    let p = take(&c);
    reject_finish(&root);
    c.shared.stopping.store(true, Ordering::Release);
    execute_and_publish(&c.shared, p);
    let a = c.shared.active.lock().unwrap();
    assert_eq!(a.publication_attempts, 1);
    assert_eq!(
        a.status().unwrap().phase,
        GraphPhase::UnpublishedKnownStopped
    );
}

#[test]
fn unknown_exit_quarantines_before_terminal_write_and_keeps_interval() {
    let (_root, mut w, _) = specimen();
    let q = queue(&mut w);
    let c = manual(
        w,
        GraphExecution::Synthetic(Arc::new(|_, _| {
            Err(Error::TerminationUnverified("synthetic".into()))
        })),
        None,
    );
    let p = take(&c);
    execute_and_publish(&c.shared, p);
    assert!(!c.shared.ownership.held());
    assert_eq!(
        job(&c, &q.id).failure,
        Some(ProcessingFailure::WorkerExitUnverified)
    );
    assert_eq!(
        c.graph_status().unwrap().unwrap().phase,
        GraphPhase::RecoveryRequired
    );
    assert!(c.shutdown().is_err());
}

#[test]
fn collection_fifo_and_non_draining_faults_are_resolved_before_graph_claim() {
    for faulted in [false, true] {
        let (_root, mut w, _) = specimen();
        w.queue_collection_protocol(
            input(),
            &uuid::Uuid::new_v4().to_string(),
            chrono::Utc::now().timestamp_millis(),
            CollectionProtocol::SyntheticV4,
        )
        .unwrap();
        let q = queue(&mut w);
        let c = manual(w, synthetic(), Some(CollectionProtocol::SyntheticV4));
        if faulted {
            c.shared.collection.as_ref().unwrap().scheduling_fixture(
                collection::LanePhase::Faulted,
                None,
                false,
            );
        }
        assert!(matches!(admit_processing(&c), Admission::Wait));
        assert!(c.graph_status().unwrap().is_none());
        assert_eq!(
            job(&c, &q.id).state,
            if faulted {
                ProcessingState::Blocked
            } else {
                ProcessingState::Queued
            }
        );
    }
}

#[test]
fn preexisting_admitted_ticket_is_frozen_and_only_that_ticket_may_enter_drain() {
    let (_root, mut w, _) = specimen();
    let q = queue(&mut w);
    let c = manual(w, synthetic(), Some(CollectionProtocol::SyntheticV4));
    let lane = c.shared.collection.as_ref().unwrap();
    let ticket = CollectionTicket {
        job_id: uuid::Uuid::new_v4().to_string(),
        generation: 1,
        lease: uuid::Uuid::new_v4().to_string(),
    };
    lane.scheduling_fixture(collection::LanePhase::Idle, Some(ticket.clone()), true);
    assert!(matches!(admit_processing(&c), Admission::Wait));
    assert!(!c.shared.active.lock().unwrap().allows_resume());
    {
        let mut w = c.shared.workspace.lock().unwrap();
        let mut a = c.shared.active.lock().unwrap();
        assert!(matches!(
            admit(&c.shared, &mut w, &mut a, Lane::Collection).unwrap(),
            Admission::Open
        ));
    }
    lane.scheduling_fixture(
        collection::LanePhase::SettlementPending,
        Some(ticket),
        false,
    );
    assert!(matches!(admit_processing(&c), Admission::Wait));
    lane.scheduling_fixture(collection::LanePhase::Idle, None, false);
    assert!(matches!(admit_processing(&c),Admission::Graph(job) if job.id==q.id));
}

#[test]
fn actual_collection_run_drains_all_requests_before_graph_capture() {
    use crate::collection_transport::tests::canonical_tls;
    let (_root, mut w, _) = specimen();
    let collection = w
        .queue_collection_protocol(
            input(),
            &uuid::Uuid::new_v4().to_string(),
            chrono::Utc::now().timestamp_millis(),
            CollectionProtocol::SyntheticV4,
        )
        .unwrap();
    let (started, receive) = mpsc::channel();
    let (release, gate) = mpsc::channel();
    let gate = Mutex::new(gate);
    let (graph_started, graph_receive) = mpsc::channel();
    let c=JobCoordinator::with_graph_execution(w,2,no_documents(),Some((Arc::new(move|ticket,input,window,pacing,token| {
        if ticket.sequence==0 {started.send(()).unwrap();gate.lock().unwrap().recv_timeout(Duration::from_secs(10)).unwrap();}
        assert!(!token.is_cancelled());
        let wire=if ticket.sequence==0 {b"HTTP/1.1 404 Not Found\r\nContent-Length: 0\r\n\r\n".to_vec()} else {b"HTTP/1.1 200 OK\r\nContent-Type: text/plain\r\nContent-Length: 9\r\n\r\nSynthetic".to_vec()};
        canonical_tls(ticket,input,window,pacing,token,wire,false)
    }),CollectionProtocol::SyntheticV4)),GraphExecution::Synthetic(Arc::new(move|input,_|{graph_started.send(()).unwrap();Ok(response(input))}))).unwrap();
    receive.recv_timeout(Duration::from_secs(10)).unwrap();
    let q = queue(&mut c.shared.workspace.lock().unwrap());
    c.shared.wake.notify_all();
    wait_activity(&c, |a| {
        a.status().is_some_and(|s| s.phase == GraphPhase::Draining)
    });
    assert!(graph_receive.try_recv().is_err());
    assert_eq!(job(&c, &q.id).state, ProcessingState::Queued);
    release.send(()).unwrap();
    graph_receive.recv_timeout(Duration::from_secs(10)).unwrap();
    wait_activity(&c, |a| a.publication_attempts == 1 && a.status().is_none());
    assert_eq!(job(&c, &q.id).state, ProcessingState::Completed);
    let finished = c
        .shared
        .workspace
        .lock()
        .unwrap()
        .inspect_durable_collection(&collection.id)
        .unwrap();
    assert_ne!(
        finished.checkpoint.state,
        crate::collection_jobs::CollectionState::Running
    );
    assert_eq!(finished.checkpoint.requests.len(), 2);
    c.shutdown().unwrap();
}

#[test]
fn public_and_internal_resume_are_no_write_refusals_during_drain_but_prepared_cancel_works() {
    use crate::collection_api::CollectionRunInspection;
    let (_root, mut w, _) = specimen();
    let owner = w.collection_ownership().unwrap();
    let interrupted = w
        .queue_collection_protocol(
            input(),
            &uuid::Uuid::new_v4().to_string(),
            chrono::Utc::now().timestamp_millis(),
            CollectionProtocol::SyntheticV4,
        )
        .unwrap();
    w.start_durable_collection(
        &interrupted.id,
        1,
        &owner,
        chrono::Utc::now().timestamp_millis(),
    )
    .unwrap()
    .unwrap();
    w.recover_collections(
        &owner,
        chrono::Utc::now().timestamp_millis(),
        Some(CollectionProtocol::SyntheticV4),
    )
    .unwrap();
    drop(owner);
    let q = queue(&mut w);
    let c = manual(w, synthetic(), Some(CollectionProtocol::SyntheticV4));
    let inspect = || -> CollectionRunInspection {
        serde_json::from_value(
            c.dispatch(Command::InspectCollectionRun {
                job_id: interrupted.id.clone(),
            })
            .unwrap(),
        )
        .unwrap()
    };
    assert!(inspect().controls.can_resume);
    c.shared
        .active
        .lock()
        .unwrap()
        .tokens
        .insert("existing-document".into(), CancellationToken::default());
    assert!(matches!(admit_processing(&c), Admission::Wait));
    let before = c.shared.workspace.lock().unwrap().revision().unwrap();
    let current = inspect();
    assert!(!current.controls.can_resume);
    assert!(c
        .dispatch(Command::ResumeCollection {
            job_id: interrupted.id.clone(),
            expected_generation: current.run.generation
        })
        .is_err());
    assert!(c
        .resume_collection(&interrupted.id, current.run.generation)
        .is_err());
    assert_eq!(
        c.shared.workspace.lock().unwrap().revision().unwrap(),
        before
    );
    let cancelled: CollectionRunInspection = serde_json::from_value(
        c.dispatch(Command::CancelCollection {
            job_id: interrupted.id.clone(),
            expected_generation: current.run.generation,
        })
        .unwrap(),
    )
    .unwrap();
    assert!(!cancelled.controls.can_resume);
    assert_eq!(job(&c, &q.id).state, ProcessingState::Queued);
    c.shutdown().unwrap();
    assert!(c.graph_status().unwrap().is_none());
}

#[test]
fn failed_atomic_capture_rolls_back_running_and_releases_after_blocked_commit() {
    let (root, mut w, _) = specimen();
    let q = queue(&mut w);
    let c = manual(w, synthetic(), None);
    // Accepted source points outside this fixed text-anchor recipe; no silent edge omission.
    sql(&root,"UPDATE records SET body=json_set(body,'$.anchor.evidence_id','missing-original') WHERE kind='observation' AND id='obs-r1'");
    let before = c.shared.workspace.lock().unwrap().revision().unwrap();
    let mut w = c.shared.workspace.lock().unwrap();
    let mut a = c.shared.active.lock().unwrap();
    let Admission::Graph(candidate) = admit(&c.shared, &mut w, &mut a, Lane::Processing).unwrap()
    else {
        panic!()
    };
    assert!(claim(&c.shared, &mut w, &mut a, &candidate)
        .unwrap()
        .is_none());
    let blocked = w.processing_job(&q.id).unwrap();
    assert_eq!(blocked.state, ProcessingState::Blocked);
    assert!(blocked.lease.is_none() && blocked.started_at.is_none());
    assert_eq!(w.revision().unwrap(), before + 1);
    assert!(a.status().is_none());
}

#[test]
fn capture_failure_followed_by_storage_failure_retains_unclaimed_drain_until_repaired() {
    let (root, mut w, _) = specimen();
    let q = queue(&mut w);
    let c = manual(w, synthetic(), None);
    sql(&root,"UPDATE records SET body=json_set(body,'$.anchor.evidence_id','missing-original') WHERE kind='observation' AND id='obs-r1'; CREATE TRIGGER reject_graph_block BEFORE UPDATE ON records WHEN OLD.kind='processing_job' AND json_extract(NEW.body,'$.state')='blocked' BEGIN SELECT RAISE(ABORT,'synthetic'); END;");
    let before = c.shared.workspace.lock().unwrap().revision().unwrap();
    {
        let mut w = c.shared.workspace.lock().unwrap();
        let mut a = c.shared.active.lock().unwrap();
        let Admission::Graph(candidate) =
            admit(&c.shared, &mut w, &mut a, Lane::Processing).unwrap()
        else {
            panic!()
        };
        assert!(claim(&c.shared, &mut w, &mut a, &candidate).is_err());
        assert_eq!(w.revision().unwrap(), before);
        assert_eq!(
            w.processing_job(&q.id).unwrap().state,
            ProcessingState::Queued
        );
        assert_eq!(a.status().unwrap().phase, GraphPhase::Draining);
    }
    sql(&root, "DROP TRIGGER reject_graph_block");
    let mut w = c.shared.workspace.lock().unwrap();
    let mut a = c.shared.active.lock().unwrap();
    let Admission::Graph(candidate) = admit(&c.shared, &mut w, &mut a, Lane::Processing).unwrap()
    else {
        panic!()
    };
    assert!(claim(&c.shared, &mut w, &mut a, &candidate)
        .unwrap()
        .is_none());
    assert_eq!(
        w.processing_job(&q.id).unwrap().state,
        ProcessingState::Blocked
    );
}

#[test]
fn retained_collection_settlement_keeps_drain_until_exact_publication_retry() {
    use crate::collection_transport::tests::canonical_tls;
    let (root, mut w, _) = specimen();
    let collection = w
        .queue_collection_protocol(
            input(),
            &uuid::Uuid::new_v4().to_string(),
            chrono::Utc::now().timestamp_millis(),
            CollectionProtocol::SyntheticV4,
        )
        .unwrap();
    sql(&root,"CREATE TRIGGER reject_collection_evidence BEFORE INSERT ON records WHEN NEW.kind='evidence' BEGIN SELECT RAISE(ABORT,'synthetic publication fault'); END;");
    let calls = Arc::new(AtomicUsize::new(0));
    let count = calls.clone();
    let c=JobCoordinator::with_graph_execution(w,2,no_documents(),Some((Arc::new(move|ticket,input,window,pacing,token|{
        count.fetch_add(1,Ordering::SeqCst);
        let wire=if ticket.sequence==0 {b"HTTP/1.1 404 Not Found\r\nContent-Length: 0\r\n\r\n".to_vec()}else{b"HTTP/1.1 200 OK\r\nContent-Type: text/plain\r\nContent-Length: 9\r\n\r\nSynthetic".to_vec()};
        canonical_tls(ticket,input,window,pacing,token,wire,false)
    }),CollectionProtocol::SyntheticV4)),synthetic()).unwrap();
    let lane = c.shared.collection.as_ref().unwrap();
    lane.wait_scheduling_phase(collection::LanePhase::SettlementPending);
    assert_eq!(calls.load(Ordering::SeqCst), 1);
    let q = queue(&mut c.shared.workspace.lock().unwrap());
    c.shared.wake.notify_all();
    wait_activity(&c, |a| {
        a.status().is_some_and(|s| s.phase == GraphPhase::Draining)
    });
    assert_eq!(job(&c, &q.id).state, ProcessingState::Queued);
    assert_eq!(c.shared.active.lock().unwrap().publication_attempts, 0);
    sql(&root, "DROP TRIGGER reject_collection_evidence");
    c.retry_collection_settlement(&collection.id, 1, 0).unwrap();
    wait_activity(&c, |a| a.publication_attempts == 1 && a.status().is_none());
    assert_eq!(calls.load(Ordering::SeqCst), 2);
    assert_eq!(job(&c, &q.id).state, ProcessingState::Completed);
    c.shutdown().unwrap();
}

#[test]
fn cancellation_before_initial_publication_consumes_its_single_settlement() {
    let (root, mut w, _) = specimen();
    let q = queue(&mut w);
    let c = manual(w, synthetic(), None);
    let p = take(&c);
    c.dispatch(Command::CancelProcessingJob {
        job_id: q.id.clone(),
        expected_attempt: 1,
    })
    .unwrap();
    reject_finish(&root);
    run(&c, p);
    pending(&c, 1);
    {
        let a = c.shared.active.lock().unwrap();
        let Interval::Owned(owned) = &a.interval else {
            panic!()
        };
        assert!(owned.cancel_settlement_used);
        assert_eq!(a.publication_attempts, 1);
    }
    assert!(c.shutdown().is_err());
    assert_eq!(c.shared.active.lock().unwrap().publication_attempts, 2);
}

#[test]
fn standalone_interruption_remains_typed() {
    let (_root, mut w, _) = specimen();
    let q = queue(&mut w);
    let c = manual(
        w,
        GraphExecution::Synthetic(Arc::new(|_, _| Err(Error::Interrupted("synthetic".into())))),
        None,
    );
    let p = take(&c);
    execute_and_publish(&c.shared, p);
    assert_eq!(job(&c, &q.id).failure, Some(ProcessingFailure::Interrupted));
}

#[test]
fn actual_cleanup_failure_takes_precedence_over_inflight_cancellation() {
    let (_root, mut w, _) = specimen();
    let q = queue(&mut w);
    let (started, ready) = mpsc::channel();
    let (release, wait) = mpsc::channel();
    let wait = Mutex::new(wait);
    let c = manual(
        w,
        GraphExecution::Synthetic(Arc::new(move |_, token| {
            started.send(()).unwrap();
            wait.lock()
                .unwrap()
                .recv_timeout(Duration::from_secs(10))
                .unwrap();
            assert!(token.is_cancelled());
            Err(Error::Cleanup("synthetic".into()))
        })),
        None,
    );
    let p = take(&c);
    run(&c, p);
    ready.recv_timeout(Duration::from_secs(10)).unwrap();
    c.dispatch(Command::CancelProcessingJob {
        job_id: q.id.clone(),
        expected_attempt: 1,
    })
    .unwrap();
    release.send(()).unwrap();
    wait_activity(&c, |a| a.status().is_none());
    let finished = job(&c, &q.id);
    assert_eq!(finished.state, ProcessingState::Cancelled);
    assert_eq!(finished.failure, Some(ProcessingFailure::CleanupFailed));
    c.shutdown().unwrap();
}

#[path = "coordinator_graph_api_tests.rs"]
mod public_api;
