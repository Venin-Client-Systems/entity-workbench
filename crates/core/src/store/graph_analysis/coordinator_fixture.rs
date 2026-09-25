//! Closed synthetic fixtures and complete-table checks for the ignored native campaign.
use super::*;
use std::collections::BTreeMap;

pub(crate) fn chain_id(index: usize) -> String {
    format!("00000000-0000-4000-8000-{index:012}")
}
pub(crate) fn seed_chain(path: &Path) -> Result<(Workspace, String)> {
    let mut w = Workspace::open(path)?;
    let source = w.import(
        "synthetic-chain.txt",
        b"Synthetic anchored chain; every edge is fictional.\n",
    )?;
    w.change(None, "synthetic.graph_chain", false, |conn| {
        for n in 0..700 {
            let key = chain_id(n);
            put(
                conn,
                "entity",
                &key,
                &Entity {
                    id: key.clone(),
                    name: format!("Synthetic node {n}"),
                    kind: EntityKind::Person,
                    identifiers: vec![],
                    merged_into: None,
                },
            )?;
            if n > 0 {
                let observation = Observation {
                    id: format!("chain-observation-{n}"),
                    entity_id: chain_id(n - 1),
                    field: "relationship mention".into(),
                    value: "Synthetic chain evidence".into(),
                    anchor: SourceAnchor::Text {
                        evidence_id: source.clone(),
                        line_start: 1,
                        line_end: 1,
                    },
                    extraction_quality: Some(0.5),
                    review: ReviewState::Accepted,
                };
                put(conn, "observation", &observation.id, &observation)?;
                let assertion = Assertion {
                    id: format!("chain-assertion-{n}"),
                    subject_id: chain_id(n - 1),
                    predicate: "recorded alongside".into(),
                    object_id: key,
                    observation_ids: vec![observation.id],
                    valid_from: None,
                    valid_to: None,
                    confidence: "Synthetic analyst assessment".into(),
                    review: ReviewState::Accepted,
                };
                put(conn, "assertion", &assertion.id, &assertion)?;
            }
        }
        Ok(())
    })?;
    Ok((w, source))
}

#[derive(Clone, serde::Serialize)]
pub(crate) struct Snapshot {
    pub(crate) tables: BTreeMap<String, Vec<Vec<Value>>>,
    pub(crate) originals: BTreeMap<String, (u64, String)>,
}
impl Snapshot {
    pub(crate) fn identity(&self) -> Result<Value> {
        let bytes = serde_json::to_vec(self)?;
        Ok(json!({"bytes":bytes.len(),"sha256":hash(&bytes)}))
    }
}
pub(crate) fn snapshot(w: &Workspace) -> Result<Snapshot> {
    let tx = w.conn.unchecked_transaction()?;
    let names = tx
        .prepare("SELECT name FROM sqlite_schema WHERE type='table' ORDER BY name")?
        .query_map([], |row| row.get::<_, String>(0))?
        .collect::<rusqlite::Result<Vec<_>>>()?;
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
        "Unexpected native proof tables",
    )?;
    let mut tables = BTreeMap::new();
    for (name, sql) in [
        (
            "schema",
            "SELECT type,name,tbl_name,sql FROM sqlite_schema ORDER BY type,name",
        ),
        ("version", "PRAGMA user_version"),
        ("meta", "SELECT rowid,revision FROM meta ORDER BY rowid"),
        (
            "records",
            "SELECT sequence,kind,id,body FROM records ORDER BY sequence",
        ),
        (
            "history",
            "SELECT sequence,kind,id,body,revision FROM history ORDER BY sequence",
        ),
        (
            "events",
            "SELECT sequence,revision,action,at FROM events ORDER BY sequence",
        ),
        (
            "derivative_objects",
            "SELECT sha256,bytes FROM derivative_objects ORDER BY sha256",
        ),
        (
            "sqlite_sequence",
            "SELECT name,seq FROM sqlite_sequence ORDER BY name",
        ),
    ] {
        let mut query = tx.prepare(sql)?;
        let count = query.column_count();
        let rows = query
            .query_map([], |row| {
                (0..count)
                    .map(|n| {
                        Ok(match row.get_ref(n)? {
                            rusqlite::types::ValueRef::Null => Value::Null,
                            rusqlite::types::ValueRef::Integer(n) => json!(n),
                            rusqlite::types::ValueRef::Text(t) => json!(std::str::from_utf8(t)
                                .map_err(|_| rusqlite::Error::InvalidQuery)?),
                            _ => return Err(rusqlite::Error::InvalidQuery),
                        })
                    })
                    .collect::<rusqlite::Result<Vec<_>>>()
            })?
            .collect::<rusqlite::Result<Vec<_>>>()?;
        tables.insert(name.into(), rows);
    }
    let mut originals = BTreeMap::new();
    for entry in fs::read_dir(w.root.join("originals"))? {
        let entry = entry?;
        let metadata = entry.file_type()?;
        require(metadata.is_file(), "Unexpected synthetic original entry")?;
        require(
            entry.metadata()?.len() <= 64 * 1024,
            "Unexpected synthetic original size",
        )?;
        let bytes = fs::read(entry.path())?;
        require(
            bytes.len() <= 64 * 1024,
            "Unexpected synthetic original size",
        )?;
        originals.insert(
            entry
                .file_name()
                .into_string()
                .map_err(|_| Error::Validation("Synthetic original name".into()))?,
            (bytes.len() as u64, hash(&bytes)),
        );
    }
    tx.commit()?;
    Ok(Snapshot { tables, originals })
}

/// Compare every table, allowing only named job bodies and one completed graph record.
/// Original source bytes and every pre-existing history/event row remain exact.
pub(crate) fn verify_delta(
    before: &Snapshot,
    after: &Snapshot,
    mutable: &[(String, String)],
    result: Option<&str>,
) -> Result<Value> {
    require(
        before.originals == after.originals,
        "Native proof originals changed",
    )?;
    for name in ["schema", "version", "derivative_objects"] {
        require(
            before.tables[name] == after.tables[name],
            "Native proof static table changed",
        )?;
    }
    let allowed = |row: &[Value]| {
        mutable
            .iter()
            .any(|(kind, id)| row[1] == *kind && row[2] == *id)
    };
    let old = &before.tables["records"];
    let new = &after.tables["records"];
    require(
        new.len() == old.len() + usize::from(result.is_some()),
        "Native proof unexpected record count",
    )?;
    for prior in old {
        let current = new
            .iter()
            .find(|r| r[0] == prior[0])
            .ok_or_else(|| Error::Validation("Native proof deleted record".into()))?;
        require(
            current == prior || (current[..3] == prior[..3] && allowed(current)),
            "Native proof unrelated canonical mutation",
        )?;
    }
    let inserted: Vec<_> = new
        .iter()
        .filter(|r| !old.iter().any(|p| p[0] == r[0]))
        .collect();
    require(
        inserted
            .iter()
            .all(|r| r[1] == "graph_analysis" && r[2].as_str() == result),
        "Native proof unrelated inserted record",
    )?;
    for name in ["history", "events"] {
        let old = &before.tables[name];
        let new = &after.tables[name];
        require(
            new.starts_with(old),
            "Native proof existing audit history changed",
        )?;
        for row in &new[old.len()..] {
            require(
                if name == "history" {
                    allowed(row)
                        && row[4].as_u64().is_some_and(|r| {
                            r > before.tables["meta"][0][1].as_u64().unwrap()
                                && r <= after.tables["meta"][0][1].as_u64().unwrap()
                        })
                } else {
                    matches!(
                        row[2].as_str(),
                        Some(
                            "processing.graph_claim"
                                | "processing.graph_finish"
                                | "processing.cancel"
                                | "processing.claim"
                                | "processing.finish"
                                | "collection.durable.checkpoint"
                        )
                    )
                },
                "Native proof unexpected audit mutation",
            )?;
        }
    }
    let old_revision = before.tables["meta"][0][1].as_u64().unwrap();
    let revision = after.tables["meta"][0][1].as_u64().unwrap();
    let events = &after.tables["events"][before.tables["events"].len()..];
    require(
        revision == old_revision + events.len() as u64,
        "Native proof revision/event mismatch",
    )?;
    for (n, event) in events.iter().enumerate() {
        require(
            event[1] == old_revision + n as u64 + 1,
            "Native proof noncontiguous revisions",
        )?;
    }
    require(
        after.tables["meta"] == vec![vec![json!(1), json!(revision)]],
        "Native proof unexpected metadata",
    )?;
    let sequences = &after.tables["sqlite_sequence"];
    for row in sequences {
        let table = row[0]
            .as_str()
            .ok_or_else(|| Error::Validation("Native proof sequence name".into()))?;
        require(
            matches!(table, "records" | "history" | "events"),
            "Native proof unknown sequence",
        )?;
        let previous = before.tables["sqlite_sequence"]
            .iter()
            .find(|r| r[0] == table)
            .map_or(0, |r| r[1].as_u64().unwrap());
        let added = after.tables[table].len() - before.tables[table].len();
        let updates = if table == "records" {
            after.tables["history"].len() - before.tables["history"].len()
        } else {
            0
        };
        require(
            row[1] == previous + (added + updates) as u64,
            "Native proof sequence does not match exact writes",
        )?;
    }
    require(
        before.tables["sqlite_sequence"]
            .iter()
            .all(|old| sequences.iter().any(|row| row[0] == old[0])),
        "Native proof sequence removed",
    )?;
    Ok(
        json!({"before":before.identity()?,"after":after.identity()?,"originals":after.originals,"expected_mutable_records":mutable,"inserted_graph_record":result,"revision_before":old_revision,"revision_after":revision,"new_events":events,"all_tables_checked":true}),
    )
}
#[test]
fn genuine_chain_exceeds_legacy_bound_without_padding() {
    let root = tempfile::tempdir_in(std::env::temp_dir().canonicalize().unwrap()).unwrap();
    let (w, _) = seed_chain(&root.path().join("case")).unwrap();
    let capture = w
        .capture_graph_path(w.revision().unwrap(), &chain_id(0), &chain_id(699))
        .unwrap();
    let raw = capture.worker_input();
    assert!(raw.len() > 64 * 1024 && raw.len() <= 1024 * 1024);
    let v: Value = serde_json::from_slice(raw).unwrap();
    assert_eq!(v["nodes"].as_array().unwrap().len(), 700);
    assert_eq!(v["edges"].as_array().unwrap().len(), 699);
}
#[test]
fn full_table_delta_refuses_unrelated_changes_and_history_rewrites() {
    let (_root, w, _) = super::probe_fixture::specimen();
    let baseline = snapshot(&w).unwrap();
    verify_delta(&baseline, &baseline, &[], None).unwrap();
    for name in [
        "schema",
        "version",
        "meta",
        "derivative_objects",
        "sqlite_sequence",
        "history",
        "events",
        "records",
    ] {
        let mut altered = baseline.clone();
        altered.tables.get_mut(name).unwrap().push(vec![
            json!(999),
            json!("entity"),
            json!("intruder"),
            json!("{}"),
        ]);
        assert!(
            verify_delta(&baseline, &altered, &[], None).is_err(),
            "{name}"
        );
    }
    let mut altered = baseline.clone();
    altered.originals.clear();
    assert!(verify_delta(&baseline, &altered, &[], None).is_err());
}

#[test]
fn full_table_delta_accepts_only_the_real_graph_claim_and_publication_set() {
    let (_root, mut w, _) = super::probe_fixture::specimen();
    let q = w
        .queue_graph_path(
            w.revision().unwrap(),
            "a",
            "c",
            &uuid::Uuid::new_v4().to_string(),
        )
        .unwrap();
    let before = snapshot(&w).unwrap();
    let (ticket, mut attempt) = w.claim_graph_exclusive(&q).unwrap();
    let mut response: Value = serde_json::from_slice(attempt.worker_input()).unwrap();
    for key in ["nodes", "edges", "source_id", "target_id"] {
        response.as_object_mut().unwrap().remove(key);
    }
    response["outcome"] = json!({"state":"path","nodes":["a","b","c"]});
    let output = crate::processing::ProcessingOutput::Graph(serde_json::to_vec(&response).unwrap());
    let done = w
        .finish_graph_processing_job(&ticket, &mut attempt, Ok(&output))
        .unwrap();
    let after = snapshot(&w).unwrap();
    verify_delta(
        &before,
        &after,
        &[("processing_job".into(), q.id)],
        Some(&done.result_ids[0]),
    )
    .unwrap();
    assert!(verify_delta(&before, &after, &[], Some(&done.result_ids[0])).is_err());
}
