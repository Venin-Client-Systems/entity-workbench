use super::*;
use crate::collection_transport::{Observation, Outcome, Phase, ResolvedCandidates, ResponseHead};

const HTML: &[u8] = b"<p>Retained original</p><a href='/next'>Next</a>";
struct Fixture {
    temp: tempfile::TempDir,
    workspace: Workspace,
    owner: CollectionOwnership,
    execution: CollectionTicket,
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
    fn prepare(&self) -> PreparedCollectionCancellation {
        self.workspace
            .capture_collection_cancellation(
                &self.execution.job_id,
                1,
                &self.owner,
                CollectionProtocol::SyntheticV4,
            )
            .unwrap()
            .prepare()
            .unwrap()
    }
    fn ready(
        &self,
        prepared: PreparedCollectionCancellation,
        at: i64,
    ) -> Result<ReadyCollectionCancellation> {
        self.workspace
            .ready_collection_cancellation(prepared, &self.owner, at)
    }
    fn commit(
        &mut self,
        prepared: PreparedCollectionCancellation,
        at: i64,
    ) -> Result<crate::collection_api::CollectionRunInspection> {
        let ready = self.ready(prepared, at)?;
        let response = ready.inspection()?;
        self.workspace
            .commit_collection_cancellation(ready, &self.owner)?;
        Ok(response)
    }
    fn inspect(&self) -> DurableCollectionJob {
        self.workspace
            .inspect_durable_collection(&self.execution.job_id)
            .unwrap()
    }
}

#[test]
fn prepared_cancel_replays_once_matches_legacy_event_bytes_and_projection_then_reopens() {
    let mut f = Fixture::history();
    let backup = f.workspace.backup().unwrap();
    let mut reference = Workspace::restore(&backup, &f.temp.path().join("reference")).unwrap();
    let before = f.inspect();
    let revision = f.workspace.revision().unwrap();
    let replays = collection_machine::replay_calls();
    let result = f.commit(f.prepare(), f.at + 1).unwrap();
    assert_eq!(collection_machine::replay_calls() - replays, 1);
    assert!(result.run.cancellation_requested);
    assert_eq!(result.run.state, CollectionState::Running);
    assert_eq!(result.workspace_revision, revision + 1);
    reference
        .cancel_durable_collection(&before.id, 1, f.at + 1)
        .unwrap();
    assert_eq!(
        f.inspect(),
        reference.inspect_durable_collection(&before.id).unwrap()
    );
    assert_eq!(
        serde_json::to_value(&result).unwrap(),
        serde_json::to_value(reference.inspect_collection_run(&before.id).unwrap()).unwrap()
    );
    assert_eq!(f.inspect().checkpoint.requests, before.checkpoint.requests);
    assert_eq!(
        f.inspect().checkpoint.deadline_at_ms,
        before.checkpoint.deadline_at_ms
    );
    assert_eq!(
        serde_json::to_value(f.workspace.view().unwrap().evidence).unwrap(),
        serde_json::to_value(reference.view().unwrap().evidence).unwrap()
    );
    let revision = f.workspace.revision().unwrap();
    let again = f.commit(f.prepare(), i64::MAX).unwrap(); // No new timestamp when already canonical.
    assert_eq!(
        serde_json::to_value(again).unwrap(),
        serde_json::to_value(result).unwrap()
    );
    assert_eq!(f.workspace.revision().unwrap(), revision);
    let reopened = Workspace::open(f.workspace.root.clone()).unwrap();
    assert_eq!(
        reopened.inspect_durable_collection(&before.id).unwrap(),
        f.inspect()
    );
}
#[test]
fn queued_and_interrupted_cancel_preserve_existing_terminal_semantics() {
    for interrupted in [false, true] {
        let mut f = Fixture::fresh();
        let job = if interrupted {
            f.workspace
                .recover_collections(&f.owner, f.at, Some(CollectionProtocol::SyntheticV4))
                .unwrap();
            f.inspect()
        } else {
            f.workspace
                .queue_collection_protocol(
                    f.inspect().input,
                    &id(),
                    f.at,
                    CollectionProtocol::SyntheticV4,
                )
                .unwrap()
        };
        let revision = f.workspace.revision().unwrap();
        let prepared = f
            .workspace
            .capture_collection_cancellation(
                &job.id,
                job.checkpoint.generation,
                &f.owner,
                CollectionProtocol::SyntheticV4,
            )
            .unwrap()
            .prepare()
            .unwrap();
        let ready = f
            .workspace
            .ready_collection_cancellation(prepared, &f.owner, f.at + 1)
            .unwrap();
        let projected = ready.inspection().unwrap();
        assert_eq!(projected.run.state, CollectionState::Cancelled);
        f.workspace
            .commit_collection_cancellation(ready, &f.owner)
            .unwrap();
        assert_eq!(f.workspace.revision().unwrap(), revision + 1);
        let done = f.workspace.inspect_durable_collection(&job.id).unwrap();
        assert_eq!(done.checkpoint.requests, job.checkpoint.requests);
        assert_eq!(done.events.len(), job.events.len() + 1);
    }
}
#[test]
fn exact_cancel_suffix_is_idempotent_but_recovery_or_reservation_drift_is_refused() {
    let mut f = Fixture::history();
    let prepared = f.prepare();
    f.workspace
        .cancel_durable_collection(&f.execution.job_id, 1, f.at + 1)
        .unwrap();
    let revision = f.workspace.revision().unwrap();
    f.commit(prepared, f.at + 2).unwrap();
    assert_eq!(f.workspace.revision().unwrap(), revision);
    for recover in [false, true] {
        let mut f = Fixture::history();
        let prepared = f.prepare();
        if recover {
            f.workspace
                .recover_collections(&f.owner, f.at + 1, Some(CollectionProtocol::SyntheticV4))
                .unwrap();
        } else {
            f.workspace
                .advance_durable_collection(&f.execution, &f.owner, f.at + 1)
                .unwrap();
        }
        let before = f.inspect();
        let revision = f.workspace.revision().unwrap();
        assert!(f.commit(prepared, f.at + 2).is_err());
        assert_eq!(f.inspect(), before);
        assert_eq!(f.workspace.revision().unwrap(), revision);
    }
}
#[test]
fn strict_fresh_clock_rejection_and_historical_cancel_after_rollback_are_distinct() {
    let mut f = Fixture::history();
    for time in [f.at - 1, chrono::Utc::now().timestamp_millis() + 60_000] {
        let before = f.inspect();
        let revision = f.workspace.revision().unwrap();
        assert!(f.commit(f.prepare(), time).is_err());
        assert_eq!(f.inspect(), before);
        assert_eq!(f.workspace.revision().unwrap(), revision);
    }
    // Deterministic historical future anchor models a later wall-clock rollback.
    let future = chrono::Utc::now().timestamp_millis() + 60_000;
    let mut loaded = f.workspace.load_collection(&f.execution.job_id).unwrap();
    let event = CollectionEvent::Cancel { at_ms: future };
    let prepared = f.prepare();
    loaded.machine.replay_cancel_suffix(&event).unwrap();
    loaded.job.events.push(event);
    loaded.job.checkpoint = loaded.machine.checkpoint.clone();
    f.workspace.publish_collection(&loaded, None, None).unwrap();
    let revision = f.workspace.revision().unwrap();
    let result = f
        .commit(prepared, chrono::Utc::now().timestamp_millis())
        .unwrap();
    assert_eq!(result.run.updated_at_ms, future);
    assert_eq!(f.workspace.revision().unwrap(), revision);
    assert_eq!(result.run.state, CollectionState::Running);
    f.commit(f.prepare(), chrono::Utc::now().timestamp_millis())
        .unwrap();
    assert_eq!(f.workspace.revision().unwrap(), revision);
}
#[test]
fn owner_source_and_protocol_changes_refuse_without_cancellation_or_write() {
    for kind in 0..5 {
        let mut f = Fixture::history();
        let prepared = f.prepare();
        match kind {
            0 => f.owner.quarantine(),
            1 => {
                f.owner.release().unwrap();
                f.owner = f.workspace.collection_ownership().unwrap();
            }
            2 => {
                let sha = hash(HTML);
                let mut source = get_evidence(&f.workspace.conn, &sha).unwrap();
                source.name = "Changed source".into();
                put(&f.workspace.conn, "evidence", &sha, &source).unwrap();
            }
            3 => {
                let original = f.workspace.root.join("originals").join(hash(HTML));
                #[cfg(windows)]
                {
                    // Clear only the owned fixture's DOS read-only attribute.
                    #[allow(clippy::permissions_set_readonly_false)]
                    let writable = {
                        let mut p = fs::metadata(&original).unwrap().permissions();
                        p.set_readonly(false);
                        p
                    };
                    fs::set_permissions(&original, writable).unwrap();
                }
                fs::remove_file(&original).unwrap();
                fs::write(original, vec![b'x'; HTML.len()]).unwrap();
            }
            _ => {
                let mut job = f.inspect();
                job.synthetic = false;
                put(&f.workspace.conn, "collection_run", &job.id, &job).unwrap();
            }
        }
        let revision = f.workspace.revision().unwrap();
        assert!(f.commit(prepared, f.at + 1).is_err());
        assert_eq!(f.workspace.revision().unwrap(), revision);
    }
    for protocol in [
        CollectionProtocol::SyntheticV2,
        CollectionProtocol::SyntheticV3,
        CollectionProtocol::NativeV4,
    ] {
        let f = Fixture::fresh();
        assert!(f
            .workspace
            .capture_collection_cancellation(&f.execution.job_id, 1, &f.owner, protocol)
            .is_err());
    }
}
#[test]
fn preflight_and_publication_conflicts_do_not_write_or_adopt_a_new_revision() {
    let mut f = Fixture::history();
    let ready = f.ready(f.prepare(), f.at + 1).unwrap();
    let mut ack = ready.inspection().unwrap();
    ack.limitations
        .push("x".repeat(crate::collection_api::RESPONSE_BYTES));
    assert!(public_api::response_bound(&ack).is_err());
    let before = f.inspect();
    let revision = f.workspace.revision().unwrap();
    drop(ready);
    assert_eq!(f.inspect(), before);
    assert_eq!(f.workspace.revision().unwrap(), revision);
    let ready = f.ready(f.prepare(), f.at + 1).unwrap();
    f.workspace
        .import("unrelated.txt", b"canonical revision change")
        .unwrap();
    let revision = f.workspace.revision().unwrap();
    assert!(f
        .workspace
        .commit_collection_cancellation(ready, &f.owner)
        .is_err());
    assert_eq!(f.workspace.revision().unwrap(), revision);
    assert_eq!(f.inspect(), before);
    let ready = f.ready(f.prepare(), f.at + 1).unwrap();
    f.workspace.conn.execute_batch("CREATE TRIGGER refuse_cancel BEFORE UPDATE ON records WHEN NEW.kind='collection_run' BEGIN SELECT RAISE(ABORT,'fixed cancellation refusal'); END;").unwrap();
    assert!(f
        .workspace
        .commit_collection_cancellation(ready, &f.owner)
        .is_err());
    assert_eq!(f.workspace.revision().unwrap(), revision);
    assert_eq!(f.inspect(), before);
}

#[test]
fn historical_protocols_have_no_prepared_fallback_and_ready_owner_is_rechecked() {
    let mut f = Fixture::fresh();
    for protocol in [
        CollectionProtocol::FoundationV1,
        CollectionProtocol::SyntheticV2,
        CollectionProtocol::SyntheticV3,
        CollectionProtocol::NativeV3,
        CollectionProtocol::NativeV4,
    ] {
        let job = f
            .workspace
            .queue_collection_protocol(f.inspect().input, &id(), f.at, protocol)
            .unwrap();
        let before = serde_json::to_vec(&job).unwrap();
        let revision = f.workspace.revision().unwrap();
        assert!(f
            .workspace
            .capture_collection_cancellation(&job.id, 1, &f.owner, CollectionProtocol::SyntheticV4)
            .is_err());
        assert_eq!(
            serde_json::to_vec(&f.workspace.inspect_durable_collection(&job.id).unwrap()).unwrap(),
            before
        );
        assert_eq!(f.workspace.revision().unwrap(), revision);
    }
    for quarantine in [false, true] {
        let mut f = Fixture::history();
        let ready = f.ready(f.prepare(), f.at + 1).unwrap();
        let revision = f.workspace.revision().unwrap();
        if quarantine {
            f.owner.quarantine();
        } else {
            f.owner.release().unwrap();
            f.owner = f.workspace.collection_ownership().unwrap();
        }
        assert!(f
            .workspace
            .commit_collection_cancellation(ready, &f.owner)
            .is_err());
        assert_eq!(f.workspace.revision().unwrap(), revision);
        assert!(!f.inspect().checkpoint.cancellation_requested);
    }
}
