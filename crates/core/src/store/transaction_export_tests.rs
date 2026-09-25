use super::*;
use crate::transaction_page::{TransactionPageFilter, TransactionPageRequest};
use crate::transaction_search::TransactionSearchRequest;
fn workspace() -> (tempfile::TempDir, Workspace) {
    let temp = tempfile::tempdir_in(std::env::temp_dir().canonicalize().unwrap()).unwrap();
    let mut w = Workspace::open(temp.path().join("case")).unwrap();
    let mut csv = "account,date,description,amount,currency\n".to_owned();
    for i in 0..305 {
        csv.push_str(&format!(
            "{:04},2025-01-{:02},Synthetic %_ <b>inert</b> row {i},-0.10000001,{}\n",
            i % 3,
            1 + i % 3,
            if i % 2 == 0 { "AUD" } else { "USD" }
        ));
    }
    w.import("export.csv", csv.as_bytes()).unwrap();
    (temp, w)
}
fn request() -> TransactionExportRequest {
    TransactionExportRequest {
        query: String::new(),
        filter: TransactionPageFilter::default(),
        order: TransactionPageOrder::DateAscending,
    }
}
fn decoded(value: &TransactionExport) -> Vec<crate::domain::Transaction> {
    serde_json::from_str(&value.json).unwrap()
}
#[test]
fn complete_export_matches_all_search_pages_for_exact_filters_and_date_orders() {
    let (_temp, mut w) = workspace();
    let mut rows = w.view().unwrap().transactions;
    w.review_transaction(
        &rows[0].id,
        ReviewState::Accepted,
        "Synthetic export review",
        w.revision().unwrap(),
    )
    .unwrap();
    rows = w.view().unwrap().transactions;
    for mode in 0..7 {
        let mut req = request();
        match mode {
            1 => req.order = TransactionPageOrder::DateDescending,
            2 => req.query = "%_".into(),
            3 => {
                req.filter.account = Some("0001".into());
                req.filter.currency = Some("USD".into());
            }
            4 => req.filter.review = Some(ReviewState::Accepted),
            5 => {
                req.filter.date_from = Some("2025-01-02".into());
                req.filter.date_to = Some("2025-01-03".into());
            }
            6 => req.query = "' OR 1=1 --".into(),
            _ => (),
        }
        let revision = w.revision().unwrap();
        let export = w.export_transactions(&req, revision).unwrap();
        let mut search = TransactionSearchRequest {
            query: req.query.clone(),
            page: TransactionPageRequest {
                filter: req.filter.clone(),
                order: req.order,
                page_size: 100,
                cursor: None,
            },
        };
        let mut expected = vec![];
        loop {
            let response = w.search_transactions(&search, revision).unwrap();
            assert_eq!(response.page.selected_count, export.row_count);
            expected.extend(response.page.rows);
            search.page.cursor = response.page.next_cursor;
            if search.page.cursor.is_none() {
                break;
            }
        }
        assert_eq!(
            serde_json::to_value(decoded(&export)).unwrap(),
            serde_json::to_value(expected).unwrap()
        );
        assert_eq!(export.bytes, export.json.len() as u64);
        assert_eq!(export.sha256, hash(export.json.as_bytes()));
        assert_eq!(export.workspace_revision, revision);
        assert_eq!(export.matching, LiteralMatching::default());
        assert_eq!(
            serde_json::to_value(&export.request).unwrap(),
            serde_json::to_value(&req).unwrap()
        );
        if mode == 0 {
            assert_eq!(export.row_count, 305);
            assert!(export.json.contains("-0.10000001"));
        }
        if mode == 6 {
            assert_eq!(export.json, "[]");
        }
    }
    let exported = decoded(
        &w.export_transactions(&request(), w.revision().unwrap())
            .unwrap(),
    );
    assert_eq!(
        exported
            .iter()
            .filter(|r| r.date == "2025-01-01")
            .map(|r| &r.id)
            .collect::<Vec<_>>(),
        rows.iter()
            .filter(|r| r.date == "2025-01-01")
            .map(|r| &r.id)
            .collect::<Vec<_>>()
    );
}
#[test]
fn exact_byte_limit_fails_instead_of_truncating_a_json_array() {
    let (_temp, w) = workspace();
    let rows = w.view().unwrap().transactions;
    let expected = serde_json::to_vec_pretty(&rows[..2]).unwrap();
    assert_eq!(
        encode(&rows[..2], expected.len()).unwrap().as_bytes(),
        expected
    );
    assert!(encode(&rows[..2], expected.len() - 1).is_err());
    assert!(encode(&rows[..2], 0).is_err());
    assert_eq!(encode(&[], 2).unwrap(), "[]");
    assert!(encode(&[], 1).is_err());
}
#[test]
fn revision_query_and_scope_validation_never_returns_a_stale_or_partial_export() {
    let (_temp, w) = workspace();
    let revision = w.revision().unwrap();
    assert!(w.export_transactions(&request(), revision - 1).is_err());
    for mode in 0..5 {
        let mut req = request();
        match mode {
            0 => req.query = "é".repeat(513),
            1 => req.filter.account = Some("bad\0account".into()),
            2 => req.filter.currency = Some("aud".into()),
            3 => req.filter.date_from = Some("2025-02-30".into()),
            _ => {
                req.filter.date_from = Some("2025-02-01".into());
                req.filter.date_to = Some("2025-01-01".into());
            }
        }
        assert!(w.export_transactions(&req, revision).is_err());
    }
    let mut unknown = serde_json::to_value(request()).unwrap();
    unknown["cursor"] = json!("pretend-page");
    assert!(serde_json::from_value::<TransactionExportRequest>(unknown).is_err());
    assert!(w.conn.is_autocommit());
    assert_eq!(w.revision().unwrap(), revision);
}
#[test]
fn missing_and_retargeted_originals_fail_without_changing_retained_bytes_or_reports() {
    let (_temp, mut w) = workspace();
    let second = w
        .import("other.txt", b"Synthetic unrelated source")
        .unwrap();
    w.save_report().unwrap();
    let before = serde_json::to_value(w.view().unwrap()).unwrap();
    let v = w.view().unwrap();
    let first = &v.evidence[0];
    let path = w.root.join("originals").join(&first.id);
    let moved = w.root.join("originals").join("synthetic-unavailable");
    fs::rename(&path, &moved).unwrap();
    assert!(w.export_transactions(&request(), v.revision).is_err());
    fs::rename(&moved, &path).unwrap();
    let b: Evidence = get(&w.conn, "evidence", &second).unwrap();
    w.conn.execute("UPDATE records SET body=json_set(body,'$.sha256',?1,'$.bytes',?2) WHERE kind='evidence' AND id=?3", params![b.sha256,b.bytes,first.id]).unwrap();
    assert!(w.export_transactions(&request(), v.revision).is_err());
    put(&w.conn, "evidence", &first.id, first).unwrap();
    assert_eq!(
        w.export_transactions(&request(), v.revision)
            .unwrap()
            .row_count,
        305
    );
    assert_eq!(serde_json::to_value(w.view().unwrap()).unwrap(), before);
    assert_eq!(hash(&fs::read(path).unwrap()), first.sha256);
    assert_eq!(
        w.inspect_report_snapshot(&v.reports[0].id, &v.reports[0].sha256)
            .unwrap()
            .html,
        v.reports[0].html
    );
}
#[test]
fn malformed_unselected_records_and_scoped_nonmatching_text_are_not_silently_dropped() {
    let (_temp, w) = workspace();
    let rows = w.view().unwrap().transactions;
    let mut req = request();
    req.query = "no matches".into();
    req.filter.review = Some(ReviewState::Accepted);
    for (field, value) in [
        ("id", json!("wrong")),
        ("amount", json!("NaN")),
        ("description", json!("x".repeat(4001))),
    ] {
        let mut changed = serde_json::to_value(&rows[0]).unwrap();
        changed[field] = value;
        w.conn
            .execute(
                "UPDATE records SET body=? WHERE kind='transaction' AND id=?",
                params![changed.to_string(), rows[0].id],
            )
            .unwrap();
        assert!(
            w.export_transactions(&req, w.revision().unwrap()).is_err(),
            "{field}"
        );
        assert!(w.conn.is_autocommit());
    }
    put(&w.conn, "transaction", &rows[0].id, &rows[0]).unwrap();
    assert_eq!(
        w.export_transactions(&req, w.revision().unwrap())
            .unwrap()
            .row_count,
        0
    );
}
#[test]
fn export_is_a_single_snapshot_during_a_real_concurrent_correction() {
    use rusqlite::hooks::{AuthAction, AuthContext, Authorization};
    use std::sync::{Arc, Mutex};
    let (_temp, w) = workspace();
    let row = w.view().unwrap().transactions[0].clone();
    let key = row.id.clone();
    let revision = w.revision().unwrap();
    let mut writer = Workspace::open(&w.root).unwrap();
    w.conn.pragma_update(None, "journal_mode", "WAL").unwrap();
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
                        "2.00",
                        "Synthetic concurrent export correction",
                        revision,
                    )
                    .unwrap();
                *done = true;
            }
        }
        Authorization::Allow
    }));
    let export = w.export_transactions(&request(), revision).unwrap();
    assert!(*changed.lock().unwrap());
    assert_eq!(export.workspace_revision, revision);
    let old = decoded(&export);
    let old = old.iter().find(|r| r.id == row.id).unwrap();
    assert_eq!(old.amount, "-0.10000001");
    assert_eq!(old.version, row.version);
    assert!(w.export_transactions(&request(), revision).is_err());
    let current = w.export_transactions(&request(), revision + 1).unwrap();
    let rows = decoded(&current);
    let corrected = rows.iter().find(|r| r.id == row.id).unwrap();
    assert_eq!(corrected.amount, "2.00");
    assert_eq!(corrected.version, row.version + 1);
    assert_ne!(current.query_sha256, export.query_sha256);
    assert_ne!(current.sha256, export.sha256);
}

#[test]
fn public_export_is_identical_in_all_response_modes_and_rejects_oversized_filters() {
    let (_temp, mut w) = workspace();
    let revision = w.revision().unwrap();
    let before = serde_json::to_value(w.view().unwrap()).unwrap();
    let command = Command::ExportTransactions {
        request: request(),
        expected_revision: revision,
    };
    let value = w.dispatch(command.clone()).unwrap();
    assert_eq!(value, w.dispatch_presentation(command.clone()).unwrap());
    assert_eq!(value, w.dispatch_summary(command).unwrap());
    assert!(value.get("workspace").is_none());
    let export: TransactionExport = serde_json::from_value(value).unwrap();
    assert_eq!(export.row_count, 305);
    assert_eq!(hash(export.json.as_bytes()), export.sha256);
    for dimension in 0..4 {
        let mut bad = request();
        let large = Some("x".repeat(1_048_576));
        match dimension {
            0 => bad.filter.account = large,
            1 => bad.filter.currency = large,
            2 => bad.filter.date_from = large,
            _ => bad.filter.date_to = large,
        }
        let error = bad.filter.validate().unwrap_err().to_string();
        assert_eq!(bad.validate().unwrap_err().to_string(), error);
        assert!(w
            .dispatch_summary(Command::ExportTransactions {
                request: bad,
                expected_revision: revision
            })
            .is_err());
    }
    assert_eq!(serde_json::to_value(w.view().unwrap()).unwrap(), before);
}
