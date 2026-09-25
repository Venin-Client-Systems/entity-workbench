use super::*;
use crate::{
    transaction_export::TransactionExportRequest,
    transaction_page::{TransactionPageFilter, TransactionPageOrder},
};
fn request() -> TransactionCsvRequest {
    TransactionCsvRequest {
        selection: TransactionExportRequest {
            query: String::new(),
            filter: TransactionPageFilter::default(),
            order: TransactionPageOrder::DateAscending,
        },
        non_accepted: NonAcceptedCsvPolicy::AllowSelected,
    }
}
fn fixture() -> (tempfile::TempDir, Workspace) {
    let temp = tempfile::tempdir_in(std::env::temp_dir().canonicalize().unwrap()).unwrap();
    let mut w = Workspace::open(temp.path().join("case")).unwrap();
    let mut csv = csv::Writer::from_writer(Vec::new());
    csv.write_record(["account", "date", "description", "amount", "currency"])
        .unwrap();
    let descriptions = [
        "=1+1",
        "+SUM(A1)",
        "-2+3",
        "@SUM(A1)",
        "  =1",
        "\t=1",
        "＝１",
        "＋１",
        "－１",
        "＠name",
        "line1\r\n=HYPERLINK(\"https://example.invalid\")",
        "quote\"comma,semi;",
        "Καλημέρα 🚲",
        "null",
    ];
    for i in 0..305 {
        let amount = if i == 0 {
            "79228162514264337593543950335"
        } else if i == 1 {
            "-79228162514264337593543950335"
        } else {
            "-0.10000001"
        };
        csv.write_record([
            format!("{:06}", i % 3),
            format!("2024-02-{:02}", 27 + i % 3),
            descriptions[i % descriptions.len()].to_owned(),
            amount.into(),
            if i % 2 == 0 { "AUD" } else { "USD" }.into(),
        ])
        .unwrap();
    }
    w.import("typed-csv.csv", &csv.into_inner().unwrap())
        .unwrap();
    (temp, w)
}
fn decode(export: &TransactionCsvExport) -> Vec<Value> {
    let mut reader = csv::Reader::from_reader(export.csv.as_bytes());
    assert_eq!(
        reader.headers().unwrap().iter().collect::<Vec<_>>(),
        export
            .dictionary
            .columns
            .iter()
            .map(|c| c.name.as_str())
            .collect::<Vec<_>>()
    );
    reader
        .records()
        .map(|record| {
            let record = record.unwrap();
            let mut row = serde_json::Map::new();
            for (raw, column) in record.iter().zip(&export.dictionary.columns) {
                assert!(raw.as_bytes()[0].is_ascii_alphabetic());
                let value = if raw == export.dictionary.null_literal {
                    assert!(column.nullable);
                    Value::Null
                } else {
                    let raw = raw
                        .strip_prefix(&column.prefix)
                        .expect("Declared typed prefix");
                    match column.logical_type {
                        CsvLogicalType::UnsignedInteger => json!(raw.parse::<u64>().unwrap()),
                        CsvLogicalType::SourceAnchorJson | CsvLogicalType::StringArrayJson => {
                            serde_json::from_str(raw).unwrap()
                        }
                        _ => json!(raw),
                    }
                };
                row.insert(column.name.clone(), value);
            }
            assert_eq!(
                row.remove("workspace_revision").unwrap(),
                json!(export.workspace_revision)
            );
            Value::Object(row)
        })
        .collect()
}
#[test]
fn full_scope_csv_matches_raw_json_across_filters_orders_and_exact_extremes() {
    let (_temp, w) = fixture();
    let revision = w.revision().unwrap();
    let before = serde_json::to_value(w.view().unwrap()).unwrap();
    let mut format_hash = None;
    for mode in 0..7 {
        let mut request = request();
        match mode {
            1 => request.selection.order = TransactionPageOrder::DateDescending,
            2 => request.selection.filter.account = Some("000001".into()),
            3 => request.selection.filter.currency = Some("USD".into()),
            4 => request.selection.query = "HYPERLINK".into(),
            5 => {
                request.selection.filter.date_from = Some("2024-02-29".into());
                request.selection.filter.date_to = Some("2024-02-29".into());
            }
            6 => request.selection.query = "missing synthetic phrase".into(),
            _ => (),
        }
        let raw = w.export_transactions(&request.selection, revision).unwrap();
        let export = w.export_transaction_csv(&request, revision).unwrap();
        assert_eq!(
            decode(&export),
            serde_json::from_str::<Vec<Value>>(&raw.json).unwrap()
        );
        assert_eq!(export.selection_sha256, raw.query_sha256);
        assert_eq!(export.row_count, raw.row_count);
        assert_eq!(export.matching, raw.matching);
        assert_eq!(export.bytes, export.csv.len() as u64);
        assert_eq!(export.sha256, hash(export.csv.as_bytes()));
        assert!(export.csv.starts_with('\u{feff}'));
        assert!(export.csv.ends_with("\r\n"));
        assert_eq!(
            export.format_sha256,
            hash(
                &serde_json::to_vec(
                    &serde_json::to_value((export.format, &export.dictionary)).unwrap()
                )
                .unwrap()
            )
        );
        if let Some(previous) = &format_hash {
            assert_eq!(previous, &export.format_sha256)
        } else {
            format_hash = Some(export.format_sha256.clone());
        }
        if mode == 0 {
            assert_eq!(export.row_count, 305);
            assert!(export.csv.contains("decimal:79228162514264337593543950335"));
            assert!(export.csv.contains("decimal:-0.10000001"));
            assert!(export.csv.contains("text:000001"));
            assert!(export.csv.contains("text:  =1"));
            assert!(export.csv.contains("text:\t=1"));
        }
    }
    assert_eq!(serde_json::to_value(w.view().unwrap()).unwrap(), before);
}
#[test]
fn nonaccepted_policy_is_required_and_reject_never_filters_a_mixed_selection() {
    let (_temp, mut w) = fixture();
    let rows = w.view().unwrap().transactions;
    for (row, state) in rows.iter().zip([
        ReviewState::Accepted,
        ReviewState::Rejected,
        ReviewState::Deferred,
    ]) {
        w.review_transaction(
            &row.id,
            state,
            "Synthetic CSV review",
            w.revision().unwrap(),
        )
        .unwrap();
    }
    let revision = w.revision().unwrap();
    let mut req = request();
    req.non_accepted = NonAcceptedCsvPolicy::Reject;
    assert!(w.export_transaction_csv(&req, revision).is_err());
    assert!(w.conn.is_autocommit());
    req.selection.filter.review = Some(ReviewState::Accepted);
    assert_eq!(
        w.export_transaction_csv(&req, revision).unwrap().row_count,
        1
    );
    for state in [
        ReviewState::Pending,
        ReviewState::Rejected,
        ReviewState::Deferred,
    ] {
        req.selection.filter.review = Some(state);
        assert!(w.export_transaction_csv(&req, revision).is_err());
        req.non_accepted = NonAcceptedCsvPolicy::AllowSelected;
        assert!(w.export_transaction_csv(&req, revision).unwrap().row_count > 0);
        req.non_accepted = NonAcceptedCsvPolicy::Reject;
    }
    let mut missing = serde_json::to_value(request()).unwrap();
    missing.as_object_mut().unwrap().remove("non_accepted");
    assert!(serde_json::from_value::<TransactionCsvRequest>(missing).is_err());
    assert_eq!(w.revision().unwrap(), revision);
}
#[test]
fn exact_csv_byte_boundary_includes_bom_header_quotes_newlines_and_no_partial_prefix() {
    let (_temp, w) = fixture();
    let rows = w.view().unwrap().transactions;
    let csv = encode(
        &rows[..14],
        w.revision().unwrap(),
        NonAcceptedCsvPolicy::AllowSelected,
        MAX_CSV_BYTES,
    )
    .unwrap();
    assert_eq!(
        encode(
            &rows[..14],
            w.revision().unwrap(),
            NonAcceptedCsvPolicy::AllowSelected,
            csv.len()
        )
        .unwrap(),
        csv
    );
    assert!(encode(
        &rows[..14],
        w.revision().unwrap(),
        NonAcceptedCsvPolicy::AllowSelected,
        csv.len() - 1
    )
    .is_err());
    let empty = encode(&[], 42, NonAcceptedCsvPolicy::Reject, MAX_CSV_BYTES).unwrap();
    assert_eq!(
        csv::Reader::from_reader(empty.as_bytes()).records().count(),
        0
    );
    assert!(encode(&[], 42, NonAcceptedCsvPolicy::Reject, empty.len() - 1).is_err());
    assert!(encode(&[], 42, NonAcceptedCsvPolicy::Reject, 2).is_err());
}
#[test]
fn null_empty_literal_null_and_all_anchor_variants_round_trip_without_normalization() {
    let (_temp, w) = fixture();
    let mut row = w.view().unwrap().transactions[0].clone();
    let evidence_id = row.anchor.evidence_id().to_owned();
    let anchors = [
        SourceAnchor::Text {
            evidence_id: evidence_id.clone(),
            line_start: 1,
            line_end: 2,
        },
        SourceAnchor::Page {
            evidence_id: evidence_id.clone(),
            page: 1,
            region: Some([0.25, 0.5, 10.0, 20.0]),
        },
        row.anchor.clone(),
        SourceAnchor::Message {
            evidence_id: evidence_id.clone(),
            message_id: "<synthetic>\0".into(),
        },
        SourceAnchor::Capture {
            evidence_id,
            selector: "#quoted\"inert".into(),
        },
    ];
    for (index, anchor) in anchors.into_iter().enumerate() {
        row.anchor = anchor;
        row.balance = Some("0.00000000".into());
        row.posting_date = Some("2024-03-01".into());
        row.merchant = match index {
            0 => None,
            1 => Some(String::new()),
            _ => Some("null".into()),
        };
        row.transfer_peer = Some("000007".into());
        row.duplicate_candidates = vec!["candidate-1".into(), "=inert".into()];
        let csv = encode(
            std::slice::from_ref(&row),
            999,
            NonAcceptedCsvPolicy::AllowSelected,
            MAX_CSV_BYTES,
        )
        .unwrap();
        let export = TransactionCsvExport {
            schema_version: 1,
            workspace_revision: 999,
            request: request(),
            matching: Default::default(),
            selection_sha256: String::new(),
            format: TransactionCsvFormat::TypedLiteralV1,
            format_sha256: String::new(),
            dictionary: dictionary(),
            row_count: 1,
            bytes: csv.len() as u64,
            sha256: hash(csv.as_bytes()),
            csv,
        };
        assert_eq!(decode(&export), vec![serde_json::to_value(&row).unwrap()]);
    }
}
#[test]
fn unsupported_literal_controls_fail_csv_but_raw_json_remains_exact() {
    let (_temp, w) = fixture();
    let mut row = w.view().unwrap().transactions[0].clone();
    for character in ['\0', '\u{000b}', '\u{0085}'] {
        row.description = format!("Synthetic {character} value");
        put(&w.conn, "transaction", &row.id, &row).unwrap();
        assert!(w
            .export_transaction_csv(&request(), w.revision().unwrap())
            .is_err());
        let raw = w
            .export_transactions(&request().selection, w.revision().unwrap())
            .unwrap();
        assert!(serde_json::from_str::<Vec<Transaction>>(&raw.json)
            .unwrap()
            .iter()
            .any(|r| r.description == row.description));
    }
}
#[test]
fn csv_refuses_stale_malformed_and_substituted_sources_preserving_originals_and_report() {
    let (_temp, mut w) = fixture();
    let other = w
        .import("other.txt", b"Synthetic unrelated original")
        .unwrap();
    w.save_report().unwrap();
    let before = serde_json::to_value(w.view().unwrap()).unwrap();
    let revision = w.revision().unwrap();
    assert!(w.export_transaction_csv(&request(), revision - 1).is_err());
    let mut bad = request();
    bad.selection.filter.account = Some("x".repeat(4001));
    assert!(w.export_transaction_csv(&bad, revision).is_err());
    let view = w.view().unwrap();
    let source = view
        .evidence
        .iter()
        .find(|e| e.media_type == "text/csv")
        .unwrap();
    let raw = fs::read(w.root.join("originals").join(&source.id)).unwrap();
    let path = w.root.join("originals").join(&source.id);
    let moved = w.root.join("originals").join("synthetic-moved");
    fs::rename(&path, &moved).unwrap();
    assert!(w.export_transaction_csv(&request(), revision).is_err());
    fs::rename(&moved, &path).unwrap();
    let b: Evidence = get_evidence(&w.conn, &other).unwrap();
    put(&w.conn, "evidence", &source.id, &b).unwrap();
    assert!(w.export_transaction_csv(&request(), revision).is_err());
    put(&w.conn, "evidence", &source.id, source).unwrap();
    let mut row = view.transactions[0].clone();
    row.amount = "inexact".into();
    put(&w.conn, "transaction", &row.id, &row).unwrap();
    let mut no_match = request();
    no_match.selection.query = "no match".into();
    assert!(w.export_transaction_csv(&no_match, revision).is_err());
    put(
        &w.conn,
        "transaction",
        &view.transactions[0].id,
        &view.transactions[0],
    )
    .unwrap();
    assert_eq!(fs::read(path).unwrap(), raw);
    assert_eq!(serde_json::to_value(w.view().unwrap()).unwrap(), before);
}
#[test]
fn csv_snapshot_is_pinned_during_real_concurrent_correction_and_dispatches_identically() {
    use rusqlite::hooks::{AuthAction, AuthContext, Authorization};
    use std::sync::{Arc, Mutex};
    let (_temp, mut w) = fixture();
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
                        "Synthetic concurrent CSV correction",
                        revision,
                    )
                    .unwrap();
                *done = true;
            }
        }
        Authorization::Allow
    }));
    let old = w.export_transaction_csv(&request(), revision).unwrap();
    assert!(*changed.lock().unwrap());
    let decoded = decode(&old);
    let original = decoded.iter().find(|r| r["id"] == row.id).unwrap();
    assert_eq!(original["amount"], row.amount);
    assert_eq!(original["version"], row.version);
    assert!(w.export_transaction_csv(&request(), revision).is_err());
    let command = Command::ExportTransactionCsv {
        request: request(),
        expected_revision: revision + 1,
    };
    let full = w.dispatch(command.clone()).unwrap();
    assert_eq!(full, w.dispatch_presentation(command.clone()).unwrap());
    assert_eq!(full, w.dispatch_summary(command).unwrap());
    let new: TransactionCsvExport = serde_json::from_value(full).unwrap();
    assert_ne!(new.sha256, old.sha256);
    assert_ne!(new.selection_sha256, old.selection_sha256);
    assert_eq!(new.format_sha256, old.format_sha256);
}
