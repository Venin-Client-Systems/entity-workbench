use super::*;

fn workspace() -> (tempfile::TempDir, Workspace, Vec<Transaction>) {
    let temp = tempfile::tempdir_in(std::env::temp_dir().canonicalize().unwrap()).unwrap();
    let mut w = Workspace::open(temp.path().join("case")).unwrap();
    w.import("synthetic.csv", b"account,date,description,amount,currency\n0001,2025-01-03,First,-0.10000001,AUD\n0001,2025-01-01,Second,12.00,AUD\n0002,2025-01-03,Third,-12.00,USD\n").unwrap();
    let rows = w.view().unwrap().transactions;
    (temp, w, rows)
}
fn request(rows: &[Transaction]) -> TransactionSourcesRequest {
    TransactionSourcesRequest {
        rows: rows
            .iter()
            .map(|r| TransactionSourceKey {
                id: r.id.clone(),
                expected_version: Some(r.version),
            })
            .collect(),
    }
}

#[test]
fn selected_rows_preserve_request_order_money_anchors_and_dispatch_shape() {
    let (_temp, mut w, rows) = workspace();
    let mut expected = vec![rows[2].clone(), rows[0].clone()];
    // Canonical keys are opaque, including legacy non-UUID IDs and leading zeros.
    let old = expected[0].id.clone();
    expected[0].id = "00042/legacy".into();
    w.conn
        .execute(
            "UPDATE records SET id=?,body=? WHERE kind='transaction' AND id=?",
            params![
                expected[0].id,
                serde_json::to_string(&expected[0]).unwrap(),
                old
            ],
        )
        .unwrap();
    let req = request(&expected);
    let revision = w.revision().unwrap();
    let full = w
        .dispatch(Command::ReadTransactionSources {
            request: req.clone(),
            expected_revision: revision,
        })
        .unwrap();
    let presentation = w
        .dispatch_presentation(Command::ReadTransactionSources {
            request: req,
            expected_revision: revision,
        })
        .unwrap();
    assert_eq!(full, presentation);
    assert_eq!(full["rows"], serde_json::to_value(&expected).unwrap());
    assert_eq!(full["workspace_revision"], revision);
    assert!(full.get("workspace").is_none());
    assert_eq!(w.revision().unwrap(), revision);
    assert!(w.conn.is_autocommit());
}

#[test]
fn rejected_keys_and_missing_rows_never_return_a_partial_batch() {
    let (_temp, w, rows) = workspace();
    let revision = w.revision().unwrap();
    for invalid in ["", "bad\0key", "' OR 1=1 --"] {
        let mut req = request(&rows[..1]);
        req.rows.push(TransactionSourceKey {
            id: invalid.into(),
            expected_version: None,
        });
        assert!(w.read_transaction_sources(&req, revision).is_err());
        assert!(w.conn.is_autocommit());
    }
    let mut req = request(&rows);
    req.rows[1] = req.rows[0].clone();
    assert!(req.validate().is_err());
    req.rows.clear();
    assert!(req.validate().is_err());
    req.rows = (0..26)
        .map(|i| TransactionSourceKey {
            id: i.to_string(),
            expected_version: None,
        })
        .collect();
    assert!(req.validate().is_err());
    let mut req = request(&rows[..1]);
    req.rows[0].id = "é".repeat(129);
    assert!(req.validate().is_err());
    req = request(&rows[..1]);
    req.rows[0].expected_version = Some(0);
    assert!(req.validate().is_err());
    let mut json = serde_json::to_value(request(&rows)).unwrap();
    json["rows"][0]["sql"] = json!("SELECT * FROM records");
    assert!(serde_json::from_value::<TransactionSourcesRequest>(json).is_err());
    let mut json = serde_json::to_value(request(&rows)).unwrap();
    json["extra"] = json!(true);
    assert!(serde_json::from_value::<TransactionSourcesRequest>(json).is_err());
}

#[test]
fn revision_and_row_version_checks_require_an_explicit_fresh_result() {
    let (_temp, mut w, rows) = workspace();
    let revision = w.revision().unwrap();
    let mut req = request(&rows[..1]);
    w.correct_transaction(&rows[0].id, "-0.10000002", "Synthetic correction", revision)
        .unwrap();
    assert!(matches!(
        w.read_transaction_sources(&req, revision),
        Err(Error::Conflict(_))
    ));
    let updated = w.revision().unwrap();
    assert!(matches!(
        w.read_transaction_sources(&req, updated),
        Err(Error::Conflict(_))
    ));
    req.rows[0].expected_version = None;
    let current = w.read_transaction_sources(&req, updated).unwrap();
    assert_eq!(current.rows[0].amount, "-0.10000002");
    assert_eq!(current.rows[0].version, rows[0].version + 1);
    assert_eq!(current.rows[0].review, ReviewState::Pending);
    assert!(w.conn.is_autocommit());
}

#[test]
fn whole_batch_size_is_checked_before_any_body_is_decoded() {
    let (_temp, w, rows) = workspace();
    // First row is valid JSON but deliberately invalid as a Transaction. The
    // second row makes the complete batch too large. Budget rejection must win.
    w.conn
        .execute(
            "UPDATE records SET body='{}' WHERE kind='transaction' AND id=?",
            [&rows[0].id],
        )
        .unwrap();
    let mut large = rows[1].clone();
    large.description = "x".repeat(MAX_SOURCE_BODY_BYTES as usize);
    put(&w.conn, "transaction", &large.id, &large).unwrap();
    let error = w
        .read_transaction_sources(&request(&rows[..2]), w.revision().unwrap())
        .unwrap_err();
    assert!(error.to_string().contains("2 MiB"));
    // Also enforce the aggregate rather than just each individual record.
    for row in &rows[..2] {
        let mut large = row.clone();
        large.description = "x".repeat(MAX_SOURCE_BODY_BYTES as usize / 2);
        put(&w.conn, "transaction", &large.id, &large).unwrap();
    }
    assert!(w
        .read_transaction_sources(&request(&rows[..2]), w.revision().unwrap())
        .unwrap_err()
        .to_string()
        .contains("2 MiB"));
    assert!(w.conn.is_autocommit());
}

#[test]
fn canonical_corruption_and_modified_original_reject_the_whole_read() {
    let (temp, w, rows) = workspace();
    let revision = w.revision().unwrap();
    for (field, value) in [
        ("id", "wrong-key"),
        ("amount", "NaN"),
        ("date", "2025-02-30"),
        ("currency", "AUDX"),
    ] {
        let mut body = serde_json::to_value(&rows[0]).unwrap();
        body[field] = json!(value);
        w.conn
            .execute(
                "UPDATE records SET body=? WHERE kind='transaction' AND id=?",
                params![serde_json::to_string(&body).unwrap(), rows[0].id],
            )
            .unwrap();
        assert!(w
            .read_transaction_sources(&request(&rows), revision)
            .is_err());
    }
    put(&w.conn, "transaction", &rows[0].id, &rows[0]).unwrap();
    assert_eq!(
        w.read_transaction_sources(&request(&rows), revision)
            .unwrap()
            .rows
            .len(),
        3
    );
    let path = temp
        .path()
        .join("case/originals")
        .join(rows[0].anchor.evidence_id());
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        fs::set_permissions(&path, fs::Permissions::from_mode(0o600)).unwrap();
    }
    fs::write(path, b"corrupt").unwrap();
    assert!(w
        .read_transaction_sources(&request(&rows), revision)
        .is_err());
    assert!(w.conn.is_autocommit());
}

#[test]
fn revision_size_metadata_and_rows_remain_one_snapshot_during_a_writer_commit() {
    use rusqlite::hooks::{AuthAction, AuthContext, Authorization};
    use std::sync::{Arc, Mutex};
    let (_temp, w, rows) = workspace();
    let mut writer = Workspace::open(&w.root).unwrap();
    w.conn.pragma_update(None, "journal_mode", "WAL").unwrap();
    let revision = w.revision().unwrap();
    let key = rows[0].id.clone();
    let changed = Arc::new(Mutex::new(false));
    let observed = changed.clone();
    w.conn.authorizer(Some(move |context: AuthContext<'_>| {
        if matches!(
            context.action,
            AuthAction::Read {
                table_name: "records",
                ..
            }
        ) {
            let mut done = observed.lock().unwrap();
            if !*done {
                writer
                    .correct_transaction(
                        &key,
                        "-0.20000001",
                        "Synthetic concurrent correction",
                        revision,
                    )
                    .unwrap();
                *done = true;
            }
        }
        Authorization::Allow
    }));
    let result = w
        .read_transaction_sources(&request(&rows), revision)
        .unwrap();
    assert!(*changed.lock().unwrap());
    assert_eq!(result.workspace_revision, revision);
    assert_eq!(
        serde_json::to_value(result.rows).unwrap(),
        serde_json::to_value(&rows).unwrap()
    );
    assert_eq!(w.revision().unwrap(), revision + 1);
    assert!(matches!(
        w.read_transaction_sources(&request(&rows), revision + 1),
        Err(Error::Conflict(_))
    ));
    assert!(w.conn.is_autocommit());
}
