//! Synthetic only: no broker, DNS, external server or coordinator is started.
use super::*;

#[test]
fn poisoned_shared_ownership_is_not_released_by_drop() {
    let (_temp, workspace) = workspace();
    let owner = std::sync::Arc::new(workspace.collection_ownership().unwrap());
    let poisoned = owner.clone();
    assert!(std::thread::spawn(move || {
        let _held = poisoned.state.lock().unwrap();
        panic!("Synthetic ownership-state poison");
    })
    .join()
    .is_err());
    assert!(!owner.held());
    assert!(!owner.publication_held());
    assert!(owner.release().is_err());
    drop(owner);
    assert!(workspace.collection_ownership().is_err());
}
use tempfile::TempDir;
const AT: i64 = 1_700_000_000_000;
fn workspace() -> (TempDir, Workspace) {
    let temp = TempDir::new_in(std::env::temp_dir().canonicalize().unwrap()).unwrap();
    let workspace = Workspace::open(temp.path().join("workspace")).unwrap();
    (temp, workspace)
}
fn input() -> CollectionInput {
    CollectionInput {
        urls: vec!["https://example.com/start".into()],
        max_hops: 2,
        max_requests: 50,
        max_seconds: 600,
    }
}
fn response(status: u16, text: &str) -> CollectionResponse {
    CollectionResponse::Complete {
        status,
        content_type: "text/html; charset=utf-8".into(),
        location: None,
        body: text.as_bytes().to_vec(),
    }
}
fn queue(w: &mut Workspace) -> DurableCollectionJob {
    w.queue_durable_collection(input(), &id(), AT).unwrap()
}
fn begin(w: &mut Workspace, owner: &CollectionOwnership) -> (String, CollectionTicket) {
    let job = queue(w);
    let ticket = w
        .start_durable_collection(&job.id, 1, owner, AT + 1)
        .unwrap()
        .unwrap();
    (job.id, ticket)
}
fn robots(w: &mut Workspace, execution: &CollectionTicket, owner: &CollectionOwnership) {
    let request = w
        .advance_durable_collection(execution, owner, AT + 2)
        .unwrap()
        .unwrap();
    assert_eq!(request.url, "https://example.com/robots.txt");
    w.complete_durable_collection(&request, &response(404, ""), owner, AT + 3)
        .unwrap();
}
fn raw(w: &Workspace, key: &str) -> DurableCollectionJob {
    get(&w.conn, "collection_run", key).unwrap()
}
fn corrupt(w: &Workspace, job: &DurableCollectionJob) {
    w.conn
        .execute(
            "UPDATE records SET body=? WHERE kind='collection_run' AND id=?",
            params![serde_json::to_string(job).unwrap(), job.id],
        )
        .unwrap();
}
fn history_count(w: &Workspace) -> u64 {
    w.conn
        .query_row("SELECT count(*) FROM history", [], |r| r.get(0))
        .unwrap()
}

#[test]
fn queue_identity_bounds_and_original_legacy_schemas_stay_untouched() {
    let (_temp, mut w) = workspace();
    let key = id();
    let job = w.queue_durable_collection(input(), &key, AT).unwrap();
    let revision = w.revision().unwrap();
    assert_eq!(
        w.queue_durable_collection(input(), &key, AT + 1).unwrap(),
        job
    );
    assert_eq!(w.revision().unwrap(), revision);
    let mut changed = input();
    changed.max_hops = 1;
    assert!(w.queue_durable_collection(changed, &key, AT).is_err());
    let mut hostile = input();
    hostile.urls[0] = "https://user:secret@example.com".into();
    assert!(w.queue_durable_collection(hostile, &id(), AT).is_err());
    let mut oversized = input();
    oversized.max_requests = 51;
    assert!(w.queue_durable_collection(oversized, &id(), AT).is_err());
    assert!(w.queue_durable_collection(input(), "not-uuid", AT).is_err());
    assert_eq!(
        w.conn
            .pragma_query_value::<u32, _>(None, "user_version", |r| r.get(0))
            .unwrap(),
        5
    );
    assert!(w.view().unwrap().jobs.is_empty());
    assert!(w.view().unwrap().observations.is_empty());
    for _ in 1..8 {
        queue(&mut w);
    }
    assert!(w.queue_durable_collection(input(), &id(), AT).is_err());
}

#[test]
fn completed_response_checkpoint_is_atomic_and_replay_is_exactly_once() {
    let (_temp, mut w) = workspace();
    let owner = w.collection_ownership().unwrap();
    let (key, execution) = begin(&mut w, &owner);
    robots(&mut w, &execution, &owner);
    let request = w
        .advance_durable_collection(&execution, &owner, AT + 4)
        .unwrap()
        .unwrap();
    let received = response(
        200,
        "<p>synthetic <script>untrusted()</script> source</p><a href='/next'>next</a>",
    );
    let complete = w
        .complete_durable_collection(&request, &received, &owner, AT + 5)
        .unwrap();
    assert_eq!(complete.checkpoint.requests_used(), 2);
    assert_eq!(complete.checkpoint.pages_retained, 1);
    assert_eq!(complete.checkpoint.frontier[0].parent, Some(1));
    assert_eq!(complete.checkpoint.frontier[0].hop, 1);
    let revision = w.revision().unwrap();
    let history = history_count(&w);
    assert_eq!(
        w.complete_durable_collection(&request, &received, &owner, AT + 5)
            .unwrap(),
        complete
    );
    assert_eq!(w.revision().unwrap(), revision);
    assert_eq!(history_count(&w), history);
    assert!(w
        .complete_durable_collection(&request, &response(200, "different"), &owner, AT + 5)
        .is_err());
    assert!(w
        .complete_durable_collection(&request, &received, &owner, AT + 6)
        .is_err());
    let evidence = w.view().unwrap().evidence;
    assert_eq!(
        evidence.iter().map(|e| e.acquisitions.len()).sum::<usize>(),
        2
    );
    assert!(evidence
        .iter()
        .any(|e| e.text.as_deref() == Some("synthetic source next")));
    assert!(w.view().unwrap().observations.is_empty());
    assert!(w.collection_receipt(&key).is_err());
    assert_eq!(
        w.conn
            .query_row::<u64, _, _>(
                "SELECT count(*) FROM records WHERE kind='collection_receipt'",
                [],
                |r| r.get(0)
            )
            .unwrap(),
        0
    );
}

#[test]
fn failed_transaction_leaves_charged_reservation_and_unreferenced_file_only() {
    let (_temp, mut w) = workspace();
    let owner = w.collection_ownership().unwrap();
    let (key, execution) = begin(&mut w, &owner);
    robots(&mut w, &execution, &owner);
    let request = w
        .advance_durable_collection(&execution, &owner, AT + 4)
        .unwrap()
        .unwrap();
    let before = w.revision().unwrap();
    let history = history_count(&w);
    w.conn.execute_batch("CREATE TRIGGER fail_checkpoint BEFORE UPDATE ON records WHEN NEW.kind='collection_run' BEGIN SELECT RAISE(ABORT,'synthetic publication fault'); END;").unwrap();
    let received = response(200, "rollback specimen");
    assert!(w
        .complete_durable_collection(&request, &received, &owner, AT + 5)
        .is_err());
    assert_eq!(w.revision().unwrap(), before);
    assert_eq!(history_count(&w), history);
    assert_eq!(w.view().unwrap().evidence.len(), 1);
    assert!(w
        .root
        .join("originals")
        .join(hash(b"rollback specimen"))
        .exists());
    let snapshot = w.backup().unwrap();
    assert!(!snapshot
        .join("originals")
        .join(hash(b"rollback specimen"))
        .exists());
    assert!(matches!(
        w.inspect_durable_collection(&key)
            .unwrap()
            .checkpoint
            .requests[1]
            .progress,
        RequestProgress::Reserved
    ));
    w.conn
        .execute_batch("DROP TRIGGER fail_checkpoint;")
        .unwrap();
    w.complete_durable_collection(&request, &received, &owner, AT + 5)
        .unwrap();
    assert_eq!(w.view().unwrap().evidence.len(), 2);
    assert_eq!(
        w.inspect_durable_collection(&key)
            .unwrap()
            .checkpoint
            .pages_retained,
        1
    );
}

#[test]
fn crash_after_reservation_is_charged_unknown_and_resume_never_reissues_it() {
    let (temp, mut w) = workspace();
    let owner = w.collection_ownership().unwrap();
    let (key, old) = begin(&mut w, &owner);
    robots(&mut w, &old, &owner);
    let page = w
        .advance_durable_collection(&old, &owner, AT + 4)
        .unwrap()
        .unwrap();
    w.complete_durable_collection(
        &page,
        &response(200, "<a href='/one'>one</a><a href='/two'>two</a>"),
        &owner,
        AT + 5,
    )
    .unwrap();
    let uncertain = w
        .advance_durable_collection(&old, &owner, AT + 6)
        .unwrap()
        .unwrap();
    assert_eq!(uncertain.url, "https://example.com/one");
    drop(owner);
    drop(w);
    let mut w = Workspace::open(temp.path().join("workspace")).unwrap();
    // Ordinary open is read/recovery compatible with the old schema4 path.
    assert_eq!(
        w.inspect_durable_collection(&key).unwrap().checkpoint.state,
        CollectionState::Running
    );
    let owner = w.collection_ownership().unwrap();
    assert_eq!(w.recover_durable_collections(&owner, AT + 7).unwrap(), 1);
    assert_eq!(w.recover_durable_collections(&owner, AT + 7).unwrap(), 0);
    let recovered = w.inspect_durable_collection(&key).unwrap();
    assert_eq!(recovered.checkpoint.requests_used(), 3);
    assert!(matches!(
        recovered.checkpoint.requests[2].progress,
        RequestProgress::InterruptedUnknown { .. }
    ));
    let execution = w
        .start_durable_collection(&key, 1, &owner, AT + 8)
        .unwrap()
        .unwrap();
    assert_eq!(execution.generation, 2);
    assert!(w.cancel_durable_collection(&key, 1, AT + 8).is_err());
    assert!(w.advance_durable_collection(&old, &owner, AT + 8).is_err());
    assert!(w
        .complete_durable_collection(&uncertain, &response(200, "late"), &owner, AT + 8)
        .is_err());
    let next = w
        .advance_durable_collection(&execution, &owner, AT + 9)
        .unwrap()
        .unwrap();
    assert_eq!(next.url, "https://example.com/two");
    assert_eq!(next.sequence, 3);
    w.complete_durable_collection(&next, &response(204, ""), &owner, AT + 10)
        .unwrap();
    assert!(w
        .advance_durable_collection(&execution, &owner, AT + 11)
        .unwrap()
        .is_none());
    assert_eq!(
        w.inspect_durable_collection(&key).unwrap().checkpoint.state,
        CollectionState::Partial
    );
}

#[test]
fn unknown_robots_is_fail_closed_and_recovery_requires_exclusive_ownership() {
    let (temp, mut w) = workspace();
    let owner = w.collection_ownership().unwrap();
    let (key, execution) = begin(&mut w, &owner);
    w.advance_durable_collection(&execution, &owner, AT + 2)
        .unwrap()
        .unwrap();
    let another = Workspace::open(temp.path().join("workspace")).unwrap();
    assert!(another.collection_ownership().is_err());
    assert_eq!(
        another
            .inspect_durable_collection(&key)
            .unwrap()
            .checkpoint
            .state,
        CollectionState::Running
    );
    w.recover_durable_collections(&owner, AT + 3).unwrap();
    let execution = w
        .start_durable_collection(&key, 1, &owner, AT + 4)
        .unwrap()
        .unwrap();
    assert!(w
        .advance_durable_collection(&execution, &owner, AT + 5)
        .unwrap()
        .is_none());
    let job = w.inspect_durable_collection(&key).unwrap();
    assert_eq!(job.checkpoint.requests_used(), 1);
    assert_eq!(job.checkpoint.state, CollectionState::Failed);
    assert!(!job.checkpoint.robots[0].usable);
    let (_other_temp, mut other) = workspace();
    let other_job = queue(&mut other);
    assert!(other
        .start_durable_collection(&other_job.id, 1, &owner, AT + 1)
        .is_err());
}

#[test]
fn immutable_deadline_budget_and_clock_cannot_reset_on_resume() {
    let (_temp, mut w) = workspace();
    let owner = w.collection_ownership().unwrap();
    let mut bounds = input();
    bounds.max_seconds = 1;
    let key = w.queue_durable_collection(bounds, &id(), AT).unwrap().id;
    w.start_durable_collection(&key, 1, &owner, AT + 1)
        .unwrap()
        .unwrap();
    w.recover_durable_collections(&owner, AT + 2).unwrap();
    let revision = w.revision().unwrap();
    assert!(w.start_durable_collection(&key, 1, &owner, AT).is_err());
    assert_eq!(w.revision().unwrap(), revision);
    assert!(w
        .start_durable_collection(&key, 1, &owner, AT + 1001)
        .unwrap()
        .is_none());
    let job = w.inspect_durable_collection(&key).unwrap();
    assert_eq!(job.checkpoint.state, CollectionState::QuotaExhausted);
    assert_eq!(job.checkpoint.deadline_at_ms, Some(AT + 1001));
    assert_eq!(job.checkpoint.requests_used(), 0);
    let mut limits = input();
    limits.max_requests = 1;
    let key = w.queue_durable_collection(limits, &id(), AT).unwrap().id;
    let execution = w
        .start_durable_collection(&key, 1, &owner, AT + 1)
        .unwrap()
        .unwrap();
    robots(&mut w, &execution, &owner);
    assert!(w
        .advance_durable_collection(&execution, &owner, AT + 4)
        .unwrap()
        .is_none());
    let job = w.inspect_durable_collection(&key).unwrap();
    assert_eq!(job.checkpoint.requests_used(), 1);
    assert_eq!(job.checkpoint.state, CollectionState::QuotaExhausted);
}

#[test]
fn cancellation_keeps_received_original_without_expansion_and_requires_acknowledgement() {
    let (_temp, mut w) = workspace();
    let owner = w.collection_ownership().unwrap();
    let queued = queue(&mut w);
    let cancelled = w.cancel_durable_collection(&queued.id, 1, AT + 1).unwrap();
    assert_eq!(cancelled.checkpoint.state, CollectionState::Cancelled);
    assert_eq!(cancelled.checkpoint.requests_used(), 0);
    let (key, execution) = begin(&mut w, &owner);
    robots(&mut w, &execution, &owner);
    let request = w
        .advance_durable_collection(&execution, &owner, AT + 4)
        .unwrap()
        .unwrap();
    w.cancel_durable_collection(&key, 1, AT + 5).unwrap();
    assert_eq!(
        w.inspect_durable_collection(&key).unwrap().checkpoint.state,
        CollectionState::Running
    );
    let body = "<p>retained after cancellation race</p><a href='/never'>never</a>";
    w.complete_durable_collection(&request, &response(200, body), &owner, AT + 6)
        .unwrap();
    let original = w
        .view()
        .unwrap()
        .evidence
        .into_iter()
        .find(|e| e.sha256 == hash(body.as_bytes()))
        .unwrap();
    assert!(original.text.is_none());
    let job = w
        .acknowledge_collection_stop(&execution, &owner, AT + 7)
        .unwrap();
    assert_eq!(job.checkpoint.state, CollectionState::Cancelled);
    assert_eq!(job.checkpoint.pages_retained, 0);
    assert!(job.checkpoint.frontier.is_empty());
    assert!(w
        .advance_durable_collection(&execution, &owner, AT + 8)
        .is_err());
}

#[test]
fn redirect_ancestry_hops_and_static_body_policy_are_replayed_from_originals() {
    let (_temp, mut w) = workspace();
    let owner = w.collection_ownership().unwrap();
    let (key, execution) = begin(&mut w, &owner);
    robots(&mut w, &execution, &owner);
    let request = w
        .advance_durable_collection(&execution, &owner, AT + 4)
        .unwrap()
        .unwrap();
    let redirect = CollectionResponse::Complete {
        status: 302,
        content_type: "text/html".into(),
        location: Some("/destination#fragment".into()),
        body: b"redirect original".to_vec(),
    };
    w.complete_durable_collection(&request, &redirect, &owner, AT + 5)
        .unwrap();
    let next = w
        .advance_durable_collection(&execution, &owner, AT + 6)
        .unwrap()
        .unwrap();
    assert_eq!(next.url, "https://example.com/destination");
    w.complete_durable_collection(
        &next,
        &response(
            200,
            "<a href='/child'>child</a><a href='https://other.example/no'>off-host</a>",
        ),
        &owner,
        AT + 7,
    )
    .unwrap();
    let job = w.inspect_durable_collection(&key).unwrap();
    assert_eq!(job.checkpoint.requests[2].entry.parent, Some(1));
    assert_eq!(job.checkpoint.requests[2].entry.hop, 0);
    assert_eq!(job.checkpoint.frontier.len(), 1);
    assert_eq!(job.checkpoint.frontier[0].hop, 1);
    let child = w
        .advance_durable_collection(&execution, &owner, AT + 8)
        .unwrap()
        .unwrap();
    w.complete_durable_collection(
        &child,
        &CollectionResponse::Incomplete {
            status: 200,
            content_type: "text/html".into(),
        },
        &owner,
        AT + 9,
    )
    .unwrap();
    assert!(w
        .advance_durable_collection(&execution, &owner, AT + 10)
        .unwrap()
        .is_none());
    let job = w.inspect_durable_collection(&key).unwrap();
    assert_eq!(job.checkpoint.state, CollectionState::Partial);
    assert!(matches!(
        job.checkpoint.requests[3].progress,
        RequestProgress::Settled {
            result: FetchRecord::Incomplete { .. },
            ..
        }
    ));
    assert_eq!(w.view().unwrap().evidence.len(), 3);
}

#[test]
fn altered_checkpoint_event_identity_and_original_fail_closed() {
    let (_temp, mut w) = workspace();
    let owner = w.collection_ownership().unwrap();
    let (key, execution) = begin(&mut w, &owner);
    robots(&mut w, &execution, &owner);
    let good = raw(&w, &key);
    for change in 0..5 {
        let mut bad = good.clone();
        match change {
            0 => bad.checkpoint.frontier[0].url = "https://other.example/steal".into(),
            1 => bad.checkpoint.requests[0].entry.hop = 2,
            2 => bad.checkpoint.deadline_at_ms = Some(AT + 9_000_000),
            3 => bad.id = id(),
            _ => {
                if let CollectionEvent::Complete {
                    result: FetchRecord::Complete { sha256, .. },
                    ..
                } = &mut bad.events[2]
                {
                    *sha256 = "0".repeat(64);
                }
            }
        }
        w.conn
            .execute(
                "UPDATE records SET body=? WHERE kind='collection_run' AND id=?",
                params![serde_json::to_string(&bad).unwrap(), key],
            )
            .unwrap();
        assert!(
            w.inspect_durable_collection(&key).is_err(),
            "change {change}"
        );
        corrupt(&w, &good);
    }
    let digest = hash(b"");
    let mut evidence: Evidence = get(&w.conn, "evidence", &digest).unwrap();
    let other = w.import("other.txt", b"unrelated").unwrap();
    evidence.sha256 = other;
    evidence.bytes = b"unrelated".len() as u64;
    w.conn
        .execute(
            "UPDATE records SET body=? WHERE kind='evidence' AND id=?",
            params![serde_json::to_string(&evidence).unwrap(), digest],
        )
        .unwrap();
    assert!(w.inspect_durable_collection(&key).is_err());
}

#[test]
fn backup_restore_preserves_requests_originals_and_prior_receipt_records() {
    let (temp, mut w) = workspace();
    let owner = w.collection_ownership().unwrap();
    // A historical record is copied unchanged; this foundation never rewrites its kind.
    w.conn.execute("INSERT INTO records(kind,id,body) VALUES('collection_receipt','legacy-sentinel','{\"untouched\":true}')",[]).unwrap();
    let (key, execution) = begin(&mut w, &owner);
    robots(&mut w, &execution, &owner);
    let request = w
        .advance_durable_collection(&execution, &owner, AT + 4)
        .unwrap()
        .unwrap();
    w.complete_durable_collection(
        &request,
        &CollectionResponse::Complete {
            status: 500,
            content_type: "application/octet-stream".into(),
            location: None,
            body: b"retained error bytes".to_vec(),
        },
        &owner,
        AT + 5,
    )
    .unwrap();
    let expected = w.inspect_durable_collection(&key).unwrap();
    let backup = w.backup().unwrap();
    let restored = Workspace::restore(&backup, &temp.path().join("restored")).unwrap();
    assert_eq!(restored.inspect_durable_collection(&key).unwrap(), expected);
    assert_eq!(restored.view().unwrap().evidence.len(), 2);
    assert_eq!(
        fs::read(
            restored
                .root
                .join("originals")
                .join(hash(b"retained error bytes"))
        )
        .unwrap(),
        b"retained error bytes"
    );
    assert!(restored.view().unwrap().observations.is_empty());
    assert_eq!(
        restored
            .conn
            .query_row::<String, _, _>(
                "SELECT body FROM records WHERE kind='collection_receipt'",
                [],
                |r| r.get(0)
            )
            .unwrap(),
        "{\"untouched\":true}"
    );
}

#[test]
fn terminal_empty_blocked_quota_failed_and_partial_results_remain_distinct() {
    for (robots_status, page_status, expected) in [
        (403, None, CollectionState::Blocked),
        (429, None, CollectionState::QuotaExhausted),
        (500, None, CollectionState::Failed),
        (404, Some(204), CollectionState::SuccessfulNoResults),
        (404, Some(500), CollectionState::Failed),
    ] {
        let (_temp, mut w) = workspace();
        let owner = w.collection_ownership().unwrap();
        let (key, execution) = begin(&mut w, &owner);
        let robots = w
            .advance_durable_collection(&execution, &owner, AT + 2)
            .unwrap()
            .unwrap();
        w.complete_durable_collection(&robots, &response(robots_status, ""), &owner, AT + 3)
            .unwrap();
        let page = w
            .advance_durable_collection(&execution, &owner, AT + 4)
            .unwrap();
        if let Some(status) = page_status {
            let page = page.unwrap();
            w.complete_durable_collection(&page, &response(status, ""), &owner, AT + 5)
                .unwrap();
            assert!(w
                .advance_durable_collection(&execution, &owner, AT + 6)
                .unwrap()
                .is_none());
        } else {
            assert!(page.is_none());
        }
        assert_eq!(
            w.inspect_durable_collection(&key).unwrap().checkpoint.state,
            expected
        );
    }
}

#[test]
fn completion_at_exact_request_limit_is_success_when_no_frontier_remains() {
    let (_temp, mut w) = workspace();
    let owner = w.collection_ownership().unwrap();
    let mut exact = input();
    exact.max_requests = 2;
    exact.max_hops = 0;
    let key = w.queue_durable_collection(exact, &id(), AT).unwrap().id;
    let execution = w
        .start_durable_collection(&key, 1, &owner, AT + 1)
        .unwrap()
        .unwrap();
    robots(&mut w, &execution, &owner);
    let request = w
        .advance_durable_collection(&execution, &owner, AT + 4)
        .unwrap()
        .unwrap();
    w.complete_durable_collection(
        &request,
        &response(200, "<a href='/beyond-hop'>bounded</a>"),
        &owner,
        AT + 5,
    )
    .unwrap();
    assert!(w
        .advance_durable_collection(&execution, &owner, AT + 6)
        .unwrap()
        .is_none());
    let job = w.inspect_durable_collection(&key).unwrap();
    assert_eq!(job.checkpoint.state, CollectionState::Successful);
    assert_eq!(job.checkpoint.requests_used(), 2);
    assert!(job.checkpoint.frontier.is_empty());
}

#[test]
fn wrong_completion_ticket_future_event_and_oversized_record_are_rejected_without_writes() {
    let (_temp, mut w) = workspace();
    let owner = w.collection_ownership().unwrap();
    let (key, execution) = begin(&mut w, &owner);
    let request = w
        .advance_durable_collection(&execution, &owner, AT + 2)
        .unwrap()
        .unwrap();
    let revision = w.revision().unwrap();
    let mut wrong = request.clone();
    wrong.run.lease = id();
    assert!(w
        .complete_durable_collection(&wrong, &response(404, ""), &owner, AT + 3)
        .is_err());
    let mut wrong = request.clone();
    wrong.url = "https://other.example/stolen".into();
    assert!(w
        .complete_durable_collection(&wrong, &response(404, ""), &owner, AT + 3)
        .is_err());
    let mut wrong = request.clone();
    wrong.sequence = 49;
    assert!(w
        .complete_durable_collection(&wrong, &response(404, ""), &owner, AT + 3)
        .is_err());
    assert_eq!(w.revision().unwrap(), revision);
    w.complete_durable_collection(&request, &response(404, ""), &owner, AT + 3)
        .unwrap();
    let good = raw(&w, &key);
    let mut invalid = good.clone();
    if let CollectionEvent::Complete { at_ms, .. } = &mut invalid.events[2] {
        *at_ms = i64::MAX;
    }
    corrupt(&w, &invalid);
    assert!(w.inspect_durable_collection(&key).is_err());
    corrupt(&w, &good);
    let mut json = serde_json::to_value(&good).unwrap();
    json["unknown_field"] = json!(true);
    w.conn
        .execute(
            "UPDATE records SET body=? WHERE kind='collection_run' AND id=?",
            params![serde_json::to_string(&json).unwrap(), key],
        )
        .unwrap();
    assert!(w.inspect_durable_collection(&key).is_err());
    json["unknown_field"] = json!("x".repeat(MAX_RECORD_BYTES));
    w.conn
        .execute(
            "UPDATE records SET body=? WHERE kind='collection_run' AND id=?",
            params![serde_json::to_string(&json).unwrap(), key],
        )
        .unwrap();
    assert!(w
        .inspect_durable_collection(&key)
        .unwrap_err()
        .to_string()
        .contains("size bound"));
}

#[test]
fn recovered_cancel_and_pre_reservation_crash_have_no_phantom_request() {
    let (_temp, mut w) = workspace();
    let owner = w.collection_ownership().unwrap();
    let (key, old) = begin(&mut w, &owner);
    w.recover_durable_collections(&owner, AT + 2).unwrap();
    let resumed = w
        .start_durable_collection(&key, 1, &owner, AT + 3)
        .unwrap()
        .unwrap();
    assert_eq!(resumed.generation, 2);
    assert_eq!(
        w.inspect_durable_collection(&key)
            .unwrap()
            .checkpoint
            .requests_used(),
        0
    );
    assert!(w.advance_durable_collection(&old, &owner, AT + 4).is_err());
    let request = w
        .advance_durable_collection(&resumed, &owner, AT + 4)
        .unwrap()
        .unwrap();
    assert_eq!(request.sequence, 0);
    w.recover_durable_collections(&owner, AT + 5).unwrap();
    let cancelled = w.cancel_durable_collection(&key, 2, AT + 6).unwrap();
    assert_eq!(cancelled.checkpoint.state, CollectionState::Cancelled);
    assert!(matches!(
        cancelled.checkpoint.requests[0].progress,
        RequestProgress::InterruptedUnknown { .. }
    ));
    assert!(w.start_durable_collection(&key, 2, &owner, AT + 7).is_err());
}

#[cfg(debug_assertions)]
#[test]
fn actual_legacy_receipts_and_existing_observations_are_not_rewritten() {
    let (_temp, mut w) = workspace();
    w.seed_collection_review().unwrap();
    let source = w
        .import("existing.txt", b"Existing synthetic observation")
        .unwrap();
    let entity = w
        .add_entity(
            EntityInput {
                name: "Synthetic prior entity".into(),
                kind: EntityKind::Person,
                identifiers: vec![],
            },
            "Prior observation fixture",
            w.revision().unwrap(),
        )
        .unwrap();
    w.add_observation(
        ObservationInput {
            entity_id: entity,
            field: "prior note".into(),
            value: "Existing synthetic observation".into(),
            anchor: SourceAnchor::Text {
                evidence_id: source,
                line_start: 1,
                line_end: 1,
            },
        },
        "Prior observation fixture",
        w.revision().unwrap(),
    )
    .unwrap();
    let receipts: Vec<(String, String)> = w
        .conn
        .prepare("SELECT id,body FROM records WHERE kind='collection_receipt' ORDER BY sequence")
        .unwrap()
        .query_map([], |r| Ok((r.get(0)?, r.get(1)?)))
        .unwrap()
        .map(|r| r.unwrap())
        .collect();
    for (key, _) in &receipts {
        w.collection_receipt(key).unwrap();
    }
    let history: u64 = w
        .conn
        .query_row(
            "SELECT count(*) FROM history WHERE kind='collection_receipt'",
            [],
            |r| r.get(0),
        )
        .unwrap();
    let observations = serde_json::to_value(w.view().unwrap().observations).unwrap();
    let owner = w.collection_ownership().unwrap();
    let (_key, execution) = begin(&mut w, &owner);
    robots(&mut w, &execution, &owner);
    let request = w
        .advance_durable_collection(&execution, &owner, AT + 4)
        .unwrap()
        .unwrap();
    w.complete_durable_collection(
        &request,
        &response(200, "new synthetic source"),
        &owner,
        AT + 5,
    )
    .unwrap();
    for (key, body) in receipts {
        assert_eq!(
            w.conn
                .query_row::<String, _, _>(
                    "SELECT body FROM records WHERE kind='collection_receipt' AND id=?",
                    [&key],
                    |r| r.get(0)
                )
                .unwrap(),
            body
        );
        w.collection_receipt(&key).unwrap();
    }
    assert_eq!(
        w.conn
            .query_row::<u64, _, _>(
                "SELECT count(*) FROM history WHERE kind='collection_receipt'",
                [],
                |r| r.get(0)
            )
            .unwrap(),
        history
    );
    assert_eq!(
        serde_json::to_value(w.view().unwrap().observations).unwrap(),
        observations
    );
}

#[test]
#[ignore = "requires a separately built source-pinned pre-foundation ew-dev binary"]
fn prior_binary_opens_and_backs_up_new_checkpoint_and_every_original() {
    use std::process::{Command, Stdio};
    let previous =
        std::env::var_os("EW_PREVIOUS_READER").expect("provide source-pinned previous reader");
    let (temp, mut w) = workspace();
    let owner = w.collection_ownership().unwrap();
    let (key, execution) = begin(&mut w, &owner);
    robots(&mut w, &execution, &owner);
    let request = w
        .advance_durable_collection(&execution, &owner, AT + 4)
        .unwrap()
        .unwrap();
    w.complete_durable_collection(
        &request,
        &response(500, "old reader must retain error original"),
        &owner,
        AT + 5,
    )
    .unwrap();
    let expected = w.inspect_durable_collection(&key).unwrap();
    let revision = w.revision().unwrap();
    let mut child = Command::new(previous)
        .arg(&w.root)
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .unwrap();
    child
        .stdin
        .take()
        .unwrap()
        .write_all(b"{\"action\":\"backup\"}")
        .unwrap();
    let output = child.wait_with_output().unwrap();
    assert!(output.status.success(), "previous reader failed");
    let result: Value = serde_json::from_slice(&output.stdout).unwrap();
    let backup = w
        .root
        .join("backups")
        .join(result["backup"].as_str().expect("backup identity"));
    assert_eq!(w.revision().unwrap(), revision);
    assert_eq!(w.inspect_durable_collection(&key).unwrap(), expected);
    let restored =
        Workspace::restore(&backup, &temp.path().join("restored-by-previous-reader")).unwrap();
    assert_eq!(restored.inspect_durable_collection(&key).unwrap(), expected);
    assert_eq!(restored.view().unwrap().evidence.len(), 2);
}

#[test]
fn identical_original_has_distinct_acquisitions_without_erasing_prior_text() {
    let (_temp, mut w) = workspace();
    let owner = w.collection_ownership().unwrap();
    let mut selected = input();
    selected.urls.push("https://example.com/error".into());
    let job = w.queue_durable_collection(selected, &id(), AT).unwrap();
    let execution = w
        .start_durable_collection(&job.id, 1, &owner, AT + 1)
        .unwrap()
        .unwrap();
    robots(&mut w, &execution, &owner);
    let first = w
        .advance_durable_collection(&execution, &owner, AT + 4)
        .unwrap()
        .unwrap();
    w.complete_durable_collection(
        &first,
        &response(200, "same synthetic bytes"),
        &owner,
        AT + 5,
    )
    .unwrap();
    let second = w
        .advance_durable_collection(&execution, &owner, AT + 6)
        .unwrap()
        .unwrap();
    w.complete_durable_collection(
        &second,
        &response(500, "same synthetic bytes"),
        &owner,
        AT + 7,
    )
    .unwrap();
    let digest = hash(b"same synthetic bytes");
    let original: Evidence = get(&w.conn, "evidence", &digest).unwrap();
    assert_eq!(original.text.as_deref(), Some("same synthetic bytes"));
    assert_eq!(original.origin_group, digest);
    assert_eq!(original.acquisitions.len(), 2);
    assert_ne!(original.acquisitions[0].url, original.acquisitions[1].url);
    let before = w.revision().unwrap();
    w.complete_durable_collection(
        &second,
        &response(500, "same synthetic bytes"),
        &owner,
        AT + 7,
    )
    .unwrap();
    assert_eq!(w.revision().unwrap(), before);
    assert!(w
        .advance_durable_collection(&execution, &owner, AT + 8)
        .unwrap()
        .is_none());
    assert_eq!(
        w.inspect_durable_collection(&job.id)
            .unwrap()
            .checkpoint
            .state,
        CollectionState::Partial
    );
}

#[test]
fn known_failed_transport_remains_charged_without_an_original_or_phantom_retry() {
    let (_temp, mut w) = workspace();
    let owner = w.collection_ownership().unwrap();
    let (key, execution) = begin(&mut w, &owner);
    robots(&mut w, &execution, &owner);
    let request = w
        .advance_durable_collection(&execution, &owner, AT + 4)
        .unwrap()
        .unwrap();
    w.complete_durable_collection(
        &request,
        &CollectionResponse::Failed(TransportFailure::Network),
        &owner,
        AT + 5,
    )
    .unwrap();
    assert!(w
        .advance_durable_collection(&execution, &owner, AT + 6)
        .unwrap()
        .is_none());
    let job = w.inspect_durable_collection(&key).unwrap();
    assert_eq!(job.checkpoint.requests_used(), 2);
    assert_eq!(job.checkpoint.state, CollectionState::Failed);
    assert!(!job.checkpoint.saw_unknown);
    assert_eq!(w.view().unwrap().evidence.len(), 1);
    assert!(matches!(
        job.checkpoint.requests[1].progress,
        RequestProgress::Settled {
            result: FetchRecord::Failed { .. },
            ..
        }
    ));
}

#[test]
fn cancel_then_crash_recovers_terminal_without_resetting_or_stranding_request() {
    let (_temp, mut w) = workspace();
    let owner = w.collection_ownership().unwrap();
    let (key, execution) = begin(&mut w, &owner);
    let request = w
        .advance_durable_collection(&execution, &owner, AT + 2)
        .unwrap()
        .unwrap();
    w.cancel_durable_collection(&key, 1, AT + 3).unwrap();
    assert_eq!(w.recover_durable_collections(&owner, AT + 4).unwrap(), 1);
    let recovered = w.inspect_durable_collection(&key).unwrap();
    assert_eq!(recovered.checkpoint.state, CollectionState::Cancelled);
    assert_eq!(recovered.checkpoint.requests_used(), 1);
    assert!(recovered.checkpoint.cancellation_requested);
    assert!(
        matches!(recovered.checkpoint.requests[0].progress,RequestProgress::InterruptedUnknown{recovered_at_ms: at} if at==AT+4)
    );
    let revision = w.revision().unwrap();
    assert_eq!(
        w.cancel_durable_collection(&key, 1, AT + 5).unwrap(),
        recovered
    );
    assert_eq!(w.revision().unwrap(), revision);
    assert!(w
        .complete_durable_collection(&request, &response(404, ""), &owner, AT + 5)
        .is_err());
    assert!(w.start_durable_collection(&key, 1, &owner, AT + 5).is_err());
}

#[test]
fn escaped_response_metadata_cannot_publish_a_checkpoint_that_its_reader_refuses() {
    let (_temp, mut w) = workspace();
    let owner = w.collection_ownership().unwrap();
    let (key, execution) = begin(&mut w, &owner);
    robots(&mut w, &execution, &owner);
    let request = w
        .advance_durable_collection(&execution, &owner, AT + 4)
        .unwrap()
        .unwrap();
    let before = w.inspect_durable_collection(&key).unwrap();
    let revision = w.revision().unwrap();
    let history = history_count(&w);
    let body = vec![0; crate::collection::PAGE_BYTES];
    let digest = hash(&body);
    let result = w.complete_durable_collection(
        &request,
        &CollectionResponse::Complete {
            status: 200,
            content_type: "text/plain".into(),
            location: None,
            body,
        },
        &owner,
        AT + 5,
    );
    assert!(result.unwrap_err().to_string().contains("metadata exceeds"));
    assert_eq!(w.revision().unwrap(), revision);
    assert_eq!(history_count(&w), history);
    assert_eq!(w.inspect_durable_collection(&key).unwrap(), before);
    assert_eq!(w.view().unwrap().evidence.len(), 1);
    assert!(w.root.join("originals").join(digest).exists());
}
