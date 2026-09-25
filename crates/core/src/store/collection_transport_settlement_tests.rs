use super::*;
use crate::{
    collection_settlement::HttpDelivery,
    collection_transport::{
        CallerContextState, Phase, ResolvedCandidates, ResolverUncertainty, ResponseHead,
        StopReason,
    },
};

fn fixture() -> (
    tempfile::TempDir,
    Workspace,
    CollectionOwnership,
    CollectionTicket,
    RequestTicket,
) {
    let temp = tempfile::tempdir_in(std::env::temp_dir().canonicalize().unwrap()).unwrap();
    let mut w = Workspace::open(temp.path().join("case")).unwrap();
    let owner = w.collection_ownership().unwrap();
    let now = chrono::Utc::now().timestamp_millis() - 1000;
    let job = w
        .queue_collection_transport(
            CollectionInput {
                urls: vec!["https://collection.invalid/start".into()],
                max_hops: 2,
                max_requests: 8,
                max_seconds: 60,
            },
            &id(),
            now,
        )
        .unwrap();
    let execution = w
        .start_durable_collection(&job.id, 1, &owner, now)
        .unwrap()
        .unwrap();
    let request = w
        .advance_durable_collection(&execution, &owner, now)
        .unwrap()
        .unwrap();
    (temp, w, owner, execution, request)
}
fn complete(at: i64, body: &[u8]) -> Observation {
    Observation {
        outcome: Outcome::Complete {
            head: ResponseHead {
                status: 404,
                media_type: Some("text/plain".into()),
                redirect_url: None,
                identity_encoding: true,
            },
            body: body.to_vec(),
        },
        phase: Phase::Body,
        elapsed_milliseconds: 12,
        observed_wall_ms: at,
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
fn raw(w: &Workspace, key: &str) -> String {
    w.conn
        .query_row(
            "SELECT body FROM records WHERE kind='collection_run' AND id=?",
            [key],
            |row| row.get(0),
        )
        .unwrap()
}
fn put_raw(w: &Workspace, key: &str, body: &str) {
    w.conn
        .execute(
            "UPDATE records SET body=? WHERE kind='collection_run' AND id=?",
            params![body, key],
        )
        .unwrap();
}

#[test]
fn anchored_stop_survives_a_large_clock_rollback_and_exact_publication_retry() {
    let (_temp, mut w, owner, execution, request) = fixture();
    // Model a previously valid clock which has since moved back over five
    // seconds. Shift the complete, internally consistent synthetic history;
    // no OS clock is changed and replay still validates every event/checkpoint.
    let mut historic = w.inspect_durable_collection(&execution.job_id).unwrap();
    let shift = chrono::Utc::now().timestamp_millis() + 60_000 - historic.created_at_ms;
    historic.created_at_ms += shift;
    for event in &mut historic.events {
        match event {
            CollectionEvent::Start { at_ms, .. } | CollectionEvent::Advance { at_ms } => {
                *at_ms += shift;
            }
            _ => panic!("Fixture must contain only start and reservation"),
        }
    }
    historic.checkpoint.first_started_at_ms =
        historic.checkpoint.first_started_at_ms.map(|at| at + shift);
    historic.checkpoint.deadline_at_ms = historic.checkpoint.deadline_at_ms.map(|at| at + shift);
    historic.checkpoint.updated_at_ms += shift;
    for charged in &mut historic.checkpoint.requests {
        charged.reserved_at_ms += shift;
    }
    put_raw(
        &w,
        &execution.job_id,
        &serde_json::to_string(&historic).unwrap(),
    );
    assert_eq!(
        w.inspect_durable_collection(&execution.job_id).unwrap(),
        historic
    );
    let anchor = historic.checkpoint.updated_at_ms;
    let revision = w.revision().unwrap();
    assert!(w
        .cancel_durable_collection(&execution.job_id, 1, chrono::Utc::now().timestamp_millis())
        .unwrap_err()
        .to_string()
        .contains("clock moved backwards"));
    assert!(w
        .cancel_durable_collection(&execution.job_id, 1, anchor)
        .unwrap_err()
        .to_string()
        .contains("future collection timestamp"));
    assert_eq!(w.revision().unwrap(), revision);
    owner.quarantine();
    let cancelled = w
        .cancel_collection_settlement_at_checkpoint(&request, &owner)
        .unwrap();
    assert_eq!(cancelled.checkpoint.updated_at_ms, anchor);
    assert!(cancelled.checkpoint.cancellation_requested);
    assert_eq!(
        cancelled.events.last(),
        Some(&CollectionEvent::Cancel { at_ms: anchor })
    );
    let revision = w.revision().unwrap();
    assert_eq!(
        w.cancel_collection_settlement_at_checkpoint(&request, &owner)
            .unwrap(),
        cancelled
    );
    assert_eq!(w.revision().unwrap(), revision);
    let observation = complete(anchor - 500, b"Exact synthetic rollback response");
    let expected = TransportReceipt::from_observation(&observation);
    w.conn.execute_batch("CREATE TRIGGER reject_clock_receipt BEFORE UPDATE ON records WHEN NEW.kind='collection_run' AND json_extract(NEW.body,'$.events[#-1].event')='transport_observed' BEGIN SELECT RAISE(ABORT,'synthetic clock publication failure'); END;").unwrap();
    assert!(w
        .settle_collection_transport(&request, &observation, &owner)
        .is_err());
    assert_eq!(
        w.inspect_durable_collection(&execution.job_id).unwrap(),
        cancelled
    );
    w.conn
        .execute_batch("DROP TRIGGER reject_clock_receipt;")
        .unwrap();
    let settled = w
        .settle_collection_transport(&request, &observation, &owner)
        .unwrap();
    assert_eq!(settled.checkpoint.state, CollectionState::Failed);
    assert_eq!(settled.checkpoint.updated_at_ms, anchor);
    assert!(
        matches!(&settled.checkpoint.requests[0].progress, RequestProgress::Observed { receipt } if receipt == &expected)
    );
    let revision = w.revision().unwrap();
    assert_eq!(
        w.settle_collection_transport(&request, &observation, &owner)
            .unwrap(),
        settled
    );
    assert_eq!(w.revision().unwrap(), revision);
    let evidence = w.view().unwrap().evidence;
    assert_eq!(evidence.len(), 1);
    assert_eq!(evidence[0].acquisitions.len(), 1);
    assert!(evidence[0].text.is_none());
    assert_eq!(
        read_original(&w.root, &evidence[0]).unwrap(),
        b"Exact synthetic rollback response"
    );
    assert_eq!(
        w.inspect_durable_collection(&execution.job_id).unwrap(),
        settled
    );
}

#[test]
fn anchored_stop_requires_the_exact_still_reserved_request_and_owner() {
    let (_temp, mut w, owner, execution, request) = fixture();
    let before = w.inspect_durable_collection(&execution.job_id).unwrap();
    let revision = w.revision().unwrap();
    let mut wrong = request.clone();
    wrong.run.lease = id();
    assert!(w
        .cancel_collection_settlement_at_checkpoint(&wrong, &owner)
        .is_err());
    wrong = request.clone();
    wrong.run.generation += 1;
    assert!(w
        .cancel_collection_settlement_at_checkpoint(&wrong, &owner)
        .is_err());
    wrong = request.clone();
    wrong.sequence += 1;
    assert!(w
        .cancel_collection_settlement_at_checkpoint(&wrong, &owner)
        .is_err());
    wrong = request.clone();
    wrong.url.push_str("?changed");
    assert!(w
        .cancel_collection_settlement_at_checkpoint(&wrong, &owner)
        .is_err());
    let (_other_temp, _other_workspace, other_owner, _, _) = fixture();
    assert!(w
        .cancel_collection_settlement_at_checkpoint(&request, &other_owner)
        .is_err());
    assert_eq!(
        w.inspect_durable_collection(&execution.job_id).unwrap(),
        before
    );
    assert_eq!(w.revision().unwrap(), revision);
    let observation = complete(
        before.checkpoint.updated_at_ms,
        b"Synthetic ordinary response",
    );
    w.settle_collection_transport(&request, &observation, &owner)
        .unwrap();
    let revision = w.revision().unwrap();
    assert!(w
        .cancel_collection_settlement_at_checkpoint(&request, &owner)
        .is_err());
    assert_eq!(w.revision().unwrap(), revision);
    owner.release().unwrap();
    assert!(w
        .cancel_collection_settlement_at_checkpoint(&request, &owner)
        .is_err());
}

#[test]
fn v2_backwards_clock_preserves_raw_sample_body_and_last_valid_clock_without_invented_end_time() {
    let (temp, mut w, owner, execution, request) = fixture();
    let anchor = w
        .inspect_durable_collection(&execution.job_id)
        .unwrap()
        .checkpoint
        .updated_at_ms;
    let mut observation = complete(
        anchor - 500,
        b"synthetic complete bytes during clock change",
    );
    observation.stop_observed = Some(StopReason::ClockChanged);
    let settled = w
        .settle_collection_transport(&request, &observation, &owner)
        .unwrap();
    assert_eq!(settled.checkpoint.state, CollectionState::Failed);
    assert_eq!(settled.checkpoint.updated_at_ms, anchor);
    let RequestProgress::Observed { receipt } = &settled.checkpoint.requests[0].progress else {
        panic!()
    };
    assert_eq!(receipt.observed_wall_ms, anchor - 500);
    assert_eq!(receipt.elapsed_milliseconds, 12);
    assert_eq!(receipt.http_delivery, HttpDelivery::MayHaveBeenSent);
    let encoded = raw(&w, &execution.job_id);
    assert!(!encoded.contains("ended_at_ms"));
    assert_eq!(
        w.inspect_durable_collection(&execution.job_id).unwrap(),
        settled
    );
    assert_eq!(w.view().unwrap().evidence.len(), 1);
    assert!(w.view().unwrap().evidence[0].text.is_none());
    drop(w);
    drop(owner);
    let reopened = Workspace::open(temp.path().join("case")).unwrap();
    assert_eq!(
        reopened
            .inspect_durable_collection(&execution.job_id)
            .unwrap(),
        settled
    );
    assert_eq!(raw(&reopened, &execution.job_id), encoded);
}

#[test]
fn resolver_uncertainty_quarantines_restart_and_other_jobs_without_false_cancel_or_body() {
    for context in [
        CallerContextState::ReleasedAfterCompletion,
        CallerContextState::RetainedPendingCompletion,
    ] {
        let (temp, mut w, owner, execution, request) = fixture();
        let at = chrono::Utc::now().timestamp_millis();
        w.cancel_durable_collection(&execution.job_id, 1, at)
            .unwrap();
        let observation = Observation {
            outcome: Outcome::Stopped {
                reason: StopReason::QuiescenceUnverified,
                head: None,
            },
            phase: Phase::Dns,
            elapsed_milliseconds: 3,
            observed_wall_ms: at,
            resolved: None,
            resolver_uncertainty: Some(ResolverUncertainty {
                method: "windows_overlapped_dns",
                caller_context: context,
            }),
            stop_observed: Some(StopReason::Cancelled),
            locally_quiescent: false,
        };
        let settled = w
            .settle_collection_transport(&request, &observation, &owner)
            .unwrap();
        assert_eq!(settled.checkpoint.state, CollectionState::RecoveryRequired);
        assert_eq!(settled.checkpoint.requests_used(), 1);
        assert!(w.view().unwrap().evidence.is_empty());
        assert!(w
            .advance_durable_collection(&execution, &owner, at)
            .is_err());
        assert!(w
            .acknowledge_collection_stop(&execution, &owner, at)
            .is_err());
        assert!(w
            .start_durable_collection(&execution.job_id, 1, &owner, at)
            .is_err());
        let second = w
            .queue_collection_transport(settled.input.clone(), &id(), at)
            .unwrap();
        assert!(w
            .start_durable_collection(&second.id, 1, &owner, at)
            .is_err());
        drop(w);
        drop(owner);
        let mut reopened = Workspace::open(temp.path().join("case")).unwrap();
        let owner = reopened.collection_ownership().unwrap();
        assert_eq!(
            reopened
                .inspect_durable_collection(&execution.job_id)
                .unwrap(),
            settled
        );
        assert!(reopened
            .start_durable_collection(&second.id, 1, &owner, at)
            .is_err());
    }
}

#[test]
fn v2_malformed_receipts_duplicate_json_and_revoked_tickets_fail_without_partial_publication() {
    let (_temp, mut w, owner, execution, request) = fixture();
    let at = chrono::Utc::now().timestamp_millis();
    let revision = w.revision().unwrap();
    let mut wrong = request.clone();
    wrong.run.lease = id();
    assert!(w
        .settle_collection_transport(&wrong, &complete(at, b"synthetic"), &owner)
        .is_err());
    let mut invalid = complete(at, b"synthetic");
    invalid.locally_quiescent = false;
    assert!(w
        .settle_collection_transport(&request, &invalid, &owner)
        .is_err());
    assert_eq!(w.revision().unwrap(), revision);
    assert!(w.view().unwrap().evidence.is_empty());
    let settled = w
        .settle_collection_transport(&request, &complete(at, b"synthetic"), &owner)
        .unwrap();
    let original = raw(&w, &execution.job_id);
    for field in [
        "phase",
        "elapsed_milliseconds",
        "identity_encoding",
        "locally_quiescent",
    ] {
        let needle = match field {
            "phase" => "\"phase\":\"body\"",
            "elapsed_milliseconds" => "\"elapsed_milliseconds\":12",
            "identity_encoding" => "\"identity_encoding\":true",
            _ => "\"locally_quiescent\":true",
        };
        assert!(original.contains(needle));
        let duplicate = original.replacen(needle, &format!("{needle},{needle}"), 1);
        put_raw(&w, &execution.job_id, &duplicate);
        assert!(
            w.inspect_durable_collection(&execution.job_id).is_err(),
            "{field}"
        );
        assert_eq!(raw(&w, &execution.job_id), duplicate);
    }
    put_raw(&w, &execution.job_id, &original);
    assert_eq!(
        w.inspect_durable_collection(&execution.job_id).unwrap(),
        settled
    );
    assert!(w
        .settle_collection_transport(&request, &complete(at, b"different"), &owner)
        .is_err());
    let next = w
        .advance_durable_collection(&execution, &owner, at)
        .unwrap()
        .unwrap();
    w.recover_durable_collections(&owner, at).unwrap();
    assert!(w
        .settle_collection_transport(&next, &complete(at, b"late"), &owner)
        .is_err());
    assert_eq!(
        w.inspect_durable_collection(&execution.job_id)
            .unwrap()
            .checkpoint
            .requests_used(),
        2
    );
}

#[test]
fn v1_history_stays_byte_identical_and_cannot_adopt_v2_receipts() {
    let (_temp, mut w, owner, execution, request) = fixture();
    let v2 = w.inspect_durable_collection(&execution.job_id).unwrap();
    let at = chrono::Utc::now().timestamp_millis();
    let old = w
        .queue_durable_collection(v2.input.clone(), &id(), at)
        .unwrap();
    let before = raw(&w, &old.id);
    w.settle_collection_transport(&request, &complete(at, b""), &owner)
        .unwrap();
    assert_eq!(raw(&w, &old.id), before);
    assert_eq!(w.inspect_durable_collection(&old.id).unwrap(), old);
    let mut altered = w.inspect_durable_collection(&execution.job_id).unwrap();
    altered.schema_version = 1;
    altered.collector_policy = "direct-https-durable-foundation-v1".into();
    put_raw(&w, &altered.id, &serde_json::to_string(&altered).unwrap());
    assert!(w.inspect_durable_collection(&altered.id).is_err());
    assert_eq!(raw(&w, &old.id), before);
}

#[test]
fn a_later_canonical_cancel_does_not_misclassify_an_earlier_response_as_clock_regression() {
    let (_temp, mut w, owner, execution, request) = fixture();
    let reserved = w
        .inspect_durable_collection(&execution.job_id)
        .unwrap()
        .checkpoint
        .updated_at_ms;
    let observation = complete(reserved + 1, b"complete before cancel publication");
    w.cancel_durable_collection(&execution.job_id, 1, reserved + 2)
        .unwrap();
    let result = w
        .settle_collection_transport(&request, &observation, &owner)
        .unwrap();
    assert_eq!(result.checkpoint.state, CollectionState::Cancelled);
    assert_eq!(result.checkpoint.updated_at_ms, reserved + 2);
    let RequestProgress::Observed { receipt } = &result.checkpoint.requests[0].progress else {
        panic!()
    };
    assert_eq!(receipt.observed_wall_ms, reserved + 1);
    assert_eq!(receipt.stop_observed, None);
    assert_eq!(w.view().unwrap().evidence.len(), 1);
}

#[test]
fn released_guard_cannot_authorize_settlement_even_with_unchanged_request_and_lifetime() {
    let (_temp, mut workspace, owner, execution, request) = fixture();
    let before = workspace
        .inspect_durable_collection(&execution.job_id)
        .unwrap();
    let revision = workspace.revision().unwrap();
    owner.release().unwrap();
    assert!(workspace
        .settle_collection_transport(
            &request,
            &complete(now_for_test(), b"Synthetic held response"),
            &owner
        )
        .is_err());
    assert_eq!(workspace.revision().unwrap(), revision);
    assert_eq!(
        workspace
            .inspect_durable_collection(&execution.job_id)
            .unwrap(),
        before
    );
    assert!(workspace.view().unwrap().evidence.is_empty());
}

fn now_for_test() -> i64 {
    chrono::Utc::now().timestamp_millis()
}
