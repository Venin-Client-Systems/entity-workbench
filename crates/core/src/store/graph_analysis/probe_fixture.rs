//! Test-only canonical fixture and owned capture. No application entry point.
use super::*;
use tempfile::TempDir;

pub(crate) fn specimen() -> (TempDir, Workspace, String) {
    let root = tempfile::tempdir_in(std::env::temp_dir().canonicalize().unwrap()).unwrap();
    let (workspace, source) = seed_case(&root.path().join("case")).unwrap();
    (root, workspace, source)
}

pub(crate) fn seed_case(path: &Path) -> Result<(Workspace, String)> {
    let mut w = Workspace::open(path)?;
    let source = w.import(
        "synthetic.txt",
        b"Synthetic retained relationship source\nSecond source line\n",
    )?;
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
    })?;
    Ok((w, source))
}

pub(crate) fn canonical_state(workspace: &Workspace) -> Result<String> {
    // Fixed synthetic schema only. No caller SQL or table names; reject a new table
    // rather than silently omit its contents from the unchanged-state claim.
    let mut tables = workspace
        .conn
        .prepare("SELECT name FROM sqlite_schema WHERE type='table' ORDER BY name")?;
    let names = tables
        .query_map([], |row| row.get::<_, String>(0))?
        .collect::<std::result::Result<Vec<_>, _>>()?;
    require(
        names
            == [
                "derivative_objects",
                "events",
                "history",
                "meta",
                "records",
                "sqlite_sequence",
            ],
        "Unexpected canonical probe schema",
    )?;
    let mut contents = Vec::new();
    for query in [
        "PRAGMA user_version",
        "SELECT type,name,tbl_name,sql FROM sqlite_schema ORDER BY type,name",
        "SELECT rowid,revision FROM meta ORDER BY rowid",
        "SELECT sequence,kind,id,body FROM records ORDER BY sequence",
        "SELECT sequence,kind,id,body,revision FROM history ORDER BY sequence",
        "SELECT sequence,revision,action,at FROM events ORDER BY sequence",
        "SELECT sha256,bytes FROM derivative_objects ORDER BY sha256",
        "SELECT name,seq FROM sqlite_sequence ORDER BY name",
    ] {
        let mut statement = workspace.conn.prepare(query)?;
        let columns = statement.column_count();
        let rows = statement
            .query_map([], |row| {
                (0..columns)
                    .map(|column| {
                        Ok(match row.get_ref(column)? {
                            rusqlite::types::ValueRef::Null => json!(["null"]),
                            rusqlite::types::ValueRef::Integer(value) => json!(["integer", value]),
                            rusqlite::types::ValueRef::Real(value) => {
                                json!(["real", value.to_bits()])
                            }
                            rusqlite::types::ValueRef::Text(value) => json!(["text", value]),
                            rusqlite::types::ValueRef::Blob(value) => json!(["blob", value]),
                        })
                    })
                    .collect::<rusqlite::Result<Vec<_>>>()
            })?
            .collect::<rusqlite::Result<Vec<_>>>()?;
        contents.push(rows);
    }
    Ok(hash(&serde_json::to_vec(&contents)?))
}

pub(crate) struct CanonicalProbe {
    workspace: Workspace,
    captured: Option<CapturedGraph>,
    request: Vec<u8>,
    before: String,
}
impl CanonicalProbe {
    pub(crate) fn new(artifacts: &Path) -> Result<Self> {
        // Fresh fixed name, retained for diagnosis on all outcomes. Never adopt a supplied case.
        let root = artifacts.join("canonical-workspace");
        fs::create_dir(&root)?;
        let (workspace, _) = seed_case(&root)?;
        let captured = workspace.capture_graph_path(workspace.revision()?, "a", "c")?;
        let request = captured.worker_input().to_vec();
        require(
            request.len() <= 64 * 1024,
            "Fixed graph probe input exceeds bound",
        )?;
        let before = canonical_state(&workspace)?;
        Ok(Self {
            workspace,
            captured: Some(captured),
            request,
            before,
        })
    }
    pub(crate) fn request(&self) -> &[u8] {
        &self.request
    }
    pub(crate) fn metadata(&self) -> Result<Value> {
        let request: Value = serde_json::from_slice(&self.request)?;
        Ok(
            json!({"nonce":request["nonce"],"workspace_revision":request["workspace_revision"],
            "snapshot_sha256":request["snapshot_sha256"],"request_identity":{"bytes":self.request.len(),"sha256":hash(&self.request)},
            "source_id":"a","target_id":"c","canonical_state_sha256":self.before}),
        )
    }
    pub(crate) fn accept(&mut self, bytes: &[u8]) -> Result<Value> {
        let captured = self
            .captured
            .take()
            .ok_or_else(|| Error::Conflict("Graph probe capture already consumed".into()))?;
        let validated = self.workspace.validate_graph_path(captured, bytes)?;
        let ValidatedPath::Path { nodes, hops } = validated.path() else {
            return Err(Error::InvalidWorkerResult(
                "Fixed graph path is missing".into(),
            ));
        };
        require(
            nodes == &["a", "b", "c"]
                && hops.len() == 2
                && hops[0].assertion_ids == ["r1", "r1-parallel"]
                && hops[1].assertion_ids == ["r2"],
            "Fixed canonical graph assertions differ",
        )?;
        require(
            canonical_state(&self.workspace)? == self.before,
            "Canonical graph workspace changed",
        )?;
        Ok(
            json!({"validated":true,"canonical_unchanged":true,"workspace_revision":validated.workspace_revision(),
            "snapshot_sha256":validated.snapshot_sha256(),"nodes":nodes,
            "hop_assertion_ids":hops.iter().map(|hop| &hop.assertion_ids).collect::<Vec<_>>(),
            "limitation":validated.limitation()}),
        )
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    fn response(probe: &CanonicalProbe) -> Vec<u8> {
        let mut value: Value = serde_json::from_slice(probe.request()).unwrap();
        for field in ["source_id", "target_id", "nodes", "edges"] {
            value.as_object_mut().unwrap().remove(field);
        }
        value["outcome"] = json!({"state":"path","nodes":["a","b","c"]});
        serde_json::to_vec(&value).unwrap()
    }
    fn root() -> TempDir {
        tempfile::tempdir_in(std::env::temp_dir().canonicalize().unwrap()).unwrap()
    }
    #[test]
    fn fixed_probe_is_fresh_bound_read_only_and_preserves_parallel_provenance() {
        let temp = root();
        let mut probe = CanonicalProbe::new(temp.path()).unwrap();
        assert!(CanonicalProbe::new(temp.path()).is_err());
        let metadata = probe.metadata().unwrap();
        assert_eq!(metadata["workspace_revision"], 2);
        assert_eq!(
            metadata["request_identity"]["sha256"],
            hash(probe.request())
        );
        let validated = probe.accept(&response(&probe)).unwrap();
        assert_eq!(validated["canonical_unchanged"], true);
        assert_eq!(
            validated["hop_assertion_ids"],
            json!([["r1", "r1-parallel"], ["r2"]])
        );
        assert!(probe.accept(&response(&probe)).is_err());
    }
    #[test]
    fn stale_cross_workspace_and_substituted_results_cannot_consume_another_capture() {
        let first = root();
        let second = root();
        let mut a = CanonicalProbe::new(first.path()).unwrap();
        let b = CanonicalProbe::new(second.path()).unwrap();
        assert!(a.accept(&response(&b)).is_err());
        assert!(a.accept(&response(&a)).is_err()); // Failure consumes authority too.
        let third = root();
        let mut stale = CanonicalProbe::new(third.path()).unwrap();
        let raw = response(&stale);
        stale
            .workspace
            .import("later.txt", b"Synthetic correction")
            .unwrap();
        assert!(stale.accept(&raw).is_err());
        let fourth = root();
        let mut malformed = CanonicalProbe::new(fourth.path()).unwrap();
        assert!(malformed.accept(&vec![b' '; MAX_RESULT_BYTES + 1]).is_err());
    }
}

#[test]
fn canonical_probe_state_detects_same_count_history_event_and_metadata_edits() {
    let (_temp, workspace, _) = specimen();
    workspace
        .conn
        .execute(
            "INSERT INTO history(kind,id,body,revision) VALUES('entity','synthetic','{}',0)",
            [],
        )
        .unwrap();
    let before = canonical_state(&workspace).unwrap();
    workspace
        .conn
        .execute(
            "UPDATE history SET body='{\"changed\":true}' WHERE id='synthetic'",
            [],
        )
        .unwrap();
    let history_changed = canonical_state(&workspace).unwrap();
    assert_ne!(before, history_changed);
    workspace.conn.execute("UPDATE events SET action='synthetic.changed' WHERE sequence=(SELECT min(sequence) FROM events)", []).unwrap();
    let event_changed = canonical_state(&workspace).unwrap();
    assert_ne!(history_changed, event_changed);
    workspace
        .conn
        .execute(
            "UPDATE sqlite_sequence SET seq=seq+1 WHERE name='history'",
            [],
        )
        .unwrap();
    assert_ne!(event_changed, canonical_state(&workspace).unwrap());
    workspace
        .conn
        .execute("CREATE TABLE untracked(value TEXT)", [])
        .unwrap();
    assert!(canonical_state(&workspace).is_err());
}
