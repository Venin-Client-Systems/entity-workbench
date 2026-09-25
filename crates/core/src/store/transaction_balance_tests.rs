use super::*;

fn workspace() -> (tempfile::TempDir, Workspace) {
    let temp = tempfile::tempdir_in(std::env::temp_dir().canonicalize().unwrap()).unwrap();
    let mut w = Workspace::open(temp.path().join("case")).unwrap();
    w.import("synthetic-balances.csv", b"account,date,description,amount,currency,balance\n0001,2025-01-03,Opening,100.00,AUD,100.00\n0002,2025-01-01,Other account,40.00,AUD,40.00\n0001,2025-01-01,Repeated,-0.10000001,AUD,\n0001,2025-01-01,Repeated,-0.10000001,AUD,\n0001,2025-01-02,Closing,0.00,AUD,99.79999998\n0001,2025-01-04,Mismatch,-2.00,AUD,98.00\n0001,2025-01-05,Later movement,1.00,AUD,\n0001,2025-01-01,Other currency,5.00,USD,5.00\n").unwrap();
    w.import("synthetic-second.csv", b"account,date,description,amount,currency,balance\n0001,2025-01-06,Separate source,1.00,AUD,500.00\n").unwrap();
    (temp, w)
}
fn request(rows: &[crate::domain::Transaction]) -> TransactionBalancesRequest {
    TransactionBalancesRequest {
        rows: rows
            .iter()
            .map(|row| TransactionBalanceKey {
                id: row.id.clone(),
                expected_version: row.version,
            })
            .collect(),
    }
}
#[test]
fn selected_states_include_intervening_rows_in_source_order_across_all_review_states() {
    let (_temp, mut w) = workspace();
    let original = w.view().unwrap().transactions;
    for (row, state) in original.iter().zip([
        ReviewState::Accepted,
        ReviewState::Pending,
        ReviewState::Rejected,
        ReviewState::Deferred,
        ReviewState::Accepted,
        ReviewState::Rejected,
    ]) {
        w.review_transaction(
            &row.id,
            state,
            "Synthetic balance state",
            w.revision().unwrap(),
        )
        .unwrap();
    }
    let rows = w.view().unwrap().transactions;
    let chosen = [5, 0, 2, 4, 8, 1, 7].map(|i| rows[i].clone());
    let response = w
        .read_transaction_balances(&request(&chosen), w.revision().unwrap())
        .unwrap();
    assert_eq!(
        response.rows.iter().map(|r| &r.id).collect::<Vec<_>>(),
        chosen.iter().map(|r| &r.id).collect::<Vec<_>>()
    );
    assert_eq!(
        response.rows[0].balance,
        TransactionBalanceState::Checked {
            previous_id: rows[4].id.clone(),
            previous_version: rows[4].version,
            contributing_row_count: 1,
            difference: "0.20000002".into(),
            reconciled: false,
        }
    );
    assert_eq!(
        response.rows[1].balance,
        TransactionBalanceState::NoPriorBalance
    );
    assert_eq!(response.rows[2].balance, TransactionBalanceState::NoBalance);
    assert_eq!(
        response.rows[3].balance,
        TransactionBalanceState::Checked {
            previous_id: rows[0].id.clone(),
            previous_version: rows[0].version,
            contributing_row_count: 3,
            difference: "0.00000000".into(),
            reconciled: true,
        }
    );
    for row in &response.rows[4..] {
        assert_eq!(row.balance, TransactionBalanceState::NoPriorBalance);
    }
    let all = w
        .read_transaction_balances(&request(&rows), w.revision().unwrap())
        .unwrap();
    let legacy = analytics::analyse(&rows).unwrap();
    for check in legacy.balance_checks {
        let value = all
            .rows
            .iter()
            .find(|row| row.id == check.transaction_id)
            .unwrap();
        let TransactionBalanceState::Checked {
            difference,
            reconciled,
            contributing_row_count,
            ..
        } = &value.balance
        else {
            panic!("missing legacy check");
        };
        assert_eq!(difference, &check.difference);
        assert_eq!(*reconciled, check.reconciled);
        assert_eq!(*contributing_row_count, check.transaction_ids.len() as u64);
    }
}
#[test]
fn request_bounds_missing_versions_and_unknown_fields_fail_without_partial_results() {
    let (_temp, w) = workspace();
    let rows = w.view().unwrap().transactions;
    let good = request(&rows[..2]);
    let revision = w.revision().unwrap();
    for mode in 0..8 {
        let mut bad = good.clone();
        match mode {
            0 => bad.rows.clear(),
            1 => {
                bad.rows = (0..201)
                    .map(|i| TransactionBalanceKey {
                        id: format!("id-{i}"),
                        expected_version: 1,
                    })
                    .collect()
            }
            2 => bad.rows[1] = bad.rows[0].clone(),
            3 => bad.rows[1].id = "missing".into(),
            4 => bad.rows[1].id = "x".repeat(257),
            5 => bad.rows[1].id = "bad\0id".into(),
            6 => bad.rows[1].expected_version = 0,
            _ => bad.rows[1].expected_version += 1,
        }
        assert!(
            w.read_transaction_balances(&bad, revision).is_err(),
            "mode {mode}"
        );
        assert!(w.conn.is_autocommit());
    }
    assert!(w.read_transaction_balances(&good, revision - 1).is_err());
    let mut unknown = serde_json::to_value(&good).unwrap();
    unknown["sql"] = json!("SELECT body FROM records");
    assert!(serde_json::from_value::<TransactionBalancesRequest>(unknown).is_err());
    assert_eq!(w.revision().unwrap(), revision);
}
#[test]
fn corrupt_canonical_identity_and_unselected_money_fail_instead_of_clearing_warnings() {
    let (_temp, w) = workspace();
    let rows = w.view().unwrap().transactions;
    let req = request(&rows[4..5]);
    let revision = w.revision().unwrap();
    let target = &rows[6];
    for (field, value) in [
        ("id", json!("different")),
        ("version", json!(0)),
        ("amount", json!("NaN")),
        ("date", json!("2025-02-30")),
    ] {
        let mut body = serde_json::to_value(target).unwrap();
        body[field] = value;
        w.conn
            .execute(
                "UPDATE records SET body=? WHERE kind='transaction' AND id=?",
                params![body.to_string(), target.id],
            )
            .unwrap();
        assert!(
            w.read_transaction_balances(&req, revision).is_err(),
            "{field}"
        );
        assert!(w.conn.is_autocommit());
    }
    put(&w.conn, "transaction", &target.id, target).unwrap();
    assert!(w.read_transaction_balances(&req, revision).is_ok());
    assert_eq!(w.revision().unwrap(), revision);
}
#[test]
fn maximum_batch_is_small_even_when_one_balance_window_has_many_contributors() {
    let temp = tempfile::tempdir_in(std::env::temp_dir().canonicalize().unwrap()).unwrap();
    let mut w = Workspace::open(temp.path().join("case")).unwrap();
    let mut csv =
        "account,date,description,amount,currency,balance\nA,2025-01-01,Opening,0.00,AUD,0.00\n"
            .to_owned();
    for i in 0..300 {
        csv.push_str(&format!("A,2025-01-01,Movement {i},0.01,AUD,\n"));
    }
    csv.push_str("A,2025-01-01,Closing,0.00,AUD,3.00\n");
    w.import("many.csv", csv.as_bytes()).unwrap();
    let rows = w.view().unwrap().transactions;
    let result = w
        .read_transaction_balances(&request(&rows[102..]), w.revision().unwrap())
        .unwrap();
    assert_eq!(result.rows.len(), 200);
    assert!(matches!(
        result.rows[199].balance,
        TransactionBalanceState::Checked {
            contributing_row_count: 301,
            reconciled: true,
            ..
        }
    ));
    let bytes = serde_json::to_vec(&result).unwrap();
    assert!(bytes.len() < 32_768);
    let wire = serde_json::to_value(&result).unwrap();
    assert!(wire["rows"][199]["balance"]
        .get("transaction_ids")
        .is_none());
}
#[test]
fn all_dispatch_modes_preserve_rows_originals_reports_and_revision() {
    let (_temp, mut w) = workspace();
    w.save_report().unwrap();
    let before = serde_json::to_value(w.view().unwrap()).unwrap();
    let view = w.view().unwrap();
    let originals: Vec<_> = view
        .evidence
        .iter()
        .map(|e| fs::read(w.root.join("originals").join(&e.id)).unwrap())
        .collect();
    let command = Command::ReadTransactionBalances {
        request: request(&view.transactions),
        expected_revision: view.revision,
    };
    let value = w.dispatch(command.clone()).unwrap();
    assert_eq!(value, w.dispatch_presentation(command.clone()).unwrap());
    assert_eq!(value, w.dispatch_summary(command).unwrap());
    assert!(value.get("workspace").is_none());
    assert!(value.get("analysis").is_none());
    let _: TransactionBalances = serde_json::from_value(value).unwrap();
    assert_eq!(serde_json::to_value(w.view().unwrap()).unwrap(), before);
    for (evidence, original) in view.evidence.iter().zip(originals) {
        assert_eq!(
            fs::read(w.root.join("originals").join(&evidence.id)).unwrap(),
            original
        );
    }
    assert_eq!(
        w.inspect_report_snapshot(&view.reports[0].id, &view.reports[0].sha256)
            .unwrap()
            .html,
        view.reports[0].html
    );
}
#[test]
fn concurrent_correction_keeps_old_snapshot_then_invalidates_old_revision_and_version() {
    use rusqlite::hooks::{AuthAction, AuthContext, Authorization};
    use std::sync::{Arc, Mutex};
    let (_temp, w) = workspace();
    let rows = w.view().unwrap().transactions;
    let selected = request(&rows[4..5]);
    let revision = w.revision().unwrap();
    let mut writer = Workspace::open(&w.root).unwrap();
    w.conn.pragma_update(None, "journal_mode", "WAL").unwrap();
    let changed = Arc::new(Mutex::new(false));
    let observed = changed.clone();
    let key = rows[4].id.clone();
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
                    .correct_transaction(&key, "1.00", "Synthetic concurrent correction", revision)
                    .unwrap();
                *done = true;
            }
        }
        Authorization::Allow
    }));
    let result = w.read_transaction_balances(&selected, revision).unwrap();
    assert!(*changed.lock().unwrap());
    assert_eq!(result.workspace_revision, revision);
    assert!(matches!(
        result.rows[0].balance,
        TransactionBalanceState::Checked {
            reconciled: true,
            ..
        }
    ));
    assert_eq!(w.revision().unwrap(), revision + 1);
    assert!(w.read_transaction_balances(&selected, revision).is_err());
    assert!(w
        .read_transaction_balances(&selected, revision + 1)
        .is_err());
    let current = w.view().unwrap().transactions;
    let refreshed = w
        .read_transaction_balances(&request(&current[4..5]), revision + 1)
        .unwrap();
    assert!(
        matches!(&refreshed.rows[0].balance, TransactionBalanceState::Checked { difference, reconciled: false, .. } if difference == "-1.00000000")
    );
    assert!(w.conn.is_autocommit());
}
