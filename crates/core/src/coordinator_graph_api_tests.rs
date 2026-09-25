//! Public dispatch integration with the existing closed synthetic scheduler fixture.
use super::*;
use crate::graph_api::{
    GraphAvailability, GraphExecutionPhase, GraphJobInspection, GraphJobPageRequest,
};

fn inspect(c: &JobCoordinator, id: &str) -> GraphJobInspection {
    serde_json::from_value(
        c.dispatch(Command::InspectGraphJob { job_id: id.into() })
            .unwrap(),
    )
    .unwrap()
}
fn queued_command(revision: u64, key: &str) -> Command {
    Command::QueueGraphPath {
        expected_revision: revision,
        source_id: "a".into(),
        target_id: "c".into(),
        request_key: key.into(),
    }
}
#[test]
fn unavailable_queue_ack_is_durable_and_all_dispatch_modes_preserve_direct_identity() {
    let (_root, w, _) = specimen();
    let revision = w.revision().unwrap();
    let c = manual(w, GraphExecution::Unavailable, None);
    let key = uuid::Uuid::new_v4().to_string();
    let first = c.dispatch(queued_command(revision, &key)).unwrap();
    assert_eq!(
        first,
        c.dispatch_presentation(queued_command(revision, &key))
            .unwrap()
    );
    assert_eq!(
        first,
        c.dispatch_summary(queued_command(revision, &key)).unwrap()
    );
    let q: GraphJobInspection = serde_json::from_value(first).unwrap();
    assert_eq!(q.availability, GraphAvailability::RuntimeUnavailable);
    assert_eq!(q.job.state, ProcessingState::Queued);
    assert!(q.execution.is_none());
    assert!(c
        .dispatch(Command::QueueGraphPath {
            expected_revision: revision,
            source_id: "a".into(),
            target_id: "b".into(),
            request_key: key.clone()
        })
        .is_err());
    assert!(c
        .dispatch_summary(queued_command(revision + 1, &key))
        .is_err());
    assert_eq!(inspect(&c, &q.job.id).workspace_revision, revision + 1);
    for dispatch in [
        JobCoordinator::dispatch,
        JobCoordinator::dispatch_presentation,
        JobCoordinator::dispatch_summary,
    ] {
        let read = dispatch(
            &c,
            Command::InspectGraphJob {
                job_id: q.job.id.clone(),
            },
        )
        .unwrap();
        assert_eq!(read, serde_json::to_value(&q).unwrap());
        let page = dispatch(
            &c,
            Command::PageGraphJobs {
                request: GraphJobPageRequest {
                    page_size: 25,
                    cursor: None,
                },
                expected_revision: Some(revision + 1),
            },
        )
        .unwrap();
        assert_eq!(
            page["rows"][0]["job"],
            serde_json::to_value(&q.job).unwrap()
        );
        assert_eq!(page["workspace_revision"], revision + 1);
        assert!(page.get("workspace").is_none());
    }
    assert!(matches!(admit_processing(&c), Admission::Wait));
    let blocked = inspect(&c, &q.job.id);
    assert_eq!(blocked.job.state, ProcessingState::Blocked);
    assert_eq!(
        blocked.job.failure,
        Some(ProcessingFailure::SchedulingOrRuntimeUnavailable)
    );
    assert!(blocked.job.started_at.is_none() && blocked.job.lease.is_none());
    let replay: GraphJobInspection =
        serde_json::from_value(c.dispatch(queued_command(revision, &key)).unwrap()).unwrap();
    assert_eq!(replay.job.id, q.job.id);
    assert_eq!(replay.workspace_revision, blocked.workspace_revision);
    let page = c
        .dispatch(Command::PageGraphJobs {
            request: GraphJobPageRequest {
                page_size: 25,
                cursor: None,
            },
            expected_revision: Some(blocked.workspace_revision),
        })
        .unwrap();
    assert_eq!(page["total_count"], 1);
    assert_eq!(page["availability"], "runtime_unavailable");
    assert!(page["rows"][0].get("frozen").is_none());
    c.shutdown().unwrap();
}
#[test]
fn exact_public_retry_retains_one_invocation_and_rejects_changed_bindings() {
    let (root, mut w, _) = specimen();
    let q = queue(&mut w);
    let calls = Arc::new(AtomicUsize::new(0));
    let count = calls.clone();
    let c = manual(
        w,
        GraphExecution::Synthetic(Arc::new(move |bytes, _| {
            count.fetch_add(1, Ordering::SeqCst);
            Ok(response(bytes))
        })),
        None,
    );
    let p = take(&c);
    reject_finish(&root);
    run(&c, p);
    pending(&c, 1);
    let pending_status = inspect(&c, &q.id);
    assert_eq!(
        pending_status.availability,
        GraphAvailability::SyntheticFixture
    );
    assert!(pending_status.controls.can_retry_publication);
    let execution = pending_status.execution.unwrap();
    assert_eq!(execution.phase, GraphExecutionPhase::PublicationPending);
    let retry_command = |attempt, lease: String, sha: String| Command::RetryGraphPublication {
        job_id: q.id.clone(),
        expected_attempt: attempt,
        host_attempt_lease: lease,
        request_sha256: sha,
    };
    let lease = execution.host_attempt_lease.unwrap();
    let sha = execution.request_sha256.unwrap();
    for command in [
        retry_command(2, lease.clone(), sha.clone()),
        retry_command(1, uuid::Uuid::new_v4().to_string(), sha.clone()),
        retry_command(1, lease.clone(), "0".repeat(64)),
    ] {
        assert!(c.dispatch(command).is_err());
    }
    assert_eq!(c.shared.active.lock().unwrap().publication_attempts, 1);
    sql(&root, "DROP TRIGGER reject_graph_finish");
    let ack: GraphJobInspection =
        serde_json::from_value(c.dispatch_summary(retry_command(1, lease, sha)).unwrap()).unwrap();
    assert!(!ack.controls.can_retry_publication);
    assert_eq!(ack.execution.unwrap().publication_retries, 1);
    wait_activity(&c, |a| a.status().is_none());
    assert_eq!(calls.load(Ordering::SeqCst), 1);
    assert_eq!(job(&c, &q.id).state, ProcessingState::Completed);
    assert_eq!(c.shared.active.lock().unwrap().publication_attempts, 2);
    c.shutdown().unwrap();
}
#[test]
fn public_cancel_signals_exact_current_attempt_and_stale_attempt_never_signals() {
    let (_root, mut w, _) = specimen();
    let q = queue(&mut w);
    w.cancel_processing_job(&q.id, 1).unwrap();
    let q = w
        .retry_processing_job(&q.id, 1, "Synthetic requested retry")
        .unwrap();
    let c = manual(w, synthetic(), None);
    let p = take(&c);
    assert_eq!(p.ticket.attempt, 2);
    assert!(c
        .dispatch(Command::CancelGraphJob {
            job_id: q.id.clone(),
            expected_attempt: 1
        })
        .is_err());
    assert!(!p.token.is_cancelled());
    assert!(!job(&c, &q.id).cancellation_requested);
    let result: GraphJobInspection = serde_json::from_value(
        c.dispatch(Command::CancelGraphJob {
            job_id: q.id.clone(),
            expected_attempt: 2,
        })
        .unwrap(),
    )
    .unwrap();
    assert!(p.token.is_cancelled());
    assert!(result.job.cancellation_requested);
    execute_and_publish(&c.shared, p);
    assert_eq!(job(&c, &q.id).state, ProcessingState::Cancelled);
    assert!(job(&c, &q.id).result_ids.is_empty());
    c.shutdown().unwrap();
}
#[test]
fn immutable_inspection_binds_both_digests_preserves_frozen_bytes_and_reports_tamper() {
    let (root, mut w, source) = specimen();
    let q = queue(&mut w);
    let c = manual(w, synthetic(), None);
    let p = take(&c);
    execute_and_publish(&c.shared, p);
    let completed = job(&c, &q.id);
    let record = c
        .shared
        .workspace
        .lock()
        .unwrap()
        .graph_analysis_record(&completed.result_ids[0])
        .unwrap();
    let selected = inspect(&c, &completed.id);
    assert_eq!(selected.results.len(), 1);
    let reference = &selected.results[0];
    assert_eq!(reference.id, record.id);
    assert_eq!(reference.request_sha256, record.request_sha256);
    assert_eq!(reference.result_sha256, record.result_sha256);
    let command = |request: String, result: String| Command::InspectGraphAnalysis {
        id: reference.id.clone(),
        expected_request_sha256: request,
        expected_result_sha256: result,
    };
    let expected = c
        .dispatch(command(
            record.request_sha256.clone(),
            record.result_sha256.clone(),
        ))
        .unwrap();
    for dispatch in [
        JobCoordinator::dispatch_presentation,
        JobCoordinator::dispatch_summary,
    ] {
        assert_eq!(
            dispatch(
                &c,
                command(record.request_sha256.clone(), record.result_sha256.clone())
            )
            .unwrap(),
            expected
        );
    }
    assert_eq!(expected["original_integrity"], "verified");
    assert_eq!(expected["freshness"], "current_at_publication");
    for dispatch in [
        JobCoordinator::dispatch,
        JobCoordinator::dispatch_presentation,
        JobCoordinator::dispatch_summary,
    ] {
        let job: GraphJobInspection = serde_json::from_value(
            dispatch(
                &c,
                Command::InspectGraphJob {
                    job_id: completed.id.clone(),
                },
            )
            .unwrap(),
        )
        .unwrap();
        let selected = &job.results[0];
        let inspected = dispatch(
            &c,
            Command::InspectGraphAnalysis {
                id: selected.id.clone(),
                expected_request_sha256: selected.request_sha256.clone(),
                expected_result_sha256: selected.result_sha256.clone(),
            },
        )
        .unwrap();
        assert_eq!(inspected, expected);
    }
    assert!(c
        .dispatch(command("0".repeat(64), record.result_sha256.clone()))
        .is_err());
    assert!(c
        .dispatch(command(record.request_sha256.clone(), "0".repeat(64)))
        .is_err());
    c.shared
        .workspace
        .lock()
        .unwrap()
        .import("later.txt", b"Synthetic later canonical import")
        .unwrap();
    let later = c
        .dispatch_presentation(command(
            record.request_sha256.clone(),
            record.result_sha256.clone(),
        ))
        .unwrap();
    assert_eq!(later["record"], expected["record"]);
    assert_eq!(later["freshness"], "workspace_advanced");
    // Owned disposable fixture only: remove immutable original before replacing it.
    let original = root.path().join("case/originals").join(&source);
    std::fs::remove_file(&original).unwrap();
    std::fs::write(&original, b"Synthetic altered original").unwrap();
    let broken = c
        .dispatch_summary(command(
            record.request_sha256.clone(),
            record.result_sha256.clone(),
        ))
        .unwrap();
    assert_eq!(broken["record"], expected["record"]);
    assert_eq!(broken["original_integrity"], "unavailable");
    assert!(broken["freshness"].is_null());
    // Metadata still exposes the same reference and never claims original verification.
    assert_eq!(
        serde_json::to_value(inspect(&c, &completed.id).results).unwrap(),
        serde_json::to_value(&selected.results).unwrap()
    );
    c.shutdown().unwrap();
}
#[test]
fn quarantine_permits_known_queue_ack_only_and_has_no_retry_or_fabricated_state() {
    let (_root, w, _) = specimen();
    let revision = w.revision().unwrap();
    let c = manual(w, GraphExecution::Unavailable, None);
    let key = uuid::Uuid::new_v4().to_string();
    let first = c.dispatch(queued_command(revision, &key)).unwrap();
    c.shared.ownership.quarantine();
    let replay = c.dispatch(queued_command(revision, &key)).unwrap();
    assert_eq!(replay["job"], first["job"]);
    assert_eq!(replay["workspace_revision"], first["workspace_revision"]);
    assert_eq!(replay["availability"], "recovery_required");
    assert_eq!(replay["controls"]["can_cancel"], false);
    assert!(replay["execution"].is_null());
    assert!(c
        .dispatch(queued_command(
            revision + 1,
            &uuid::Uuid::new_v4().to_string()
        ))
        .is_err());
    assert!(c
        .dispatch(Command::CancelGraphJob {
            job_id: first["job"]["id"].as_str().unwrap().into(),
            expected_attempt: 1
        })
        .is_err());
    assert!(c.shutdown().is_err());
}

#[test]
fn consumed_publication_retry_cannot_admit_a_duplicate_while_waiting_for_workspace() {
    let (root, mut w, _) = specimen();
    let q = queue(&mut w);
    let calls = Arc::new(AtomicUsize::new(0));
    let count = calls.clone();
    let c = manual(
        w,
        GraphExecution::Synthetic(Arc::new(move |bytes, _| {
            count.fetch_add(1, Ordering::SeqCst);
            Ok(response(bytes))
        })),
        None,
    );
    let p = take(&c);
    reject_finish(&root);
    run(&c, p);
    pending(&c, 1);
    // Hold the next settlement at the workspace boundary after retry consumption.
    let workspace = c.shared.workspace.lock().unwrap();
    let status = c.graph_status().unwrap().unwrap();
    c.retry_graph_publication(
        &q.id,
        1,
        status.lease.as_deref().unwrap(),
        status.request_sha256.as_deref().unwrap(),
    )
    .unwrap();
    wait_activity(
        &c,
        |a| matches!(&a.interval, Interval::Owned(owned) if !owned.retry_requested),
    );
    let attempted = crate::coordinator::graph_api::retry(
        &c,
        &workspace,
        &q.id,
        1,
        status.lease.as_deref().unwrap(),
        status.request_sha256.as_deref().unwrap(),
    );
    let phase = c.graph_status().unwrap().unwrap();
    // Always unblock and join before asserting, including the failing-before case.
    drop(workspace);
    sql(&root, "DROP TRIGGER reject_graph_finish");
    c.shutdown().unwrap();
    assert!(
        attempted.is_err(),
        "consumed retry admitted another public retry"
    );
    assert_eq!(phase.phase, GraphPhase::Publishing);
    assert_eq!(phase.retries, 1);
    assert!(!phase.can_retry);
    assert_eq!(calls.load(Ordering::SeqCst), 1);
}

#[test]
fn cancellation_signals_current_token_even_when_corrupt_result_reference_breaks_response() {
    let (root, mut w, _) = specimen();
    let q = queue(&mut w);
    let c = manual(w, synthetic(), None);
    let p = take(&c);
    assert!(!p.token.is_cancelled());
    // Owned synthetic corruption: selected metadata is bounded, but its artifact is absent.
    rusqlite::Connection::open(root.path().join("case/workspace.db")).unwrap().execute(
        "UPDATE records SET body=json_set(body,'$.result_ids',json_array(?)) WHERE kind='processing_job' AND id=?",
        rusqlite::params!["0".repeat(64),q.id]).unwrap();
    let result = c.dispatch(Command::CancelGraphJob {
        job_id: q.id.clone(),
        expected_attempt: 1,
    });
    let signalled = p.token.is_cancelled();
    let intent = job(&c, &q.id).cancellation_requested;
    execute_and_publish(&c.shared, p);
    c.shutdown().unwrap();
    assert!(
        matches!(result,Err(Error::Validation(message)) if message.contains("Referenced graph result is missing"))
    );
    assert!(intent, "canonical cancellation did not commit");
    assert!(
        signalled,
        "committed cancellation skipped its token after response projection failed"
    );
}

#[test]
fn corrupt_reference_response_cannot_leave_cancelled_publication_waiter_asleep() {
    let (root, mut w, _) = specimen();
    let q = queue(&mut w);
    let c = manual(w, synthetic(), None);
    let p = take(&c);
    let token = p.token.clone();
    reject_finish(&root);
    run(&c, p);
    pending(&c, 1);
    rusqlite::Connection::open(root.path().join("case/workspace.db")).unwrap().execute(
        "UPDATE records SET body=json_set(body,'$.result_ids',json_array(?)) WHERE kind='processing_job' AND id=?",
        rusqlite::params!["0".repeat(64),q.id]).unwrap();
    sql(&root, "DROP TRIGGER reject_graph_finish");
    let result = c.dispatch(Command::CancelGraphJob {
        job_id: q.id.clone(),
        expected_attempt: 1,
    });
    assert!(
        matches!(result,Err(Error::Validation(message)) if message.contains("Referenced graph result is missing"))
    );
    wait_activity(&c, |a| a.status().is_none());
    assert!(token.is_cancelled());
    assert_eq!(job(&c, &q.id).state, ProcessingState::Cancelled);
    assert_eq!(c.shared.active.lock().unwrap().publication_attempts, 2);
    c.shutdown().unwrap();
}
