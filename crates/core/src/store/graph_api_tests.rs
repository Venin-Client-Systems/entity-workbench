use super::*;
use crate::store::graph_analysis::probe_fixture::specimen;

fn queue(w: &mut Workspace) -> GraphJobInspection {
    w.queue_graph_job(w.revision().unwrap(), "a", "c", &id())
        .unwrap()
}
fn request(size: u32, cursor: Option<String>) -> GraphJobPageRequest {
    GraphJobPageRequest {
        page_size: size,
        cursor,
    }
}
#[test]
fn dispatch_queue_replay_preserves_uuid_revision_and_direct_responses_in_all_modes() {
    let (_root, mut w, _) = specimen();
    let revision = w.revision().unwrap();
    let command = json!({"action":"queue_graph_path","expected_revision":revision,"source_id":"a","target_id":"c","request_key":id()});
    let first = w
        .dispatch(serde_json::from_value(command.clone()).unwrap())
        .unwrap();
    let second = w
        .dispatch_presentation(serde_json::from_value(command.clone()).unwrap())
        .unwrap();
    let third = w
        .dispatch_summary(serde_json::from_value(command.clone()).unwrap())
        .unwrap();
    assert_eq!(first, second);
    assert_eq!(first, third);
    assert_eq!(first["availability"], "standalone_unavailable");
    assert_eq!(first["job"]["input"]["requested_revision"], revision);
    assert_eq!(w.revision().unwrap(), revision + 1);
    for (field, value) in [
        ("target_id", json!("b")),
        ("expected_revision", json!(revision + 1)),
    ] {
        let mut altered = command.clone();
        altered[field] = value;
        assert!(w
            .dispatch(serde_json::from_value(altered).unwrap())
            .is_err());
    }
    let mut fresh = command;
    fresh["request_key"] = json!(id());
    assert!(matches!(
        w.dispatch(serde_json::from_value(fresh).unwrap()),
        Err(Error::Conflict(_))
    ));
    assert_eq!(w.revision().unwrap(), revision + 1);
}
#[test]
fn pages_cover_sequence_once_include_terminal_jobs_and_reject_cross_revision_or_size() {
    let (_root, mut w, source) = specimen();
    let mut expected = vec![];
    for i in 0..29 {
        let job = queue(&mut w).job;
        if i % 2 == 0 {
            w.cancel_graph_job(&job.id, 1).unwrap();
        }
        expected.push(job.id);
    }
    w.queue_document_parse(&source, &id()).unwrap();
    let revision = w.revision().unwrap();
    let mut cursor = None;
    let mut actual = vec![];
    let mut sequence = 0;
    loop {
        let page = w
            .page_graph_jobs(&request(7, cursor), Some(revision))
            .unwrap();
        assert_eq!(page.total_count, 29);
        for row in page.rows {
            assert!(row.sequence > sequence);
            sequence = row.sequence;
            actual.push(row.job.id);
        }
        cursor = page.next_cursor;
        if cursor.is_none() {
            break;
        }
    }
    assert_eq!(actual, expected);
    let first = w.page_graph_jobs(&request(7, None), None).unwrap();
    assert!(matches!(
        w.page_graph_jobs(&request(8, first.next_cursor.clone()), None),
        Err(Error::Conflict(_))
    ));
    let mut forged = Cursor::decode(
        first.next_cursor.as_ref().unwrap(),
        &hash(
            &serde_json::to_vec(&(1u32, "graph_jobs", "sequence_ascending", revision, 7u32))
                .unwrap(),
        ),
    )
    .unwrap();
    forged.id = id();
    assert!(w
        .page_graph_jobs(&request(7, Some(forged.encode().unwrap())), None)
        .is_err());
    queue(&mut w);
    assert!(matches!(
        w.page_graph_jobs(&request(7, first.next_cursor), None),
        Err(Error::Conflict(_))
    ));
    assert!(matches!(
        w.page_graph_jobs(&request(7, None), Some(revision)),
        Err(Error::Conflict(_))
    ));
}
#[test]
fn malformed_metadata_and_oversized_next_page_fail_without_skipping_rows() {
    let (_root, mut w, _) = specimen();
    let a = queue(&mut w).job;
    let b = queue(&mut w).job;
    let raw = serde_json::to_string(&b).unwrap();
    let mut hostile = b.clone();
    hostile.detail = "x".repeat(JOB_BYTES);
    w.conn
        .execute(
            "UPDATE records SET body=? WHERE kind='processing_job' AND id=?",
            params![serde_json::to_string(&hostile).unwrap(), b.id],
        )
        .unwrap();
    let first = w.page_graph_jobs(&request(1, None), None).unwrap();
    assert_eq!(first.rows[0].job.id, a.id);
    assert!(first.next_cursor.is_some());
    assert!(w
        .page_graph_jobs(&request(1, first.next_cursor), None)
        .is_err());
    assert!(w.page_graph_jobs(&request(25, None), None).is_err());
    for changed in [json!("not-a-date"), json!(null)] {
        let mut value: Value = serde_json::from_str(&raw).unwrap();
        value["updated_at"] = changed;
        w.conn
            .execute(
                "UPDATE records SET body=? WHERE kind='processing_job' AND id=?",
                params![value.to_string(), b.id],
            )
            .unwrap();
        assert!(w.inspect_graph_job(&b.id).is_err());
    }
    w.conn
        .execute(
            "UPDATE records SET body=? WHERE kind='processing_job' AND id=?",
            params![raw, a.id],
        )
        .unwrap();
    assert!(w.inspect_graph_job(&a.id).is_err());
    assert!(w.cancel_graph_job(&a.id, 1).is_err());
}
#[test]
fn hostile_requests_and_stale_cancellation_never_mutate_a_later_attempt() {
    let (_root, mut w, source) = specimen();
    let q = queue(&mut w).job;
    w.cancel_graph_job(&q.id, 1).unwrap();
    let retry = w
        .retry_processing_job(&q.id, 1, "Synthetic explicit retry")
        .unwrap();
    let revision = w.revision().unwrap();
    assert_eq!(retry.attempt, 2);
    assert!(w.cancel_graph_job(&q.id, 1).is_err());
    assert!(
        !w.inspect_graph_job(&q.id)
            .unwrap()
            .job
            .cancellation_requested
    );
    let doc = w.queue_document_parse(&source, &id()).unwrap();
    assert!(w.cancel_graph_job(&doc.id, 1).is_err());
    for source in ["x\n".into(), "x".repeat(129), "x' OR 1=1 --".into()] {
        assert!(w
            .queue_graph_job(w.revision().unwrap(), &source, "c", &id())
            .is_err());
    }
    for size in [0, 26, u32::MAX] {
        assert!(w.page_graph_jobs(&request(size, None), None).is_err());
    }
    for cursor in ["%".into(), "00".repeat(513), "ff".into()] {
        assert!(w.page_graph_jobs(&request(2, Some(cursor)), None).is_err());
    }
    assert_eq!(w.revision().unwrap(), revision + 1);
    let before = w.revision().unwrap();
    assert!(w
        .dispatch(Command::RetryGraphPublication {
            job_id: q.id,
            expected_attempt: 2,
            host_attempt_lease: id(),
            request_sha256: "0".repeat(64)
        })
        .is_err());
    assert_eq!(w.revision().unwrap(), before);
}
#[test]
fn page_revision_count_and_rows_share_snapshot_with_actual_concurrent_cancellation() {
    use rusqlite::hooks::{AuthAction, AuthContext, Authorization};
    use std::sync::{
        atomic::{AtomicBool, Ordering},
        Arc,
    };
    let (root, mut w, _) = specimen();
    let q = queue(&mut w).job;
    let other = Workspace::open(root.path().join("case")).unwrap();
    w.conn.pragma_update(None, "journal_mode", "WAL").unwrap();
    other
        .conn
        .pragma_update(None, "journal_mode", "WAL")
        .unwrap();
    let before = w.page_graph_jobs(&request(25, None), None).unwrap();
    let hit = Arc::new(AtomicBool::new(false));
    let observed = hit.clone();
    let writer = std::sync::Mutex::new(other);
    let key = q.id.clone();
    w.conn.authorizer(Some(move |ctx: AuthContext<'_>| {
        if matches!(
            ctx.action,
            AuthAction::Read {
                table_name: "records",
                ..
            }
        ) && !observed.swap(true, Ordering::SeqCst)
        {
            writer.lock().unwrap().cancel_graph_job(&key, 1).unwrap();
        }
        Authorization::Allow
    }));
    let after = w
        .page_graph_jobs(&request(25, None), Some(before.workspace_revision))
        .unwrap();
    w.conn
        .authorizer(None::<fn(AuthContext<'_>) -> Authorization>);
    assert!(hit.load(Ordering::SeqCst));
    assert_eq!(
        serde_json::to_value(&before).unwrap(),
        serde_json::to_value(after).unwrap()
    );
    assert!(matches!(
        w.page_graph_jobs(&request(25, None), Some(before.workspace_revision)),
        Err(Error::Conflict(_))
    ));
    assert_eq!(
        w.inspect_graph_job(&q.id).unwrap().job.state,
        ProcessingState::Cancelled
    );
}

#[test]
fn request_mapping_and_schema_five_metadata_cannot_hide_or_retarget_graph_jobs() {
    let (_root, mut w, _) = specimen();
    let q = queue(&mut w).job;
    let revision = w.revision().unwrap();
    let ProcessingInput::ShortestConnectionPath {
        requested_revision, ..
    } = q.input
    else {
        unreachable!()
    };
    w.conn
        .execute(
            "UPDATE records SET body=? WHERE kind='processing_request' AND id=?",
            params![
                serde_json::to_string(&"x".repeat(129)).unwrap(),
                q.request_key
            ],
        )
        .unwrap();
    let error = w
        .queue_graph_job(requested_revision, "a", "c", &q.request_key)
        .unwrap_err();
    assert!(error.to_string().contains("mapping exceeds bound"));
    assert_eq!(w.revision().unwrap(), revision);
    let mut raw = serde_json::to_value(&q).unwrap();
    raw["input"] =
        json!({"operation":"parse_document","evidence_id":"x","sha256":"0".repeat(64),"bytes":0});
    w.conn
        .execute(
            "UPDATE records SET body=? WHERE kind='processing_job' AND id=?",
            params![raw.to_string(), q.id],
        )
        .unwrap();
    assert!(w.page_graph_jobs(&request(25, None), None).is_err());
    assert!(w.inspect_graph_job(&q.id).is_err());
    assert!(serde_json::from_value::<Command>(
        json!({"action":"queue_graph_path", "expected_revision":revision,
        "source_id":"a","target_id":"c","request_key":id(),"runtime_path":"untrusted"})
    )
    .is_err());
    assert!(serde_json::from_value::<GraphJobPageRequest>(
        json!({"page_size":1,"cursor":null,"sql":"SELECT 1"})
    )
    .is_err());
    assert!(w
        .queue_graph_job(revision, "a", "c", "AAAAAAAA-AAAA-4AAA-8AAA-AAAAAAAAAAAA")
        .is_err());
}

fn completed(w: &mut Workspace) -> ProcessingJob {
    let q = queue(w).job;
    let (ticket, mut attempt) = w.claim_graph_exclusive(&q).unwrap();
    let mut result: Value = serde_json::from_slice(attempt.worker_input()).unwrap();
    for key in ["nodes", "edges", "source_id", "target_id"] {
        result.as_object_mut().unwrap().remove(key);
    }
    result["outcome"] = json!({"state":"path","nodes":["a","b","c"]});
    w.finish_graph_processing_job(
        &ticket,
        &mut attempt,
        Ok(&ProcessingOutput::Graph(
            serde_json::to_vec(&result).unwrap(),
        )),
    )
    .unwrap()
}
#[test]
fn result_reference_is_bounded_discoverable_metadata_and_corruption_is_never_omitted() {
    let (_root, mut w, _) = specimen();
    let job = completed(&mut w);
    let result_id = &job.result_ids[0];
    let before = w.inspect_graph_job(&job.id).unwrap();
    let reference = &before.results[0];
    let immutable = w
        .inspect_graph_result(
            &reference.id,
            &reference.request_sha256,
            &reference.result_sha256,
        )
        .unwrap();
    assert_eq!(immutable.record.id, *result_id);
    let raw: String = w
        .conn
        .query_row(
            "SELECT body FROM records WHERE kind='graph_analysis' AND id=?",
            [result_id],
            |r| r.get(0),
        )
        .unwrap();
    for (key, value) in [
        ("job_id", json!(id())),
        ("request_key", json!(id())),
        ("id", json!("0".repeat(64))),
        ("request_sha256", json!("x".repeat(65 * 1024))),
        ("result_sha256", json!(false)),
        ("captured_revision", json!(1.5)),
        ("published_revision", json!(u64::MAX)),
        ("schema_version", json!(true)),
    ] {
        let mut changed: Value = serde_json::from_str(&raw).unwrap();
        changed[key] = value;
        w.conn
            .execute(
                "UPDATE records SET body=? WHERE kind='graph_analysis' AND id=?",
                params![changed.to_string(), result_id],
            )
            .unwrap();
        assert!(
            w.inspect_graph_job(&job.id).is_err(),
            "accepted malformed reference field {key}"
        );
        // A page is explicitly job metadata; selecting it performs reference validation.
        assert_eq!(
            w.page_graph_jobs(&request(25, None), None).unwrap().rows[0]
                .job
                .result_ids,
            job.result_ids
        );
    }
    w.conn
        .execute(
            "DELETE FROM records WHERE kind='graph_analysis' AND id=?",
            [result_id],
        )
        .unwrap();
    assert!(w
        .inspect_graph_job(&job.id)
        .unwrap_err()
        .to_string()
        .contains("missing"));
    assert_eq!(w.revision().unwrap(), before.workspace_revision);
}
#[test]
fn result_references_and_job_remain_one_snapshot_during_real_canonical_change() {
    use rusqlite::hooks::{AuthAction, AuthContext, Authorization};
    use std::sync::{
        atomic::{AtomicBool, Ordering},
        Arc,
    };
    let (root, mut w, _) = specimen();
    let job = completed(&mut w);
    let before = w.inspect_graph_job(&job.id).unwrap();
    let other = Workspace::open(root.path().join("case")).unwrap();
    w.conn.pragma_update(None, "journal_mode", "WAL").unwrap();
    other
        .conn
        .pragma_update(None, "journal_mode", "WAL")
        .unwrap();
    let hit = Arc::new(AtomicBool::new(false));
    let observed = hit.clone();
    let writer = std::sync::Mutex::new(other);
    w.conn.authorizer(Some(move |ctx: AuthContext<'_>| {
        if matches!(
            ctx.action,
            AuthAction::Read {
                table_name: "records",
                ..
            }
        ) && !observed.swap(true, Ordering::SeqCst)
        {
            writer
                .lock()
                .unwrap()
                .import("later.txt", b"Synthetic concurrent later canonical import")
                .unwrap();
        }
        Authorization::Allow
    }));
    let after = w.inspect_graph_job(&job.id).unwrap();
    w.conn
        .authorizer(None::<fn(AuthContext<'_>) -> Authorization>);
    assert!(hit.load(Ordering::SeqCst));
    assert_eq!(
        serde_json::to_value(&before).unwrap(),
        serde_json::to_value(after).unwrap()
    );
    let current = w.inspect_graph_job(&job.id).unwrap();
    assert_eq!(current.workspace_revision, before.workspace_revision + 1);
    assert_eq!(
        serde_json::to_value(current.results).unwrap(),
        serde_json::to_value(before.results).unwrap()
    );
}
