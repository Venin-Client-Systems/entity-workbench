use super::*;
use crate::transaction_sources::{TransactionSourceKey, TransactionSourcesRequest};

fn workspace() -> (tempfile::TempDir, Workspace, Vec<Transaction>) {
    let temp = tempfile::tempdir_in(std::env::temp_dir().canonicalize().unwrap()).unwrap();
    let mut w = Workspace::open(temp.path().join("case")).unwrap();
    w.import(
        "debit.csv",
        b"account,date,description,amount,currency\n0001,2024-01-31,Synthetic debit,-10.00,AUD\n",
    )
    .unwrap();
    w.import(
        "credit.csv",
        b"account,date,description,amount,currency\n0002,2024-02-01,Synthetic credit,10.00,AUD\n",
    )
    .unwrap();
    let rows = all::<Transaction>(&w.conn, "transaction").unwrap();
    for row in &rows {
        w.review_transaction(
            &row.id,
            ReviewState::Accepted,
            "Synthetic reviewed transfer",
            w.revision().unwrap(),
        )
        .unwrap();
    }
    w.match_transfer(
        &rows[0].id,
        &rows[1].id,
        "Synthetic exact transfer",
        w.revision().unwrap(),
    )
    .unwrap();
    let rows = all::<Transaction>(&w.conn, "transaction").unwrap();
    (temp, w, rows)
}
fn raw(w: &Workspace) -> Vec<(String, String, String)> {
    let mut s = w
        .conn
        .prepare("SELECT kind,id,body FROM records ORDER BY sequence")
        .unwrap();
    s.query_map([], |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?)))
        .unwrap()
        .map(|r| r.unwrap())
        .collect()
}

#[test]
fn account_flow_dispatch_is_read_only_and_every_source_drills_through_at_exact_revision() {
    let (_temp, mut w, rows) = workspace();
    let revision = w.revision().unwrap();
    let before = raw(&w);
    let response = w
        .dispatch(Command::AnalyzeAccountFlows {
            request: AccountFlowRequest {
                account: Some("0001".into()),
                ..Default::default()
            },
            expected_revision: revision,
        })
        .unwrap();
    let result: AccountFlows = serde_json::from_value(response).unwrap();
    assert_eq!(
        (
            result.scope_transaction_count,
            result.support_transaction_count
        ),
        (1, 1)
    );
    assert_eq!(
        (result.edges.len(), result.edges[0].amount.as_str()),
        (1, "10.00")
    );
    for source in &result.sources {
        let selected = w
            .read_transaction_sources(
                &TransactionSourcesRequest {
                    rows: vec![TransactionSourceKey {
                        id: source.transaction_id.clone(),
                        expected_version: Some(source.version),
                    }],
                },
                revision,
            )
            .unwrap();
        assert_eq!(
            serde_json::to_value(&source.anchor).unwrap(),
            serde_json::to_value(&selected.rows[0].anchor).unwrap()
        );
        assert!(w
            .inspect_source(&source.anchor)
            .unwrap()
            .quote
            .contains("10.00"));
    }
    assert_eq!(raw(&w), before);
    assert_eq!(w.revision().unwrap(), revision);
    assert_eq!(result.edges[0].pairs[0].debit.version, rows[0].version);
}

#[test]
fn correction_unlinks_pairs_old_revision_refused_and_previous_result_is_unchanged() {
    let (_temp, mut w, rows) = workspace();
    let report_id = w.save_report().unwrap();
    let historical: String = w
        .conn
        .query_row(
            "SELECT body FROM records WHERE kind='report' AND id=?",
            [&report_id],
            |row| row.get(0),
        )
        .unwrap();
    let html = fs::read(w.root.join("exports").join(format!("{report_id}.html"))).unwrap();
    let revision = w.revision().unwrap();
    let prior = w
        .analyze_account_flows(&AccountFlowRequest::default(), revision)
        .unwrap();
    let frozen = serde_json::to_vec(&prior).unwrap();
    w.correct_transaction(&rows[0].id, "-9.00", "Synthetic correction", revision)
        .unwrap();
    assert!(matches!(
        w.analyze_account_flows(&AccountFlowRequest::default(), revision),
        Err(Error::Conflict(_))
    ));
    let current = w
        .analyze_account_flows(&AccountFlowRequest::default(), w.revision().unwrap())
        .unwrap();
    assert!(current.edges.is_empty());
    assert_eq!(
        current
            .sources
            .iter()
            .filter(|r| r.review == ReviewState::Pending)
            .count(),
        1
    );
    assert_eq!(
        current
            .nodes
            .iter()
            .map(|n| n.accepted_unmapped.transaction_ids.len())
            .sum::<usize>(),
        1
    );
    assert_eq!(serde_json::to_vec(&prior).unwrap(), frozen);
    let retained: String = w
        .conn
        .query_row(
            "SELECT body FROM records WHERE kind='report' AND id=?",
            [&report_id],
            |row| row.get(0),
        )
        .unwrap();
    assert_eq!(historical, retained);
    assert_eq!(
        html,
        fs::read(w.root.join("exports").join(format!("{report_id}.html"))).unwrap()
    );
}

#[test]
fn selected_and_out_of_scope_support_original_tampering_fail_without_mutation() {
    for source in [0, 1] {
        let (_temp, w, rows) = workspace();
        let revision = w.revision().unwrap();
        let before = raw(&w);
        let path = w
            .root
            .join("originals")
            .join(rows[source].anchor.evidence_id());
        #[cfg(windows)]
        {
            // This test owns the temporary original; clear only its DOS read-only attribute.
            let mut permissions = fs::metadata(&path).unwrap().permissions();
            #[allow(clippy::permissions_set_readonly_false)]
            permissions.set_readonly(false);
            fs::set_permissions(&path, permissions).unwrap();
        }
        fs::remove_file(&path).unwrap();
        fs::write(&path, b"altered synthetic original").unwrap();
        assert!(w
            .analyze_account_flows(
                &AccountFlowRequest {
                    account: Some("0001".into()),
                    ..Default::default()
                },
                revision
            )
            .is_err());
        assert_eq!(raw(&w), before);
        assert_eq!(w.revision().unwrap(), revision);
    }
}

#[test]
fn valid_other_row_under_wrong_key_and_retargeted_evidence_fail() {
    for mode in 0..3 {
        let (_temp, w, rows) = workspace();
        if mode == 0 {
            w.conn
                .execute(
                    "UPDATE records SET body=? WHERE kind='transaction' AND id=?",
                    params![serde_json::to_string(&rows[1]).unwrap(), rows[0].id],
                )
                .unwrap();
        } else {
            let first = rows[0].anchor.evidence_id();
            let second = get_evidence(&w.conn, rows[1].anchor.evidence_id()).unwrap();
            let mut value = serde_json::to_value(&second).unwrap();
            if mode == 2 {
                value["id"] = serde_json::json!(first);
            }
            w.conn
                .execute(
                    "UPDATE records SET body=? WHERE kind='evidence' AND id=?",
                    params![value.to_string(), first],
                )
                .unwrap();
        }
        assert!(w
            .analyze_account_flows(&AccountFlowRequest::default(), w.revision().unwrap())
            .is_err());
    }
}

#[test]
fn retained_input_preflight_rejects_size_types_and_identity_even_outside_scope() {
    for value in [
        serde_json::json!(0),
        serde_json::json!(1.5),
        serde_json::json!("1"),
        serde_json::json!(4294967296u64),
    ] {
        let (_temp, w, rows) = workspace();
        let mut body = serde_json::to_value(&rows[0]).unwrap();
        body["version"] = value;
        w.conn
            .execute(
                "UPDATE records SET body=? WHERE kind='transaction' AND id=?",
                params![body.to_string(), rows[0].id],
            )
            .unwrap();
        assert!(w
            .analyze_account_flows(
                &AccountFlowRequest {
                    account: Some("absent".into()),
                    ..Default::default()
                },
                w.revision().unwrap()
            )
            .is_err());
    }
    let (_temp, w, rows) = workspace();
    let mut body = serde_json::to_value(&rows[0]).unwrap();
    body["description"] = serde_json::json!("x".repeat(MAX_FLOW_RECORD_BYTES as usize));
    w.conn
        .execute(
            "UPDATE records SET body=? WHERE kind='transaction' AND id=?",
            params![body.to_string(), rows[0].id],
        )
        .unwrap();
    assert!(w
        .analyze_account_flows(
            &AccountFlowRequest {
                account: Some("absent".into()),
                ..Default::default()
            },
            w.revision().unwrap()
        )
        .unwrap_err()
        .to_string()
        .contains("2 MiB"));
}

#[test]
fn complete_ledger_count_and_aggregate_bytes_are_preflighted_before_loading() {
    let (_temp, w, rows) = workspace();
    let mut body = serde_json::to_value(&rows[0]).unwrap();
    body["description"] = serde_json::json!("x".repeat(2_000_000));
    let transaction = w.conn.unchecked_transaction().unwrap();
    for index in 0..34 {
        let id = format!("synthetic-oversize-{index}");
        body["id"] = serde_json::json!(id);
        transaction
            .execute(
                "INSERT INTO records(kind,id,body) VALUES('transaction',?,?)",
                params![id, body.to_string()],
            )
            .unwrap();
    }
    transaction.commit().unwrap();
    assert!(w
        .analyze_account_flows(
            &AccountFlowRequest {
                account: Some("absent".into()),
                ..Default::default()
            },
            w.revision().unwrap()
        )
        .unwrap_err()
        .to_string()
        .contains("64 MiB"));
    // Row-count refusal precedes body parsing even for an empty requested scope.
    w.conn
        .execute("DELETE FROM records WHERE kind='transaction'", [])
        .unwrap();
    w.conn.execute("WITH RECURSIVE n(x) AS (VALUES(1) UNION ALL SELECT x+1 FROM n WHERE x<100001) INSERT INTO records(kind,id,body) SELECT 'transaction',printf('synthetic-%d',x),'{}' FROM n",[]).unwrap();
    assert!(w
        .analyze_account_flows(
            &AccountFlowRequest {
                account: Some("absent".into()),
                ..Default::default()
            },
            w.revision().unwrap()
        )
        .unwrap_err()
        .to_string()
        .contains("100,000"));
}

#[test]
fn concurrent_canonical_correction_cannot_mix_flow_revision_peer_versions_or_totals() {
    use rusqlite::hooks::{AuthAction, AuthContext, Authorization};
    use std::sync::{Arc, Mutex};
    let (_temp, w, rows) = workspace();
    let revision = w.revision().unwrap();
    let request = AccountFlowRequest {
        account: Some("0001".into()),
        ..Default::default()
    };
    let expected =
        serde_json::to_value(w.analyze_account_flows(&request, revision).unwrap()).unwrap();
    let writer = Workspace::open(&w.root).unwrap();
    w.conn.pragma_update(None, "journal_mode", "WAL").unwrap();
    let writer = Arc::new(Mutex::new(Some(writer)));
    let callback = Arc::clone(&writer);
    let key = rows[0].id.clone();
    // Preparing the first records query occurs after revision has pinned the read snapshot.
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
                    .correct_transaction(
                        &key,
                        "-9.00",
                        "Synthetic concurrent flow correction",
                        revision,
                    )
                    .unwrap();
            }
        }
        Authorization::Allow
    }));
    let observed = w.analyze_account_flows(&request, revision).unwrap();
    w.conn
        .authorizer(None::<fn(AuthContext<'_>) -> Authorization>);
    assert!(
        writer.lock().unwrap().is_none(),
        "concurrent canonical writer did not run"
    );
    assert_eq!(serde_json::to_value(observed).unwrap(), expected);
    assert_eq!(w.revision().unwrap(), revision + 1);
    assert!(matches!(
        w.analyze_account_flows(&request, revision),
        Err(Error::Conflict(_))
    ));
    let current = w.analyze_account_flows(&request, revision + 1).unwrap();
    assert!(current.edges.is_empty());
    assert_eq!(current.support_transaction_count, 0);
    assert_eq!(current.sources[0].review, ReviewState::Pending);
}
