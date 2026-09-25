use super::*;
use crate::{
    collection_jobs::{CollectionProtocol, CollectionState, RequestProgress},
    collection_settlement::{HttpDelivery, ReceiptOutcome},
    collection_transport::{
        tests::{canonical_before_http, canonical_tls, canonical_truncated_tls},
        Outcome, Phase, StopReason,
    },
};
use rusqlite::Connection;
use std::{path::PathBuf, sync::Arc};

struct Fixture {
    workspace: Arc<Mutex<Workspace>>,
    owner: CollectionOwnership,
    driver: CollectionDriver,
    id: String,
    path: PathBuf,
    _temp: tempfile::TempDir,
}
impl Fixture {
    fn new() -> Self {
        Self::protocol(CollectionProtocol::SyntheticV2)
    }
    fn v4() -> Self {
        Self::protocol(CollectionProtocol::SyntheticV4)
    }
    fn protocol(protocol: CollectionProtocol) -> Self {
        let temp = tempfile::tempdir_in(std::env::temp_dir().canonicalize().unwrap()).unwrap();
        let path = temp.path().join("case");
        let mut workspace = Workspace::open(&path).unwrap();
        let owner = workspace.collection_ownership().unwrap();
        let job = workspace
            .queue_collection_protocol(
                CollectionInput {
                    urls: vec!["https://collection.invalid/start".into()],
                    max_hops: 2,
                    max_requests: 8,
                    max_seconds: 60,
                },
                &uuid::Uuid::new_v4().to_string(),
                chrono::Utc::now().timestamp_millis(),
                protocol,
            )
            .unwrap();
        let ticket = workspace
            .start_durable_collection(&job.id, 1, &owner, chrono::Utc::now().timestamp_millis())
            .unwrap()
            .unwrap();
        let driver = CollectionDriver::attach(&workspace, &owner, ticket).unwrap();
        Self {
            workspace: Arc::new(Mutex::new(workspace)),
            owner,
            driver,
            id: job.id,
            path,
            _temp: temp,
        }
    }
    fn inspect(&self) -> DurableCollectionJob {
        self.workspace
            .lock()
            .unwrap()
            .inspect_durable_collection(&self.id)
            .unwrap()
    }
    fn sql(&self, sql: &str) {
        Connection::open(self.path.join("workspace.db"))
            .unwrap()
            .execute_batch(sql)
            .unwrap();
    }
    fn response(
        &mut self,
        status: u16,
        headers: &str,
        body: &[u8],
        cancel_complete: bool,
    ) -> PendingSettlement {
        let cancel = CancellationToken::default();
        let mut response = format!("HTTP/1.1 {status} Synthetic\r\nContent-Length: {}\r\nContent-Type: text/html\r\n{headers}\r\n", body.len()).into_bytes();
        response.extend_from_slice(body);
        let path = self.path.clone();
        let id = self.id.clone();
        let workspace = &self.workspace;
        self.driver
            .next_with(
                workspace,
                &self.owner,
                &cancel,
                |ticket, input, window, pacing, token| {
                    // A second connection sees the committed reservation before DNS/TLS;
                    // the workspace mutex is also free while the owned transport runs.
                    assert!(workspace.try_lock().is_ok());
                    let db = Connection::open(path.join("workspace.db")).unwrap();
                    let body: String = db
                        .query_row(
                            "SELECT body FROM records WHERE kind='collection_run' AND id=?",
                            [&id],
                            |row| row.get(0),
                        )
                        .unwrap();
                    let stored: DurableCollectionJob = serde_json::from_str(&body).unwrap();
                    assert_eq!(stored.checkpoint.requests_used(), ticket.sequence + 1);
                    assert!(matches!(
                        stored.checkpoint.requests[ticket.sequence as usize].progress,
                        RequestProgress::Reserved
                    ));
                    canonical_tls(
                        ticket,
                        input,
                        window,
                        pacing,
                        token,
                        response,
                        cancel_complete,
                    )
                },
            )
            .unwrap()
            .unwrap_or_else(|| {
                panic!(
                    "No reserved request for synthetic status {status}: {:?}",
                    self.inspect()
                )
            })
    }
    fn settle(&mut self, status: u16, headers: &str, body: &[u8]) -> DurableCollectionJob {
        let pending = self.response(status, headers, body, false);
        pending
            .settle(&mut self.workspace.lock().unwrap(), &self.owner)
            .unwrap()
    }
}

#[test]
fn durable_tls_lifecycle_retains_robots_redirect_page_ancestry_and_exact_replay() {
    let mut f = Fixture::new();
    let deadline = f.inspect().checkpoint.deadline_at_ms;
    f.settle(404, "", b"");
    f.settle(302, "Location: /landing\r\n", b"redirect-body");
    f.settle(
        200,
        "",
        b"<html><body>Harmless synthetic page <a href='/next'>next</a></body></html>",
    );
    let pending = f.response(
        200,
        "Location: /informational-only\r\n",
        b"<html><body>Second synthetic page</body></html>",
        false,
    );
    let job = pending
        .settle(&mut f.workspace.lock().unwrap(), &f.owner)
        .unwrap();
    assert_eq!(job.checkpoint.requests_used(), 4);
    let revision = f.workspace.lock().unwrap().revision().unwrap();
    pending
        .settle(&mut f.workspace.lock().unwrap(), &f.owner)
        .unwrap();
    assert_eq!(f.workspace.lock().unwrap().revision().unwrap(), revision);
    assert!(f
        .driver
        .next_with(
            &f.workspace,
            &f.owner,
            &CancellationToken::default(),
            |_, _, _, _, _| panic!("frontier exhausted")
        )
        .unwrap()
        .is_none());
    let job = f.inspect();
    assert_eq!(job.checkpoint.state, CollectionState::Successful);
    assert_eq!(job.checkpoint.deadline_at_ms, deadline);
    assert_eq!(job.checkpoint.requests[2].entry.parent, Some(1));
    assert_eq!(job.checkpoint.requests[3].entry.parent, Some(2));
    for request in &job.checkpoint.requests {
        let RequestProgress::Observed { receipt } = &request.progress else {
            panic!("missing receipt")
        };
        assert!(receipt.locally_quiescent);
        assert_eq!(receipt.http_delivery, HttpDelivery::MayHaveBeenSent);
        assert_eq!(
            receipt.resolved.as_ref().unwrap().method,
            "synthetic_fixed_candidates"
        );
        assert!(matches!(receipt.outcome, ReceiptOutcome::Complete { .. }));
    }
    let mut workspace = f.workspace.lock().unwrap();
    let view = workspace.view().unwrap();
    assert_eq!(view.evidence.len(), 4);
    assert_eq!(view.evidence.iter().filter(|e| e.text.is_some()).count(), 2);
    assert!(view.observations.is_empty());
    let backup = workspace.backup().unwrap();
    let restored = Workspace::restore(&backup, &f._temp.path().join("restored")).unwrap();
    assert_eq!(restored.inspect_durable_collection(&f.id).unwrap(), job);
}

#[test]
fn reserve_and_publish_faults_never_repeat_transport_or_refund_the_charge() {
    let mut f = Fixture::new();
    f.sql("CREATE TRIGGER reject_reservation BEFORE UPDATE ON records WHEN NEW.kind='collection_run' AND json_array_length(NEW.body,'$.checkpoint.requests')>json_array_length(OLD.body,'$.checkpoint.requests') BEGIN SELECT RAISE(ABORT,'synthetic reserve fault'); END;");
    assert!(f
        .driver
        .next_with(
            &f.workspace,
            &f.owner,
            &CancellationToken::default(),
            |_, _, _, _, _| panic!("uncommitted request")
        )
        .is_err());
    assert_eq!(f.inspect().checkpoint.requests_used(), 0);
    f.sql("DROP TRIGGER reject_reservation;");
    let pending = f.response(404, "", b"", false);
    let reserved_revision = f.workspace.lock().unwrap().revision().unwrap();
    f.sql("CREATE TRIGGER reject_evidence BEFORE INSERT ON records WHEN NEW.kind='evidence' BEGIN SELECT RAISE(ABORT,'synthetic publish fault'); END;");
    assert!(pending
        .settle(&mut f.workspace.lock().unwrap(), &f.owner)
        .is_err());
    assert_eq!(
        f.workspace.lock().unwrap().revision().unwrap(),
        reserved_revision
    );
    assert!(matches!(
        f.inspect().checkpoint.requests[0].progress,
        RequestProgress::Reserved
    ));
    assert!(f
        .workspace
        .lock()
        .unwrap()
        .view()
        .unwrap()
        .evidence
        .is_empty());
    assert!(f
        .driver
        .next_with(
            &f.workspace,
            &f.owner,
            &CancellationToken::default(),
            |_, _, _, _, _| panic!("unresolved request cannot rerun")
        )
        .is_err());
    f.sql("DROP TRIGGER reject_evidence;");
    pending
        .settle(&mut f.workspace.lock().unwrap(), &f.owner)
        .unwrap();
    assert_eq!(f.inspect().checkpoint.requests_used(), 1);
    assert_eq!(
        f.workspace.lock().unwrap().view().unwrap().evidence[0]
            .acquisitions
            .len(),
        1
    );
}

#[test]
fn completed_tls_bytes_survive_cancel_race_without_text_or_expansion() {
    let mut f = Fixture::new();
    f.settle(404, "", b"");
    let pending = f.response(200, "", b"<a href='/must-not-fetch'>Synthetic</a>", true);
    assert!(matches!(
        pending.observation.outcome,
        Outcome::Complete { .. }
    ));
    assert_eq!(
        pending.observation.stop_observed,
        Some(StopReason::Cancelled)
    );
    let job = pending
        .settle(&mut f.workspace.lock().unwrap(), &f.owner)
        .unwrap();
    assert_eq!(job.checkpoint.state, CollectionState::Cancelled);
    assert_eq!(job.checkpoint.requests_used(), 2);
    assert_eq!(job.checkpoint.pages_retained, 0);
    assert!(job.checkpoint.frontier.is_empty());
    let view = f.workspace.lock().unwrap().view().unwrap();
    assert_eq!(view.evidence.len(), 2);
    assert!(view.evidence.iter().all(|e| e.text.is_none()));
}

#[test]
fn encoded_robots_and_pages_are_retained_without_rules_or_text() {
    for robots in [true, false] {
        let mut f = Fixture::new();
        if !robots {
            f.settle(404, "", b"");
        }
        f.settle(
            200,
            "Content-Encoding: gzip\r\n",
            b"User-agent: *\nAllow: /\n<a href='/hidden'>synthetic</a>",
        );
        assert!(f
            .driver
            .next_with(
                &f.workspace,
                &f.owner,
                &CancellationToken::default(),
                |_, _, _, _, _| panic!("encoded bytes are not executable policy or frontier")
            )
            .unwrap()
            .is_none());
        assert_eq!(f.inspect().checkpoint.state, CollectionState::Blocked);
        let view = f.workspace.lock().unwrap().view().unwrap();
        assert!(!view.evidence.is_empty());
        assert!(view.evidence.iter().all(|e| e.text.is_none()));
    }
}

#[test]
fn lost_response_and_reservation_survive_restart_as_charged_unknown_without_retry() {
    for received in [false, true] {
        let mut f = Fixture::new();
        let execution = f.driver.execution.clone();
        if received {
            drop(f.response(404, "", b"", false));
        } else {
            f.workspace
                .lock()
                .unwrap()
                .advance_durable_collection(
                    &execution,
                    &f.owner,
                    chrono::Utc::now().timestamp_millis(),
                )
                .unwrap()
                .unwrap();
        }
        let before = f.inspect();
        let Fixture {
            workspace,
            owner,
            driver,
            id,
            path,
            _temp,
        } = f;
        drop(driver);
        drop(workspace);
        drop(owner);
        let mut reopened = Workspace::open(path).unwrap();
        assert_eq!(reopened.inspect_durable_collection(&id).unwrap(), before);
        let owner = reopened.collection_ownership().unwrap();
        reopened
            .recover_durable_collections(&owner, chrono::Utc::now().timestamp_millis())
            .unwrap();
        let recovered = reopened.inspect_durable_collection(&id).unwrap();
        assert_eq!(recovered.checkpoint.requests_used(), 1);
        assert!(matches!(
            recovered.checkpoint.requests[0].progress,
            RequestProgress::InterruptedUnknown { .. }
        ));
        assert!(reopened.view().unwrap().evidence.is_empty());
        let resumed = reopened
            .start_durable_collection(&id, 1, &owner, chrono::Utc::now().timestamp_millis())
            .unwrap()
            .unwrap();
        assert_eq!(
            reopened
                .inspect_durable_collection(&id)
                .unwrap()
                .checkpoint
                .deadline_at_ms,
            before.checkpoint.deadline_at_ms
        );
        let mut driver = CollectionDriver::attach(&reopened, &owner, resumed).unwrap();
        let workspace = Mutex::new(reopened);
        assert!(driver
            .next_with(
                &workspace,
                &owner,
                &CancellationToken::default(),
                |_, _, _, _, _| panic!("unknown robots cannot be silently reacquired")
            )
            .unwrap()
            .is_none());
        assert_eq!(
            workspace
                .lock()
                .unwrap()
                .inspect_durable_collection(&id)
                .unwrap()
                .checkpoint
                .requests_used(),
            1
        );
    }
}

#[test]
fn truncated_tls_body_retains_headers_and_unknown_delivery_without_phantom_original() {
    let mut f = Fixture::new();
    f.settle(404, "", b"");
    let before = f.workspace.lock().unwrap().view().unwrap().evidence;
    let pending = f.driver.next_with(&f.workspace, &f.owner, &CancellationToken::default(), |ticket, input, window, pacing, token| {
        canonical_truncated_tls(ticket, input, window, pacing, token,
            b"HTTP/1.1 200 OK\r\nContent-Type: text/html\r\nContent-Length: 500\r\n\r\n<a href='/incomplete'>partial".to_vec())
    }).unwrap().unwrap();
    assert!(matches!(
        pending.observation.outcome,
        Outcome::Stopped {
            reason: StopReason::Network,
            head: Some(_)
        }
    ));
    let job = pending
        .settle(&mut f.workspace.lock().unwrap(), &f.owner)
        .unwrap();
    let RequestProgress::Observed { receipt } = &job.checkpoint.requests[1].progress else {
        panic!()
    };
    assert_eq!(receipt.phase, Phase::Body);
    assert_eq!(receipt.http_delivery, HttpDelivery::MayHaveBeenSent);
    assert!(matches!(receipt.outcome, ReceiptOutcome::Stopped { .. }));
    assert!(!serde_json::to_string(receipt).unwrap().contains("sha256"));
    assert_eq!(
        serde_json::to_value(f.workspace.lock().unwrap().view().unwrap().evidence).unwrap(),
        serde_json::to_value(before).unwrap()
    );
    assert_eq!(job.checkpoint.pages_retained, 0);
    assert!(job.checkpoint.frontier.is_empty());
    assert!(f
        .driver
        .next_with(
            &f.workspace,
            &f.owner,
            &CancellationToken::default(),
            |_, _, _, _, _| panic!("truncated page cannot expand")
        )
        .unwrap()
        .is_none());
    assert_eq!(f.inspect().checkpoint.state, CollectionState::Failed);
}

#[test]
fn cancellation_before_reservation_is_uncharged_but_after_reservation_is_never_refunded() {
    let mut f = Fixture::new();
    let token = CancellationToken::default();
    token.cancel();
    assert!(f
        .driver
        .next_with(&f.workspace, &f.owner, &token, |_, _, _, _, _| panic!(
            "cancel before reservation"
        ))
        .unwrap()
        .is_none());
    assert_eq!(f.inspect().checkpoint.state, CollectionState::Cancelled);
    assert_eq!(f.inspect().checkpoint.requests_used(), 0);

    let mut f = Fixture::new();
    let token = CancellationToken::default();
    let pending = f
        .driver
        .next_with(
            &f.workspace,
            &f.owner,
            &token,
            |ticket, input, window, pacing, token| {
                token.cancel();
                canonical_before_http(ticket, input, window, pacing, token)
            },
        )
        .unwrap()
        .unwrap();
    let job = pending
        .settle(&mut f.workspace.lock().unwrap(), &f.owner)
        .unwrap();
    assert_eq!(job.checkpoint.state, CollectionState::Cancelled);
    assert_eq!(job.checkpoint.requests_used(), 1);
    let RequestProgress::Observed { receipt } = &job.checkpoint.requests[0].progress else {
        panic!()
    };
    assert_eq!(receipt.http_delivery, HttpDelivery::DefinitivelyBeforeHttp);
    assert!(matches!(
        receipt.outcome,
        ReceiptOutcome::Stopped {
            reason: StopReason::Cancelled,
            head: None
        }
    ));
    assert!(f
        .workspace
        .lock()
        .unwrap()
        .view()
        .unwrap()
        .evidence
        .is_empty());
}

#[test]
fn exhausted_segment_deadline_cannot_be_extended_by_another_reserved_request() {
    let mut f = Fixture::new();
    let deadline = f.inspect().checkpoint.deadline_at_ms;
    // The segment clock can expire while the wall clock is still before its deadline.
    f.driver.monotonic_deadline = Instant::now() - Duration::from_secs(1);
    let pending = f
        .driver
        .next_with(
            &f.workspace,
            &f.owner,
            &CancellationToken::default(),
            canonical_before_http,
        )
        .unwrap()
        .unwrap();
    let job = pending
        .settle(&mut f.workspace.lock().unwrap(), &f.owner)
        .unwrap();
    assert_eq!(job.checkpoint.state, CollectionState::QuotaExhausted);
    assert_eq!(job.checkpoint.deadline_at_ms, deadline);
    assert_eq!(job.checkpoint.requests_used(), 1);
    let RequestProgress::Observed { receipt } = &job.checkpoint.requests[0].progress else {
        panic!()
    };
    assert_eq!(receipt.http_delivery, HttpDelivery::DefinitivelyBeforeHttp);
    assert!(receipt.stopped_for(StopReason::Deadline));
    assert!(f
        .workspace
        .lock()
        .unwrap()
        .view()
        .unwrap()
        .evidence
        .is_empty());
}

#[test]
fn pending_response_cannot_cross_ownership_lifetimes_even_before_recovery_revokes_lease() {
    let mut f = Fixture::new();
    let pending = f.response(404, "", b"", false);
    let before = f.inspect();
    let Fixture {
        workspace,
        owner,
        mut driver,
        id,
        path,
        _temp,
    } = f;
    drop(workspace);
    drop(owner);
    let mut reopened = Workspace::open(path).unwrap();
    let owner = reopened.collection_ownership().unwrap();
    let revision = reopened.revision().unwrap();
    assert!(pending.settle(&mut reopened, &owner).is_err());
    assert_eq!(reopened.revision().unwrap(), revision);
    assert_eq!(reopened.inspect_durable_collection(&id).unwrap(), before);
    let workspace = Mutex::new(reopened);
    assert!(driver
        .next_with(
            &workspace,
            &owner,
            &CancellationToken::default(),
            |_, _, _, _, _| panic!("previous owner cannot execute")
        )
        .is_err());
    let mut reopened = workspace.into_inner().unwrap();
    reopened
        .recover_durable_collections(&owner, chrono::Utc::now().timestamp_millis())
        .unwrap();
    assert!(pending.settle(&mut reopened, &owner).is_err());
    let after = reopened.inspect_durable_collection(&id).unwrap();
    assert_eq!(after.checkpoint.requests_used(), 1);
    assert!(matches!(
        after.checkpoint.requests[0].progress,
        RequestProgress::InterruptedUnknown { .. }
    ));
    assert!(reopened.view().unwrap().evidence.is_empty());
}

#[test]
fn v4_driver_reservation_replays_once_outside_mutex_and_keeps_committed_tls_charge() {
    let mut f = Fixture::v4();
    f.settle(404, "", b"missing");
    f.settle(200, "", b"<p>First</p><a href='/next'>Next</a>");
    let deadline = f.inspect().checkpoint.deadline_at_ms;
    let monotonic = f.driver.monotonic_deadline;
    let workspace = f.workspace.clone();
    f.driver.reservation_hook = Some(Box::new(move || {
        // Hook is after capture, before replay. A separate canonical write commits
        // under the same mutex, proving neither that lock nor its read tx escaped.
        let mut w = workspace
            .try_lock()
            .expect("reservation replay holds workspace mutex");
        w.import("independent.txt", b"Unrelated revision advancement")
            .unwrap();
    }));
    let count = crate::collection_machine::replay_calls();
    let pending = f.response(200, "", b"<p>Second</p>", false);
    assert_eq!(crate::collection_machine::replay_calls() - count, 1);
    assert_eq!(pending.request.sequence, 2);
    assert_eq!(f.driver.monotonic_deadline, monotonic);
    assert_eq!(f.inspect().checkpoint.deadline_at_ms, deadline);
    let complete = pending
        .settle(&mut f.workspace.lock().unwrap(), &f.owner)
        .unwrap();
    assert_eq!(complete.checkpoint.requests_used(), 3);
}

#[test]
fn v4_driver_canonical_cancel_during_paused_replay_never_charges_or_fetches_again() {
    let mut f = Fixture::v4();
    f.settle(404, "", b"missing");
    f.settle(200, "", b"<p>First</p><a href='/next'>Next</a>");
    let before = f.inspect();
    let workspace = f.workspace.clone();
    let job = f.id.clone();
    let (entered_tx, entered_rx) = std::sync::mpsc::channel();
    let (release_tx, release_rx) = std::sync::mpsc::channel();
    f.driver.reservation_hook = Some(Box::new(move || {
        entered_tx.send(()).unwrap();
        release_rx.recv_timeout(Duration::from_secs(10)).unwrap();
    }));
    let join = std::thread::spawn(move || {
        let token = CancellationToken::default();
        let count = crate::collection_machine::replay_calls();
        let result = f
            .driver
            .next_with(&f.workspace, &f.owner, &token, |_, _, _, _, _| {
                panic!("cancelled reservation fetched")
            });
        assert_eq!(crate::collection_machine::replay_calls() - count, 1);
        (f, result)
    });
    entered_rx.recv_timeout(Duration::from_secs(10)).unwrap();
    {
        let mut w = workspace
            .try_lock()
            .expect("paused preparation holds mutex");
        w.cancel_durable_collection(&job, 1, chrono::Utc::now().timestamp_millis())
            .unwrap();
    }
    release_tx.send(()).unwrap();
    let (f, result) = join.join().unwrap();
    assert!(result.unwrap().is_none());
    let done = f.inspect();
    assert_eq!(done.checkpoint.state, CollectionState::Cancelled);
    assert_eq!(
        done.checkpoint.requests_used(),
        before.checkpoint.requests_used()
    );
    assert_eq!(
        done.checkpoint.deadline_at_ms,
        before.checkpoint.deadline_at_ms
    );
}

#[test]
fn v4_driver_reservation_failures_do_not_fetch_or_retry_and_cancel_writes_stay_separate() {
    for refusal in [
        "run_drift",
        "source_drift",
        "publish",
        "cancel_then_publish",
    ] {
        let mut f = Fixture::v4();
        f.settle(404, "", b"missing");
        f.settle(200, "", b"<p>First</p><a href='/next'>Next</a>");
        let before = f.inspect();
        let workspace = f.workspace.clone();
        let path = f.path.clone();
        let execution = f.driver.execution.clone();
        f.driver.reservation_hook = Some(Box::new(move || {
            assert!(workspace.try_lock().is_ok());
            let conn = Connection::open(path.join("workspace.db")).unwrap();
            match refusal {
                "run_drift" => {
                    // A forged checkpoint keeps the same event prefix. It must
                    // not be accepted as a cancellation suffix or a new ticket.
                    let owner_free_action = format!("UPDATE records SET body=json_set(body,'$.checkpoint.generation',2) WHERE kind='collection_run' AND id='{}'", execution.job_id);
                    conn.execute_batch(&owner_free_action).unwrap();
                }
                "source_drift" => {
                    conn.execute("UPDATE records SET body=json_set(body,'$.name','changed') WHERE kind='evidence'", []).unwrap();
                }
                _ => {
                    conn.execute_batch("CREATE TRIGGER refuse_driver_advance BEFORE UPDATE ON records WHEN NEW.kind='collection_run' AND json_extract(NEW.body,'$.events[#-1].event')='advance' BEGIN SELECT RAISE(ABORT,'fixed driver reservation refusal'); END;").unwrap();
                }
            }
        }));
        let token = CancellationToken::default();
        if refusal == "cancel_then_publish" {
            token.cancel();
        }
        let revision = f.workspace.lock().unwrap().revision().unwrap();
        let count = crate::collection_machine::replay_calls();
        assert!(f
            .driver
            .next_with(&f.workspace, &f.owner, &token, |_, _, _, _, _| panic!(
                "uncommitted or drifted request fetched"
            ))
            .is_err());
        assert_eq!(crate::collection_machine::replay_calls() - count, 1);
        let conn = Connection::open(f.path.join("workspace.db")).unwrap();
        let raw: String = conn
            .query_row(
                "SELECT body FROM records WHERE kind='collection_run' AND id=?",
                [&f.id],
                |row| row.get(0),
            )
            .unwrap();
        let after: DurableCollectionJob = serde_json::from_str(&raw).unwrap();
        assert_eq!(
            after.checkpoint.requests_used(),
            before.checkpoint.requests_used()
        );
        assert_eq!(
            f.workspace.lock().unwrap().revision().unwrap(),
            revision + u64::from(refusal == "cancel_then_publish")
        );
        if refusal == "cancel_then_publish" {
            assert!(after.checkpoint.cancellation_requested);
        }
    }
}

#[test]
fn attached_v4_driver_refuses_historical_policy_or_mode_drift_without_legacy_fallback() {
    for protocol in [
        CollectionProtocol::SyntheticV2,
        CollectionProtocol::SyntheticV3,
        CollectionProtocol::NativeV4,
    ] {
        let mut f = Fixture::v4();
        let mut changed = f.inspect();
        changed.schema_version = protocol.version();
        changed.collector_policy = protocol.policy().into();
        changed.synthetic = protocol.synthetic();
        let raw = serde_json::to_string(&changed).unwrap();
        let conn = Connection::open(f.path.join("workspace.db")).unwrap();
        conn.execute(
            "UPDATE records SET body=? WHERE kind='collection_run' AND id=?",
            [&raw, &f.id],
        )
        .unwrap();
        // This is a fully replayable alternate record, not malformed JSON. It
        // still cannot change the protocol/mode of the already attached driver.
        assert_eq!(f.inspect(), changed);
        let revision = f.workspace.lock().unwrap().revision().unwrap();
        let count = crate::collection_machine::replay_calls();
        assert!(f
            .driver
            .next_with(
                &f.workspace,
                &f.owner,
                &CancellationToken::default(),
                |_, _, _, _, _| panic!("protocol drift fell through to legacy fetch")
            )
            .is_err());
        assert_eq!(crate::collection_machine::replay_calls(), count);
        assert_eq!(f.workspace.lock().unwrap().revision().unwrap(), revision);
        let after: String = conn
            .query_row(
                "SELECT body FROM records WHERE kind='collection_run' AND id=?",
                [&f.id],
                |row| row.get(0),
            )
            .unwrap();
        assert_eq!(after, raw);
    }
    // A genuinely attached historical synthetic driver keeps the existing path.
    let mut historical = Fixture::protocol(CollectionProtocol::SyntheticV3);
    let pending = historical.response(404, "", b"missing", false);
    assert_eq!(pending.request.sequence, 0);
}
