use super::*;
use crate::engines::{search_lifecycle::Completion, Runtime, SearchResults};

fn fixture() -> (tempfile::TempDir, Workspace, String) {
    let root = tempfile::tempdir_in(std::env::temp_dir().canonicalize().unwrap()).unwrap();
    let mut w = Workspace::open(root.path()).unwrap();
    let key = w.import("synthetic.txt", b"Exact synthetic text").unwrap();
    w.attach_runtime(Runtime {
        root: root.path().join("not-executed"),
    });
    (root, w, key)
}
fn state(w: &Workspace) -> String {
    // Stream every typed canonical cell: oversized rejection fixtures must not allocate
    // a second JSON representation (or a JSON integer array for each byte) of the database.
    let mut digest = Sha256::new();
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
        digest.update(query.as_bytes());
        let mut statement = w.conn.prepare(query).unwrap();
        let columns = statement.column_count();
        let mut rows = statement.query([]).unwrap();
        while let Some(row) = rows.next().unwrap() {
            digest.update(b"row");
            for column in 0..columns {
                match row.get_ref(column).unwrap() {
                    ValueRef::Null => digest.update(b"null"),
                    ValueRef::Integer(value) => {
                        digest.update(b"int");
                        digest.update(value.to_le_bytes());
                    }
                    ValueRef::Real(value) => {
                        digest.update(b"real");
                        digest.update(value.to_bits().to_le_bytes());
                    }
                    ValueRef::Text(value) | ValueRef::Blob(value) => {
                        digest.update(
                            if matches!(row.get_ref(column).unwrap(), ValueRef::Text(_)) {
                                b"text"
                            } else {
                                b"blob"
                            },
                        );
                        digest.update((value.len() as u64).to_le_bytes());
                        digest.update(value);
                    }
                }
            }
        }
    }
    format!("{:x}", digest.finalize())
}
fn refused_unchanged(w: &mut Workspace, key: &str, expected: &str) {
    let before = state(w);
    let original = fs::read(w.root.join("originals").join(key)).unwrap();
    let owner = w.collection_ownership().unwrap();
    let error = w
        .search_owned_with(&owner, "needle", |_, _, _, _, _| {
            panic!("executor must not run")
        })
        .expect_err("admission must fail");
    assert!(
        error.to_string().contains(expected),
        "unexpected error: {error}"
    );
    assert_eq!(state(w), before);
    assert_eq!(
        fs::read(w.root.join("originals").join(key)).unwrap(),
        original
    );
    assert!(!w.root.join("indexes").exists());
    assert!(
        owner.held(),
        "capture refusal is not unverified worker termination"
    );
    owner.release().unwrap();
}

#[test]
fn search_capture_real_second_connection_cannot_mix_revision_and_corpus() {
    use rusqlite::hooks::{AuthAction, AuthContext, Authorization};
    use std::sync::{Arc, Mutex};
    let (_root, w, key) = fixture();
    let expected = w.capture_search().unwrap();
    let writer = Workspace::open(&w.root).unwrap();
    w.conn.pragma_update(None, "journal_mode", "WAL").unwrap();
    let shared = Arc::new(Mutex::new(Some(writer)));
    let callback = shared.clone();
    // The evidence statement is prepared only after revision's read pins the snapshot.
    w.conn.authorizer(Some(move |ctx: AuthContext<'_>| {
        if matches!(
            ctx.action,
            AuthAction::Read {
                table_name: "records",
                ..
            }
        ) {
            if let Some(mut writer) = callback.lock().unwrap().take() {
                writer
                    .import("concurrent.txt", b"Independent synthetic second original")
                    .unwrap();
            }
        }
        Authorization::Allow
    }));
    let observed = w.capture_search().unwrap();
    w.conn
        .authorizer(None::<fn(AuthContext<'_>) -> Authorization>);
    assert!(
        shared.lock().unwrap().is_none(),
        "second canonical writer did not execute"
    );
    assert_eq!(observed.revision(), expected.revision());
    assert_eq!(observed.manifest(), expected.manifest());
    assert_eq!(observed.document_count(), 1);
    assert!(observed.knows(&key));
    let next = w.capture_search().unwrap();
    assert_eq!(next.revision(), expected.revision() + 1);
    assert_eq!(next.document_count(), 2);
}

#[test]
fn search_capture_malformed_borrowed_metadata_and_identity_refuse_before_execution() {
    for fault in [
        "blob",
        "utf8",
        "json",
        "body_id",
        "digest",
        "bytes",
        "acquisition",
    ] {
        let (_root, mut w, key) = fixture();
        // Deliberately corrupt fixture bypasses the schema CHECK; normal canonical writers cannot.
        w.conn
            .pragma_update(None, "ignore_check_constraints", true)
            .unwrap();
        let mut body: Value = serde_json::from_str(
            &w.conn
                .query_row(
                    "SELECT body FROM records WHERE kind='evidence' AND id=?",
                    [&key],
                    |r| r.get::<_, String>(0),
                )
                .unwrap(),
        )
        .unwrap();
        let expected = match fault {
            "blob" => {
                w.conn
                    .execute("UPDATE records SET body=x'7b7d' WHERE kind='evidence'", [])
                    .unwrap();
                "must be SQLite text"
            }
            "utf8" => {
                w.conn
                    .execute(
                        "UPDATE records SET body=CAST(x'ff' AS TEXT) WHERE kind='evidence'",
                        [],
                    )
                    .unwrap();
                "not UTF-8"
            }
            "json" => {
                w.conn
                    .execute("UPDATE records SET body='{' WHERE kind='evidence'", [])
                    .unwrap();
                "EOF"
            }
            _ => {
                let expected = match fault {
                    "body_id" => {
                        body["id"] = json!("0".repeat(64));
                        "lookup key"
                    }
                    "digest" => {
                        body["sha256"] = json!("0".repeat(64));
                        "original digest"
                    }
                    "bytes" => {
                        body["bytes"] = json!(policy::MAX_IMPORT_BYTES + 1);
                        "size bound"
                    }
                    "acquisition" => {
                        body["acquisitions"] = json!([{"job_id":5}]);
                        "invalid type"
                    }
                    _ => unreachable!(),
                };
                w.conn
                    .execute(
                        "UPDATE records SET body=? WHERE kind='evidence'",
                        [body.to_string()],
                    )
                    .unwrap();
                expected
            }
        };
        w.conn
            .pragma_update(None, "ignore_check_constraints", false)
            .unwrap();
        refused_unchanged(&mut w, &key, expected);
    }
}

#[test]
fn search_capture_raw_row_and_aggregate_limits_precede_typed_decode() {
    let (_root, mut w, key) = fixture();
    w.conn
        .pragma_update(None, "ignore_check_constraints", true)
        .unwrap();
    // Not valid JSON: the expected size error proves admission precedes deserialization.
    w.conn
        .execute(
            "UPDATE records SET body=? WHERE kind='evidence'",
            ["x".repeat(MAX_ROW_BYTES + 1)],
        )
        .unwrap();
    w.conn
        .pragma_update(None, "ignore_check_constraints", false)
        .unwrap();
    refused_unchanged(&mut w, &key, "metadata row limit");

    let (_root, mut w, key) = fixture();
    let mut body = serde_json::to_value(get_evidence(&w.conn, &key).unwrap()).unwrap();
    body["text"] = Value::Null;
    body["origin_group"] = json!("x".repeat(16 * 1024 * 1024));
    for number in 0..4 {
        let id = format!("{number:064x}");
        body["id"] = json!(id);
        body["sha256"] = json!(id);
        put(&w.conn, "evidence", &id, &body).unwrap();
    }
    refused_unchanged(&mut w, &key, "aggregate metadata limit");
}

#[test]
fn search_capture_manifest_escaping_limit_refuses_without_cache_or_canonical_changes() {
    let (_root, mut w, key) = fixture();
    // The raw row is admitted, but text plus the manifest envelope exceeds the exact cap.
    let mut evidence = get_evidence(&w.conn, &key).unwrap();
    evidence.text = Some("x".repeat(16 * 1024 * 1024));
    put(&w.conn, "evidence", &key, &evidence).unwrap();
    refused_unchanged(&mut w, &key, "Index manifest exceeds");
}

#[test]
fn search_capture_actual_document_and_row_ceilings_refuse_before_execution() {
    for (rows, text, expected) in [
        (
            crate::engines::search_corpus::MAX_DOCUMENTS,
            json!(""),
            "indexed document limit",
        ),
        (MAX_EVIDENCE_ROWS, Value::Null, "evidence row limit"),
    ] {
        let (_root, mut w, key) = fixture();
        let mut body = serde_json::to_value(get_evidence(&w.conn, &key).unwrap()).unwrap();
        body["text"] = text;
        // Fixed synthetic rows, no original verification claim. First fill the exact
        // ceiling including the ordinary original, then add one more canonical-shaped row.
        w.conn.execute(
            "WITH RECURSIVE fixture(n) AS (SELECT 0 UNION ALL SELECT n+1 FROM fixture WHERE n+1 < ?1)
             INSERT INTO records(kind,id,body)
             SELECT 'evidence',printf('%064x',n),json_set(?2,'$.id',printf('%064x',n),'$.sha256',printf('%064x',n)) FROM fixture",
            params![rows - 1, body.to_string()],
        ).unwrap();
        let exact = w.capture_search().unwrap();
        assert_eq!(
            exact.document_count(),
            if expected == "indexed document limit" {
                rows as u64
            } else {
                1
            }
        );
        drop(exact);
        let extra = format!("{:064x}", rows - 1);
        body["id"] = json!(extra);
        body["sha256"] = json!(extra);
        put(&w.conn, "evidence", &extra, &body).unwrap();
        refused_unchanged(&mut w, &key, expected);
    }
}

#[test]
fn search_capture_refusal_preserves_existing_cache_and_invalid_query_is_early() {
    let (_root, mut w, key) = fixture();
    let cache = w.root.join("indexes/lucene");
    fs::create_dir_all(cache.join("index")).unwrap();
    fs::write(cache.join("index/sentinel"), b"existing derivative").unwrap();
    fs::write(cache.parent().unwrap().join("lucene-revision.txt"), b"1").unwrap();
    let mut body = serde_json::to_value(get_evidence(&w.conn, &key).unwrap()).unwrap();
    body["origin_group"] = json!("x".repeat(MAX_ROW_BYTES));
    put(&w.conn, "evidence", &key, &body).unwrap();
    let owner = w.collection_ownership().unwrap();
    let before = state(&w);
    for query in [" ".to_owned(), "x".repeat(1025), "needle".to_owned()] {
        let error = w
            .search_owned_with(&owner, &query, |_, _, _, _, _| panic!("no executor"))
            .unwrap_err();
        let expected = if query == "needle" {
            "metadata row limit"
        } else {
            "Query must contain"
        };
        assert!(error.to_string().contains(expected));
        assert_eq!(
            fs::read(cache.join("index/sentinel")).unwrap(),
            b"existing derivative"
        );
        assert_eq!(
            fs::read(cache.parent().unwrap().join("lucene-revision.txt")).unwrap(),
            b"1"
        );
        assert!(!cache
            .join(crate::engines::search_lifecycle::INTENT)
            .exists());
        assert_eq!(state(&w), before);
    }
    assert!(owner.held());
    owner.release().unwrap();
}

#[test]
fn search_capture_exact_manifest_reaches_existing_two_stage_recipe_without_canonical_writes() {
    use crate::policy::WorkerOperation;
    let (_root, mut w, key) = fixture();
    let legacy = all_evidence(&w.conn).unwrap();
    let revision = w.revision().unwrap();
    let expected = serde_json::to_vec(&json!({"workspace_revision":revision,"documents":
        legacy.iter().filter_map(|e|e.text.as_ref().map(|text|json!({"id":e.id,"name":e.name,"text":text}))).collect::<Vec<_>>() })).unwrap();
    let before = state(&w);
    let original = fs::read(w.root.join("originals").join(&key)).unwrap();
    let owner = w.collection_ownership().unwrap();
    let mut operations = Vec::new();
    let result = w
        .search_owned_with(&owner, "\"Exact\"", |runtime, cache, _, corpus, query| {
            runtime.search_corpus_with(cache, corpus, query, |cache, operation, input| {
                let bytes = fs::read(cache.join(input))?;
                match operation {
                    WorkerOperation::Index => {
                        operations.push("index");
                        assert_eq!(bytes, expected);
                        fs::create_dir_all(cache.join("index"))?;
                        fs::write(cache.join("index/segment"), b"synthetic index")?;
                        Ok(serde_json::to_vec(
                            &json!({"indexed":1,"workspace_revision":revision}),
                        )?)
                    }
                    WorkerOperation::Search => {
                        operations.push("search");
                        assert_eq!(bytes, serde_json::to_vec(&json!({"query":query}))?);
                        Ok(serde_json::to_vec(
                            &json!({"workspace_revision":revision.to_string(),
                        "hits":[{"id":key,"name":"synthetic.txt","score":1.25}],"total":1}),
                        )?)
                    }
                    _ => panic!("unexpected operation"),
                }
            })
        })
        .unwrap();
    assert_eq!(operations, ["index", "search"]);
    assert_eq!(result.hits[0].id, key);
    assert_eq!(result.hits[0].score, 1.25);
    assert_eq!(state(&w), before);
    assert_eq!(
        fs::read(w.root.join("originals").join(&key)).unwrap(),
        original
    );
    assert!(owner.held());
    owner.release().unwrap();
}

#[test]
fn search_capture_success_keeps_entire_canonical_state_and_all_originals() {
    let (_root, mut w, key) = fixture();
    w.import("unprocessed.bin", b"Synthetic nontext original")
        .unwrap();
    let before = state(&w);
    let originals: Vec<_> = all_evidence(&w.conn)
        .unwrap()
        .iter()
        .map(|e| {
            (
                e.id.clone(),
                fs::read(w.root.join("originals").join(&e.sha256)).unwrap(),
            )
        })
        .collect();
    let owner = w.collection_ownership().unwrap();
    let result = w
        .search_owned_with(&owner, "needle", |_, _, revision, corpus, query| {
            assert_eq!(query, "needle");
            assert_eq!(revision, corpus.revision());
            assert_eq!(corpus.document_count(), 1);
            assert!(corpus.knows(&key));
            Completion::released(Ok(SearchResults {
                workspace_revision: revision.to_string(),
                hits: vec![],
                total: 0,
            }))
        })
        .unwrap();
    assert_eq!(result.workspace_revision, w.revision().unwrap().to_string());
    assert_eq!(state(&w), before);
    for (id, bytes) in originals {
        assert_eq!(fs::read(w.root.join("originals").join(id)).unwrap(), bytes);
    }
    assert!(!w.root.join("indexes").exists());
    assert!(owner.held());
    owner.release().unwrap();
}
