use super::*;
use crate::collection_transport::{Phase, ResolvedCandidates, ResponseHead};
struct Fixture {
    temp: tempfile::TempDir,
    workspace: Workspace,
    owner: CollectionOwnership,
    request: RequestTicket,
    observation: Observation,
    at: i64,
}
fn observation(at: i64, status: u16, body: &[u8]) -> Observation {
    Observation {
        outcome: Outcome::Complete {
            head: ResponseHead {
                status,
                media_type: Some("text/html".into()),
                redirect_url: None,
                identity_encoding: true,
            },
            body: body.to_vec(),
        },
        phase: Phase::Body,
        elapsed_milliseconds: 1,
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
impl Fixture {
    fn new() -> Self {
        let temp = tempfile::tempdir_in(std::env::temp_dir().canonicalize().unwrap()).unwrap();
        let mut workspace = Workspace::open(temp.path().join("case")).unwrap();
        let owner = workspace.collection_ownership().unwrap();
        let at = chrono::Utc::now().timestamp_millis() - 1000;
        let job = workspace
            .queue_collection_protocol(
                CollectionInput {
                    urls: vec!["https://collection.invalid/page".into()],
                    max_hops: 1,
                    max_requests: 4,
                    max_seconds: 60,
                },
                &id(),
                at,
                CollectionProtocol::SyntheticV4,
            )
            .unwrap();
        let execution = workspace
            .start_durable_collection(&job.id, 1, &owner, at)
            .unwrap()
            .unwrap();
        let robots = workspace
            .advance_durable_collection(&execution, &owner, at)
            .unwrap()
            .unwrap();
        workspace
            .settle_collection_transport(&robots, &observation(at, 404, b"missing"), &owner)
            .unwrap();
        let request = workspace
            .advance_durable_collection(&execution, &owner, at)
            .unwrap()
            .unwrap();
        Self {
            temp,
            workspace,
            owner,
            request,
            observation: observation(at, 200, b"<p>Prepared text</p><a href='/next'>next</a>"),
            at,
        }
    }
    fn capture(&self) -> CollectionCapture {
        self.workspace
            .capture_collection_settlement(&self.request, &self.observation, &self.owner)
            .unwrap()
            .unwrap()
    }
    fn prepare(&self) -> PreparedCollectionSettlement {
        self.capture().prepare(&self.observation).unwrap()
    }
    fn commit(&mut self, prepared: PreparedCollectionSettlement) -> Result<DurableCollectionJob> {
        self.workspace.commit_prepared_collection(
            prepared,
            &self.request,
            &self.observation,
            &self.owner,
            false,
            self.at,
        )
    }
    fn stored(&self) -> DurableCollectionJob {
        self.workspace
            .inspect_durable_collection(&self.request.run.job_id)
            .unwrap()
    }
}
#[test]
fn prepared_publication_accepts_unrelated_revision_but_preserves_exact_idempotent_receipt() {
    let mut f = Fixture::new();
    let prepared = f.prepare();
    f.workspace
        .import("separate.txt", b"unrelated canonical change")
        .unwrap();
    let done = f.commit(prepared).unwrap();
    assert_eq!(done.checkpoint.requests_used(), 2);
    assert_eq!(done.checkpoint.pages_retained, 1);
    let revision = f.workspace.revision().unwrap();
    let again = f.prepare();
    assert_eq!(f.commit(again).unwrap(), done);
    assert_eq!(f.workspace.revision().unwrap(), revision);
    assert_eq!(
        get_evidence(&f.workspace.conn, &hash(body(&f.observation).unwrap()))
            .unwrap()
            .acquisitions
            .len(),
        1
    );
}
#[test]
fn only_exact_cancellation_suffix_is_accepted_and_never_promotes_prepared_text() {
    let mut f = Fixture::new();
    let prepared = f.prepare();
    f.workspace
        .cancel_durable_collection(&f.request.run.job_id, 1, f.at + 1)
        .unwrap();
    let done = f.commit(prepared).unwrap();
    assert_eq!(done.checkpoint.state, CollectionState::Cancelled);
    assert_eq!(done.checkpoint.pages_retained, 0);
    let evidence = get_evidence(&f.workspace.conn, &hash(body(&f.observation).unwrap())).unwrap();
    assert!(evidence.text.is_none());
    assert_eq!(
        read_original(&f.workspace.root, &evidence).unwrap(),
        body(&f.observation).unwrap()
    );
    let mut f = Fixture::new();
    let prepared = f.prepare();
    f.workspace
        .recover_collections(&f.owner, f.at + 1, Some(CollectionProtocol::SyntheticV4))
        .unwrap();
    let before = f.stored();
    assert!(f.commit(prepared).is_err());
    assert_eq!(f.stored(), before);
}
#[test]
fn changed_owner_source_receipt_and_forged_cancel_checkpoint_are_refused_without_write() {
    let mut f = Fixture::new();
    let prepared = f.prepare();
    f.owner.release().unwrap();
    f.owner = f.workspace.collection_ownership().unwrap();
    let revision = f.workspace.revision().unwrap();
    assert!(f.commit(prepared).is_err());
    assert_eq!(f.workspace.revision().unwrap(), revision);
    let prepared = f.prepare();
    f.observation.observed_wall_ms += 1;
    assert!(f.commit(prepared).is_err());
    f.observation.observed_wall_ms -= 1;
    let capture = f.capture();
    let original_body = body(&f.observation).unwrap().to_vec();
    if let Outcome::Complete { body, .. } = &mut f.observation.outcome {
        body[3] ^= 1; // Same byte length, different exact body digest.
    }
    assert!(capture.prepare(&f.observation).is_err());
    if let Outcome::Complete { body, .. } = &mut f.observation.outcome {
        *body = original_body.clone();
    }
    let prepared = f.prepare();
    if let Outcome::Complete { body, .. } = &mut f.observation.outcome {
        body[3] ^= 1;
    }
    assert!(f.commit(prepared).is_err());
    assert_eq!(f.workspace.revision().unwrap(), revision);
    if let Outcome::Complete { body, .. } = &mut f.observation.outcome {
        *body = original_body;
    }
    let prepared = f.prepare();
    let sha = hash(b"missing");
    let mut evidence = get_evidence(&f.workspace.conn, &sha).unwrap();
    evidence.name = "changed canonical source metadata".into();
    put(&f.workspace.conn, "evidence", &sha, &evidence).unwrap();
    assert!(f.commit(prepared).is_err());
    let prepared = f.prepare();
    f.workspace
        .cancel_durable_collection(&f.request.run.job_id, 1, f.at + 1)
        .unwrap();
    let mut changed = f.stored();
    changed.checkpoint.pages_retained = 9;
    put(&f.workspace.conn, "collection_run", &changed.id, &changed).unwrap();
    let revision = f.workspace.revision().unwrap();
    assert!(f.commit(prepared).is_err());
    assert_eq!(f.workspace.revision().unwrap(), revision);
}
#[test]
fn original_mutation_after_prepare_and_failed_preparation_cannot_publish() {
    let mut f = Fixture::new();
    let prepared = f.prepare();
    let original = f.workspace.root.join("originals").join(hash(b"missing"));
    #[cfg(windows)]
    {
        // This test owns the fixture and clears its DOS read-only attribute.
        #[allow(clippy::permissions_set_readonly_false)]
        let writable = {
            let mut permissions = fs::metadata(&original).unwrap().permissions();
            permissions.set_readonly(false);
            permissions
        };
        fs::set_permissions(&original, writable).unwrap();
    }
    fs::remove_file(&original).unwrap();
    fs::write(&original, b"changed").unwrap();
    let revision = f.workspace.revision().unwrap();
    assert!(f.commit(prepared).is_err());
    assert!(f.capture().prepare(&f.observation).is_err());
    assert_eq!(f.workspace.revision().unwrap(), revision);
    assert!(body(&f.observation).unwrap().starts_with(b"<p>Prepared"));
}
#[test]
fn publication_rollback_reprepares_exact_receipt_and_crash_recovery_keeps_charge_unknown() {
    let mut f = Fixture::new();
    let prepared = f.prepare();
    let before = f.stored();
    let revision = f.workspace.revision().unwrap();
    f.workspace.conn.execute_batch("CREATE TRIGGER block_prepared BEFORE UPDATE ON records WHEN NEW.kind='collection_run' BEGIN SELECT RAISE(ABORT,'fixed publication refusal'); END;").unwrap();
    assert!(f.commit(prepared).is_err());
    assert_eq!(f.workspace.revision().unwrap(), revision);
    assert_eq!(f.stored(), before);
    assert!(
        find_evidence(&f.workspace.conn, &hash(body(&f.observation).unwrap()))
            .unwrap()
            .is_none()
    );
    f.workspace
        .conn
        .execute_batch("DROP TRIGGER block_prepared")
        .unwrap();
    let prepared = f.prepare();
    let done = f.commit(prepared).unwrap();
    assert_eq!(done.checkpoint.requests_used(), 2);
    let backup = f.workspace.backup().unwrap();
    let restored = Workspace::restore(&backup, &f.temp.path().join("restored")).unwrap();
    assert_eq!(restored.inspect_durable_collection(&done.id).unwrap(), done);
    let f = Fixture::new();
    let prepared = f.prepare();
    drop(prepared); // Model crash before publication, with no durable derived cache.
    f.owner.release().unwrap();
    let mut reopened = Workspace::open(f.temp.path().join("case")).unwrap();
    let owner = reopened.collection_ownership().unwrap();
    reopened
        .recover_collections(&owner, f.at + 1, Some(CollectionProtocol::SyntheticV4))
        .unwrap();
    let recovered = reopened
        .inspect_durable_collection(&f.request.run.job_id)
        .unwrap();
    assert_eq!(recovered.checkpoint.state, CollectionState::Interrupted);
    assert_eq!(recovered.checkpoint.requests_used(), 2);
    assert!(matches!(
        recovered.checkpoint.requests[1].progress,
        RequestProgress::InterruptedUnknown { .. }
    ));
    assert_eq!(
        recovered.checkpoint.deadline_at_ms,
        f.stored().checkpoint.deadline_at_ms
    );
    assert!(
        find_evidence(&reopened.conn, &hash(body(&f.observation).unwrap()))
            .unwrap()
            .is_none()
    );
}

/// Model a previously valid wall clock moving backwards without touching the OS
/// clock. Only start/reservation exist, so no acquisition timestamp is fabricated.
fn future_reserved_fixture() -> Fixture {
    let temp = tempfile::tempdir_in(std::env::temp_dir().canonicalize().unwrap()).unwrap();
    let mut workspace = Workspace::open(temp.path().join("case")).unwrap();
    let owner = workspace.collection_ownership().unwrap();
    let at = chrono::Utc::now().timestamp_millis() - 1000;
    let job = workspace
        .queue_collection_protocol(
            CollectionInput {
                urls: vec!["https://collection.invalid/page".into()],
                max_hops: 1,
                max_requests: 4,
                max_seconds: 60,
            },
            &id(),
            at,
            CollectionProtocol::SyntheticV4,
        )
        .unwrap();
    let execution = workspace
        .start_durable_collection(&job.id, 1, &owner, at)
        .unwrap()
        .unwrap();
    let request = workspace
        .advance_durable_collection(&execution, &owner, at)
        .unwrap()
        .unwrap();
    let mut history = workspace.inspect_durable_collection(&job.id).unwrap();
    let shift = chrono::Utc::now().timestamp_millis() + 60_000 - history.created_at_ms;
    history.created_at_ms += shift;
    for event in &mut history.events {
        match event {
            CollectionEvent::Start { at_ms, .. } | CollectionEvent::Advance { at_ms } => {
                *at_ms += shift
            }
            _ => panic!("Fixture contains start and reservation only"),
        }
    }
    history.checkpoint.first_started_at_ms =
        history.checkpoint.first_started_at_ms.map(|at| at + shift);
    history.checkpoint.deadline_at_ms = history.checkpoint.deadline_at_ms.map(|at| at + shift);
    history.checkpoint.updated_at_ms += shift;
    history.checkpoint.requests[0].reserved_at_ms += shift;
    put(&workspace.conn, "collection_run", &history.id, &history).unwrap();
    assert_eq!(
        workspace.inspect_durable_collection(&history.id).unwrap(),
        history
    );
    Fixture {
        temp,
        workspace,
        owner,
        request,
        observation: observation(at, 404, b"Exact retained rollback response"),
        at,
    }
}

fn assert_prepared_clock_rollback(published_cancel: bool) {
    let mut f = future_reserved_fixture();
    let prepared = f.prepare();
    let before = f.stored();
    let anchor = before.checkpoint.updated_at_ms;
    assert!(f
        .workspace
        .cancel_durable_collection(&before.id, 1, anchor)
        .unwrap_err()
        .to_string()
        .contains("future collection timestamp"));
    if published_cancel {
        // Exact synthetic canonical history, as if Cancel was published before
        // the OS clock rolled back. Normal new cancellation still rejects it.
        let mut current = before.clone();
        let mut replay =
            Machine::new_version(&current.input, current.created_at_ms, 4, true).unwrap();
        for event in &current.events {
            replay.apply(event, None).unwrap();
        }
        let event = CollectionEvent::Cancel { at_ms: anchor + 1 };
        replay.apply(&event, None).unwrap();
        current.events.push(event);
        current.checkpoint = replay.checkpoint;
        put(&f.workspace.conn, "collection_run", &current.id, &current).unwrap();
        assert_eq!(f.stored(), current);
    }
    let done = f
        .workspace
        .commit_prepared_collection(prepared, &f.request, &f.observation, &f.owner, true, f.at)
        .unwrap();
    assert_eq!(done.checkpoint.state, CollectionState::Failed); // Clock failure outranks cancellation.
    assert_eq!(done.checkpoint.requests_used(), 1);
    assert!(done.checkpoint.cancellation_requested);
    assert_eq!(
        done.checkpoint.updated_at_ms,
        anchor + i64::from(published_cancel)
    );
    assert!(
        matches!(&done.checkpoint.requests[0].progress, RequestProgress::Observed { receipt } if receipt == &TransportReceipt::from_observation(&f.observation))
    );
    let evidence = get_evidence(&f.workspace.conn, &hash(body(&f.observation).unwrap())).unwrap();
    assert!(evidence.text.is_none());
    assert_eq!(
        read_original(&f.workspace.root, &evidence).unwrap(),
        body(&f.observation).unwrap()
    );
    let revision = f.workspace.revision().unwrap();
    let again = f.prepare();
    assert_eq!(f.commit(again).unwrap(), done);
    assert_eq!(f.workspace.revision().unwrap(), revision);
    let reopened = Workspace::open(f.temp.path().join("case")).unwrap();
    assert_eq!(reopened.inspect_durable_collection(&done.id).unwrap(), done);
}
#[test]
fn prepared_stop_survives_clock_rollback() {
    assert_prepared_clock_rollback(false);
}
#[test]
fn prepared_canonical_cancel_suffix_survives_clock_rollback() {
    assert_prepared_clock_rollback(true);
}

#[test]
fn source_capture_preflights_aggregate_metadata_and_reference_count_before_decoding() {
    let f = Fixture::new();
    let original = f.stored();
    let mut hostile = original.clone();
    let oversized_metadata = "x".repeat(3_500_000);
    for sequence in 0..5 {
        let observed = observation(f.at, 200, format!("fixed reference {sequence}").as_bytes());
        let sha = hash(body(&observed).unwrap());
        // Valid JSON, deliberately not an Evidence object. The aggregate bound
        // must fire before any of these large metadata bodies is decoded.
        put(&f.workspace.conn, "evidence", &sha, &oversized_metadata).unwrap();
        hostile.events.push(CollectionEvent::TransportObserved {
            clock_anchor_ms: f.at,
            sequence,
            receipt: TransportReceipt::from_observation(&observed),
        });
    }
    put(&f.workspace.conn, "collection_run", &hostile.id, &hostile).unwrap();
    let revision = f.workspace.revision().unwrap();
    let error = f
        .workspace
        .capture_collection_settlement(&f.request, &f.observation, &f.owner)
        .err()
        .unwrap();
    assert!(
        error.to_string().contains("source metadata exceeds 16 MiB"),
        "{error}"
    );
    assert_eq!(f.workspace.revision().unwrap(), revision);
    hostile = original;
    for sequence in 0..50 {
        let observed = observation(f.at, 200, format!("count reference {sequence}").as_bytes());
        hostile.events.push(CollectionEvent::TransportObserved {
            clock_anchor_ms: f.at,
            sequence,
            receipt: TransportReceipt::from_observation(&observed),
        });
    }
    put(&f.workspace.conn, "collection_run", &hostile.id, &hostile).unwrap();
    let error = f
        .workspace
        .capture_collection_settlement(&f.request, &f.observation, &f.owner)
        .err()
        .unwrap();
    assert!(
        error
            .to_string()
            .contains("source capture exceeds request limit"),
        "{error}"
    );
    assert_eq!(f.workspace.revision().unwrap(), revision);
    assert!(body(&f.observation).unwrap().starts_with(b"<p>Prepared"));
}
