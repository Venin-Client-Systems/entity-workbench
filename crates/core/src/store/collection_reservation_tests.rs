use super::*;
use crate::collection_transport::{Observation, Outcome, Phase, ResolvedCandidates, ResponseHead};

const HTML: &[u8] = b"<p>Retained original</p><a href='/next'>Next</a>";
struct Fixture {
    temp: tempfile::TempDir,
    workspace: Workspace,
    owner: CollectionOwnership,
    execution: CollectionTicket,
    input: CollectionInput,
    deadline: i64,
    at: i64,
}
fn observation(at: i64, status: u16, bytes: &[u8]) -> Observation {
    Observation {
        outcome: Outcome::Complete {
            head: ResponseHead {
                status,
                media_type: Some("text/html".into()),
                redirect_url: None,
                identity_encoding: true,
            },
            body: bytes.to_vec(),
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
    fn fresh() -> Self {
        Self::fresh_at(chrono::Utc::now().timestamp_millis() - 1000)
    }
    fn fresh_at(at: i64) -> Self {
        let temp = tempfile::tempdir_in(std::env::temp_dir().canonicalize().unwrap()).unwrap();
        let mut workspace = Workspace::open(temp.path().join("case")).unwrap();
        let owner = workspace.collection_ownership().unwrap();
        let input = CollectionInput {
            urls: vec!["https://collection.invalid/start".into()],
            max_hops: 2,
            max_requests: 8,
            max_seconds: 60,
        };
        let job = workspace
            .queue_collection_protocol(input.clone(), &id(), at, CollectionProtocol::SyntheticV4)
            .unwrap();
        let execution = workspace
            .start_durable_collection(&job.id, 1, &owner, at)
            .unwrap()
            .unwrap();
        Self {
            temp,
            workspace,
            owner,
            execution,
            input,
            deadline: at + 60_000,
            at,
        }
    }
    fn history() -> Self {
        let mut f = Self::fresh();
        for (status, bytes) in [(404, b"missing".as_slice()), (200, HTML)] {
            let request = f
                .workspace
                .advance_durable_collection(&f.execution, &f.owner, f.at)
                .unwrap()
                .unwrap();
            f.workspace
                .settle_collection_transport(&request, &observation(f.at, status, bytes), &f.owner)
                .unwrap();
        }
        f
    }
    fn capture(&self) -> CollectionReservationCapture {
        self.workspace
            .capture_collection_reservation(
                &self.execution,
                &self.owner,
                CollectionProtocol::SyntheticV4,
                &self.input,
                self.deadline,
            )
            .unwrap()
    }
    fn prepare(&self) -> PreparedCollectionReservation {
        self.capture().prepare().unwrap()
    }
    fn commit(
        &mut self,
        prepared: PreparedCollectionReservation,
        cancelled: bool,
    ) -> Result<Option<RequestTicket>> {
        self.workspace.reserve_prepared_collection(
            prepared,
            &self.execution,
            &self.owner,
            cancelled,
            self.at + 1,
        )
    }
    fn inspect(&self) -> DurableCollectionJob {
        self.workspace
            .inspect_durable_collection(&self.execution.job_id)
            .unwrap()
    }
}

#[test]
fn prepared_reservation_replays_once_and_matches_existing_event_bytes_and_revision() {
    for cancelled in [false, true] {
        let mut f = Fixture::history();
        let before = f.inspect();
        let revision = f.workspace.revision().unwrap();
        let backup = f.workspace.backup().unwrap();
        let mut reference = Workspace::restore(&backup, &f.temp.path().join("reference")).unwrap();
        let reference_owner = reference.collection_ownership().unwrap();
        let replays = collection_machine::replay_calls();
        let prepared = f.prepare();
        let next = f.commit(prepared, cancelled).unwrap();
        assert_eq!(collection_machine::replay_calls() - replays, 1);
        if cancelled {
            reference
                .cancel_durable_collection(&f.execution.job_id, 1, f.at + 1)
                .unwrap();
        }
        let expected = reference
            .advance_durable_collection(&f.execution, &reference_owner, f.at + 1)
            .unwrap();
        assert_eq!(next, expected);
        assert_eq!(
            f.workspace.revision().unwrap(),
            revision + if cancelled { 2 } else { 1 }
        );
        assert_eq!(
            f.workspace.revision().unwrap(),
            reference.revision().unwrap()
        );
        let after = f.inspect();
        assert_eq!(
            serde_json::to_vec(&after).unwrap(),
            serde_json::to_vec(&reference.inspect_durable_collection(&before.id).unwrap()).unwrap()
        );
        assert_eq!(
            after.checkpoint.deadline_at_ms,
            before.checkpoint.deadline_at_ms
        );
        assert_eq!(after.events[..before.events.len()], before.events);
        assert_eq!(
            serde_json::to_vec(&f.workspace.view().unwrap().evidence).unwrap(),
            serde_json::to_vec(&reference.view().unwrap().evidence).unwrap()
        );
    }
}
#[test]
fn canonical_cancel_suffix_wins_without_new_charge_and_other_drift_is_rejected() {
    let mut f = Fixture::history();
    let prepared = f.prepare();
    f.workspace
        .cancel_durable_collection(&f.execution.job_id, 1, f.at + 1)
        .unwrap();
    assert!(f.commit(prepared, true).unwrap().is_none());
    let done = f.inspect();
    assert_eq!(done.checkpoint.state, CollectionState::Cancelled);
    assert_eq!(done.checkpoint.requests_used(), 2);
    let mut f = Fixture::history();
    let prepared = f.prepare();
    f.workspace
        .recover_collections(&f.owner, f.at + 1, Some(CollectionProtocol::SyntheticV4))
        .unwrap();
    let revision = f.workspace.revision().unwrap();
    let before = f.inspect();
    assert!(f.commit(prepared, false).is_err());
    assert_eq!(f.workspace.revision().unwrap(), revision);
    assert_eq!(f.inspect(), before);
}
#[test]
fn unrelated_revision_is_allowed_but_owner_quarantine_and_replacement_are_not() {
    let mut f = Fixture::history();
    let prepared = f.prepare();
    f.workspace
        .import("unrelated.txt", b"Independent canonical write")
        .unwrap();
    let revision = f.workspace.revision().unwrap();
    assert_eq!(f.commit(prepared, false).unwrap().unwrap().sequence, 2);
    assert_eq!(f.workspace.revision().unwrap(), revision + 1);
    for quarantine in [false, true] {
        let mut f = Fixture::history();
        let prepared = f.prepare();
        if quarantine {
            f.owner.quarantine();
            assert!(f.owner.publication_held());
        } else {
            f.owner.release().unwrap();
            f.owner = f.workspace.collection_ownership().unwrap();
        }
        let revision = f.workspace.revision().unwrap();
        assert!(f.commit(prepared, false).is_err());
        assert_eq!(f.workspace.revision().unwrap(), revision);
        assert_eq!(f.inspect().checkpoint.requests_used(), 2);
    }
}
#[test]
fn source_changes_and_driver_input_or_deadline_mismatch_cannot_reserve() {
    let mut f = Fixture::history();
    let mut input = f.input.clone();
    input.max_requests -= 1;
    assert!(f
        .workspace
        .capture_collection_reservation(
            &f.execution,
            &f.owner,
            CollectionProtocol::SyntheticV4,
            &input,
            f.deadline
        )
        .is_err());
    assert!(f
        .workspace
        .capture_collection_reservation(
            &f.execution,
            &f.owner,
            CollectionProtocol::SyntheticV4,
            &f.input,
            f.deadline + 1
        )
        .is_err());
    let prepared = f.prepare();
    let sha = hash(HTML);
    let mut source = get_evidence(&f.workspace.conn, &sha).unwrap();
    source.name = "Changed canonical metadata".into();
    put(&f.workspace.conn, "evidence", &sha, &source).unwrap();
    assert!(f.commit(prepared, false).is_err());
    let prepared = f.prepare();
    let original = f.workspace.root.join("originals").join(sha);
    #[cfg(windows)]
    {
        // The test owns this fixture; clear only its DOS read-only attribute.
        #[allow(clippy::permissions_set_readonly_false)]
        let writable = {
            let mut p = fs::metadata(&original).unwrap().permissions();
            p.set_readonly(false);
            p
        };
        fs::set_permissions(&original, writable).unwrap();
    }
    fs::remove_file(&original).unwrap();
    let mut changed = HTML.to_vec();
    changed[3] ^= 1;
    fs::write(original, changed).unwrap();
    let revision = f.workspace.revision().unwrap();
    assert!(f.commit(prepared, false).is_err());
    assert!(f.capture().prepare().is_err());
    assert_eq!(f.workspace.revision().unwrap(), revision);
}
#[test]
fn cancelled_advance_failure_keeps_separate_cancel_write_and_never_adopts_new_revision() {
    for additional_revision in [false, true] {
        let mut f = Fixture::history();
        let prepared = f.prepare();
        let revision = f.workspace.revision().unwrap();
        if additional_revision {
            // An injected intervening revision must not be silently adopted.
            f.workspace.conn.execute_batch("CREATE TRIGGER injected_revision AFTER UPDATE ON records WHEN NEW.kind='collection_run' AND json_extract(NEW.body,'$.events[#-1].event')='cancel' BEGIN UPDATE meta SET revision=revision+1; END;").unwrap();
        } else {
            f.workspace.conn.execute_batch("CREATE TRIGGER refuse_advance BEFORE UPDATE ON records WHEN NEW.kind='collection_run' AND json_extract(NEW.body,'$.events[#-1].event')='advance' BEGIN SELECT RAISE(ABORT,'fixed advance refusal'); END;").unwrap();
        }
        assert!(f.commit(prepared, true).is_err());
        let after = f.inspect();
        assert_eq!(after.checkpoint.state, CollectionState::Running);
        assert!(after.checkpoint.cancellation_requested);
        assert_eq!(after.checkpoint.requests_used(), 2);
        assert!(matches!(
            after.events.last(),
            Some(CollectionEvent::Cancel { .. })
        ));
        assert_eq!(
            f.workspace.revision().unwrap(),
            revision + if additional_revision { 2 } else { 1 }
        );
    }
}
#[test]
fn future_history_replays_but_fresh_cancel_and_advance_keep_strict_clock_rules() {
    for cancelled in [false, true] {
        let mut f = Fixture::fresh();
        let mut history = f.inspect();
        let future = chrono::Utc::now().timestamp_millis() + 60_000;
        history.created_at_ms = future;
        let CollectionEvent::Start { at_ms, .. } = &mut history.events[0] else {
            panic!("start only")
        };
        *at_ms = future;
        history.checkpoint.first_started_at_ms = Some(future);
        history.checkpoint.updated_at_ms = future;
        history.checkpoint.deadline_at_ms = Some(future + 60_000);
        f.deadline = future + 60_000;
        put(&f.workspace.conn, "collection_run", &history.id, &history).unwrap();
        let prepared = f.prepare();
        let revision = f.workspace.revision().unwrap();
        assert!(f.commit(prepared, cancelled).is_err());
        assert_eq!(f.workspace.revision().unwrap(), revision);
        assert_eq!(f.inspect(), history);
        let prepared = f.prepare();
        assert!(f
            .workspace
            .reserve_prepared_collection(prepared, &f.execution, &f.owner, cancelled, future)
            .is_err());
        assert_eq!(f.inspect(), history);
    }
}

#[test]
fn expired_first_deadline_finishes_without_reservation_or_clock_reset() {
    let mut f = Fixture::fresh_at(chrono::Utc::now().timestamp_millis() - 61_000);
    let before = f.inspect();
    let prepared = f.prepare();
    let result = f
        .workspace
        .reserve_prepared_collection(
            prepared,
            &f.execution,
            &f.owner,
            false,
            chrono::Utc::now().timestamp_millis(),
        )
        .unwrap();
    assert!(result.is_none());
    let done = f.inspect();
    assert_eq!(done.checkpoint.state, CollectionState::QuotaExhausted);
    assert_eq!(done.checkpoint.requests_used(), 0);
    assert_eq!(
        done.checkpoint.first_started_at_ms,
        before.checkpoint.first_started_at_ms
    );
    assert_eq!(
        done.checkpoint.deadline_at_ms,
        before.checkpoint.deadline_at_ms
    );
}
