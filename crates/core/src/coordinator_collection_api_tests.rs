use super::*;
use crate::{
    collection_jobs::RequestProgress,
    collection_transport::{
        tests::canonical_tls, CallerContextState, Outcome, Phase, ResolverUncertainty, StopReason,
    },
};
use std::sync::atomic::AtomicUsize;
fn fixture() -> (tempfile::TempDir, Workspace) {
    let temp = tempfile::tempdir_in(std::env::temp_dir().canonicalize().unwrap()).unwrap();
    let workspace = Workspace::open(temp.path().join("case")).unwrap();
    (temp, workspace)
}
fn input() -> CollectionInput {
    CollectionInput {
        urls: vec!["https://collection.invalid/start".into()],
        max_hops: 2,
        max_requests: 8,
        max_seconds: 60,
    }
}
fn key() -> String {
    uuid::Uuid::new_v4().to_string()
}
fn no_documents() -> Arc<Executor> {
    Arc::new(|_, _, _, _, _| panic!("No document in this synthetic fixture"))
}
fn until(mut ready: impl FnMut() -> bool) {
    let deadline = Instant::now() + Duration::from_secs(15);
    while !ready() {
        assert!(Instant::now() < deadline, "Synthetic deadline exceeded");
        thread::sleep(Duration::from_millis(5));
    }
}
fn inspect(c: &JobCoordinator, id: &str) -> CollectionRunInspection {
    serde_json::from_value(
        c.dispatch(Command::InspectCollectionRun { job_id: id.into() })
            .unwrap(),
    )
    .unwrap()
}
fn queue(c: &JobCoordinator, k: &str) -> CollectionRunInspection {
    let preview: CollectionPreview = serde_json::from_value(
        c.dispatch(Command::PreviewCollection { input: input() })
            .unwrap(),
    )
    .unwrap();
    serde_json::from_value(
        c.dispatch(Command::QueueCollection {
            input: input(),
            preview_sha256: preview.preview_sha256,
            request_key: k.into(),
        })
        .unwrap(),
    )
    .unwrap()
}
fn response(sequence: u32) -> Vec<u8> {
    if sequence == 0 {
        b"HTTP/1.1 404 Not Found\r\nContent-Length: 0\r\n\r\n".to_vec()
    } else {
        b"HTTP/1.1 200 OK\r\nContent-Type: text/plain\r\nContent-Length: 9\r\n\r\nSynthetic"
            .to_vec()
    }
}
fn sql(temp: &tempfile::TempDir, sql: &str) {
    rusqlite::Connection::open(temp.path().join("case/workspace.db"))
        .unwrap()
        .execute_batch(sql)
        .unwrap();
}
#[test]
fn normal_coordinator_exposes_disabled_catalogue_and_blocks_old_and_new_execution_without_mutation()
{
    let (_temp, mut w) = fixture();
    let legacy = w
        .queue_collection_transport(input(), &key(), now())
        .unwrap();
    let revision = w.revision().unwrap();
    let c = JobCoordinator::start(w, 1).unwrap();
    let page: CollectionRunPage = serde_json::from_value(
        c.dispatch(Command::PageCollectionRuns {
            request: CollectionRunPageRequest {
                page_size: 25,
                cursor: None,
            },
            expected_revision: None,
        })
        .unwrap(),
    )
    .unwrap();
    assert_eq!(page.availability, CollectionAvailability::NativeDisabled);
    assert!(!page.native_execution_enabled);
    assert_eq!(page.scope_count, 1);
    let old = inspect(&c, &legacy.id);
    assert!(!old.controls.can_resume && !old.controls.can_cancel);
    let p = preview(input()).unwrap();
    assert!(c
        .dispatch(Command::QueueCollection {
            input: input(),
            preview_sha256: p.preview_sha256,
            request_key: key()
        })
        .is_err());
    assert!(c
        .dispatch(Command::CollectWeb {
            urls: input().urls,
            max_hops: 2,
            max_requests: 8,
            max_seconds: 60
        })
        .is_err());
    assert_eq!(
        c.shared.workspace.lock().unwrap().revision().unwrap(),
        revision
    );
    c.shutdown().unwrap();
}
#[test]
fn public_v3_queue_owned_tls_receipts_idempotency_and_backup_preserve_originals() {
    let (temp, w) = fixture();
    let calls = Arc::new(AtomicUsize::new(0));
    let count = calls.clone();
    let c = JobCoordinator::with_public_collection_executor(
        w,
        no_documents(),
        Arc::new(move |ticket, input, window, pacing, token| {
            count.fetch_add(1, Ordering::AcqRel);
            canonical_tls(
                ticket,
                input,
                window,
                pacing,
                token,
                response(ticket.sequence),
                false,
            )
        }),
    )
    .unwrap();
    let request_key = key();
    let first = queue(&c, &request_key);
    let again = queue(&c, &request_key);
    assert_eq!(first.run.id, again.run.id);
    assert_eq!(first.run.record_version, 4);
    assert_eq!(first.availability, CollectionAvailability::SyntheticFixture);
    assert!(!first.native_execution_enabled);
    until(|| inspect(&c, &first.run.id).run.state == CollectionState::Successful);
    let done = inspect(&c, &first.run.id);
    assert_eq!(done.run.requests_used, 2);
    assert_eq!(done.run.pages_retained, 1);
    assert_eq!(calls.load(Ordering::Acquire), 2);
    assert!(done
        .requests
        .iter()
        .all(|r| r.original.is_some() && matches!(r.progress, RequestProgress::Observed { .. })));
    let serialized = serde_json::to_string(&done).unwrap();
    assert!(!serialized.contains("lease"));
    assert!(!serialized.contains("Synthetic</") && !serialized.contains("response_body"));
    let mut workspace = c.shared.workspace.lock().unwrap();
    assert_eq!(workspace.view().unwrap().observations.len(), 0);
    assert_eq!(
        workspace
            .view()
            .unwrap()
            .evidence
            .iter()
            .filter(|e| e.text.as_deref() == Some("Synthetic"))
            .count(),
        1
    );
    let backup = workspace.backup().unwrap();
    drop(workspace);
    let restored = Workspace::restore(&backup, &temp.path().join("restored")).unwrap();
    let retained = restored.inspect_collection_run(&first.run.id).unwrap();
    assert_eq!(retained.requests, done.requests);
    assert_eq!(retained.run, done.run);
    assert!(!retained.controls.can_resume);
    // A valid synthetic journal cannot be relabelled as native provenance.
    sql(&temp, &format!("UPDATE records SET body=json_set(body,'$.synthetic',json('false')) WHERE kind='collection_run' AND id='{}'", first.run.id));
    assert!(c
        .dispatch(Command::InspectCollectionRun {
            job_id: first.run.id.clone()
        })
        .is_err());
    c.shutdown().unwrap();
}
#[test]
fn public_cancel_retains_complete_body_without_promotion_and_stale_generation_fails() {
    let (_temp, w) = fixture();
    let entered = Arc::new(AtomicBool::new(false));
    let marker = entered.clone();
    let c = JobCoordinator::with_public_collection_executor(
        w,
        no_documents(),
        Arc::new(move |ticket, input, window, pacing, token| {
            if ticket.sequence == 1 {
                marker.store(true, Ordering::Release);
                until(|| token.is_cancelled());
            }
            // A completed response can race the cancellation acknowledgement. Its exact bytes survive.
            if ticket.sequence == 1 {
                Observation {
                    outcome: Outcome::Complete {
                        head: crate::collection_transport::ResponseHead {
                            status: 200,
                            media_type: Some("text/plain".into()),
                            redirect_url: None,
                            identity_encoding: true,
                        },
                        body: b"Synthetic completion race".to_vec(),
                    },
                    phase: Phase::Body,
                    elapsed_milliseconds: 1,
                    observed_wall_ms: now(),
                    resolved: Some(crate::collection_transport::ResolvedCandidates {
                        addresses: vec!["1.1.1.1:443".parse().unwrap()],
                        method: "synthetic_fixed_candidates",
                        authoritative_complete_set: false,
                    }),
                    resolver_uncertainty: None,
                    stop_observed: None,
                    locally_quiescent: true,
                }
            } else {
                canonical_tls(
                    ticket,
                    input,
                    window,
                    pacing,
                    token,
                    response(ticket.sequence),
                    false,
                )
            }
        }),
    )
    .unwrap();
    let first = queue(&c, &key());
    until(|| entered.load(Ordering::Acquire));
    assert!(inspect(&c, &first.run.id).controls.can_cancel);
    assert!(c
        .dispatch(Command::CancelCollection {
            job_id: first.run.id.clone(),
            expected_generation: 2
        })
        .is_err());
    c.dispatch(Command::CancelCollection {
        job_id: first.run.id.clone(),
        expected_generation: 1,
    })
    .unwrap();
    until(|| inspect(&c, &first.run.id).run.state == CollectionState::Cancelled);
    let done = inspect(&c, &first.run.id);
    assert_eq!(done.run.requests_used, 2);
    assert_eq!(done.run.pages_retained, 0);
    assert!(done.requests[1].original.is_some());
    assert!(c
        .shared
        .workspace
        .lock()
        .unwrap()
        .view()
        .unwrap()
        .evidence
        .iter()
        .all(|e| e.text.is_none()));
    c.shutdown().unwrap();
}
#[test]
fn queued_cancel_is_uncharged_and_historical_v2_is_never_admitted_to_public_lane() {
    let (_temp, mut w) = fixture();
    let old = w
        .queue_collection_transport(input(), &key(), now())
        .unwrap();
    let raw = serde_json::to_vec(&old).unwrap();
    let entered = Arc::new(AtomicBool::new(false));
    let marker = entered.clone();
    let c = JobCoordinator::with_public_collection_executor(
        w,
        no_documents(),
        Arc::new(move |_, _, _, _, token| {
            marker.store(true, Ordering::Release);
            until(|| token.is_cancelled());
            Observation {
                outcome: Outcome::Stopped {
                    reason: StopReason::Cancelled,
                    head: None,
                },
                phase: Phase::Dns,
                elapsed_milliseconds: 1,
                observed_wall_ms: now(),
                resolved: None,
                resolver_uncertainty: None,
                stop_observed: Some(StopReason::Cancelled),
                locally_quiescent: true,
            }
        }),
    )
    .unwrap();
    let first = queue(&c, &key());
    until(|| entered.load(Ordering::Acquire));
    let second = queue(&c, &key());
    let cancelled: CollectionRunInspection = serde_json::from_value(
        c.dispatch(Command::CancelCollection {
            job_id: second.run.id.clone(),
            expected_generation: 1,
        })
        .unwrap(),
    )
    .unwrap();
    assert_eq!(cancelled.run.state, CollectionState::Cancelled);
    assert_eq!(cancelled.run.requests_used, 0);
    assert!(c
        .dispatch(Command::CancelCollection {
            job_id: old.id.clone(),
            expected_generation: 1
        })
        .is_err());
    assert_eq!(
        serde_json::to_vec(
            &c.shared
                .workspace
                .lock()
                .unwrap()
                .inspect_durable_collection(&old.id)
                .unwrap()
        )
        .unwrap(),
        raw
    );
    c.dispatch(Command::CancelCollection {
        job_id: first.run.id.clone(),
        expected_generation: 1,
    })
    .unwrap();
    until(|| inspect(&c, &first.run.id).run.state == CollectionState::Cancelled);
    c.shutdown().unwrap();
}
#[test]
fn crash_recovery_retains_charge_and_fixed_deadline_and_requires_explicit_generation_resume() {
    let (_temp, mut w) = fixture();
    let owner = w.collection_ownership().unwrap();
    let job = w
        .queue_collection_protocol(input(), &key(), now(), CollectionProtocol::SyntheticV4)
        .unwrap();
    let ticket = w
        .start_durable_collection(&job.id, 1, &owner, now())
        .unwrap()
        .unwrap();
    w.advance_durable_collection(&ticket, &owner, now())
        .unwrap()
        .unwrap();
    let before = w.inspect_durable_collection(&job.id).unwrap();
    drop(owner);
    let c = JobCoordinator::with_public_collection_executor(
        w,
        no_documents(),
        Arc::new(|_, _, _, _, _| panic!("Unknown robots must never be retried")),
    )
    .unwrap();
    let interrupted = inspect(&c, &job.id);
    assert_eq!(interrupted.run.state, CollectionState::Interrupted);
    assert!(interrupted.controls.can_resume);
    assert_eq!(interrupted.run.requests_used, 1);
    assert!(matches!(
        interrupted.requests[0].progress,
        RequestProgress::InterruptedUnknown { .. }
    ));
    let value = c
        .dispatch(Command::ResumeCollection {
            job_id: job.id.clone(),
            expected_generation: 1,
        })
        .unwrap();
    let resumed: CollectionRunInspection = serde_json::from_value(value).unwrap();
    assert_eq!(resumed.run.generation, 2);
    assert_eq!(resumed.run.deadline_at_ms, before.checkpoint.deadline_at_ms);
    until(|| inspect(&c, &job.id).run.state != CollectionState::Running);
    assert_eq!(inspect(&c, &job.id).run.requests_used, 1);
    c.shutdown().unwrap();
}
#[test]
fn quarantined_pending_receipt_exposes_only_exact_publication_retry_and_no_new_execution() {
    let (temp, w) = fixture();
    sql(&temp,"CREATE TRIGGER fail_receipt BEFORE UPDATE ON records WHEN NEW.kind='collection_run' AND json_extract(NEW.body,'$.events[#-1].event')='transport_observed' BEGIN SELECT RAISE(ABORT,'synthetic fault'); END;");
    let calls = Arc::new(AtomicUsize::new(0));
    let count = calls.clone();
    let c = JobCoordinator::with_public_collection_executor(
        w,
        no_documents(),
        Arc::new(move |_, _, _, _, _| {
            count.fetch_add(1, Ordering::AcqRel);
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
                    caller_context: CallerContextState::ReleasedAfterCompletion,
                }),
                stop_observed: None,
                locally_quiescent: false,
            }
        }),
    )
    .unwrap();
    let request_key = key();
    let job = queue(&c, &request_key);
    until(|| {
        inspect(&c, &job.run.id).execution.phase == CollectionExecutionPhase::SettlementPending
    });
    let pending = inspect(&c, &job.run.id);
    assert_eq!(
        pending.availability,
        CollectionAvailability::RecoveryRequired
    );
    assert!(pending.controls.can_retry_settlement);
    assert!(!pending.controls.can_cancel && !pending.controls.can_resume);
    assert_eq!(pending.run.requests_used, 1);
    assert!(matches!(
        pending.requests[0].progress,
        RequestProgress::Reserved
    ));
    let revision = pending.workspace_revision;
    let recovered_ack = queue(&c, &request_key);
    assert_eq!(recovered_ack.run.id, job.run.id);
    assert_eq!(recovered_ack.workspace_revision, revision);
    assert_eq!(recovered_ack.run.requests_used, 1);
    assert_eq!(calls.load(Ordering::Acquire), 1);
    for changed in [
        CollectionInput {
            max_requests: 7,
            ..input()
        },
        CollectionInput {
            urls: vec!["https://other.invalid/changed".into()],
            ..input()
        },
    ] {
        let changed_preview = preview(changed.clone()).unwrap();
        assert!(c
            .dispatch(Command::QueueCollection {
                input: changed,
                preview_sha256: changed_preview.preview_sha256,
                request_key: request_key.clone(),
            })
            .is_err());
    }
    assert_eq!(inspect(&c, &job.run.id).workspace_revision, revision);
    let p = preview(input()).unwrap();
    assert!(c
        .dispatch(Command::QueueCollection {
            input: input(),
            preview_sha256: p.preview_sha256,
            request_key: key()
        })
        .is_err());
    assert!(c
        .dispatch(Command::RetryCollectionSettlement {
            job_id: job.run.id.clone(),
            expected_generation: 1,
            request_sequence: 1
        })
        .is_err());
    sql(&temp, "DROP TRIGGER fail_receipt;");
    c.dispatch(Command::RetryCollectionSettlement {
        job_id: job.run.id.clone(),
        expected_generation: 1,
        request_sequence: 0,
    })
    .unwrap();
    until(|| inspect(&c, &job.run.id).run.state == CollectionState::RecoveryRequired);
    let finished = inspect(&c, &job.run.id);
    assert!(!finished.controls.can_retry_settlement);
    assert_eq!(calls.load(Ordering::Acquire), 1);
    assert_eq!(finished.run.requests_used, 1);
    assert!(matches!(
        finished.requests[0].progress,
        RequestProgress::Observed { .. }
    ));
    assert!(c.shutdown().is_err());
    assert!(c.shutdown().is_err());
}

#[test]
fn contained_lane_failure_refuses_new_work_instead_of_admitting_stranded_queue() {
    let (temp, mut workspace) = fixture();
    let job = workspace
        .queue_collection_protocol(input(), &key(), now(), CollectionProtocol::SyntheticV4)
        .unwrap();
    sql(&temp, &format!("UPDATE records SET body=json_set(body,'$.collector_policy','invalid') WHERE kind='collection_run' AND id='{}'", job.id));
    let coordinator = JobCoordinator::with_public_collection_executor(
        workspace,
        no_documents(),
        Arc::new(|_, _, _, _, _| panic!("A malformed claim cannot execute")),
    )
    .unwrap();
    until(|| coordinator.collection_status().unwrap().phase == LanePhase::Faulted);
    assert!(coordinator.shared.ownership.held());
    let before = coordinator
        .shared
        .workspace
        .lock()
        .unwrap()
        .revision()
        .unwrap();
    let p = preview(input()).unwrap();
    let outcome = coordinator.dispatch(Command::QueueCollection {
        input: input(),
        preview_sha256: p.preview_sha256,
        request_key: key(),
    });
    assert!(matches!(outcome, Err(Error::Blocked(_))));
    assert_eq!(
        coordinator
            .shared
            .workspace
            .lock()
            .unwrap()
            .revision()
            .unwrap(),
        before
    );
    let lane = coordinator.shared.collection.as_deref().unwrap();
    let workspace = coordinator.shared.workspace.lock().unwrap();
    let state = lane.state.lock().unwrap();
    assert_eq!(
        availability(&coordinator.shared, &workspace, Some(lane), Some(&state)).unwrap(),
        CollectionAvailability::ExecutionUnavailable
    );
    drop(state);
    drop(workspace);
    coordinator.shutdown().unwrap();
}

#[test]
fn v4_interpretation_releases_workspace_for_canonical_cancel_and_preserves_complete_body() {
    let (temp, w) = fixture();
    let calls = Arc::new(AtomicUsize::new(0));
    let counter = calls.clone();
    let c = JobCoordinator::with_public_collection_executor(
        w,
        no_documents(),
        Arc::new(move |ticket, input, window, pacing, token| {
            counter.fetch_add(1, Ordering::SeqCst);
            let reply = if ticket.sequence == 0 {
                response(0)
            } else {
                let body = b"<p>Unreviewed prepared HTML</p><a href='/must-not-fetch'>next</a>";
                let mut reply = format!(
                    "HTTP/1.1 200 OK\r\nContent-Type: text/html\r\nContent-Length: {}\r\n\r\n",
                    body.len()
                )
                .into_bytes();
                reply.extend_from_slice(body);
                reply
            };
            canonical_tls(ticket, input, window, pacing, token, reply, false)
        }),
    )
    .unwrap();
    let (entered_tx, entered_rx) = std::sync::mpsc::channel();
    let (release_tx, release_rx) = std::sync::mpsc::channel();
    let release_rx = Mutex::new(release_rx);
    *c.shared
        .collection
        .as_ref()
        .unwrap()
        .preparation_hook
        .lock()
        .unwrap() = Some(Arc::new(move |request| {
        if request.sequence == 1 {
            entered_tx.send(()).unwrap();
            release_rx
                .lock()
                .unwrap()
                .recv_timeout(Duration::from_secs(10))
                .unwrap();
        }
    }));
    let run = queue(&c, &key());
    entered_rx.recv_timeout(Duration::from_secs(10)).unwrap();
    // A real command can acquire the mutex and COMMIT while the parser lane is
    // paused. No SQLite read transaction from capture blocks that cancellation.
    assert!(c.shared.workspace.try_lock().is_ok());
    let cancelled: CollectionRunInspection = serde_json::from_value(
        c.dispatch(Command::CancelCollection {
            job_id: run.run.id.clone(),
            expected_generation: 1,
        })
        .unwrap(),
    )
    .unwrap();
    assert!(cancelled.run.cancellation_requested);
    assert_eq!(cancelled.run.requests_used, 2);
    assert_eq!(cancelled.run.state, CollectionState::Running);
    release_tx.send(()).unwrap();
    until(|| inspect(&c, &run.run.id).run.state == CollectionState::Cancelled);
    let done = inspect(&c, &run.run.id);
    assert_eq!(done.run.pages_retained, 0);
    assert_eq!(calls.load(Ordering::SeqCst), 2);
    assert!(done.requests[1].original.is_some());
    assert!(c
        .shared
        .workspace
        .lock()
        .unwrap()
        .view()
        .unwrap()
        .evidence
        .iter()
        .all(|source| source.text.is_none()));
    c.shutdown().unwrap();
    drop(c);
    let reopened = Workspace::open(temp.path().join("case")).unwrap();
    let after = reopened.inspect_collection_run(&done.run.id).unwrap();
    assert_eq!(after.run, done.run);
    assert_eq!(after.requests, done.requests);
}

#[test]
fn v4_shutdown_joins_paused_preparation_before_releasing_owner_and_retains_exact_body() {
    let (temp, w) = fixture();
    let calls = Arc::new(AtomicUsize::new(0));
    let counter = calls.clone();
    let c = Arc::new(
        JobCoordinator::with_public_collection_executor(
            w,
            no_documents(),
            Arc::new(move |ticket, input, window, pacing, token| {
                counter.fetch_add(1, Ordering::SeqCst);
                canonical_tls(
                    ticket,
                    input,
                    window,
                    pacing,
                    token,
                    response(ticket.sequence),
                    false,
                )
            }),
        )
        .unwrap(),
    );
    let (entered_tx, entered_rx) = std::sync::mpsc::channel();
    let (release_tx, release_rx) = std::sync::mpsc::channel();
    let release_rx = Mutex::new(release_rx);
    *c.shared
        .collection
        .as_ref()
        .unwrap()
        .preparation_hook
        .lock()
        .unwrap() = Some(Arc::new(move |request| {
        if request.sequence == 1 {
            entered_tx.send(()).unwrap();
            release_rx
                .lock()
                .unwrap()
                .recv_timeout(Duration::from_secs(10))
                .unwrap();
        }
    }));
    let run = queue(&c, &key());
    entered_rx.recv_timeout(Duration::from_secs(10)).unwrap();
    let stopping = c.clone();
    let (finished_tx, finished_rx) = std::sync::mpsc::channel();
    let joined = thread::spawn(move || {
        finished_tx.send(stopping.shutdown()).unwrap();
    });
    until(|| c.shared.stopping.load(Ordering::Acquire));
    assert!(c.shared.ownership.publication_held());
    assert!(matches!(
        finished_rx.try_recv(),
        Err(std::sync::mpsc::TryRecvError::Empty)
    ));
    let reopened = Workspace::open(temp.path().join("case")).unwrap();
    assert!(reopened.collection_ownership().is_err());
    release_tx.send(()).unwrap();
    finished_rx
        .recv_timeout(Duration::from_secs(10))
        .unwrap()
        .unwrap();
    joined.join().unwrap();
    assert!(!c.shared.ownership.publication_held());
    c.shutdown().unwrap();
    let done = reopened.inspect_durable_collection(&run.run.id).unwrap();
    assert_eq!(done.checkpoint.state, CollectionState::Cancelled);
    assert_eq!(done.checkpoint.requests_used(), 2);
    assert_eq!(done.checkpoint.pages_retained, 0);
    assert_eq!(calls.load(Ordering::SeqCst), 2);
    let source = reopened
        .view()
        .unwrap()
        .evidence
        .into_iter()
        .find(|e| e.bytes == 9)
        .unwrap();
    assert!(source.text.is_none());
    assert_eq!(
        std::fs::read(temp.path().join("case/originals").join(&source.sha256)).unwrap(),
        b"Synthetic"
    );
    let next = reopened.collection_ownership().unwrap();
    next.release().unwrap();
}
