use super::*;
use crate::collection_profile::{ProfileLimits, PublisherAccessInput, BENCHMARK_SHA256};
use crate::collection_transport::{
    Phase as TransportPhase, ResolvedCandidates, ResponseHead, StopReason,
};

const ACCESS_HTML: &[u8] =
    b"<html><body>Fixture terms <a href='/must-not-be-frontier'>bait</a></body></html>";
fn plan(requests: u32, seconds: u64) -> ValidatedPublisherAccessPlan {
    ValidatedPublisherAccessPlan::validate(PublisherAccessInput {
        benchmark_sha256: BENCHMARK_SHA256.into(),
        task_id: "discovery-01".into(),
        publisher_id: "nasa".into(),
        publisher_domain: "nasa.gov".into(),
        seed_url: "https://www.nasa.gov/missions/".into(),
        local_query_sha256: hash(b"Artemis"),
        session_id: "11111111-1111-4111-8111-111111111111".into(),
        access_urls: vec![
            "https://www.nasa.gov/robots.txt".into(),
            "https://www.nasa.gov/terms/".into(),
        ],
        effective_limits: ProfileLimits {
            max_hops: 2,
            max_requests: requests,
            max_seconds: seconds,
        },
    })
    .unwrap()
}
fn observed(at: i64, status: u16, media: &str, bytes: &[u8]) -> Observation {
    Observation {
        outcome: Outcome::Complete {
            head: ResponseHead {
                status,
                media_type: Some(media.into()),
                redirect_url: None,
                identity_encoding: true,
            },
            body: bytes.to_vec(),
        },
        phase: TransportPhase::Body,
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
struct Fixture {
    temp: tempfile::TempDir,
    w: Workspace,
    owner: CollectionOwnership,
    job: Job,
    execution: Execution,
    at: i64,
}
impl Fixture {
    fn new(requests: u32, seconds: u64) -> Self {
        Self::at(
            requests,
            seconds,
            chrono::Utc::now().timestamp_millis() - 1000,
        )
    }
    fn at(requests: u32, seconds: u64, at: i64) -> Self {
        let temp = tempfile::tempdir_in(std::env::temp_dir().canonicalize().unwrap()).unwrap();
        let mut w = Workspace::open(temp.path().join("case")).unwrap();
        let owner = w.collection_ownership().unwrap();
        let job = w
            .queue_access_experiment(plan(requests, seconds), &id(), &owner, at)
            .unwrap();
        let (job, execution) = w.start_access_experiment(&job.id, 1, &owner, at).unwrap();
        Self {
            temp,
            w,
            owner,
            job,
            execution: execution.unwrap(),
            at,
        }
    }
    fn next(&mut self) -> Option<Ticket> {
        self.w
            .advance_access_experiment(&self.execution, &self.owner, self.at)
            .unwrap()
    }
    fn settle(&mut self, ticket: &Ticket, status: u16, media: &str, bytes: &[u8]) -> Job {
        let value = self
            .w
            .settle_access_experiment(
                ticket,
                &observed(self.at, status, media, bytes),
                &self.owner,
            )
            .unwrap();
        self.job = value.clone();
        value
    }
    fn awaiting(&mut self) -> AccessReview {
        let robots = self.next().unwrap();
        assert_eq!(robots.url, "https://www.nasa.gov/robots.txt");
        assert!(matches!(robots.purpose, Purpose::Robots { .. }));
        self.settle(&robots, 200, "text/plain", b"");
        let access = self.next().unwrap();
        assert_eq!(access.purpose, Purpose::SelectedAccess { index: 1 });
        self.settle(&access, 200, "text/html", ACCESS_HTML);
        assert!(self.next().is_none());
        self.job = self.w.inspect_access_experiment(&self.job.id).unwrap();
        assert_eq!(self.job.checkpoint.state, State::AwaitingDecision);
        self.w.review_access_experiment(&self.job.id).unwrap()
    }
    fn decide(&mut self, review: &AccessReview, outcome: DecisionOutcome) -> Decision {
        let decision = Decision {
            request_key: id(),
            review_sha256: review.review.review_sha256.clone(),
            outcome,
            reason: "Explicit synthetic fixture decision; no actual access review".into(),
        };
        self.job = self
            .w
            .decide_access_experiment(
                &self.job.id,
                review.revision,
                decision.clone(),
                &self.owner,
                self.at,
            )
            .unwrap();
        decision
    }
    fn resume(&mut self) {
        let (job, execution) = self
            .w
            .start_access_experiment(
                &self.job.id,
                self.job.checkpoint.generation,
                &self.owner,
                self.at,
            )
            .unwrap();
        self.job = job;
        self.execution = execution.unwrap();
    }
}
fn snapshot(w: &Workspace) -> serde_json::Value {
    let mut statement = w
        .conn
        .prepare("SELECT kind,id,body FROM records ORDER BY kind,id")
        .unwrap();
    let records = statement
        .query_map([], |r| {
            Ok((
                r.get::<_, String>(0)?,
                r.get::<_, String>(1)?,
                r.get::<_, String>(2)?,
            ))
        })
        .unwrap()
        .collect::<std::result::Result<Vec<_>, _>>()
        .unwrap();
    let events: u64 = w
        .conn
        .query_row("SELECT count(*) FROM events", [], |r| r.get(0))
        .unwrap();
    json!({"revision":w.revision().unwrap(),"records":records,"events":events})
}
fn raw(w: &Workspace, key: &str) -> String {
    w.conn
        .query_row(
            "SELECT body FROM records WHERE kind=? AND id=?",
            params![KIND, key],
            |r| r.get(0),
        )
        .unwrap()
}

#[test]
fn canonical_prefix_wait_decision_new_lease_seed_and_identical_body_roles() {
    let mut f = Fixture::new(50, 600);
    let first = f.execution.clone();
    let start = f.job.checkpoint.first_started_at_ms;
    let deadline = f.job.checkpoint.deadline_at_ms;
    let review = f.awaiting();
    assert_eq!(review.review.requests_used, 2);
    assert_eq!(
        review
            .review
            .inventory
            .iter()
            .map(|r| r.selected_index)
            .collect::<Vec<_>>(),
        vec![Some(0), Some(1)]
    );
    assert!(f.job.checkpoint.lease.is_none());
    assert!(all_evidence(&f.w.conn)
        .unwrap()
        .iter()
        .all(|e| e.text.is_none()));
    assert!(f
        .job
        .checkpoint
        .requests
        .iter()
        .all(|r| r.content_parent.is_none()));
    let before = snapshot(&f.w);
    assert!(f
        .w
        .advance_access_experiment(&first, &f.owner, f.at)
        .is_err());
    assert_eq!(snapshot(&f.w), before);
    let decision = f.decide(&review, DecisionOutcome::Allow);
    assert_eq!(f.job.checkpoint.state, State::ReadyContent);
    assert!(f.job.checkpoint.lease.is_none());
    f.resume();
    assert_eq!(f.execution.generation, 2);
    assert_ne!(f.execution.lease, first.lease);
    assert_eq!(f.job.checkpoint.first_started_at_ms, start);
    assert_eq!(f.job.checkpoint.deadline_at_ms, deadline);
    let seed = f.next().unwrap();
    assert_eq!(seed.url, "https://www.nasa.gov/missions/");
    assert_eq!(seed.sequence, 2);
    assert_eq!(seed.purpose, Purpose::Seed {});
    let done = f.settle(&seed, 200, "text/html", ACCESS_HTML);
    assert_eq!(done.checkpoint.state, State::ContentBoundary);
    assert_eq!(done.checkpoint.requests.len(), 3);
    assert_eq!(done.checkpoint.content_request, Some(2));
    assert_eq!(
        done.checkpoint.requests[1].purpose,
        Purpose::SelectedAccess { index: 1 }
    );
    assert!(done
        .checkpoint
        .requests
        .iter()
        .all(|r| r.content_parent.is_none()));
    let evidence = get_evidence(&f.w.conn, &hash(ACCESS_HTML)).unwrap();
    assert_eq!(evidence.acquisitions.len(), 2);
    assert_eq!(evidence.acquisitions[0].url, "https://www.nasa.gov/terms/");
    assert_eq!(evidence.acquisitions[1].url, seed.url);
    assert!(evidence.text.as_ref().unwrap().contains("Fixture terms"));
    assert_eq!(
        all_evidence(&f.w.conn)
            .unwrap()
            .iter()
            .filter(|e| e.text.is_some())
            .count(),
        1
    );
    let stable = snapshot(&f.w);
    assert!(f
        .w
        .advance_access_experiment(&f.execution, &f.owner, f.at)
        .is_err());
    assert_eq!(
        f.w.settle_access_experiment(
            &seed,
            &observed(f.at, 200, "text/html", ACCESS_HTML),
            &f.owner
        )
        .unwrap(),
        done
    );
    assert_eq!(
        f.w.decide_access_experiment(&done.id, review.revision, decision, &f.owner, f.at + 1)
            .unwrap(),
        done
    );
    assert_eq!(snapshot(&f.w), stable);
}

#[test]
fn denied_unknown_stale_and_tampered_decisions_never_reserve_content() {
    for outcome in [DecisionOutcome::Deny, DecisionOutcome::Unknown] {
        let mut f = Fixture::new(50, 600);
        let review = f.awaiting();
        let before = snapshot(&f.w);
        let mut bad = Decision {
            request_key: id(),
            review_sha256: "0".repeat(64),
            outcome,
            reason: "fixture".into(),
        };
        assert!(f
            .w
            .decide_access_experiment(&f.job.id, review.revision, bad.clone(), &f.owner, f.at)
            .is_err());
        bad.review_sha256 = review.review.review_sha256.clone();
        assert!(matches!(
            f.w.decide_access_experiment(&f.job.id, review.revision - 1, bad, &f.owner, f.at),
            Err(Error::Conflict(_))
        ));
        assert_eq!(snapshot(&f.w), before);
        let decision = f.decide(&review, outcome);
        assert_eq!(f.job.checkpoint.state, State::Blocked);
        assert_eq!(f.job.checkpoint.requests.len(), 2);
        assert!(f
            .w
            .start_access_experiment(&f.job.id, 1, &f.owner, f.at)
            .is_err());
        let mut substitution = decision;
        substitution.outcome = DecisionOutcome::Allow;
        assert!(f
            .w
            .decide_access_experiment(&f.job.id, review.revision, substitution, &f.owner, f.at)
            .is_err());
    }
}

#[test]
fn publication_rollback_preserves_charge_then_exact_retry_has_one_acquisition() {
    let mut f = Fixture::new(50, 600);
    let ticket = f.next().unwrap();
    let before = snapshot(&f.w);
    let observation = observed(f.at, 404, "text/plain", b"original robots fixture");
    f.w.conn.execute_batch("CREATE TRIGGER fail_access BEFORE UPDATE ON records WHEN NEW.kind='collection_access_experiment_v5' BEGIN SELECT RAISE(ABORT,'fixed prefix publication refusal'); END;").unwrap();
    assert!(f
        .w
        .settle_access_experiment(&ticket, &observation, &f.owner)
        .is_err());
    assert_eq!(snapshot(&f.w), before);
    assert_eq!(
        f.w.inspect_access_experiment(&f.job.id)
            .unwrap()
            .checkpoint
            .requests[0]
            .progress,
        RequestProgress::Reserved
    );
    assert!(f
        .w
        .root
        .join("originals")
        .join(hash(b"original robots fixture"))
        .exists());
    f.w.conn.execute_batch("DROP TRIGGER fail_access").unwrap();
    let done =
        f.w.settle_access_experiment(&ticket, &observation, &f.owner)
            .unwrap();
    let stable = snapshot(&f.w);
    assert_eq!(
        f.w.settle_access_experiment(&ticket, &observation, &f.owner)
            .unwrap(),
        done
    );
    assert_eq!(snapshot(&f.w), stable);
    assert_eq!(
        get_evidence(&f.w.conn, &hash(b"original robots fixture"))
            .unwrap()
            .acquisitions
            .len(),
        1
    );
    let changed = observed(f.at, 404, "text/plain", b"changed! robots fixture");
    assert!(f
        .w
        .settle_access_experiment(&ticket, &changed, &f.owner)
        .is_err());
    assert_eq!(snapshot(&f.w), stable);
}

#[test]
fn decision_write_failure_and_acknowledgement_recovery_are_not_execution() {
    let mut f = Fixture::new(50, 600);
    let review = f.awaiting();
    let decision = Decision {
        request_key: id(),
        review_sha256: review.review.review_sha256.clone(),
        outcome: DecisionOutcome::Allow,
        reason: "fixture".into(),
    };
    let before = snapshot(&f.w);
    f.w.conn.execute_batch("CREATE TRIGGER fail_decision BEFORE UPDATE ON records WHEN NEW.kind='collection_access_experiment_v5' BEGIN SELECT RAISE(ABORT,'fixed decision failure'); END;").unwrap();
    assert!(f
        .w
        .decide_access_experiment(&f.job.id, review.revision, decision.clone(), &f.owner, f.at)
        .is_err());
    assert_eq!(snapshot(&f.w), before);
    f.w.conn
        .execute_batch("DROP TRIGGER fail_decision")
        .unwrap();
    let committed =
        f.w.decide_access_experiment(&f.job.id, review.revision, decision.clone(), &f.owner, f.at)
            .unwrap();
    assert!(committed.checkpoint.lease.is_none());
    f.owner.quarantine();
    let stable = snapshot(&f.w);
    assert_eq!(
        f.w.decide_access_experiment(&f.job.id, review.revision, decision, &f.owner, f.at)
            .unwrap(),
        committed
    );
    assert_eq!(
        f.w.queue_access_experiment(plan(50, 600), &f.job.request_key, &f.owner, f.at)
            .unwrap(),
        committed
    );
    assert!(f
        .w
        .start_access_experiment(&f.job.id, 1, &f.owner, f.at)
        .is_err());
    assert_eq!(snapshot(&f.w), stable);
}

#[test]
fn common_request_budget_and_original_deadline_include_access_and_review_wait() {
    let mut zero = Fixture::new(0, 600);
    assert!(zero.next().is_none());
    assert_eq!(
        zero.w
            .inspect_access_experiment(&zero.job.id)
            .unwrap()
            .checkpoint
            .state,
        State::QuotaExhausted
    );
    let mut low = Fixture::new(2, 600);
    let review = low.awaiting();
    low.decide(&review, DecisionOutcome::Allow);
    low.resume();
    assert!(low.next().is_none());
    let exhausted = low.w.inspect_access_experiment(&low.job.id).unwrap();
    assert_eq!(exhausted.checkpoint.state, State::QuotaExhausted);
    assert_eq!(exhausted.checkpoint.requests.len(), 2);
    let at = chrono::Utc::now().timestamp_millis() - 601_000;
    let mut elapsed = Fixture::at(50, 600, at);
    let review = elapsed.awaiting();
    elapsed.at = at + 600_000;
    elapsed.decide(&review, DecisionOutcome::Allow);
    assert_eq!(elapsed.job.checkpoint.state, State::QuotaExhausted);
    assert_eq!(elapsed.job.checkpoint.deadline_at_ms, Some(at + 600_000));
    assert_eq!(elapsed.job.checkpoint.requests.len(), 2);
}

#[test]
fn robots_refusal_encoding_and_redirect_do_not_become_access_permission() {
    for mode in 0..4 {
        let mut f = Fixture::new(50, 600);
        let ticket = f.next().unwrap();
        let mut response = observed(f.at, 200, "text/plain", b"User-agent: *\nDisallow: /\n");
        if let Outcome::Complete { head, .. } = &mut response.outcome {
            match mode {
                1 => head.identity_encoding = false,
                2 => {
                    head.status = 302;
                    head.redirect_url = Some("https://www.nasa.gov/elsewhere".into());
                }
                3 => head.status = 429,
                _ => {}
            }
        }
        f.job =
            f.w.settle_access_experiment(&ticket, &response, &f.owner)
                .unwrap();
        if mode == 0 {
            assert!(f.next().is_none());
        }
        let job = f.w.inspect_access_experiment(&f.job.id).unwrap();
        assert!(matches!(
            job.checkpoint.state,
            State::Blocked | State::QuotaExhausted
        ));
        assert_eq!(job.checkpoint.requests.len(), 1);
        assert!(job.checkpoint.decision.is_none());
        assert!(all_evidence(&f.w.conn)
            .unwrap()
            .iter()
            .all(|e| e.text.is_none()));
    }
}

#[test]
fn cancel_complete_body_race_keeps_original_and_clock_rollback_keeps_raw_time() {
    let mut f = Fixture::new(50, 600);
    let review = f.awaiting();
    f.decide(&review, DecisionOutcome::Allow);
    f.resume();
    let seed = f.next().unwrap();
    f.w.cancel_access_experiment(&f.job.id, 2, &f.owner, f.at)
        .unwrap();
    let done = f.settle(&seed, 200, "text/html", b"<p>cancelled content</p>");
    assert_eq!(done.checkpoint.state, State::Cancelled);
    assert!(!done.checkpoint.promoted);
    assert!(get_evidence(&f.w.conn, &hash(b"<p>cancelled content</p>"))
        .unwrap()
        .text
        .is_none());
    let mut rollback = Fixture::new(50, 600);
    let ticket = rollback.next().unwrap();
    // Synthetic already-canonical future anchor models a subsequent OS rollback.
    let future = rollback.at + 60_000;
    let mut job = rollback
        .w
        .inspect_access_experiment(&rollback.job.id)
        .unwrap();
    job.created_at_ms = future;
    for event in &mut job.events {
        match event {
            Event::Start { at_ms, .. } | Event::Advance { at_ms } => *at_ms = future,
            _ => unreachable!(),
        }
    }
    job.checkpoint.first_started_at_ms = Some(future);
    job.checkpoint.deadline_at_ms = Some(future + 600_000);
    job.checkpoint.updated_at_ms = future;
    job.checkpoint.requests[0].reserved_at_ms = future;
    put(&rollback.w.conn, KIND, &job.id, &job).unwrap();
    assert!(rollback
        .w
        .cancel_access_experiment(&job.id, 1, &rollback.owner, rollback.at)
        .is_err());
    rollback
        .w
        .stop_access_reservation(&ticket, &rollback.owner)
        .unwrap();
    let observed = observed(rollback.at, 404, "text/plain", b"backward original");
    let failed = rollback
        .w
        .settle_access_experiment(&ticket, &observed, &rollback.owner)
        .unwrap();
    assert_eq!(failed.checkpoint.state, State::Failed);
    let RequestProgress::Observed { receipt } = &failed.checkpoint.requests[0].progress else {
        panic!("receipt")
    };
    assert_eq!(receipt.observed_wall_ms, rollback.at);
    assert_eq!(failed.checkpoint.updated_at_ms, future);
    let reopened = Workspace::open(&rollback.w.root).unwrap();
    assert_eq!(reopened.inspect_access_experiment(&job.id).unwrap(), failed);
}

#[test]
fn crash_unresolved_charge_new_owner_cannot_replay_old_pending_settlement() {
    let mut f = Fixture::new(50, 600);
    let ticket = f.next().unwrap();
    f.owner.release().unwrap();
    let owner = f.w.collection_ownership().unwrap();
    let before = snapshot(&f.w);
    assert!(f
        .w
        .settle_access_experiment(
            &ticket,
            &observed(f.at, 404, "text/plain", b"stale"),
            &owner
        )
        .is_err());
    assert!(f
        .w
        .advance_access_experiment(&f.execution, &owner, f.at)
        .is_err());
    assert_eq!(snapshot(&f.w), before);
    let recovered =
        f.w.recover_access_experiment(&f.job.id, &owner, f.at)
            .unwrap();
    assert_eq!(recovered.checkpoint.requests.len(), 1);
    assert!(matches!(
        recovered.checkpoint.requests[0].progress,
        RequestProgress::InterruptedUnknown { .. }
    ));
    assert_eq!(recovered.checkpoint.state, State::Interrupted);
    assert!(f
        .w
        .start_access_experiment(&f.job.id, 1, &owner, f.at)
        .is_err());
    let reopened = Workspace::open(&f.w.root).unwrap();
    assert_eq!(
        reopened.inspect_access_experiment(&f.job.id).unwrap(),
        recovered
    );
}

#[test]
fn unknown_quiescence_quarantines_before_failed_publication_and_only_exact_retry_survives() {
    let mut f = Fixture::new(50, 600);
    let ticket = f.next().unwrap();
    let observation = Observation {
        outcome: Outcome::Stopped {
            reason: StopReason::RecoveryRequired,
            head: None,
        },
        phase: TransportPhase::BeforeRequest,
        elapsed_milliseconds: 0,
        observed_wall_ms: f.at,
        resolved: None,
        resolver_uncertainty: None,
        stop_observed: None,
        locally_quiescent: false,
    };
    f.w.conn.execute_batch("CREATE TRIGGER fail_unknown BEFORE UPDATE ON records WHEN NEW.kind='collection_access_experiment_v5' BEGIN SELECT RAISE(ABORT,'fixed unknown publication failure'); END;").unwrap();
    assert!(f
        .w
        .settle_access_experiment(&ticket, &observation, &f.owner)
        .is_err());
    assert!(!f.owner.held());
    assert!(f.owner.publication_held());
    assert!(f
        .w
        .advance_access_experiment(&f.execution, &f.owner, f.at)
        .is_err());
    assert!(f
        .w
        .queue_access_experiment(plan(50, 600), &id(), &f.owner, f.at)
        .is_err());
    f.w.conn.execute_batch("DROP TRIGGER fail_unknown").unwrap();
    let failed =
        f.w.settle_access_experiment(&ticket, &observation, &f.owner)
            .unwrap();
    assert_eq!(failed.checkpoint.state, State::RecoveryRequired);
    assert_eq!(failed.checkpoint.requests.len(), 1);
    assert!(all_evidence(&f.w.conn).unwrap().is_empty());
    assert!(f.owner.release().is_err());
}

#[test]
fn original_metadata_journal_profile_and_duplicate_field_tampering_are_refused() {
    for mode in 0..6 {
        let mut f = Fixture::new(50, 600);
        let review = f.awaiting();
        let before = raw(&f.w, &f.job.id);
        match mode {
            0 => {
                let path = f.w.root.join("originals").join(hash(ACCESS_HTML));
                private_file(&path, 0o600).unwrap();
                fs::write(path, vec![b'x'; ACCESS_HTML.len()]).unwrap();
            }
            1 => {
                let mut e = get_evidence(&f.w.conn, &hash(ACCESS_HTML)).unwrap();
                e.text = Some("foreign text".into());
                put(&f.w.conn, "evidence", &e.id, &e).unwrap();
            }
            2 => {
                let mut job = f.job.clone();
                job.checkpoint.access_cursor = 0;
                put(&f.w.conn, KIND, &job.id, &job).unwrap();
            }
            3 => {
                let mut job = f.job.clone();
                job.plan_input.access_urls.reverse();
                put(&f.w.conn, KIND, &job.id, &job).unwrap();
            }
            4 => {
                let duplicate = before.replacen('{', "{\"schema_version\":5,", 1);
                f.w.conn
                    .execute(
                        "UPDATE records SET body=? WHERE kind=? AND id=?",
                        params![duplicate, KIND, f.job.id],
                    )
                    .unwrap();
            }
            _ => {
                let mut e = get_evidence(&f.w.conn, &hash(ACCESS_HTML)).unwrap();
                e.acquisitions[0].url = "https://www.nasa.gov/other".into();
                put(&f.w.conn, "evidence", &e.id, &e).unwrap();
            }
        }
        assert!(
            f.w.inspect_access_experiment(&f.job.id).is_err(),
            "mode {mode}"
        );
        let decision = Decision {
            request_key: id(),
            review_sha256: review.review.review_sha256,
            outcome: DecisionOutcome::Allow,
            reason: String::new(),
        };
        let revision = f.w.revision().unwrap();
        assert!(f
            .w
            .decide_access_experiment(&f.job.id, review.revision, decision, &f.owner, f.at)
            .is_err());
        assert_eq!(f.w.revision().unwrap(), revision);
    }
}

#[test]
fn fresh_only_admission_preserves_preexisting_text_and_legacy_catalogue() {
    let mut f = Fixture::new(50, 600);
    let page =
        f.w.page_collection_runs(
            &crate::collection_api::CollectionRunPageRequest {
                page_size: 25,
                cursor: None,
            },
            None,
        )
        .unwrap();
    assert_eq!(page.scope_count, 0);
    assert!(page.rows.is_empty());
    let legacy =
        f.w.queue_collection_protocol(
            crate::collection_jobs::CollectionInput {
                urls: vec!["https://fixture.invalid/start".into()],
                max_hops: 0,
                max_requests: 1,
                max_seconds: 10,
            },
            &id(),
            f.at,
            crate::collection_jobs::CollectionProtocol::SyntheticV4,
        )
        .unwrap();
    let bytes: String =
        f.w.conn
            .query_row(
                "SELECT body FROM records WHERE kind='collection_run' AND id=?",
                [&legacy.id],
                |r| r.get(0),
            )
            .unwrap();
    assert!(f.w.inspect_access_experiment(&f.job.id).is_err());
    assert_eq!(f.w.inspect_durable_collection(&legacy.id).unwrap(), legacy);
    let page =
        f.w.page_collection_runs(
            &crate::collection_api::CollectionRunPageRequest {
                page_size: 25,
                cursor: None,
            },
            None,
        )
        .unwrap();
    assert_eq!(page.scope_count, 1);
    let after: String =
        f.w.conn
            .query_row(
                "SELECT body FROM records WHERE kind='collection_run' AND id=?",
                [&legacy.id],
                |r| r.get(0),
            )
            .unwrap();
    assert_eq!(after, bytes);
    let temp = tempfile::tempdir_in(std::env::temp_dir().canonicalize().unwrap()).unwrap();
    let mut w = Workspace::open(temp.path().join("foreign")).unwrap();
    let owner = w.collection_ownership().unwrap();
    let source = Evidence {
        id: "0".repeat(64),
        name: "existing".into(),
        sha256: "0".repeat(64),
        bytes: 0,
        media_type: "text/plain".into(),
        origin_group: "0".repeat(64),
        imported_at: now(),
        extraction_status: "imported".into(),
        text: Some("Preserve this foreign corpus".into()),
        acquisitions: Vec::new(),
    };
    put(&w.conn, "evidence", &source.id, &source).unwrap();
    let stable = snapshot(&w);
    assert!(w
        .queue_access_experiment(plan(50, 600), &id(), &owner, f.at)
        .is_err());
    assert_eq!(snapshot(&w), stable);
}

#[test]
fn backup_restore_and_reopen_preserve_partial_review_and_final_source_roles() {
    let mut f = Fixture::new(50, 600);
    let review = f.awaiting();
    let awaiting = f.w.inspect_access_experiment(&f.job.id).unwrap();
    let backup = f.w.backup().unwrap();
    let mut restored = Workspace::restore(&backup, &f.temp.path().join("restored")).unwrap();
    assert_eq!(
        restored.inspect_access_experiment(&f.job.id).unwrap(),
        awaiting
    );
    assert_eq!(
        restored.review_access_experiment(&f.job.id).unwrap().review,
        review.review
    );
    let owner = restored.collection_ownership().unwrap();
    let decision = Decision {
        request_key: id(),
        review_sha256: review.review.review_sha256.clone(),
        outcome: DecisionOutcome::Allow,
        reason: "fixture restore".into(),
    };
    let revision = restored.revision().unwrap();
    restored
        .decide_access_experiment(&f.job.id, revision, decision, &owner, f.at)
        .unwrap();
    let (_, execution) = restored
        .start_access_experiment(&f.job.id, 1, &owner, f.at)
        .unwrap();
    let ticket = restored
        .advance_access_experiment(&execution.unwrap(), &owner, f.at)
        .unwrap()
        .unwrap();
    let done = restored
        .settle_access_experiment(
            &ticket,
            &observed(f.at, 200, "text/html", ACCESS_HTML),
            &owner,
        )
        .unwrap();
    let final_backup = restored.backup().unwrap();
    let final_restore = Workspace::restore(&final_backup, &f.temp.path().join("final")).unwrap();
    assert_eq!(
        final_restore.inspect_access_experiment(&f.job.id).unwrap(),
        done
    );
    let reopened = Workspace::open(&final_restore.root).unwrap();
    assert_eq!(reopened.inspect_access_experiment(&f.job.id).unwrap(), done);
    assert_eq!(
        done.checkpoint.deadline_at_ms,
        awaiting.checkpoint.deadline_at_ms
    );
    assert_eq!(done.checkpoint.requests.len(), 3);
    for e in all_evidence(&reopened.conn).unwrap() {
        assert_eq!(
            read_original(&reopened.root, &e).unwrap(),
            read_original(&restored.root, &e).unwrap()
        );
    }
}

#[test]
fn borrowed_observation_and_escaped_evidence_bounds_preserve_charged_reservation() {
    let mut f = Fixture::new(50, 600);
    let ticket = f.next().unwrap();
    let stable = snapshot(&f.w);
    for mode in 0..4 {
        let mut observation = observed(f.at, 200, "text/plain", b"User-agent: *\nAllow: /\n");
        if let Outcome::Complete { head, body } = &mut observation.outcome {
            match mode {
                0 => *body = vec![b'x'; crate::collection::PAGE_BYTES + 1],
                1 => head.media_type = Some("x".repeat(129)),
                2 => head.redirect_url = Some("x".repeat(2049)),
                _ => {
                    observation.resolved.as_mut().unwrap().addresses =
                        vec!["1.1.1.1:443".parse().unwrap(); 65]
                }
            }
        }
        assert!(f
            .w
            .settle_access_experiment(&ticket, &observation, &f.owner)
            .is_err());
        assert_eq!(snapshot(&f.w), stable);
    }
    f.settle(&ticket, 200, "text/plain", b"");
    let access = f.next().unwrap();
    f.settle(&access, 200, "text/html", ACCESS_HTML);
    assert!(f.next().is_none());
    let review = f.w.review_access_experiment(&f.job.id).unwrap();
    f.decide(&review, DecisionOutcome::Allow);
    f.resume();
    let seed = f.next().unwrap();
    let stable = snapshot(&f.w);
    // Complete <=2MiB response, but JSON escaping makes its derivative exceed 4MiB.
    let body = vec![1u8; 720_000];
    let observation = observed(f.at, 200, "text/plain", &body);
    for _ in 0..2 {
        assert!(f
            .w
            .settle_access_experiment(&seed, &observation, &f.owner)
            .is_err());
        assert_eq!(snapshot(&f.w), stable);
    }
    let pending = f.w.inspect_access_experiment(&f.job.id).unwrap();
    assert_eq!(pending.checkpoint.requests.len(), 3);
    assert_eq!(
        pending.checkpoint.requests[2].progress,
        RequestProgress::Reserved
    );
    assert!(find_evidence(&f.w.conn, &hash(&body)).unwrap().is_none());
    // Retention before transaction failure may leave this unreferenced inert original.
    assert_eq!(
        fs::read(f.w.root.join("originals").join(hash(&body))).unwrap(),
        body
    );
}

#[test]
fn all_fifty_charges_include_derived_robots_with_no_fifty_first_attempt() {
    let temp = tempfile::tempdir_in(std::env::temp_dir().canonicalize().unwrap()).unwrap();
    let mut w = Workspace::open(temp.path().join("case")).unwrap();
    let owner = w.collection_ownership().unwrap();
    let at = chrono::Utc::now().timestamp_millis() - 1000;
    let mut input = plan(50, 600).input().clone();
    input.access_urls = (0..50)
        .map(|n| format!("https://www.nasa.gov/access-{n}"))
        .collect();
    let job = w
        .queue_access_experiment(
            ValidatedPublisherAccessPlan::validate(input).unwrap(),
            &id(),
            &owner,
            at,
        )
        .unwrap();
    let (_, execution) = w.start_access_experiment(&job.id, 1, &owner, at).unwrap();
    let execution = execution.unwrap();
    for n in 0..50 {
        let ticket = w
            .advance_access_experiment(&execution, &owner, at)
            .unwrap()
            .unwrap();
        assert_eq!(ticket.sequence, n);
        assert_eq!(
            ticket.purpose,
            if n == 0 {
                Purpose::Robots {
                    host: "www.nasa.gov".into(),
                }
            } else {
                Purpose::SelectedAccess { index: n - 1 }
            }
        );
        let observation = observed(at, if n == 0 { 404 } else { 200 }, "text/plain", b"");
        w.settle_access_experiment(&ticket, &observation, &owner)
            .unwrap();
    }
    assert!(w
        .advance_access_experiment(&execution, &owner, at)
        .unwrap()
        .is_none());
    let final_job = w.inspect_access_experiment(&job.id).unwrap();
    assert_eq!(final_job.checkpoint.state, State::QuotaExhausted);
    assert_eq!(final_job.checkpoint.requests.len(), 50);
    assert_eq!(final_job.checkpoint.access_cursor, 49);
    assert!(final_job.checkpoint.decision.is_none());
    assert_eq!(
        get_evidence(&w.conn, &hash(b""))
            .unwrap()
            .acquisitions
            .len(),
        50
    );
    assert!(w.advance_access_experiment(&execution, &owner, at).is_err());
}

#[test]
fn selected_subhost_has_its_own_robots_and_never_content_ancestry() {
    let temp = tempfile::tempdir_in(std::env::temp_dir().canonicalize().unwrap()).unwrap();
    let mut w = Workspace::open(temp.path().join("case")).unwrap();
    let owner = w.collection_ownership().unwrap();
    let at = chrono::Utc::now().timestamp_millis() - 1000;
    let mut input = plan(50, 600).input().clone();
    input.access_urls = vec!["https://science.nasa.gov/selected-access".into()];
    let job = w
        .queue_access_experiment(
            ValidatedPublisherAccessPlan::validate(input).unwrap(),
            &id(),
            &owner,
            at,
        )
        .unwrap();
    let (_, execution) = w.start_access_experiment(&job.id, 1, &owner, at).unwrap();
    let execution = execution.unwrap();
    for (url, status) in [
        ("https://www.nasa.gov/robots.txt", 404),
        ("https://science.nasa.gov/robots.txt", 404),
        ("https://science.nasa.gov/selected-access", 200),
    ] {
        let ticket = w
            .advance_access_experiment(&execution, &owner, at)
            .unwrap()
            .unwrap();
        assert_eq!(ticket.url, url);
        w.settle_access_experiment(&ticket, &observed(at, status, "text/plain", b""), &owner)
            .unwrap();
    }
    assert!(w
        .advance_access_experiment(&execution, &owner, at)
        .unwrap()
        .is_none());
    let loaded = w.inspect_access_experiment(&job.id).unwrap();
    assert_eq!(loaded.checkpoint.state, State::AwaitingDecision);
    assert_eq!(loaded.checkpoint.requests[2].robots_sequence, Some(1));
    assert!(loaded
        .checkpoint
        .requests
        .iter()
        .all(|r| r.content_parent.is_none()));
    assert!(all_evidence(&w.conn)
        .unwrap()
        .iter()
        .all(|e| e.text.is_none()));
}

#[test]
fn ordinary_coordinator_startup_and_shutdown_ignore_private_experimental_runs() {
    let mut f = Fixture::new(50, 600);
    f.awaiting();
    let before = snapshot(&f.w);
    let root = f.w.root.clone();
    f.owner.release().unwrap();
    let coordinator = crate::coordinator::JobCoordinator::start(f.w, 1).unwrap();
    coordinator.shutdown().unwrap();
    drop(coordinator);
    let reopened = Workspace::open(root).unwrap();
    assert_eq!(snapshot(&reopened), before);
    assert_eq!(
        reopened.inspect_access_experiment(&f.job.id).unwrap(),
        f.job
    );
    assert_eq!(
        reopened
            .page_collection_runs(
                &crate::collection_api::CollectionRunPageRequest {
                    page_size: 25,
                    cursor: None
                },
                None
            )
            .unwrap()
            .scope_count,
        0
    );
}

#[test]
fn malformed_raw_receipt_clock_is_refused_before_source_metadata_formatting() {
    let mut f = Fixture::new(50, 600);
    f.awaiting();
    let mut stored: serde_json::Value = serde_json::from_str(&raw(&f.w, &f.job.id)).unwrap();
    let event = stored["events"]
        .as_array_mut()
        .unwrap()
        .iter_mut()
        .find(|e| e["event"] == "observed")
        .unwrap();
    event["receipt"]["observed_wall_ms"] = json!(i64::MAX);
    f.w.conn
        .execute(
            "UPDATE records SET body=? WHERE kind=? AND id=?",
            params![serde_json::to_string(&stored).unwrap(), KIND, f.job.id],
        )
        .unwrap();
    assert!(f.w.inspect_access_experiment(&f.job.id).is_err());
}

#[test]
fn review_selected_robots_alias_requires_200_instead_of_derived_404_permission() {
    let mut f = Fixture::new(50, 600);
    let robots = f.next().unwrap();
    f.settle(&robots, 404, "text/plain", b"missing robots");
    assert!(f.next().is_none());
    let blocked = f.w.inspect_access_experiment(&f.job.id).unwrap();
    assert_eq!(blocked.checkpoint.state, State::Blocked);
    assert_eq!(blocked.checkpoint.requests.len(), 1);
    assert_eq!(blocked.checkpoint.access_cursor, 0);
    assert!(f.w.review_access_experiment(&f.job.id).is_err());
    assert!(get_evidence(&f.w.conn, &hash(b"missing robots"))
        .unwrap()
        .text
        .is_none());
}

#[test]
fn review_unknown_observation_quarantines_before_bounds_replay_or_receipt_validation() {
    for mode in 0..3 {
        let mut f = Fixture::new(50, 600);
        let ticket = f.next().unwrap();
        let mut observation = observed(f.at, 200, "text/plain", b"unknown complete fixture");
        observation.locally_quiescent = false;
        if mode == 0 {
            if let Outcome::Complete { body, .. } = &mut observation.outcome {
                *body = vec![b'x'; crate::collection::PAGE_BYTES + 1];
            }
        } else if mode == 1 {
            f.w.conn
                .execute(
                    "UPDATE records SET body='{}' WHERE kind=? AND id=?",
                    params![KIND, f.job.id],
                )
                .unwrap();
        }
        let before = snapshot(&f.w);
        assert!(f
            .w
            .settle_access_experiment(&ticket, &observation, &f.owner)
            .is_err());
        assert!(
            !f.owner.held(),
            "unknown mode {mode} kept executable ownership"
        );
        assert!(f.owner.publication_held());
        assert_eq!(snapshot(&f.w), before);
        assert!(f
            .w
            .advance_access_experiment(&f.execution, &f.owner, f.at)
            .is_err());
    }
    let mut f = Fixture::new(50, 600);
    let mut ticket = f.next().unwrap();
    let mut observation = observed(f.at, 200, "text/plain", b"unbound fixture");
    observation.locally_quiescent = false;
    ticket.ownership_lifetime = id();
    assert!(f
        .w
        .settle_access_experiment(&ticket, &observation, &f.owner)
        .is_err());
    assert!(
        f.owner.held(),
        "unbound ticket cannot quarantine an unrelated owner"
    );
}

#[test]
fn new_nested_seed_and_reserved_objects_reject_raw_unknown_and_duplicate_fields() {
    let mut f = Fixture::new(50, 600);
    let review = f.awaiting();
    f.decide(&review, DecisionOutcome::Allow);
    f.resume();
    f.next().unwrap();
    let baseline = raw(&f.w, &f.job.id);
    assert!(f.w.inspect_access_experiment(&f.job.id).is_ok());
    for (needle, replacement) in [
        (
            r#""purpose":{"kind":"seed"}"#,
            r#""purpose":{"kind":"seed","unexpected":0}"#,
        ),
        (
            r#""purpose":{"kind":"seed"}"#,
            r#""purpose":{"kind":"seed","unexpected":0,"unexpected":1}"#,
        ),
        (
            r#""progress":{"state":"reserved"}"#,
            r#""progress":{"state":"reserved","unexpected":0}"#,
        ),
        (
            r#""progress":{"state":"reserved"}"#,
            r#""progress":{"state":"reserved","unexpected":0,"unexpected":1}"#,
        ),
    ] {
        assert_eq!(baseline.matches(needle).count(), 1);
        let changed = baseline.replacen(needle, replacement, 1);
        assert!(
            serde_json::from_str::<Job>(&changed).is_err(),
            "accepted {replacement}"
        );
        f.w.conn
            .execute(
                "UPDATE records SET body=? WHERE kind=? AND id=?",
                params![changed, KIND, f.job.id],
            )
            .unwrap();
        assert!(f.w.inspect_access_experiment(&f.job.id).is_err());
    }
    f.w.conn
        .execute(
            "UPDATE records SET body=? WHERE kind=? AND id=?",
            params![baseline, KIND, f.job.id],
        )
        .unwrap();
    assert!(f.w.inspect_access_experiment(&f.job.id).is_ok());
}
