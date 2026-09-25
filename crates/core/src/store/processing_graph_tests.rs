//! No Python execution: real canonical transactions and adversarial worker bytes.
use super::*;
use crate::graph_jobs::*;
use crate::store::graph_analysis::{probe_fixture::specimen, GraphAttempt};

fn queue(w: &mut Workspace) -> ProcessingJob {
    w.queue_graph_path(w.revision().unwrap(), "a", "c", &id())
        .unwrap()
}
fn claim(w: &mut Workspace) -> (JobTicket, GraphAttempt) {
    let job = queue(w);
    w.claim_graph_for_publication_test(&job.id).unwrap()
}
fn response(attempt: &GraphAttempt) -> ProcessingOutput {
    let mut value: Value = serde_json::from_slice(attempt.worker_input()).unwrap();
    let object = value.as_object_mut().unwrap();
    for field in ["nodes", "edges", "source_id", "target_id"] {
        object.remove(field);
    }
    object.insert(
        "outcome".into(),
        json!({"state":"path","nodes":["a","b","c"]}),
    );
    ProcessingOutput::Graph(serde_json::to_vec(&value).unwrap())
}
fn state(w: &Workspace) -> Value {
    let rows: Vec<(String, String, String)> = w
        .conn
        .prepare("SELECT kind,id,body FROM records ORDER BY sequence")
        .unwrap()
        .query_map([], |row| Ok((row.get(0)?, row.get(1)?, row.get(2)?)))
        .unwrap()
        .map(|row| row.unwrap())
        .collect();
    json!({"revision":w.revision().unwrap(),"rows":rows,
        "history":w.conn.query_row("SELECT count(*) FROM history",[],|row|row.get::<_,u64>(0)).unwrap(),
        "events":w.conn.query_row("SELECT count(*) FROM events",[],|row|row.get::<_,u64>(0)).unwrap()})
}
fn count(w: &Workspace) -> u64 {
    w.conn
        .query_row(
            "SELECT count(*) FROM records WHERE kind='graph_analysis'",
            [],
            |row| row.get(0),
        )
        .unwrap()
}

#[test]
fn graph_queue_replay_binds_revision_and_endpoints_without_rewriting() {
    let (_root, mut w, _) = specimen();
    let r = w.revision().unwrap();
    let key = id();
    let first = w.queue_graph_path(r, "a", "c", &key).unwrap();
    let before = state(&w);
    assert_eq!(w.queue_graph_path(r, "a", "c", &key).unwrap().id, first.id);
    assert!(w.queue_graph_path(r + 1, "a", "c", &key).is_err());
    assert!(w.queue_graph_path(r, "a", "f", &key).is_err());
    assert_eq!(state(&w), before);
    assert_eq!(first.schema_version, 5);
    assert_eq!(w.revision().unwrap(), r + 1);
}

#[test]
fn graph_queue_replay_rejects_substituted_mapping_job_id_or_request_uuid() {
    for field in ["mapping", "id", "request_key"] {
        let (_root, mut w, _) = specimen();
        let r = w.revision().unwrap();
        let key = id();
        let job = w.queue_graph_path(r, "a", "c", &key).unwrap();
        if field == "mapping" {
            // Another internally consistent job with the same input/R but another key
            // must not become the response to this request through a changed mapping.
            let mut other = job.clone();
            other.id = id();
            other.request_key = id();
            put(&w.conn, "processing_job", &other.id, &other).unwrap();
            put(&w.conn, "processing_request", &key, &other.id).unwrap();
        } else {
            let mut value = serde_json::to_value(&job).unwrap();
            value[field] = json!(id());
            w.conn
                .execute(
                    "UPDATE records SET body=? WHERE kind='processing_job' AND id=?",
                    params![serde_json::to_string(&value).unwrap(), job.id],
                )
                .unwrap();
        }
        let before = state(&w);
        assert!(w.queue_graph_path(r, "a", "c", &key).is_err(), "{field}");
        assert_eq!(state(&w), before);
    }
}

#[test]
fn graph_production_pair_is_explicitly_blocked_before_running_capture_or_worker() {
    let (_root, mut w, _) = specimen();
    let a = queue(&mut w);
    let b = queue(&mut w);
    for job in [a, b] {
        assert!(w.claim_processing_job().unwrap().is_none());
        let blocked = w.processing_job(&job.id).unwrap();
        assert_eq!(blocked.state, ProcessingState::Blocked);
        assert_eq!(
            blocked.failure,
            Some(ProcessingFailure::SchedulingOrRuntimeUnavailable)
        );
        assert!(blocked.started_at.is_none() && blocked.lease.is_none());
    }
    assert_eq!(count(&w), 0);
    let running: u64 = w.conn.query_row("SELECT count(*) FROM history WHERE kind='processing_job' AND json_extract(body,'$.state')='running'",[],|row|row.get(0)).unwrap();
    assert_eq!(running, 0);
}

#[test]
fn graph_atomic_claim_reads_actual_c_and_capture_failure_rolls_back_every_write() {
    let (_root, mut w, source) = specimen();
    w.change(None, "synthetic.unsupported", false, |conn| {
        let mut obs: Observation = get(conn, "observation", "obs-r1")?;
        obs.anchor = SourceAnchor::Page {
            evidence_id: source.clone(),
            page: 1,
            region: None,
        };
        put(conn, "observation", &obs.id, &obs)
    })
    .unwrap();
    let job = queue(&mut w);
    let before = state(&w);
    assert!(matches!(
        w.claim_graph_for_publication_test(&job.id),
        Err(Error::Blocked(_))
    ));
    assert_eq!(state(&w), before);
    assert_eq!(
        w.processing_job(&job.id).unwrap().state,
        ProcessingState::Queued
    );
}

#[test]
fn graph_r_q_c_p_publication_freezes_provenance_and_replay_is_no_write() {
    let (_root, mut w, _) = specimen();
    let r = w.revision().unwrap();
    let (ticket, mut attempt) = claim(&mut w);
    let input: Value = serde_json::from_slice(attempt.worker_input()).unwrap();
    assert_eq!(input["workspace_revision"], r + 2);
    let output = response(&attempt);
    let job = w
        .finish_graph_processing_job(&ticket, &mut attempt, Ok(&output))
        .unwrap();
    assert!(attempt.worker_input().is_empty());
    assert_eq!(job.state, ProcessingState::Completed);
    let record = w.inspect_graph_analysis(&job.result_ids[0]).unwrap();
    assert_eq!(record.record.requested_revision, r);
    assert_eq!(record.record.queued_revision, r + 1);
    assert_eq!(record.record.captured_revision, r + 2);
    assert_eq!(record.record.published_revision, r + 3);
    assert_eq!(record.record.host_attempt_lease, ticket.lease);
    assert_eq!(record.freshness, Some(GraphFreshness::CurrentAtPublication));
    assert_eq!(record.record.frozen.assertion_reviews.accepted, 6);
    assert_eq!(record.record.frozen.assertion_reviews.pending, 1);
    assert_eq!(record.record.frozen.assertion_reviews.rejected, 1);
    assert_eq!(record.record.frozen.assertion_reviews.deferred, 1);
    let GraphOutcome::Path { hops, .. } = record.record.outcome else {
        panic!("expected path")
    };
    assert_eq!(hops[0].assertion_ids, ["r1", "r1-parallel"]);
    let before = state(&w);
    w.finish_graph_processing_job(&ticket, &mut attempt, Ok(&output))
        .unwrap();
    assert_eq!(state(&w), before);
    assert_eq!(count(&w), 1);
}

#[test]
fn graph_transient_publication_write_failure_retains_same_attempt_and_rolls_back_result() {
    let (_root, mut w, _) = specimen();
    let (ticket, mut attempt) = claim(&mut w);
    let input = attempt.worker_input().to_vec();
    let output = response(&attempt);
    let before = state(&w);
    w.conn.execute_batch("CREATE TRIGGER reject_graph_finish BEFORE UPDATE ON records WHEN NEW.kind='processing_job' AND json_extract(NEW.body,'$.state')='completed' BEGIN SELECT RAISE(ABORT,'synthetic publication fault'); END;").unwrap();
    assert!(matches!(
        w.finish_graph_processing_job(&ticket, &mut attempt, Ok(&output)),
        Err(Error::Database(_))
    ));
    assert_eq!(state(&w), before);
    assert_eq!(attempt.worker_input(), input);
    assert_eq!(count(&w), 0);
    w.conn
        .execute_batch("DROP TRIGGER reject_graph_finish")
        .unwrap();
    assert_eq!(
        w.finish_graph_processing_job(&ticket, &mut attempt, Ok(&output))
            .unwrap()
            .state,
        ProcessingState::Completed
    );
    assert_eq!(count(&w), 1);
}

#[test]
fn graph_transient_recapture_database_failure_is_not_consumed_as_bad_input() {
    use rusqlite::hooks::{AuthAction, AuthContext, Authorization};
    let (_root, mut w, _) = specimen();
    let (ticket, mut attempt) = claim(&mut w);
    let output = response(&attempt);
    let before = state(&w);
    use std::sync::{
        atomic::{AtomicBool, Ordering},
        Arc,
    };
    let inside = Arc::new(AtomicBool::new(false));
    let hit = Arc::new(AtomicBool::new(false));
    let began = Arc::clone(&inside);
    let observed = Arc::clone(&hit);
    w.conn.authorizer(Some(move |ctx: AuthContext<'_>| {
        if matches!(
            ctx.action,
            AuthAction::Transaction {
                operation: rusqlite::hooks::TransactionOperation::Begin
            }
        ) {
            began.store(true, Ordering::SeqCst);
        }
        if began.load(Ordering::SeqCst)
            && matches!(
                ctx.action,
                AuthAction::Read {
                    table_name: "records",
                    ..
                }
            )
        {
            observed.store(true, Ordering::SeqCst);
            Authorization::Deny
        } else {
            Authorization::Allow
        }
    }));
    assert!(matches!(
        w.finish_graph_processing_job(&ticket, &mut attempt, Ok(&output)),
        Err(Error::Database(_))
    ));
    w.conn
        .authorizer(None::<fn(AuthContext<'_>) -> Authorization>);
    assert!(inside.load(Ordering::SeqCst) && hit.load(Ordering::SeqCst));
    assert_eq!(state(&w), before);
    assert!(!attempt.worker_input().is_empty());
    assert_eq!(
        w.finish_graph_processing_job(&ticket, &mut attempt, Ok(&output))
            .unwrap()
            .state,
        ProcessingState::Completed
    );
}

#[test]
fn graph_stale_revision_settles_without_result_or_recapture() {
    let (_root, mut w, _) = specimen();
    let (ticket, mut attempt) = claim(&mut w);
    let output = response(&attempt);
    w.change(None, "synthetic.other_write", false, |_| Ok(()))
        .unwrap();
    let job = w
        .finish_graph_processing_job(&ticket, &mut attempt, Ok(&output))
        .unwrap();
    assert_eq!(job.failure, Some(ProcessingFailure::StaleGraphCapture));
    assert_eq!(count(&w), 0);
    assert!(attempt.worker_input().is_empty());
}

#[test]
fn graph_history_inspection_survives_correction_but_reports_original_corruption() {
    let (_root, mut w, source) = specimen();
    let (ticket, mut attempt) = claim(&mut w);
    let output = response(&attempt);
    let job = w
        .finish_graph_processing_job(&ticket, &mut attempt, Ok(&output))
        .unwrap();
    let before = w.graph_analysis_record(&job.result_ids[0]).unwrap();
    w.change(None, "synthetic.correction", false, |conn| {
        let mut entity: Entity = get(conn, "entity", "a")?;
        entity.name = "Corrected later".into();
        put(conn, "entity", "a", &entity)?;
        let mut obs: Observation = get(conn, "observation", "obs-r1")?;
        obs.value = "Corrected later".into();
        put(conn, "observation", &obs.id, &obs)
    })
    .unwrap();
    let inspected = w.inspect_graph_analysis(&before.id).unwrap();
    assert_eq!(
        serde_json::to_value(&inspected.record).unwrap(),
        serde_json::to_value(before).unwrap()
    );
    assert_eq!(inspected.freshness, Some(GraphFreshness::WorkspaceAdvanced));
    let original = w.root.join("originals").join(source);
    fs::remove_file(&original).unwrap();
    fs::write(original, b"altered synthetic original").unwrap();
    let inspected = w.inspect_graph_analysis(&job.result_ids[0]).unwrap();
    assert_eq!(
        inspected.original_integrity,
        GraphOriginalIntegrity::Unavailable
    );
    assert!(inspected.freshness.is_none());
}

#[test]
fn graph_original_loss_before_acceptance_never_publishes() {
    let (_root, mut w, source) = specimen();
    let (ticket, mut attempt) = claim(&mut w);
    let output = response(&attempt);
    fs::remove_file(w.root.join("originals").join(source)).unwrap();
    let job = w
        .finish_graph_processing_job(&ticket, &mut attempt, Ok(&output))
        .unwrap();
    assert_eq!(job.failure, Some(ProcessingFailure::InputUnavailable));
    assert_eq!(count(&w), 0);
}

#[test]
fn graph_wrong_lease_cross_workspace_and_substituted_or_oversize_result_fail_closed() {
    for mode in [
        "lease",
        "workspace",
        "nonce",
        "oversize",
        "duplicate",
        "other-operation",
    ] {
        let (_root, mut w, _) = specimen();
        let (ticket, mut attempt) = claim(&mut w);
        let mut output = response(&attempt);
        if mode == "workspace" {
            let mut other = Workspace::open(&w.root).unwrap();
            let job = other
                .finish_graph_processing_job(&ticket, &mut attempt, Ok(&output))
                .unwrap();
            assert_eq!(job.failure, Some(ProcessingFailure::StaleGraphCapture));
        } else if mode == "lease" {
            let bad = JobTicket {
                lease: id(),
                ..ticket.clone()
            };
            let before = state(&w);
            assert!(w
                .finish_graph_processing_job(&bad, &mut attempt, Ok(&output))
                .is_err());
            assert_eq!(state(&w), before);
        } else {
            let ProcessingOutput::Graph(bytes) = &mut output else {
                unreachable!()
            };
            match mode {
                "oversize" => *bytes = vec![b' '; 128 * 1024 + 1],
                "duplicate" => {
                    let mut raw = bytes.clone();
                    raw.pop();
                    raw.extend_from_slice(b",\"schema_version\":1}");
                    *bytes = raw;
                }
                "nonce" => {
                    let mut v: Value = serde_json::from_slice(bytes).unwrap();
                    v["nonce"] = json!(id());
                    *bytes = serde_json::to_vec(&v).unwrap();
                }
                "other-operation" => *bytes = b"{}".to_vec(),
                _ => unreachable!(),
            }
            let job = w
                .finish_graph_processing_job(&ticket, &mut attempt, Ok(&output))
                .unwrap();
            assert_eq!(
                job.failure,
                Some(ProcessingFailure::InvalidResult),
                "{mode}"
            );
        }
        assert_eq!(count(&w), 0, "{mode}");
    }
}

#[test]
fn graph_cancellation_cleanup_and_unknown_exit_keep_failure_precedence() {
    for mode in ["cancel", "cleanup", "unknown"] {
        let (_root, mut w, _) = specimen();
        let (ticket, mut attempt) = claim(&mut w);
        let output = response(&attempt);
        let queued = queue(&mut w);
        w.cancel_processing_job(&ticket.job_id, ticket.attempt)
            .unwrap();
        let result = match mode {
            "cancel" => Ok(&output),
            "cleanup" => Err(Error::Cleanup("synthetic".into())),
            _ => Err(Error::TerminationUnverified("synthetic".into())),
        };
        let job = w
            .finish_graph_processing_job(&ticket, &mut attempt, result)
            .unwrap();
        assert_eq!(
            job.failure,
            Some(match mode {
                "cancel" => ProcessingFailure::CancelledByAnalyst,
                "cleanup" => ProcessingFailure::CleanupFailed,
                _ => ProcessingFailure::WorkerExitUnverified,
            })
        );
        if mode == "unknown" {
            assert_eq!(
                w.processing_job(&queued.id).unwrap().failure,
                Some(ProcessingFailure::RecoveryRequired)
            );
            assert!(w.claim_processing_job().is_err());
        }
        assert_eq!(count(&w), 0);
    }
}

#[test]
fn graph_backup_restores_immutable_result_originals_and_old_document_job() {
    let (root, mut w, source) = specimen();
    let old = w.queue_document_parse(&source, &id()).unwrap();
    assert_eq!(old.schema_version, 4);
    w.cancel_processing_job(&old.id, old.attempt).unwrap();
    let (ticket, mut attempt) = claim(&mut w);
    let output = response(&attempt);
    let job = w
        .finish_graph_processing_job(&ticket, &mut attempt, Ok(&output))
        .unwrap();
    let before =
        serde_json::to_value(w.inspect_graph_analysis(&job.result_ids[0]).unwrap()).unwrap();
    let backup = w.backup().unwrap();
    let restored = Workspace::restore(&backup, &root.path().join("restored")).unwrap();
    assert_eq!(
        serde_json::to_value(restored.inspect_graph_analysis(&job.result_ids[0]).unwrap()).unwrap(),
        before
    );
    assert_eq!(restored.processing_job(&old.id).unwrap().schema_version, 4);
    assert_eq!(
        restored
            .conn
            .query_row("PRAGMA user_version", [], |row| row.get::<_, u32>(0))
            .unwrap(),
        w.conn
            .query_row("PRAGMA user_version", [], |row| row.get::<_, u32>(0))
            .unwrap()
    );
}

#[test]
fn graph_frozen_retention_oversize_refuses_atomically_and_keeps_attempt_for_settlement() {
    let (_root, mut w, _) = specimen();
    // Valid bounded canonical reads can still exceed retention after adding topology,
    // fingerprints and request/result provenance. No smaller fake production limit.
    let used: usize = w.conn.query_row("SELECT sum(length(CAST(body AS BLOB))) FROM records WHERE kind IN ('entity','assertion','evidence') OR (kind='observation' AND id IN ('obs-r1','obs-r1-parallel','obs-r2','obs-long1','obs-long2','obs-long3'))",[],|row|row.get(0)).unwrap();
    let mut remaining = MAX_GRAPH_RECORD_BYTES - 2048 - used;
    w.change(None, "synthetic.bounded_large_graph", false, |conn| {
        let mut index = 0;
        while remaining > 200 {
            let mut entity = Entity {
                id: format!("padding-{index}"),
                name: String::new(),
                kind: EntityKind::Person,
                identifiers: vec![],
                merged_into: None,
            };
            let base = serde_json::to_vec(&entity)?.len();
            let bytes = remaining.min(1_000_000);
            entity.name = "x".repeat(bytes - base);
            put(conn, "entity", &entity.id, &entity)?;
            remaining -= bytes;
            index += 1;
        }
        Ok(())
    })
    .unwrap();
    let (ticket, mut attempt) = claim(&mut w);
    let output = response(&attempt);
    let before = state(&w);
    assert!(matches!(
        w.finish_graph_processing_job(&ticket, &mut attempt, Ok(&output)),
        Err(Error::QuotaExhausted(_))
    ));
    assert_eq!(state(&w), before);
    assert_eq!(count(&w), 0);
    assert!(!attempt.worker_input().is_empty());
    // Explicit terminal settlement can release authority after a confirmed worker exit;
    // no automatic recapture, truncation or engine rerun occurs.
    let job = w
        .finish_graph_processing_job(
            &ticket,
            &mut attempt,
            Err(Error::QuotaExhausted("Retention bound".into())),
        )
        .unwrap();
    assert_eq!(job.state, ProcessingState::QuotaExhausted);
    assert!(attempt.worker_input().is_empty());
}

#[test]
fn graph_publication_immediate_transaction_excludes_concurrent_writer_until_p() {
    use rusqlite::hooks::{AuthAction, AuthContext, Authorization};
    use std::sync::{Arc, Mutex};
    let (_root, mut w, _) = specimen();
    let (ticket, mut attempt) = claim(&mut w);
    let output = response(&attempt);
    let writer = Workspace::open(&w.root).unwrap();
    writer.conn.busy_timeout(std::time::Duration::ZERO).unwrap();
    let slot = Arc::new(Mutex::new(Some(writer)));
    let observed = Arc::new(Mutex::new(None));
    let callback = Arc::clone(&slot);
    let observation = Arc::clone(&observed);
    // Gate to a recapture read prepared after BEGIN IMMEDIATE, never the job lookup.
    let began = std::sync::atomic::AtomicBool::new(false);
    w.conn.authorizer(Some(move |ctx:AuthContext<'_>| {
        if matches!(ctx.action,AuthAction::Transaction { operation:rusqlite::hooks::TransactionOperation::Begin }) { began.store(true,std::sync::atomic::Ordering::SeqCst); }
        if began.load(std::sync::atomic::Ordering::SeqCst) && matches!(ctx.action,AuthAction::Read{table_name:"records",..}) {
            if let Some(mut writer)=callback.lock().unwrap().take() {
                let failed=writer.change(None,"synthetic.concurrent",false,|_|Ok(()));
                *observation.lock().unwrap()=Some(matches!(failed,Err(Error::Database(rusqlite::Error::SqliteFailure(ref code,_))) if code.code==rusqlite::ErrorCode::DatabaseBusy));
            }
        }
        Authorization::Allow
    }));
    let job = w
        .finish_graph_processing_job(&ticket, &mut attempt, Ok(&output))
        .unwrap();
    w.conn
        .authorizer(None::<fn(AuthContext<'_>) -> Authorization>);
    assert_eq!(*observed.lock().unwrap(), Some(true));
    assert_eq!(job.state, ProcessingState::Completed);
    let mut writer = Workspace::open(&w.root).unwrap();
    writer
        .change(None, "synthetic.after_publication", false, |_| Ok(()))
        .unwrap();
    assert_eq!(
        w.inspect_graph_analysis(&job.result_ids[0])
            .unwrap()
            .freshness,
        Some(GraphFreshness::WorkspaceAdvanced)
    );
}

#[test]
fn graph_completed_retarget_to_another_valid_record_does_not_replay() {
    let (_root, mut w, _) = specimen();
    let (ticket_a, mut a) = claim(&mut w);
    let output_a = response(&a);
    let mut job_a = w
        .finish_graph_processing_job(&ticket_a, &mut a, Ok(&output_a))
        .unwrap();
    let (ticket_b, mut b) = claim(&mut w);
    let output_b = response(&b);
    let job_b = w
        .finish_graph_processing_job(&ticket_b, &mut b, Ok(&output_b))
        .unwrap();
    job_a.result_ids = job_b.result_ids;
    w.conn
        .execute(
            "UPDATE records SET body=? WHERE kind='processing_job' AND id=?",
            params![serde_json::to_string(&job_a).unwrap(), job_a.id],
        )
        .unwrap();
    let before = state(&w);
    assert!(w
        .finish_graph_processing_job(&ticket_a, &mut a, Ok(&output_b))
        .is_err());
    assert_eq!(state(&w), before);
    assert_eq!(count(&w), 2);
}

#[test]
fn graph_unreachable_result_has_frozen_full_denominator_and_closed_output() {
    let (_root, mut w, _) = specimen();
    let job = w
        .queue_graph_path(w.revision().unwrap(), "a", "f", &id())
        .unwrap();
    let (ticket, mut attempt) = w.claim_graph_for_publication_test(&job.id).unwrap();
    let ProcessingOutput::Graph(bytes) = response(&attempt) else {
        unreachable!()
    };
    let mut result: Value = serde_json::from_slice(&bytes).unwrap();
    result["outcome"] = json!({"state":"unreachable"});
    let output = ProcessingOutput::Graph(serde_json::to_vec(&result).unwrap());
    let job = w
        .finish_graph_processing_job(&ticket, &mut attempt, Ok(&output))
        .unwrap();
    let record = w.graph_analysis_record(&job.result_ids[0]).unwrap();
    assert!(matches!(record.outcome, GraphOutcome::Unreachable {}));
    assert_eq!(record.frozen.nodes.len(), 6);
    assert_eq!(record.frozen.edges.len(), 5);
}

#[test]
fn graph_inspection_rejects_mutated_or_oversize_record_before_acceptance() {
    let (_root, mut w, _) = specimen();
    let (ticket, mut attempt) = claim(&mut w);
    let output = response(&attempt);
    let job = w
        .finish_graph_processing_job(&ticket, &mut attempt, Ok(&output))
        .unwrap();
    let key = &job.result_ids[0];
    w.conn.execute("UPDATE records SET body=json_set(body,'$.frozen.entities[0].name','forged') WHERE kind='graph_analysis' AND id=?",[key]).unwrap();
    assert!(w.inspect_graph_analysis(key).is_err());
    w.conn
        .execute(
            "UPDATE records SET body=? WHERE kind='graph_analysis' AND id=?",
            params![
                serde_json::to_string(&"x".repeat(MAX_GRAPH_RECORD_BYTES + 1)).unwrap(),
                key
            ],
        )
        .unwrap();
    assert!(w
        .inspect_graph_analysis(key)
        .unwrap_err()
        .to_string()
        .contains("byte bound"));
}

#[test]
fn graph_queued_cancel_manual_retry_and_orphan_recovery_remain_explicit() {
    let (_root, mut w, _) = specimen();
    let job = queue(&mut w);
    let r = w.revision().unwrap();
    w.cancel_processing_job(&job.id, job.attempt).unwrap();
    assert!(w.claim_processing_job().unwrap().is_none());
    let retry = w
        .retry_processing_job(&job.id, job.attempt, "Synthetic deliberate retry")
        .unwrap();
    let ProcessingInput::ShortestConnectionPath {
        queued_revision,
        requested_revision,
        ..
    } = retry.input
    else {
        unreachable!()
    };
    assert_eq!(queued_revision, r + 2);
    assert_eq!(requested_revision, r - 1);
    let (_ticket, _attempt) = w.claim_graph_for_publication_test(&job.id).unwrap();
    assert_eq!(w.recover_processing_jobs().unwrap(), 1);
    let recovered = w.processing_job(&job.id).unwrap();
    assert_eq!(
        recovered.failure,
        Some(ProcessingFailure::WorkerExitUnverified)
    );
    assert!(w.claim_processing_job().is_err());
    assert_eq!(count(&w), 0);
}

#[test]
fn graph_same_revision_claim_corruption_cannot_relabel_private_capture_or_history() {
    for field in [
        "target_id",
        "requested_revision",
        "queued_revision",
        "id",
        "request_key",
    ] {
        let (_root, mut w, _) = specimen();
        let (ticket, mut attempt) = claim(&mut w);
        let output = response(&attempt);
        let mut job = w.processing_job(&ticket.job_id).unwrap();
        let mut value = serde_json::to_value(&job).unwrap();
        match field {
            "target_id" => value["input"][field] = json!("f"),
            "requested_revision" => {
                value["input"][field] = json!(0);
                value["input"]["queued_revision"] = json!(1);
            }
            "queued_revision" => {
                value["input"]["requested_revision"] = json!(20);
                value["input"][field] = json!(21);
            }
            _ => value[field] = json!(id()),
        }
        w.conn
            .execute(
                "UPDATE records SET body=? WHERE kind='processing_job' AND id=?",
                params![serde_json::to_string(&value).unwrap(), ticket.job_id],
            )
            .unwrap();
        let before = state(&w);
        assert!(
            w.finish_graph_processing_job(&ticket, &mut attempt, Ok(&output))
                .is_err(),
            "{field}"
        );
        assert_eq!(state(&w), before);
        assert_eq!(count(&w), 0);
        assert!(!attempt.worker_input().is_empty());
        // Restoring the exact held claim permits the already-computed result; never rebase.
        w.conn
            .execute(
                "UPDATE records SET body=? WHERE kind='processing_job' AND id=?",
                params![serde_json::to_string(&job).unwrap(), ticket.job_id],
            )
            .unwrap();
        job = w
            .finish_graph_processing_job(&ticket, &mut attempt, Ok(&output))
            .unwrap();
        let key = job.result_ids[0].clone();
        let ProcessingInput::ShortestConnectionPath { target_id, .. } = &mut job.input else {
            unreachable!()
        };
        *target_id = "f".into();
        w.conn
            .execute(
                "UPDATE records SET body=? WHERE kind='processing_job' AND id=?",
                params![serde_json::to_string(&job).unwrap(), ticket.job_id],
            )
            .unwrap();
        assert!(w.inspect_graph_analysis(&key).is_err());
    }
}
