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
    fn inspect(&self) -> DurableCollectionJob {
        self.workspace
            .inspect_durable_collection(&self.execution.job_id)
            .unwrap()
    }
    fn export(&self, key: &str) -> Result<DurableCollectionExport> {
        self.workspace
            .export_durable_collection(&self.execution.job_id, key)
    }
}
fn rewrite(path: &Path, bytes: &[u8]) {
    fs::remove_file(path).unwrap();
    fs::write(path, bytes).unwrap();
}
fn canonical(w: &Workspace) -> Vec<u8> {
    let records: Vec<(String, String, String)> = w
        .conn
        .prepare("SELECT kind,id,body FROM records ORDER BY sequence")
        .unwrap()
        .query_map([], |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?)))
        .unwrap()
        .collect::<std::result::Result<_, _>>()
        .unwrap();
    let counts: (u64, u64) = w
        .conn
        .query_row(
            "SELECT (SELECT count(*) FROM history),(SELECT count(*) FROM events)",
            [],
            |r| Ok((r.get(0)?, r.get(1)?)),
        )
        .unwrap();
    serde_json::to_vec(&(w.revision().unwrap(), records, counts)).unwrap()
}
#[test]
fn snapshot_and_export_preserve_raw_v4_history_while_owner_is_held_or_quarantined() {
    let f = Fixture::history();
    let before = canonical(&f.workspace);
    let raw: String = f
        .workspace
        .conn
        .query_row(
            "SELECT body FROM records WHERE kind='collection_run' AND id=?",
            [&f.execution.job_id],
            |r| r.get(0),
        )
        .unwrap();
    f.owner.quarantine();
    assert!(f.owner.publication_held() && !f.owner.held());
    let snapshot = f
        .workspace
        .durable_collection_snapshot(&f.execution.job_id)
        .unwrap();
    assert_eq!(snapshot.canonical_run_json, raw);
    assert_eq!(snapshot.canonical_run_sha256, hash(raw.as_bytes()));
    assert_eq!(snapshot.inspection.run.state, CollectionState::Running);
    assert_eq!(snapshot.sources.len(), 2);
    assert!(snapshot.sources.iter().all(|s| s
        .acquisitions
        .iter()
        .all(|a| a.job_id == f.execution.job_id)));
    let key = id();
    let exported = f.export(&key).unwrap();
    assert_eq!(exported.originals.len(), 2);
    assert!(exported.originals.iter().all(|o| o.path.ends_with(".bin")));
    let inspected = f.workspace.inspect_durable_collection_export(&key).unwrap();
    assert_eq!(
        serde_json::to_value(inspected.snapshot).unwrap(),
        serde_json::to_value(snapshot).unwrap()
    );
    assert_eq!(canonical(&f.workspace), before);
    assert!(f
        .workspace
        .capture_collection_reservation(
            &f.execution,
            &f.owner,
            CollectionProtocol::SyntheticV4,
            &f.inspect().input,
            f.at + 60_000
        )
        .is_err());
}
#[test]
fn charged_unresolved_interrupted_partial_and_failed_states_are_not_reclassified() {
    for kind in 0..4 {
        let mut f = if kind == 3 {
            Fixture::fresh()
        } else {
            Fixture::history()
        };
        let request = f
            .workspace
            .advance_durable_collection(&f.execution, &f.owner, f.at)
            .unwrap()
            .unwrap();
        let expected = match kind {
            0 => CollectionState::Running,
            1 => {
                f.workspace
                    .recover_collections(&f.owner, f.at + 1, Some(CollectionProtocol::SyntheticV4))
                    .unwrap();
                CollectionState::Interrupted
            }
            _ => {
                let mut failed = observation(f.at, 200, b"");
                failed.outcome = Outcome::Stopped {
                    reason: crate::collection_transport::StopReason::Network,
                    head: None,
                };
                failed.phase = Phase::Dns;
                failed.resolved = None;
                f.workspace
                    .settle_collection_transport(&request, &failed, &f.owner)
                    .unwrap();
                f.workspace
                    .advance_durable_collection(&f.execution, &f.owner, f.at)
                    .unwrap();
                if kind == 2 {
                    CollectionState::Partial
                } else {
                    CollectionState::Failed
                }
            }
        };
        let before = f.inspect();
        assert_eq!(before.checkpoint.state, expected);
        let key = id();
        f.export(&key).unwrap();
        let frozen = f
            .workspace
            .inspect_durable_collection_export(&key)
            .unwrap()
            .snapshot;
        assert_eq!(frozen.inspection.run.state, expected);
        assert_eq!(
            serde_json::from_str::<DurableCollectionJob>(&frozen.canonical_run_json).unwrap(),
            before
        );
        if kind <= 1 {
            assert!(frozen
                .inspection
                .requests
                .last()
                .unwrap()
                .original
                .is_none());
        }
        assert_eq!(
            frozen.inspection.run.requests_used,
            before.checkpoint.requests_used()
        );
    }
}
#[test]
fn completed_id_never_overwrites_and_ack_loss_inspects_historical_bytes_after_source_changes() {
    let mut f = Fixture::history();
    let key = id();
    let first = f.export(&key).unwrap(); // Simulate acknowledgement loss by only retaining the caller's UUID.
    let before = fs::read(path(&f.workspace.root, &key).unwrap().join("complete.json")).unwrap();
    let sha = hash(HTML);
    let mut source = get_evidence(&f.workspace.conn, &sha).unwrap();
    source.name = "Later metadata".into();
    put(&f.workspace.conn, "evidence", &sha, &source).unwrap();
    rewrite(
        &f.workspace.root.join("originals").join(sha),
        b"later source corruption",
    );
    f.workspace
        .import("unrelated.txt", b"Later record")
        .unwrap();
    let reopened = Workspace::open(f.workspace.root.clone()).unwrap();
    let read = reopened.inspect_durable_collection_export(&key).unwrap();
    assert_eq!(
        serde_json::to_value(read.receipt).unwrap(),
        serde_json::to_value(first).unwrap()
    );
    assert!(f.export(&key).is_err());
    assert_eq!(
        fs::read(path(&f.workspace.root, &key).unwrap().join("complete.json")).unwrap(),
        before
    );
}
#[test]
fn drift_before_marker_rolls_back_only_this_bundle_and_leaves_canonical_state_untouched() {
    for metadata in [false, true] {
        let f = Fixture::history();
        let key = id();
        let other = Workspace::open(f.workspace.root.clone()).unwrap();
        let result = f
            .workspace
            .export_durable_collection_with(&f.execution.job_id, &key, || {
                if metadata {
                    let sha = hash(HTML);
                    let mut e = get_evidence(&other.conn, &sha)?;
                    e.name = "Drift".into();
                    put(&other.conn, "evidence", &sha, &e)?;
                } else {
                    other
                        .conn
                        .execute("UPDATE meta SET revision=revision+1", [])?;
                }
                Ok(())
            });
        assert!(result.is_err());
        assert!(!path(&f.workspace.root, &key).unwrap().exists());
        assert!(!f.inspect().checkpoint.cancellation_requested);
    }
}
#[test]
fn replaced_owned_entry_causes_explicit_cleanup_failure_without_deleting_replacement() {
    let f = Fixture::history();
    let key = id();
    let dest = path(&f.workspace.root, &key).unwrap();
    let result = f
        .workspace
        .export_durable_collection_with(&f.execution.job_id, &key, || {
            // Replacement is testable on Unix; Windows denies deletion while our read handle is held.
            #[cfg(unix)]
            {
                rewrite(&dest.join("snapshot.json"), b"unrelated replacement");
            }
            fs::write(dest.join("unrecognized.bin"), b"unrelated new entry")?;
            Err(Error::Validation("fixed prepublication refusal".into()))
        });
    assert!(matches!(result, Err(Error::Cleanup(_))));
    assert_eq!(
        fs::read(dest.join("unrecognized.bin")).unwrap(),
        b"unrelated new entry"
    );
    #[cfg(unix)]
    assert_eq!(
        fs::read(dest.join("snapshot.json")).unwrap(),
        b"unrelated replacement"
    );
    assert!(!dest.join("complete.json").exists());
}
#[test]
fn incomplete_partial_marker_bad_inventory_and_altered_original_are_refused() {
    let f = Fixture::history();
    let key = id();
    let dest = path(&f.workspace.root, &key).unwrap();
    fs::create_dir(&dest).unwrap();
    fs::create_dir(dest.join("originals")).unwrap();
    assert!(f.workspace.inspect_durable_collection_export(&key).is_err());
    fs::write(dest.join("complete.json"), b"{\"schema_version\":").unwrap();
    assert!(f.workspace.inspect_durable_collection_export(&key).is_err());
    assert!(f.export(&key).is_err());
    for kind in 0..4 {
        let key = id();
        let receipt = f.export(&key).unwrap();
        let dest = path(&f.workspace.root, &key).unwrap();
        if kind == 0 {
            rewrite(&dest.join(&receipt.originals[0].path), b"altered");
        } else if kind == 1 {
            let mut changed = receipt;
            changed.job_id = id();
            rewrite(
                &dest.join("complete.json"),
                &serde_json::to_vec(&changed).unwrap(),
            );
        } else if kind == 2 {
            fs::write(dest.join("originals/unexpected.bin"), b"unknown").unwrap();
        } else {
            rewrite(&dest.join("complete.json"), &vec![b'x'; 64 * 1024 + 1]);
        }
        assert!(f.workspace.inspect_durable_collection_export(&key).is_err());
    }
}
#[cfg(unix)]
#[test]
fn links_and_same_length_source_tampering_are_rejected() {
    use std::os::unix::fs::symlink;
    let f = Fixture::history();
    for hard in [false, true] {
        let key = id();
        let receipt = f.export(&key).unwrap();
        let dest = path(&f.workspace.root, &key).unwrap();
        let original = dest.join(&receipt.originals[0].path);
        let external = f.temp.path().join(id());
        fs::copy(&original, &external).unwrap();
        fs::remove_file(&original).unwrap();
        if hard {
            fs::hard_link(&external, &original).unwrap()
        } else {
            symlink(&external, &original).unwrap()
        }
        assert!(f.workspace.inspect_durable_collection_export(&key).is_err());
    }
    let key = id();
    let outside = f.temp.path().join("outside");
    fs::create_dir(&outside).unwrap();
    symlink(&outside, path(&f.workspace.root, &key).unwrap()).unwrap();
    assert!(f.export(&key).is_err());
    assert!(fs::read_dir(&outside).unwrap().next().is_none());
    rewrite(
        &f.workspace.root.join("originals").join(hash(HTML)),
        &vec![b'x'; HTML.len()],
    );
    assert!(f
        .workspace
        .durable_collection_snapshot(&f.execution.job_id)
        .is_err());
}
#[test]
fn backup_restore_can_reexport_same_snapshot_without_export_references_or_migration() {
    let mut f = Fixture::history();
    let key = id();
    f.export(&key).unwrap();
    let first = f
        .workspace
        .inspect_durable_collection_export(&key)
        .unwrap()
        .snapshot;
    let backup = f.workspace.backup().unwrap();
    let restored = Workspace::restore(&backup, &f.temp.path().join("restored")).unwrap();
    assert!(!path(&restored.root, &key).unwrap().exists());
    let next = id();
    restored
        .export_durable_collection(&f.execution.job_id, &next)
        .unwrap();
    let second = restored
        .inspect_durable_collection_export(&next)
        .unwrap()
        .snapshot;
    assert_eq!(
        serde_json::to_value(first).unwrap(),
        serde_json::to_value(second).unwrap()
    );
    assert_eq!(canonical(&f.workspace), canonical(&restored));
}
#[test]
fn record_and_serialization_bounds_are_enforced_without_partial_response() {
    let f = Fixture::history();
    assert!(encode(&"x".repeat(SNAPSHOT_BYTES)).is_err());
    f.workspace
        .conn
        .execute(
            "UPDATE records SET body=? WHERE kind='collection_run' AND id=?",
            params![
                format!("{{\"padding\":\"{}\"}}", "x".repeat(MAX_RECORD_BYTES)),
                f.execution.job_id
            ],
        )
        .unwrap();
    assert!(f
        .workspace
        .durable_collection_snapshot(&f.execution.job_id)
        .is_err());
    assert!(f.export(&id()).is_err());
}

#[test]
fn final_marker_publication_excludes_wal_writer_without_canonical_changes() {
    let f = Fixture::history();
    f.workspace
        .conn
        .pragma_update(None, "journal_mode", "WAL")
        .unwrap();
    let writer = Workspace::open(&f.workspace.root).unwrap();
    writer.conn.busy_timeout(std::time::Duration::ZERO).unwrap();
    let before = canonical(&f.workspace);
    let key = id();
    let attempted = std::cell::Cell::new(false);
    f.workspace
        .export_durable_collection_with_marker(
            &f.execution.job_id,
            &key,
            || Ok(()),
            || {
                attempted.set(true);
                let error = writer
                    .conn
                    .execute("UPDATE meta SET revision=revision+1", [])
                    .unwrap_err();
                assert_eq!(
                    error.sqlite_error_code(),
                    Some(rusqlite::ErrorCode::DatabaseBusy)
                );
                assert!(!path(&f.workspace.root, &key)?
                    .join("complete.json")
                    .exists());
                Ok(())
            },
        )
        .unwrap();
    assert!(attempted.get());
    assert_eq!(canonical(&f.workspace), before);
    f.workspace.inspect_durable_collection_export(&key).unwrap();
    writer
        .conn
        .execute("UPDATE meta SET revision=revision+1", [])
        .unwrap();
    assert_eq!(writer.revision().unwrap(), f.workspace.revision().unwrap());
}

#[test]
fn marker_creation_failure_retains_uncertain_bundle_and_refuses_republication() {
    let f = Fixture::history();
    let previous = id();
    f.export(&previous).unwrap();
    let key = id();
    let root = path(&f.workspace.root, &key).unwrap();
    let before = canonical(&f.workspace);
    let result = f.workspace.export_durable_collection_with_marker(
        &f.execution.job_id,
        &key,
        || Ok(()),
        || {
            fs::write(root.join("complete.json"), b"unexpected marker")?;
            Ok(())
        },
    );
    assert!(matches!(result, Err(Error::Cleanup(_))));
    assert_eq!(
        fs::read(root.join("complete.json")).unwrap(),
        b"unexpected marker"
    );
    assert!(root.join("snapshot.json").exists());
    assert!(f.workspace.inspect_durable_collection_export(&key).is_err());
    assert!(f.export(&key).is_err());
    f.workspace
        .inspect_durable_collection_export(&previous)
        .unwrap();
    assert_eq!(canonical(&f.workspace), before);
}

#[test]
fn empty_complete_original_is_retained_but_historical_protocol_is_not_reinterpreted() {
    let mut f = Fixture::fresh();
    let request = f
        .workspace
        .advance_durable_collection(&f.execution, &f.owner, f.at)
        .unwrap()
        .unwrap();
    f.workspace
        .settle_collection_transport(&request, &observation(f.at, 200, b""), &f.owner)
        .unwrap();
    let key = id();
    let receipt = f.export(&key).unwrap();
    assert_eq!(receipt.originals.len(), 1);
    assert_eq!(receipt.originals[0].bytes, 0);
    assert_eq!(receipt.originals[0].sha256, hash(b""));
    f.workspace.inspect_durable_collection_export(&key).unwrap();

    let historical = f
        .workspace
        .queue_collection_protocol(
            f.inspect().input.clone(),
            &id(),
            f.at,
            CollectionProtocol::SyntheticV3,
        )
        .unwrap();
    let before = canonical(&f.workspace);
    assert!(f
        .workspace
        .durable_collection_snapshot(&historical.id)
        .is_err());
    assert!(f
        .workspace
        .export_durable_collection(&historical.id, &id())
        .is_err());
    assert_eq!(
        f.workspace
            .inspect_durable_collection(&historical.id)
            .unwrap()
            .schema_version,
        3
    );
    assert_eq!(canonical(&f.workspace), before);
}

#[test]
fn unexpected_prepublication_entry_prevents_success_without_deleting_unrelated_bytes() {
    for directory in [false, true] {
        let f = Fixture::history();
        let key = id();
        let root = path(&f.workspace.root, &key).unwrap();
        let target = root.join("unrelated");
        let result = f
            .workspace
            .export_durable_collection_with(&f.execution.job_id, &key, || {
                if directory {
                    fs::create_dir(&target)?;
                    fs::write(target.join("keep.bin"), b"unrelated")?;
                } else {
                    fs::write(&target, b"unrelated")?;
                }
                Ok(())
            });
        assert!(matches!(result, Err(Error::Cleanup(_))));
        assert!(!root.join("complete.json").exists());
        let retained = if directory {
            target.join("keep.bin")
        } else {
            target
        };
        assert_eq!(fs::read(retained).unwrap(), b"unrelated");
        assert!(f.workspace.inspect_durable_collection_export(&key).is_err());
    }
}
