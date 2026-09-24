use super::*;
use std::collections::BTreeMap;

fn workspace() -> (tempfile::TempDir, Workspace) {
    let temp = tempfile::tempdir_in(std::env::temp_dir().canonicalize().unwrap()).unwrap();
    let mut w = Workspace::open(temp.path().join("case")).unwrap();
    w.import("synthetic.csv", b"account,date,description,amount,currency\n0001,2025-01-03,First,-0.10,AUD\n0001,2025-01-01,Second,12.00,AUD\n0002,2025-01-03,Third,-12.00,USD\n").unwrap();
    (temp, w)
}
fn request(facet: TransactionFacetKind, size: u32) -> TransactionFacetRequest {
    TransactionFacetRequest {
        facet,
        page_size: size,
        cursor: None,
    }
}
fn collect(w: &Workspace, facet: TransactionFacetKind, size: u32) -> Vec<TransactionFacetValue> {
    let revision = w.revision().unwrap();
    let mut req = request(facet, size);
    let mut result = Vec::new();
    loop {
        let page = w.page_transaction_facets(&req, revision).unwrap();
        result.extend(page.values);
        req.cursor = page.next_cursor;
        if req.cursor.is_none() {
            break;
        }
        assert!(result.len() < 200);
    }
    result
}

#[test]
fn facets_cover_the_whole_ledger_in_binary_order_without_normalizing_identifiers() {
    let (_temp, mut w) = workspace();
    let mut csv = "account,date,description,amount,currency\n".to_owned();
    for account in [
        "00001",
        "Account",
        "account",
        "é",
        "e\u{301}",
        "' OR 1=1 --",
        "%_",
        "中",
    ] {
        csv.push_str(&format!(
            "{account},2025-02-01,Facet synthetic row,-0.00000001,GBP\n"
        ));
    }
    w.import("synthetic-more.csv", csv.as_bytes()).unwrap();
    let row = w.view().unwrap().transactions[0].id.clone();
    w.review_transaction(
        &row,
        ReviewState::Rejected,
        "Synthetic selector test",
        w.revision().unwrap(),
    )
    .unwrap();
    let mut oracle = BTreeMap::<String, u64>::new();
    let transactions = w.view().unwrap().transactions;
    for row in &transactions {
        *oracle.entry(row.account.clone()).or_default() += 1;
    }
    let expected: Vec<_> = oracle
        .into_iter()
        .map(|(value, transaction_count)| TransactionFacetValue {
            value,
            transaction_count,
        })
        .collect();
    for size in [1, 2, 7, 100] {
        assert_eq!(collect(&w, TransactionFacetKind::Account, size), expected);
    }
    let page = w
        .page_transaction_facets(
            &request(TransactionFacetKind::Account, 2),
            w.revision().unwrap(),
        )
        .unwrap();
    assert_eq!(page.transaction_count, transactions.len() as u64);
    assert_eq!(page.distinct_count, expected.len() as u64);
    let currencies = collect(&w, TransactionFacetKind::Currency, 1);
    assert_eq!(
        currencies,
        vec![
            TransactionFacetValue {
                value: "AUD".into(),
                transaction_count: 2
            },
            TransactionFacetValue {
                value: "GBP".into(),
                transaction_count: 8
            },
            TransactionFacetValue {
                value: "USD".into(),
                transaction_count: 1
            },
        ]
    );
}

#[test]
fn cursors_reject_changed_revision_dimension_size_position_and_hostile_input() {
    let (_temp, mut w) = workspace();
    let revision = w.revision().unwrap();
    let mut req = request(TransactionFacetKind::Account, 1);
    let first = w.page_transaction_facets(&req, revision).unwrap();
    req.cursor = first.next_cursor;
    let good = req.clone();
    req.facet = TransactionFacetKind::Currency;
    assert!(w.page_transaction_facets(&req, revision).is_err());
    req = good.clone();
    req.page_size = 2;
    assert!(w.page_transaction_facets(&req, revision).is_err());
    for cursor in [
        "x".to_owned(),
        "a".repeat(MAX_FACET_CURSOR_BYTES + 1),
        "".into(),
        "FF".into(),
    ] {
        req = good.clone();
        req.cursor = Some(cursor);
        assert!(w.page_transaction_facets(&req, revision).is_err());
    }
    let mut cursor = Cursor::decode(good.cursor.as_ref().unwrap(), &first.query_sha256).unwrap();
    // The second 0001 row is not the representative of the distinct value.
    cursor.sequence = w.conn.query_row("SELECT max(sequence) FROM records WHERE kind='transaction' AND json_extract(body,'$.account')='0001'", [], |r| r.get(0)).unwrap();
    req = good.clone();
    req.cursor = Some(cursor.encode().unwrap());
    assert!(w.page_transaction_facets(&req, revision).is_err());
    cursor.sequence = i64::MAX;
    req.cursor = Some(cursor.encode().unwrap());
    assert!(w.page_transaction_facets(&req, revision).is_err());
    for size in [0, 101] {
        assert!(request(TransactionFacetKind::Account, size)
            .validate()
            .is_err());
    }
    let mut json = serde_json::to_value(&good).unwrap();
    json["sql"] = json!("DROP TABLE records");
    assert!(serde_json::from_value::<TransactionFacetRequest>(json).is_err());
    let mut json = serde_json::to_value(&good).unwrap();
    json["facet"] = json!("$.description");
    assert!(serde_json::from_value::<TransactionFacetRequest>(json).is_err());
    w.import(
        "later.csv",
        b"account,date,description,amount,currency\n0003,2025-02-01,Later,1.00,AUD\n",
    )
    .unwrap();
    assert!(matches!(
        w.page_transaction_facets(&good, revision),
        Err(Error::Conflict(_))
    ));
    assert!(w.page_transaction_facets(&good, revision + 1).is_err());
    assert!(w.conn.is_autocommit());
}

#[test]
fn invalid_or_oversized_facet_values_fail_instead_of_disappearing_from_counts() {
    let (_temp, mut canonical) = workspace();
    canonical.import("synthetic-control-account.csv", b"account,date,description,amount,currency\n\"00\t01\",2025-01-01,Legacy account control,1.00,AUD\n").unwrap();
    let controlled = canonical
        .view()
        .unwrap()
        .transactions
        .into_iter()
        .find(|row| row.account.contains('\t'))
        .unwrap();
    assert_eq!(controlled.account, "00\t01");
    assert!(canonical
        .page_transaction_facets(
            &request(TransactionFacetKind::Account, 100),
            canonical.revision().unwrap()
        )
        .unwrap_err()
        .to_string()
        .contains("unsupported by ledger filters"));
    for bad in [
        json!(null),
        json!(12),
        json!([]),
        json!(""),
        json!("x".repeat(4001)),
        json!("\u{2003}"),
    ] {
        let (_temp, w) = workspace();
        w.conn.execute("UPDATE records SET body=json_set(body,'$.account',json(?)) WHERE kind='transaction'",
            [serde_json::to_string(&bad).unwrap()]).unwrap();
        assert!(w
            .page_transaction_facets(
                &request(TransactionFacetKind::Account, 100),
                w.revision().unwrap()
            )
            .is_err());
        assert!(w.conn.is_autocommit());
    }
    for bad in ["aud", "AU", "AUDD", "A1D"] {
        let (_temp, w) = workspace();
        w.conn
            .execute(
                "UPDATE records SET body=json_set(body,'$.currency',?) WHERE kind='transaction'",
                [bad],
            )
            .unwrap();
        assert!(w
            .page_transaction_facets(
                &request(TransactionFacetKind::Currency, 100),
                w.revision().unwrap()
            )
            .is_err());
    }
}

#[test]
fn full_page_budget_is_preflighted_and_large_nonfacet_bodies_are_not_decoded() {
    let (_temp, mut w) = workspace();
    let mut csv = "account,date,description,amount,currency\n".to_owned();
    for i in 0..70 {
        csv.push_str(&format!(
            "{:04},2025-01-01,Synthetic long account,1.00,AUD\n",
            i + 10
        ));
    }
    w.import("long-accounts.csv", csv.as_bytes()).unwrap();
    // Current import caps account fields at 300 bytes. Explicitly simulate
    // retained legacy values at the existing analysis-filter limit of 4000.
    w.conn.execute("UPDATE records SET body=json_set(body,'$.account',json_extract(body,'$.account') || ?) WHERE kind='transaction' AND json_extract(body,'$.account')>='0010'", ["x".repeat(3996)]).unwrap();
    let revision = w.revision().unwrap();
    let error = w
        .page_transaction_facets(&request(TransactionFacetKind::Account, 100), revision)
        .unwrap_err();
    assert!(error.to_string().contains("256 KiB"));
    assert_eq!(collect(&w, TransactionFacetKind::Account, 50).len(), 72);
    // Selector reads deliberately project only metadata, without claiming source integrity.
    w.conn.execute("UPDATE records SET body=json_set(body,'$.description',?, '$.amount','not-money') WHERE sequence=(SELECT min(sequence) FROM records WHERE kind='transaction')", ["x".repeat(3 * 1024 * 1024)]).unwrap();
    let page = w
        .page_transaction_facets(&request(TransactionFacetKind::Currency, 100), revision)
        .unwrap();
    assert_eq!(page.transaction_count, 73);
    assert_eq!(page.values.len(), 2);
}

#[test]
fn empty_and_dispatch_results_preserve_views_and_make_no_canonical_writes() {
    let (temp, mut w) = workspace();
    let before = serde_json::to_value(w.view().unwrap()).unwrap();
    let command = Command::PageTransactionFacets {
        request: request(TransactionFacetKind::Account, 1),
        expected_revision: w.revision().unwrap(),
    };
    let full = w.dispatch(command.clone()).unwrap();
    assert_eq!(full, w.dispatch_presentation(command).unwrap());
    assert!(full.get("workspace").is_none());
    assert_eq!(serde_json::to_value(w.view().unwrap()).unwrap(), before);
    let empty = Workspace::open(temp.path().join("empty")).unwrap();
    let result = empty
        .page_transaction_facets(&request(TransactionFacetKind::Currency, 1), 0)
        .unwrap();
    assert_eq!(result.transaction_count, 0);
    assert_eq!(result.distinct_count, 0);
    assert!(result.values.is_empty() && result.next_cursor.is_none());
}

#[test]
fn counts_values_and_cursor_use_one_snapshot_during_a_real_concurrent_import() {
    use rusqlite::hooks::{AuthAction, AuthContext, Authorization};
    use std::sync::{Arc, Mutex};
    let (_temp, w) = workspace();
    let mut writer = Workspace::open(&w.root).unwrap();
    w.conn.pragma_update(None, "journal_mode", "WAL").unwrap();
    let revision = w.revision().unwrap();
    let changed = Arc::new(Mutex::new(false));
    let observed = changed.clone();
    w.conn.authorizer(Some(move |ctx: AuthContext<'_>| {
        if matches!(ctx.action, AuthAction::Read { table_name: "records", .. }) {
            let mut done = observed.lock().unwrap();
            if !*done {
                writer.import("concurrent.csv", b"account,date,description,amount,currency\n0000,2025-01-01,Concurrent synthetic row,1.00,CAD\n").unwrap();
                *done = true;
            }
        }
        Authorization::Allow
    }));
    let page = w
        .page_transaction_facets(&request(TransactionFacetKind::Account, 1), revision)
        .unwrap();
    assert!(*changed.lock().unwrap());
    assert_eq!(page.workspace_revision, revision);
    assert_eq!(page.transaction_count, 3);
    assert_eq!(page.distinct_count, 2);
    assert_eq!(page.values[0].value, "0001");
    assert_eq!(page.values[0].transaction_count, 2);
    assert!(page.next_cursor.is_some());
    assert_eq!(w.revision().unwrap(), revision + 1);
    let fresh = w
        .page_transaction_facets(&request(TransactionFacetKind::Account, 1), revision + 1)
        .unwrap();
    assert_eq!(fresh.transaction_count, 4);
    assert_eq!(fresh.distinct_count, 3);
    assert_eq!(fresh.values[0].value, "0000");
}
