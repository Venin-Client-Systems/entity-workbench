use super::*;
use tempfile::TempDir;

fn specimen() -> (TempDir, Workspace, String) {
    let root = tempfile::tempdir_in(std::env::temp_dir().canonicalize().unwrap()).unwrap();
    let mut w = Workspace::open(root.path().join("case")).unwrap();
    let source = w
        .import(
            "synthetic.txt",
            b"Synthetic retained relationship source\nSecond source line\n",
        )
        .unwrap();
    w.change(None, "synthetic.graph", false, |conn| {
        for key in ["a", "b", "c", "d", "e", "f"] {
            put(
                conn,
                "entity",
                key,
                &Entity {
                    id: key.into(),
                    name: format!("Synthetic {key}"),
                    kind: EntityKind::Person,
                    identifiers: vec![],
                    merged_into: None,
                },
            )?;
        }
        for (key, a, b, state) in [
            ("r1", "a", "b", ReviewState::Accepted),
            ("r1-parallel", "b", "a", ReviewState::Accepted),
            ("r2", "b", "c", ReviewState::Accepted),
            ("long1", "a", "d", ReviewState::Accepted),
            ("long2", "d", "e", ReviewState::Accepted),
            ("long3", "e", "c", ReviewState::Accepted),
            ("shortcut-rejected", "a", "c", ReviewState::Rejected),
            ("shortcut-pending", "a", "c", ReviewState::Pending),
            ("shortcut-deferred", "a", "c", ReviewState::Deferred),
        ] {
            let observation = Observation {
                id: format!("obs-{key}"),
                entity_id: a.into(),
                field: "relationship mention".into(),
                value: "Synthetic corroboration".into(),
                anchor: SourceAnchor::Text {
                    evidence_id: source.clone(),
                    line_start: 1,
                    line_end: 1,
                },
                extraction_quality: Some(0.5),
                review: ReviewState::Accepted,
            };
            put(conn, "observation", &observation.id, &observation)?;
            let from = if key == "r1" {
                "2000-01-01"
            } else {
                "2025-01-01"
            };
            let through = if key == "r1" {
                "2001-01-01"
            } else {
                "2026-01-01"
            };
            put(
                conn,
                "assertion",
                key,
                &Assertion {
                    id: key.into(),
                    subject_id: a.into(),
                    predicate: "recorded alongside".into(),
                    object_id: b.into(),
                    observation_ids: vec![observation.id],
                    valid_from: Some(from.into()),
                    valid_to: Some(through.into()),
                    confidence: "Analyst confidence remains separate".into(),
                    review: state,
                },
            )?;
        }
        Ok(())
    })
    .unwrap();
    (root, w, source)
}
fn capture(w: &Workspace) -> CapturedGraph {
    w.capture_graph_path(w.revision().unwrap(), "a", "c")
        .unwrap()
}
fn response(handle: &CapturedGraph, outcome: Value) -> Value {
    let mut value: Value = serde_json::from_slice(handle.worker_input()).unwrap();
    for key in ["source_id", "target_id", "nodes", "edges"] {
        value.as_object_mut().unwrap().remove(key);
    }
    value["outcome"] = outcome;
    value
}
fn path(handle: &CapturedGraph, nodes: &[&str]) -> Vec<u8> {
    serde_json::to_vec(&response(handle, json!({"state":"path","nodes":nodes}))).unwrap()
}
fn unreachable(handle: &CapturedGraph) -> Vec<u8> {
    serde_json::to_vec(&response(handle, json!({"state":"unreachable"}))).unwrap()
}
fn state(w: &Workspace) -> (u64, Vec<(String, String, String)>, u64, u64) {
    let mut query = w
        .conn
        .prepare("SELECT kind,id,body FROM records ORDER BY sequence")
        .unwrap();
    (
        w.revision().unwrap(),
        query
            .query_map([], |row| Ok((row.get(0)?, row.get(1)?, row.get(2)?)))
            .unwrap()
            .collect::<std::result::Result<_, _>>()
            .unwrap(),
        w.conn
            .query_row("SELECT count(*) FROM history", [], |row| row.get(0))
            .unwrap(),
        w.conn
            .query_row("SELECT count(*) FROM events", [], |row| row.get(0))
            .unwrap(),
    )
}
fn replace<T: Serialize>(w: &Workspace, kind: &str, key: &str, value: &T) {
    // Deliberate corruption bypasses normal revision updates to exercise record fingerprints.
    w.conn
        .execute(
            "UPDATE records SET body=? WHERE kind=? AND id=?",
            params![serde_json::to_string(value).unwrap(), kind, key],
        )
        .unwrap();
}

#[test]
fn real_snapshot_is_read_only_and_preserves_all_parallel_provenance_and_time_warning() {
    let (_root, w, source) = specimen();
    let before = state(&w);
    let handle = capture(&w);
    let input: Value = serde_json::from_slice(handle.worker_input()).unwrap();
    assert_eq!(input["policy"], POLICY);
    assert_eq!(
        input["edges"]
            .as_array()
            .unwrap()
            .iter()
            .filter(|edge| **edge == json!(["a", "b"]))
            .count(),
        1
    );
    let text = String::from_utf8(handle.worker_input().to_vec()).unwrap();
    for private in [
        "Synthetic",
        "corroboration",
        "originals",
        "sqlite",
        "confidence",
        "obs-r1",
        "r1-parallel",
        &source,
    ] {
        assert!(!text.contains(private));
    }
    let raw = path(&handle, &["a", "b", "c"]);
    let result = w.validate_graph_path(handle, &raw).unwrap();
    let ValidatedPath::Path { nodes, hops } = result.path() else {
        panic!("expected path")
    };
    assert_eq!(nodes, &["a", "b", "c"]);
    assert_eq!(hops[0].assertion_ids, vec!["r1", "r1-parallel"]);
    assert_eq!(hops[1].assertion_ids, vec!["r2"]);
    assert_eq!(
        result.provenance().assertions["r1-parallel"].subject_id,
        "b"
    );
    assert_eq!(
        result.provenance().assertions["r1"].valid_to.as_deref(),
        Some("2001-01-01")
    );
    assert_eq!(
        result.provenance().assertions["r2"].valid_from.as_deref(),
        Some("2025-01-01")
    );
    assert_eq!(
        result.provenance().observations["obs-r1"].extraction_quality,
        Some(0.5)
    );
    assert_eq!(result.provenance().evidence[&source].sha256, source);
    assert_eq!(
        result.provenance().assertion_reviews,
        ReviewCounts {
            accepted: 6,
            pending: 1,
            rejected: 1,
            deferred: 1
        }
    );
    assert!(result.limitation().contains("disjoint historical periods"));
    assert!(result.limitation().contains("Undirected"));
    assert_eq!(result.workspace_revision(), before.0);
    assert_eq!(result.snapshot_sha256().len(), 64);
    assert_eq!(state(&w), before);
    assert!(w.conn.is_autocommit());
}
#[test]
fn unreachable_is_independently_checked_and_undirected_reverse_path_is_allowed() {
    let (_root, w, _) = specimen();
    let handle = w
        .capture_graph_path(w.revision().unwrap(), "a", "f")
        .unwrap();
    let raw = unreachable(&handle);
    assert_eq!(
        w.validate_graph_path(handle, &raw).unwrap().path(),
        &ValidatedPath::Unreachable
    );
    let handle = w
        .capture_graph_path(w.revision().unwrap(), "c", "a")
        .unwrap();
    let raw = path(&handle, &["c", "b", "a"]);
    assert!(w.validate_graph_path(handle, &raw).is_ok());
    let handle = capture(&w);
    let raw = unreachable(&handle);
    assert!(matches!(
        w.validate_graph_path(handle, &raw),
        Err(Error::InvalidWorkerResult(_))
    ));
}
#[test]
fn shortest_path_validator_rejects_fake_nonshortest_cyclic_and_reversed_results() {
    let (_root, w, _) = specimen();
    let before = state(&w);
    for nodes in [
        &["a", "c"][..],
        &["a", "d", "e", "c"],
        &["a", "b", "a", "b", "c"],
        &["c", "b", "a"],
        &["a", "unknown", "c"],
        &["a", "e", "c"],
        &[],
        &["a"],
    ] {
        let handle = capture(&w);
        let raw = path(&handle, nodes);
        assert!(matches!(
            w.validate_graph_path(handle, &raw),
            Err(Error::InvalidWorkerResult(_))
        ));
    }
    assert_eq!(state(&w), before);
}
#[test]
fn result_identity_and_bounded_closed_shape_are_required() {
    let (_root, w, _) = specimen();
    for field in [
        "schema_version",
        "recipe",
        "policy",
        "nonce",
        "workspace_revision",
        "snapshot_sha256",
        "engine",
        "engine_version",
        "runtime_manifest_sha256",
    ] {
        let handle = capture(&w);
        let mut raw = response(&handle, json!({"state":"path","nodes":["a","b","c"]}));
        raw[field] = json!("untrusted replacement");
        assert!(matches!(
            w.validate_graph_path(handle, &serde_json::to_vec(&raw).unwrap()),
            Err(Error::InvalidWorkerResult(_))
        ));
    }
    for raw in [b"{truncated".to_vec(), vec![b' '; MAX_RESULT_BYTES + 1]] {
        assert!(matches!(
            w.validate_graph_path(capture(&w), &raw),
            Err(Error::InvalidWorkerResult(_))
        ));
    }
    let handle = capture(&w);
    let mut value = response(
        &handle,
        json!({"state":"path","nodes":["a","b","c"],"assertion_ids":["forged"]}),
    );
    assert!(w
        .validate_graph_path(handle, &serde_json::to_vec(&value).unwrap())
        .is_err());
    let handle = capture(&w);
    value = response(&handle, json!({"state":"unreachable"}));
    value["snapshot"] = json!({"canonical":true});
    assert!(w
        .validate_graph_path(handle, &serde_json::to_vec(&value).unwrap())
        .is_err());
    let handle = capture(&w);
    let raw = String::from_utf8(path(&handle, &["a", "b", "c"]))
        .unwrap()
        .replace("\"recipe\":", "\"recipe\":\"duplicate\",\"recipe\":");
    assert!(w.validate_graph_path(handle, raw.as_bytes()).is_err());
}
#[test]
fn snapshot_digest_is_not_authority_across_capture_nonce_or_workspace_instance() {
    let (root, w, _) = specimen();
    let first = capture(&w);
    let raw = path(&first, &["a", "b", "c"]);
    let second = capture(&w);
    let a: Value = serde_json::from_slice(first.worker_input()).unwrap();
    let b: Value = serde_json::from_slice(second.worker_input()).unwrap();
    assert_eq!(a["snapshot_sha256"], b["snapshot_sha256"]);
    assert_ne!(a["nonce"], b["nonce"]);
    assert!(w.validate_graph_path(second, &raw).is_err());
    let reopened = Workspace::open(root.path().join("case")).unwrap();
    assert!(matches!(
        reopened.validate_graph_path(first, &raw),
        Err(Error::Conflict(_))
    ));
    let (_other, other, _) = specimen();
    let handle = capture(&w);
    let raw = path(&handle, &["a", "b", "c"]);
    assert!(matches!(
        other.validate_graph_path(handle, &raw),
        Err(Error::Conflict(_))
    ));
}
#[test]
fn stale_revision_or_same_revision_body_tampering_refuses_validation() {
    let (_root, mut w, _) = specimen();
    let handle = capture(&w);
    let raw = path(&handle, &["a", "b", "c"]);
    w.update_entity(
        "a",
        EntityInput {
            name: "Corrected synthetic name".into(),
            kind: EntityKind::Person,
            identifiers: vec![],
        },
        "Synthetic correction",
        w.revision().unwrap(),
    )
    .unwrap();
    assert!(matches!(
        w.validate_graph_path(handle, &raw),
        Err(Error::Conflict(_))
    ));
    assert!(matches!(
        w.capture_graph_path(0, "a", "c"),
        Err(Error::Conflict(_))
    ));
    let handle = capture(&w);
    let raw = path(&handle, &["a", "b", "c"]);
    let mut entity: Entity = get(&w.conn, "entity", "a").unwrap();
    entity.name = "Out of band changed body".into();
    replace(&w, "entity", "a", &entity);
    assert!(matches!(
        w.validate_graph_path(handle, &raw),
        Err(Error::Conflict(_))
    ));
    let handle = capture(&w);
    let raw = path(&handle, &["a", "b", "c"]);
    w.conn
        .execute(
            "UPDATE records SET body=body || ' ' WHERE kind='assertion' AND id='r1'",
            [],
        )
        .unwrap();
    assert!(matches!(
        w.validate_graph_path(handle, &raw),
        Err(Error::Conflict(_))
    ));
}
#[test]
fn accepted_provenance_must_be_complete_accepted_and_supported_without_dropping_edges() {
    for mutation in [
        "missing",
        "pending",
        "unsupported",
        "out-of-range",
        "duplicate",
        "date",
        "merged",
    ] {
        let (_root, w, source) = specimen();
        let mut assertion: Assertion = get(&w.conn, "assertion", "r1").unwrap();
        let mut observation: Observation = get(&w.conn, "observation", "obs-r1").unwrap();
        match mutation {
            "missing" => assertion.observation_ids = vec!["absent".into()],
            "pending" => observation.review = ReviewState::Pending,
            "unsupported" => {
                observation.anchor = SourceAnchor::Page {
                    evidence_id: source,
                    page: 1,
                    region: None,
                }
            }
            "out-of-range" => {
                observation.anchor = SourceAnchor::Text {
                    evidence_id: source,
                    line_start: 200,
                    line_end: 200,
                }
            }
            "duplicate" => assertion.observation_ids.push("obs-r1".into()),
            "date" => assertion.valid_from = Some("2025-02-30".into()),
            "merged" => {
                let mut entity: Entity = get(&w.conn, "entity", "b").unwrap();
                entity.merged_into = Some("a".into());
                replace(&w, "entity", "b", &entity);
            }
            _ => unreachable!(),
        }
        replace(&w, "assertion", "r1", &assertion);
        replace(&w, "observation", "obs-r1", &observation);
        let error = w
            .capture_graph_path(w.revision().unwrap(), "a", "c")
            .err()
            .expect("must refuse complete graph");
        if mutation == "unsupported" {
            assert!(matches!(error, Error::Blocked(_)));
            assert!(error.to_string().contains("text anchors only"));
        }
    }
}
#[test]
fn canonical_keys_and_original_bytes_are_checked_on_capture_and_validation() {
    let (_root, w, source) = specimen();
    let handle = capture(&w);
    let raw = path(&handle, &["a", "b", "c"]);
    let original = w.root.join("originals").join(&source);
    fs::remove_file(&original).unwrap();
    fs::write(&original, b"replaced").unwrap();
    assert!(w.validate_graph_path(handle, &raw).is_err());
    assert!(w
        .capture_graph_path(w.revision().unwrap(), "a", "c")
        .is_err());
    for kind in ["entity", "assertion", "observation", "evidence"] {
        let (_root, w, source) = specimen();
        let key = match kind {
            "entity" => "a",
            "assertion" => "r1",
            "observation" => "obs-r1",
            _ => &source,
        };
        w.conn
            .execute(
                "UPDATE records SET body=json_set(body,'$.id','substituted') WHERE kind=? AND id=?",
                params![kind, key],
            )
            .unwrap();
        assert!(w
            .capture_graph_path(w.revision().unwrap(), "a", "c")
            .is_err());
    }
}
#[test]
fn canonical_counts_and_bytes_are_bounded_before_record_decoding() {
    for kind in ["entity", "assertion"] {
        let (_root, w, _) = specimen();
        let maximum = if kind == "entity" {
            MAX_NODES
        } else {
            MAX_ASSERTIONS
        };
        let tx = w.conn.unchecked_transaction().unwrap();
        for i in 0..=maximum {
            tx.execute(
                "INSERT INTO records(kind,id,body) VALUES(?,?, '{}')",
                params![kind, format!("extra-{i}")],
            )
            .unwrap();
        }
        tx.commit().unwrap();
        let error = w
            .capture_graph_path(w.revision().unwrap(), "a", "c")
            .err()
            .unwrap();
        assert!(error.to_string().contains("row count exceeds bound"));
    }
    let (_root, w, _) = specimen();
    let oversized = json!({"untrusted":"x".repeat(MAX_RECORD_BYTES)});
    replace(&w, "entity", "a", &oversized);
    assert!(w
        .capture_graph_path(w.revision().unwrap(), "a", "c")
        .err()
        .unwrap()
        .to_string()
        .contains("record exceeds byte bound"));
    let (_root, w, _) = specimen();
    let tx = w.conn.unchecked_transaction().unwrap();
    for i in 0..20 {
        let key = format!("large-{i}");
        let entity = Entity {
            id: key.clone(),
            name: "x".repeat(900_000),
            kind: EntityKind::Person,
            identifiers: vec![],
            merged_into: None,
        };
        tx.execute(
            "INSERT INTO records(kind,id,body) VALUES('entity',?,?)",
            params![key, serde_json::to_string(&entity).unwrap()],
        )
        .unwrap();
    }
    tx.commit().unwrap();
    assert!(w
        .capture_graph_path(w.revision().unwrap(), "a", "c")
        .err()
        .unwrap()
        .to_string()
        .contains("aggregate exceeds byte bound"));
}
#[test]
fn bounded_serializer_refuses_escaping_expansion_and_invalid_endpoints_are_rejected() {
    assert!(bounded_json(&vec!["\u{0001}"; 100], 100).is_err());
    let (_root, w, _) = specimen();
    for (a, b) in [("a", "a"), ("absent", "c"), ("a\n", "c"), ("", "c")] {
        assert!(w.capture_graph_path(w.revision().unwrap(), a, b).is_err());
    }
}

#[test]
fn backup_and_restore_preserve_records_but_do_not_transfer_capture_authority() {
    let (root, mut w, _) = specimen();
    let before = state(&w);
    let handle = capture(&w);
    let raw = path(&handle, &["a", "b", "c"]);
    let backup = w.backup().unwrap();
    let restored = Workspace::restore(&backup, &root.path().join("restored")).unwrap();
    assert_eq!(state(&restored), before);
    assert!(matches!(
        restored.validate_graph_path(handle, &raw),
        Err(Error::Conflict(_))
    ));
    let restored_handle = capture(&restored);
    let restored_raw = path(&restored_handle, &["a", "b", "c"]);
    assert!(restored
        .validate_graph_path(restored_handle, &restored_raw)
        .is_ok());
    assert_eq!(state(&w), before);
}

#[test]
fn linked_original_cannot_substitute_for_checked_original_even_with_matching_bytes() {
    let (root, w, source) = specimen();
    let handle = capture(&w);
    let raw = path(&handle, &["a", "b", "c"]);
    let original = w.root.join("originals").join(&source);
    let alias = root.path().join("outside-original-alias");
    fs::hard_link(&original, &alias).unwrap();
    assert!(w.validate_graph_path(handle, &raw).is_err());
    assert!(w
        .capture_graph_path(w.revision().unwrap(), "a", "c")
        .is_err());
    fs::remove_file(&alias).unwrap();
    let handle = capture(&w);
    assert!(
        w.validate_graph_path(handle, &raw).is_err(),
        "old nonce cannot be replayed after restoring integrity"
    );
    let handle = capture(&w);
    let raw = path(&handle, &["a", "b", "c"]);
    assert!(w.validate_graph_path(handle, &raw).is_ok());
}

#[test]
fn nested_result_unknown_duplicate_and_trailing_fields_fail_closed() {
    let (_root, w, _) = specimen();
    for mode in [
        "unknown",
        "duplicate-nodes",
        "duplicate-state",
        "trailing",
        "unreachable-extra",
    ] {
        let handle = capture(&w);
        let good = String::from_utf8(path(&handle, &["a", "b", "c"])).unwrap();
        let raw = match mode {
            "unknown" => good.replace("\"nodes\":", "\"evidence\":[],\"nodes\":"),
            "duplicate-nodes" => good.replace("\"nodes\":", "\"nodes\":[],\"nodes\":"),
            "duplicate-state" => good.replace("\"state\":", "\"state\":\"unreachable\",\"state\":"),
            "trailing" => format!("{good}{{}}"),
            _ => serde_json::to_string(&response(
                &handle,
                json!({"state":"unreachable","nodes":[]}),
            ))
            .unwrap(),
        };
        assert!(
            matches!(
                w.validate_graph_path(handle, raw.as_bytes()),
                Err(Error::InvalidWorkerResult(_))
            ),
            "{mode}"
        );
    }
}

#[test]
fn valid_unreachable_baseline_cannot_hide_ignored_extra_or_duplicate_fields() {
    let (_root, w, _) = specimen();
    let disconnected = || {
        w.capture_graph_path(w.revision().unwrap(), "a", "f")
            .unwrap()
    };
    let handle = disconnected();
    let raw = unreachable(&handle);
    assert_eq!(
        w.validate_graph_path(handle, &raw).unwrap().path(),
        &ValidatedPath::Unreachable
    );
    for extra in [
        "\"nodes\":[\"forged\"]",
        "\"assertion_ids\":[\"forged\"]",
        "\"extra\":false,\"extra\":true",
        "\"state\":\"unreachable\"",
    ] {
        let handle = disconnected();
        let raw = String::from_utf8(unreachable(&handle)).unwrap().replace(
            "\"state\":\"unreachable\"",
            &format!("\"state\":\"unreachable\",{extra}"),
        );
        assert!(matches!(
            w.validate_graph_path(handle, raw.as_bytes()),
            Err(Error::InvalidWorkerResult(_))
        ));
    }
}

#[test]
fn concurrent_canonical_correction_cannot_mix_pinned_graph_or_provenance() {
    use rusqlite::hooks::{AuthAction, AuthContext, Authorization};
    use std::sync::{Arc, Mutex};
    for phase in ["capture", "validation"] {
        let (_root, w, _) = specimen();
        let revision = w.revision().unwrap();
        let original = capture(&w);
        let digest = original.snapshot_sha256.clone();
        let writer = Workspace::open(&w.root).unwrap();
        w.conn.pragma_update(None, "journal_mode", "WAL").unwrap();
        let state = Arc::new(Mutex::new(Some(writer)));
        let callback = Arc::clone(&state);
        // Fixed statement boundary, after revision SELECT pins the read snapshot.
        // This is a real canonical writer on a second connection, without sleeps.
        w.conn.authorizer(Some(move |context: AuthContext<'_>| {
            if matches!(
                context.action,
                AuthAction::Read {
                    table_name: "records",
                    ..
                }
            ) {
                if let Some(mut writer) = callback.lock().unwrap().take() {
                    writer
                        .review_observation(
                            "obs-r2",
                            ReviewState::Deferred,
                            "Synthetic concurrent source review",
                            revision,
                        )
                        .unwrap();
                }
            }
            Authorization::Allow
        }));
        if phase == "capture" {
            let pinned = capture(&w);
            assert_eq!(pinned.snapshot_sha256, digest);
            assert_eq!(
                pinned.selection.observations["obs-r2"].review,
                ReviewState::Accepted
            );
            assert_eq!(
                pinned.selection.assertions["r2"].review,
                ReviewState::Accepted
            );
            w.conn
                .authorizer(None::<fn(AuthContext<'_>) -> Authorization>);
            let raw = path(&pinned, &["a", "b", "c"]);
            assert!(matches!(
                w.validate_graph_path(pinned, &raw),
                Err(Error::Conflict(_))
            ));
        } else {
            let raw = path(&original, &["a", "b", "c"]);
            let pinned = w.validate_graph_path(original, &raw).unwrap();
            w.conn
                .authorizer(None::<fn(AuthContext<'_>) -> Authorization>);
            // A read-only interpretation labels the pinned historical revision;
            // it grants no authority for publication after the writer committed.
            assert_eq!(pinned.workspace_revision(), revision);
            assert_eq!(pinned.snapshot_sha256(), digest);
            assert_eq!(
                pinned.provenance().observations["obs-r2"].review,
                ReviewState::Accepted
            );
            assert_eq!(
                pinned.provenance().assertions["r2"].review,
                ReviewState::Accepted
            );
        }
        assert!(
            state.lock().unwrap().is_none(),
            "concurrent writer did not commit"
        );
        assert_eq!(w.revision().unwrap(), revision + 1);
        assert!(matches!(
            w.capture_graph_path(revision, "a", "c"),
            Err(Error::Conflict(_))
        ));
        let current = capture(&w);
        assert_ne!(current.snapshot_sha256, digest);
        assert!(!current.selection.assertions.contains_key("r2"));
        assert!(!current.selection.observations.contains_key("obs-r2"));
        assert_eq!(shortest_distance(&current.selection, "a", "c"), Some(3));
        let raw = path(&current, &["a", "d", "e", "c"]);
        assert!(w.validate_graph_path(current, &raw).is_ok());
    }
}
