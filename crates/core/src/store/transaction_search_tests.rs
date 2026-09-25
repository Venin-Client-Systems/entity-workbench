use super::*;
use crate::transaction_page::*;
use serde::Deserialize;

fn empty_workspace() -> (tempfile::TempDir, Workspace) {
    let temp = tempfile::tempdir_in(std::env::temp_dir().canonicalize().unwrap()).unwrap();
    let workspace = Workspace::open(temp.path().join("case")).unwrap();
    (temp, workspace)
}
fn workspace() -> (tempfile::TempDir, Workspace) {
    let (temp, mut workspace) = empty_workspace();
    workspace.import("synthetic.csv", b"account,date,description,amount,currency\n0001,2025-01-03,Merchant first,-0.10000001,AUD\n0001,2025-01-01,Merchant second,12.00,AUD\n0002,2025-01-03,Merchant third,-12.00,USD\n0001,2025-01-03,Other fourth,0.10000001,AUD\n0002,2025-01-02,Merchant fifth,-1.00,USD\n0001,2025-01-03,Merchant sixth,-12.00,AUD\n").unwrap();
    (temp, workspace)
}
fn request(query: &str, size: u32) -> TransactionSearchRequest {
    TransactionSearchRequest {
        query: query.into(),
        page: TransactionPageRequest {
            page_size: size,
            ..Default::default()
        },
    }
}
fn writable_original(path: &Path) {
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        fs::set_permissions(path, fs::Permissions::from_mode(0o600)).unwrap();
    }
    #[cfg(windows)]
    {
        let mut permissions = fs::metadata(path).unwrap().permissions();
        // Only a disposable, test-owned original is deliberately altered.
        #[allow(clippy::permissions_set_readonly_false)]
        permissions.set_readonly(false);
        fs::set_permissions(path, permissions).unwrap();
    }
}
fn collect(workspace: &Workspace, mut request: TransactionSearchRequest) -> Vec<Transaction> {
    let mut rows = vec![];
    for _ in 0..100 {
        let page = workspace
            .search_transactions(&request, workspace.revision().unwrap())
            .unwrap();
        rows.extend(page.page.rows);
        request.page.cursor = page.page.next_cursor;
        if request.page.cursor.is_none() {
            return rows;
        }
    }
    panic!("Synthetic search did not exhaust");
}

#[derive(Deserialize)]
struct Cases {
    schema_version: u32,
    cases: Vec<Case>,
}
#[derive(Deserialize)]
struct Case {
    name: String,
    description: String,
    account: String,
    date: String,
    query: String,
    matches: bool,
}

#[test]
fn javascript_fixture_cases_match_actual_canonical_search_and_report_unicode_version() {
    let fixture: Cases = serde_json::from_str(include_str!(
        "../../../../fixtures/transactions/literal-search.v1.json"
    ))
    .unwrap();
    assert_eq!(fixture.schema_version, 1);
    assert_eq!(fixture.cases.len(), 32);
    let (_temp, mut workspace) = empty_workspace();
    let mut originals = Vec::new();
    for case in &fixture.cases {
        let mut csv = csv::Writer::from_writer(Vec::new());
        csv.write_record(["account", "date", "description", "amount", "currency"])
            .unwrap();
        csv.write_record([&case.account, &case.date, &case.description, "-1.00", "AUD"])
            .unwrap();
        let original = workspace
            .import("synthetic.csv", &csv.into_inner().unwrap())
            .unwrap();
        originals.push(original);
        assert_eq!(
            matches_lowered(
                &case.description,
                &case.account,
                &case.date,
                &lower_query(&case.query)
            )
            .unwrap(),
            case.matches,
            "{} helper",
            case.name
        );
    }
    for (case, original) in fixture.cases.iter().zip(originals) {
        let result = workspace
            .search_transactions(&request(&case.query, 200), workspace.revision().unwrap())
            .unwrap();
        assert_eq!(result.schema_version, 1);
        assert_eq!(result.matching, LiteralMatching::default());
        assert!(result.page.next_cursor.is_none());
        assert_eq!(
            result
                .page
                .rows
                .iter()
                .any(|row| row.anchor.evidence_id() == original),
            case.matches,
            "{} SQL/canonical",
            case.name
        );
    }
    eprintln!(
        "Search matcher: {}",
        serde_json::to_string(&LiteralMatching::default()).unwrap()
    );
}

#[test]
fn search_scope_precedes_review_selection_and_keeps_exact_values_order_and_ties() {
    let (_temp, mut w) = workspace();
    let original = w.view().unwrap().transactions;
    for (row, state) in original.iter().zip([
        ReviewState::Accepted,
        ReviewState::Pending,
        ReviewState::Rejected,
        ReviewState::Accepted,
        ReviewState::Deferred,
        ReviewState::Pending,
    ]) {
        w.review_transaction(&row.id, state, "Synthetic review", w.revision().unwrap())
            .unwrap();
    }
    let canonical = w.view().unwrap().transactions;
    for order in [
        TransactionPageOrder::DateAscending,
        TransactionPageOrder::DateDescending,
    ] {
        for currency in [None, Some("AUD"), Some("USD"), Some("EUR")] {
            for account in [None, Some("0001"), Some("1")] {
                for review in [
                    None,
                    Some(ReviewState::Accepted),
                    Some(ReviewState::Pending),
                    Some(ReviewState::Rejected),
                    Some(ReviewState::Deferred),
                ] {
                    let mut req = request("merchant", 1);
                    req.page.order = order;
                    req.page.filter = TransactionPageFilter {
                        currency: currency.map(Into::into),
                        account: account.map(Into::into),
                        review: review.clone(),
                        date_from: Some("2025-01-02".into()),
                        date_to: Some("2025-01-03".into()),
                    };
                    let scope: Vec<_> = canonical
                        .iter()
                        .filter(|row| {
                            req.page.filter.scope().includes(row)
                                && row.description.starts_with("Merchant")
                        })
                        .collect();
                    let page = w
                        .search_transactions(&req, w.revision().unwrap())
                        .unwrap()
                        .page;
                    assert_eq!(page.scope_count, scope.len() as u64);
                    assert_eq!(
                        page.review_counts.accepted,
                        scope
                            .iter()
                            .filter(|r| r.review == ReviewState::Accepted)
                            .count() as u64
                    );
                    assert_eq!(
                        page.review_counts.pending,
                        scope
                            .iter()
                            .filter(|r| r.review == ReviewState::Pending)
                            .count() as u64
                    );
                    assert_eq!(
                        page.review_counts.rejected,
                        scope
                            .iter()
                            .filter(|r| r.review == ReviewState::Rejected)
                            .count() as u64
                    );
                    assert_eq!(
                        page.review_counts.deferred,
                        scope
                            .iter()
                            .filter(|r| r.review == ReviewState::Deferred)
                            .count() as u64
                    );
                    let mut expected: Vec<_> = scope
                        .into_iter()
                        .filter(|r| review.as_ref().is_none_or(|state| r.review == *state))
                        .collect();
                    expected.sort_by(|a, b| match order {
                        TransactionPageOrder::DateAscending => a.date.cmp(&b.date),
                        TransactionPageOrder::DateDescending => b.date.cmp(&a.date),
                    });
                    assert_eq!(page.selected_count, expected.len() as u64);
                    assert_eq!(
                        serde_json::to_value(collect(&w, req)).unwrap(),
                        serde_json::to_value(expected).unwrap()
                    );
                }
            }
        }
    }
}

#[test]
fn empty_query_keeps_v12_page_semantics_including_long_legacy_text_but_not_cursor_family() {
    let (_temp, w) = workspace();
    let mut legacy = w.view().unwrap().transactions[0].clone();
    legacy.description = "Legacy ".repeat(1000);
    put(&w.conn, "transaction", &legacy.id, &legacy).unwrap();
    let revision = w.revision().unwrap();
    for order in [
        TransactionPageOrder::DateAscending,
        TransactionPageOrder::DateDescending,
    ] {
        for size in [1, 2, 5, 200] {
            let mut req = request("", size);
            req.page.order = order;
            let mut old = req.page.clone();
            loop {
                let before = w.page_transactions(&old, revision).unwrap();
                let after = w.search_transactions(&req, revision).unwrap().page;
                assert_ne!(before.query_sha256, after.query_sha256);
                let mut before_value = serde_json::to_value(&before).unwrap();
                let mut after_value = serde_json::to_value(&after).unwrap();
                for field in ["query_sha256", "next_cursor"] {
                    before_value.as_object_mut().unwrap().remove(field);
                    after_value.as_object_mut().unwrap().remove(field);
                }
                assert_eq!(before_value, after_value);
                assert_eq!(before.next_cursor.is_some(), after.next_cursor.is_some());
                old.cursor = before.next_cursor;
                req.page.cursor = after.next_cursor;
                if old.cursor.is_none() {
                    break;
                }
                let mut crossed = req.clone();
                crossed.page.cursor = old.cursor.clone();
                assert!(w.search_transactions(&crossed, revision).is_err());
                let mut crossed = old.clone();
                crossed.cursor = req.page.cursor.clone();
                assert!(w.page_transactions(&crossed, revision).is_err());
            }
        }
    }
}

#[test]
fn search_cursors_bind_text_profile_scope_size_and_revision_including_empty_scopes() {
    let (_temp, mut w) = workspace();
    let revision = w.revision().unwrap();
    let initial = request("MERCHANT", 1);
    let first = w.search_transactions(&initial, revision).unwrap().page;
    let mut next = initial.clone();
    next.query = "merchant".into();
    next.page.cursor = first.next_cursor.clone();
    assert!(w.search_transactions(&next, revision).is_ok());
    for mode in 0..5 {
        let mut changed = next.clone();
        match mode {
            0 => changed.query = "Merchant ".into(),
            1 => changed.page.page_size = 2,
            2 => changed.page.order = TransactionPageOrder::DateDescending,
            3 => changed.page.filter.currency = Some("AUD".into()),
            _ => changed.page.filter.review = Some(ReviewState::Pending),
        }
        assert!(w.search_transactions(&changed, revision).is_err());
    }
    let encoded = next.page.cursor.as_ref().unwrap();
    let bytes: Vec<u8> = (0..encoded.len())
        .step_by(2)
        .map(|i| u8::from_str_radix(&encoded[i..i + 2], 16).unwrap())
        .collect();
    let mut cursor: Value = serde_json::from_slice(&bytes).unwrap();
    let mut absent = request("no matching text", 1);
    let empty = w.search_transactions(&absent, revision).unwrap().page;
    assert_eq!(empty.scope_count, 0);
    cursor["query_sha256"] = json!(empty.query_sha256);
    absent.page.cursor = Some(
        serde_json::to_vec(&cursor)
            .unwrap()
            .iter()
            .map(|b| format!("{b:02x}"))
            .collect(),
    );
    assert!(w
        .search_transactions(&absent, revision)
        .unwrap_err()
        .to_string()
        .contains("selected scope"));
    w.correct_transaction(
        &first.rows[0].id,
        "12.00000001",
        "Synthetic correction",
        revision,
    )
    .unwrap();
    assert!(matches!(
        w.search_transactions(&next, revision),
        Err(Error::Conflict(_))
    ));
    assert!(w.search_transactions(&next, revision + 1).is_err());
    assert_eq!(
        w.search_transactions(&initial, revision + 1)
            .unwrap()
            .page
            .rows[0]
            .amount,
        "12.00000001"
    );
    assert!(w.conn.is_autocommit());
}

#[test]
fn malformed_or_oversized_in_scope_fields_fail_even_when_hidden_by_query_or_review() {
    let (_temp, w) = workspace();
    let row = w.view().unwrap().transactions[2].clone();
    let canonical = serde_json::to_value(&row).unwrap();
    let revision = w.revision().unwrap();
    for (field, invalid) in [
        ("description", json!("x".repeat(4001))),
        ("account", json!("x".repeat(4001))),
        ("date", json!("x".repeat(11))),
        ("description", json!(null)),
        ("description", json!(["Merchant"])),
        ("account", json!(123)),
        ("account", json!({"value":"0002"})),
        ("date", json!(["2025-01-01"])),
        ("description", json!("   ")),
        ("date", json!("2025-02-30")),
    ] {
        let mut value = canonical.clone();
        value[field] = invalid;
        w.conn
            .execute(
                "UPDATE records SET body=? WHERE kind='transaction' AND id=?",
                params![value.to_string(), row.id],
            )
            .unwrap();
        for query in ["merchant", "not present"] {
            let mut req = request(query, 1);
            req.page.filter.review = Some(ReviewState::Accepted);
            assert!(
                w.search_transactions(&req, revision).is_err(),
                "{field}: {query}"
            );
            // Malformed text outside the exact base scope is not evaluated.
            req.page.filter.currency = Some("AUD".into());
            assert!(
                w.search_transactions(&req, revision).is_ok(),
                "out of scope {field}"
            );
        }
        assert!(w.conn.is_autocommit());
    }
    put(&w.conn, "transaction", &row.id, &row).unwrap();
    assert_eq!(
        w.search_transactions(&request("merchant", 200), revision)
            .unwrap()
            .page
            .scope_count,
        5
    );
}

#[test]
fn strict_hostile_requests_and_fixed_function_do_not_mutate_workspace() {
    let (_temp, mut w) = workspace();
    let revision = w.revision().unwrap();
    for mode in 0..5 {
        let mut req = request("merchant", 1);
        match mode {
            0 => req.query = "x".repeat(MAX_SEARCH_QUERY_BYTES + 1),
            1 => req.query = "İ".repeat(MAX_SEARCH_QUERY_BYTES / 2 + 1),
            2 => req.page.page_size = 201,
            3 => req.page.cursor = Some("f".repeat(MAX_CURSOR_BYTES + 1)),
            _ => req.page.filter.currency = Some("aud".into()),
        }
        assert!(w.search_transactions(&req, revision).is_err());
    }
    let mut malformed = serde_json::to_value(request("merchant", 1)).unwrap();
    malformed["sql"] = json!("SELECT * FROM records");
    assert!(serde_json::from_value::<TransactionSearchRequest>(malformed).is_err());
    assert_eq!(
        w.search_transactions(&request("' OR 1=1 --", 200), revision)
            .unwrap()
            .page
            .scope_count,
        0
    );
    let before = serde_json::to_value(w.view().unwrap()).unwrap();
    let command = Command::SearchTransactions {
        request: request("merchant", 2),
        expected_revision: revision,
    };
    let full = w.dispatch(command.clone()).unwrap();
    assert_eq!(full, w.dispatch_presentation(command).unwrap());
    assert!(full.get("workspace").is_none());
    assert_eq!(full["page"]["rows"].as_array().unwrap().len(), 2);
    assert_eq!(before, serde_json::to_value(w.view().unwrap()).unwrap());
    // A persisted view cannot invoke the UDF. SQLite treats locally created TEMP
    // views differently; the boundary here is the opened database's schema.
    w.conn.execute_batch("CREATE VIEW synthetic_indirect AS SELECT ew_transaction_text_match_v1('Merchant','0001','2025-01-01','merchant')").unwrap();
    assert!(w.conn.prepare("SELECT * FROM synthetic_indirect").is_err());
}

#[test]
fn matching_rows_preserve_page_body_budget_and_original_integrity_checks() {
    let (_temp, w) = workspace();
    let mut rows = w.view().unwrap().transactions;
    // Large unrelated retained fields exercise the shared 2MiB page budget,
    // without exceeding the separately bounded text used by matching.
    for row in &mut rows {
        row.duplicate_candidates = vec!["synthetic".repeat(100_000)];
        put(&w.conn, "transaction", &row.id, row).unwrap();
    }
    let req = request("merchant", 200);
    let revision = w.revision().unwrap();
    let first = w.search_transactions(&req, revision).unwrap().page;
    assert_eq!(first.rows.len(), 2);
    assert!(first.next_cursor.is_some());
    let all = collect(&w, req.clone());
    assert_eq!(all.len(), 5);
    assert_eq!(
        all.iter()
            .map(|r| &r.id)
            .collect::<std::collections::BTreeSet<_>>()
            .len(),
        5
    );
    let mut oversized = all[0].clone();
    oversized.duplicate_candidates = vec!["x".repeat(MAX_PAGE_BODY_BYTES)];
    put(&w.conn, "transaction", &oversized.id, &oversized).unwrap();
    assert!(w
        .search_transactions(&req, revision)
        .unwrap_err()
        .to_string()
        .contains("single transaction"));
    put(&w.conn, "transaction", &all[0].id, &all[0]).unwrap();
    let path = w.root.join("originals").join(all[0].anchor.evidence_id());
    writable_original(&path);
    fs::write(path, b"synthetic changed original").unwrap();
    assert!(w.search_transactions(&req, revision).is_err());
    assert_eq!(w.revision().unwrap(), revision);
}

#[test]
fn page_and_search_reject_evidence_retargeted_to_another_intact_original() {
    let (_temp, mut w) = workspace();
    let source = w.view().unwrap().transactions[0]
        .anchor
        .evidence_id()
        .to_owned();
    let other = w
        .import(
            "other-synthetic.txt",
            b"Different intact synthetic source.\n",
        )
        .unwrap();
    let a: Evidence = get(&w.conn, "evidence", &source).unwrap();
    let b: Evidence = get(&w.conn, "evidence", &other).unwrap();
    let mut retargeted = a.clone();
    retargeted.sha256 = b.sha256;
    retargeted.bytes = b.bytes;
    put(&w.conn, "evidence", &source, &retargeted).unwrap();
    // A's canonical key/body ID and transaction anchors still identify A. An
    // intact B cannot stand in for A's removed original during row verification.
    let missing = w.root.join("originals").join(&a.sha256);
    writable_original(&missing);
    fs::remove_file(missing).unwrap();
    let revision = w.revision().unwrap();
    for result in [
        w.page_transactions(&TransactionPageRequest::default(), revision),
        w.search_transactions(&request("merchant", 200), revision)
            .map(|r| r.page),
    ] {
        assert!(result
            .unwrap_err()
            .to_string()
            .contains("Canonical transaction source identity"));
    }
    assert!(w.conn.is_autocommit());
    assert_eq!(w.revision().unwrap(), revision);
}

#[test]
fn text_counts_and_exact_rows_share_a_snapshot_during_real_concurrent_import() {
    use rusqlite::hooks::{AuthAction, AuthContext, Authorization};
    use std::sync::{Arc, Mutex};
    let (_temp, w) = workspace();
    let mut writer = Workspace::open(&w.root).unwrap();
    w.conn.pragma_update(None, "journal_mode", "WAL").unwrap();
    let revision = w.revision().unwrap();
    let committed = Arc::new(Mutex::new(false));
    let observed = committed.clone();
    w.conn.authorizer(Some(move |context: AuthContext<'_>| {
        if matches!(context.action, AuthAction::Read { table_name: "records", .. }) {
            let mut done = observed.lock().unwrap();
            if !*done {
                writer.import("concurrent.csv", b"account,date,description,amount,currency\n0001,2025-01-04,Merchant concurrent,1.00,AUD\n").unwrap();
                *done = true;
            }
        }
        Authorization::Allow
    }));
    let req = request("merchant", 200);
    let page = w.search_transactions(&req, revision).unwrap().page;
    assert!(*committed.lock().unwrap());
    assert_eq!(page.workspace_revision, revision);
    assert_eq!(page.scope_count, 5);
    assert_eq!(page.rows.len(), 5);
    assert!(page
        .rows
        .iter()
        .all(|r| r.description != "Merchant concurrent"));
    assert!(matches!(
        w.search_transactions(&req, revision),
        Err(Error::Conflict(_))
    ));
    let fresh = w.search_transactions(&req, revision + 1).unwrap().page;
    assert_eq!(fresh.scope_count, 6);
    assert_eq!(fresh.rows.len(), 6);
    assert!(w.conn.is_autocommit());
}
