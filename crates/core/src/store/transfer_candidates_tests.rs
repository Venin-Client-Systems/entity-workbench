use super::*;
use crate::{domain::Transaction, transaction_page::TransactionPageOrder};

fn workspace() -> (tempfile::TempDir, Workspace, TransferCandidatesRequest) {
    let temp = tempfile::tempdir_in(std::env::temp_dir().canonicalize().unwrap()).unwrap();
    let mut w = Workspace::open(temp.path().join("case")).unwrap();
    w.import("candidates.csv", b"account,date,description,amount,currency\n0001,2025-01-02,Target,-10.00,AUD\n0002,2025-01-01,Repeated,10.00,AUD\n0002,2025-01-01,Repeated,10.00,AUD\n0003,2025-01-03,Other currency,99.00,USD\n0003,2025-01-03,Already paired,5.00,AUD\n0004,2025-01-03,Already paired,-5.00,AUD\n0001,2025-01-01,Same account,10.00,AUD\n0002,2025-01-04,Pending,1.00,AUD\n0003,2025-01-04,Rejected,2.00,AUD\n0004,2025-01-04,Deferred,3.00,AUD\n").unwrap();
    let rows = w.view().unwrap().transactions;
    for (row, state) in rows.iter().zip([
        ReviewState::Pending,
        ReviewState::Accepted,
        ReviewState::Accepted,
        ReviewState::Accepted,
        ReviewState::Accepted,
        ReviewState::Accepted,
        ReviewState::Accepted,
        ReviewState::Pending,
        ReviewState::Rejected,
        ReviewState::Deferred,
    ]) {
        w.review_transaction(
            &row.id,
            state,
            "Synthetic candidate review",
            w.revision().unwrap(),
        )
        .unwrap();
    }
    w.match_transfer(
        &rows[4].id,
        &rows[5].id,
        "Synthetic existing pair",
        w.revision().unwrap(),
    )
    .unwrap();
    let target = &w.view().unwrap().transactions[0];
    let request = TransferCandidatesRequest {
        target_id: target.id.clone(),
        expected_target_version: target.version,
        query: String::new(),
        filter: Default::default(),
        order: TransactionPageOrder::DateAscending,
        page_size: 2,
        cursor: None,
    };
    (temp, w, request)
}
fn collect(w: &Workspace, request: &TransferCandidatesRequest) -> Vec<Transaction> {
    let mut request = request.clone();
    let mut result = Vec::new();
    for _ in 0..20 {
        let page = w
            .page_transfer_candidates(&request, w.revision().unwrap())
            .unwrap();
        result.extend(page.page.rows);
        request.cursor = page.page.next_cursor;
        if request.cursor.is_none() {
            return result;
        }
    }
    panic!("Candidate pagination did not exhaust synthetic scope");
}
#[test]
fn broad_candidates_preserve_repeats_other_currencies_and_existing_pairs_with_stable_pages() {
    let (_temp, w, mut request) = workspace();
    let original = serde_json::to_value(w.view().unwrap()).unwrap();
    for order in [
        TransactionPageOrder::DateAscending,
        TransactionPageOrder::DateDescending,
    ] {
        request.order = order;
        let view = w.view().unwrap();
        let target_account = view.transactions[0].account.clone();
        // Exact old selector predicate, independently applied to the canonical fixture.
        let mut expected: Vec<_> = view
            .transactions
            .into_iter()
            .filter(|row| {
                row.id != request.target_id
                    && row.account != target_account
                    && row.review == ReviewState::Accepted
            })
            .collect();
        expected.sort_by(|a, b| match order {
            TransactionPageOrder::DateAscending => a.date.cmp(&b.date),
            TransactionPageOrder::DateDescending => b.date.cmp(&a.date),
        });
        for size in [1, 2, 3, 5, 200] {
            request.page_size = size;
            let actual = collect(&w, &request);
            assert_eq!(
                serde_json::to_value(&actual).unwrap(),
                serde_json::to_value(&expected).unwrap()
            );
            assert_eq!(actual.len(), 5);
            assert_eq!(
                actual
                    .iter()
                    .filter(|row| row.description == "Repeated")
                    .count(),
                2
            );
            assert!(actual.iter().any(|row| row.currency == "USD"));
            assert_eq!(
                actual
                    .iter()
                    .filter(|row| row.transfer_peer.is_some())
                    .count(),
                2
            );
            let page = w
                .page_transfer_candidates(&request, w.revision().unwrap())
                .unwrap();
            assert_eq!(page.page.scope_count, 8);
            assert_eq!(page.page.selected_count, 5);
            assert_eq!(
                page.page.review_counts,
                crate::transaction_page::TransactionReviewCounts {
                    accepted: 5,
                    pending: 1,
                    rejected: 1,
                    deferred: 1,
                }
            );
        }
    }
    assert_eq!(serde_json::to_value(w.view().unwrap()).unwrap(), original);
}
#[test]
fn exact_filters_and_literal_query_preserve_denominators_and_empty_scope() {
    let (_temp, w, mut request) = workspace();
    for (query, expected) in [
        ("repeated", 2),
        ("REPEATED 0002 2025-01-01", 2),
        ("%", 0),
        ("' OR 1=1 --", 0),
    ] {
        request.query = query.into();
        assert_eq!(collect(&w, &request).len(), expected);
    }
    request.query.clear();
    request.filter.currency = Some("USD".into());
    let page = w
        .page_transfer_candidates(&request, w.revision().unwrap())
        .unwrap();
    assert_eq!((page.page.scope_count, page.page.selected_count), (1, 1));
    request.filter.currency = Some("AUD".into());
    request.filter.account = Some("0002".into());
    request.filter.date_from = Some("2025-01-04".into());
    request.filter.date_to = Some("2025-01-04".into());
    let pending = w
        .page_transfer_candidates(&request, w.revision().unwrap())
        .unwrap();
    assert_eq!(
        (pending.page.scope_count, pending.page.selected_count),
        (1, 0)
    );
    assert_eq!(pending.page.review_counts.pending, 1);
    request.filter.account = Some("2".into());
    let empty = w
        .page_transfer_candidates(&request, w.revision().unwrap())
        .unwrap();
    assert_eq!((empty.page.scope_count, empty.page.selected_count), (0, 0));
}
#[test]
fn stale_target_missing_rows_and_cross_query_cursor_reuse_are_rejected() {
    let (_temp, mut w, request) = workspace();
    let revision = w.revision().unwrap();
    let first = w.page_transfer_candidates(&request, revision).unwrap();
    let cursor = first.page.next_cursor.unwrap();
    for change in 0..6 {
        let mut altered = request.clone();
        altered.cursor = Some(cursor.clone());
        match change {
            0 => altered.query = "Repeated".into(),
            1 => altered.page_size += 1,
            2 => altered.order = TransactionPageOrder::DateDescending,
            3 => altered.filter.currency = Some("USD".into()),
            4 => {
                let row = w.view().unwrap().transactions.remove(6);
                altered.target_id = row.id;
                altered.expected_target_version = row.version;
            }
            _ => altered.expected_target_version += 1,
        }
        assert!(
            w.page_transfer_candidates(&altered, revision).is_err(),
            "change {change}"
        );
        assert!(w.conn.is_autocommit());
    }
    let mut ordinary = request.page_request();
    ordinary.cursor = Some(cursor);
    assert!(w.page_transactions(&ordinary, revision).is_err());
    w.correct_transaction(
        &request.target_id,
        "-11.00",
        "Synthetic target correction",
        revision,
    )
    .unwrap();
    assert!(matches!(
        w.page_transfer_candidates(&request, revision),
        Err(Error::Conflict(_))
    ));
    assert!(matches!(
        w.page_transfer_candidates(&request, revision + 1),
        Err(Error::Conflict(_))
    ));
    let mut fresh = request.clone();
    fresh.expected_target_version += 1;
    assert_eq!(collect(&w, &fresh).len(), 5);
    fresh.target_id = "missing".into();
    assert!(w.page_transfer_candidates(&fresh, revision + 1).is_err());
}

#[test]
fn target_review_and_existing_match_do_not_silently_narrow_the_legacy_selector() {
    let (_temp, mut w, mut request) = workspace();
    for state in [
        ReviewState::Rejected,
        ReviewState::Deferred,
        ReviewState::Accepted,
    ] {
        w.review_transaction(
            &request.target_id,
            state,
            "Synthetic target state",
            w.revision().unwrap(),
        )
        .unwrap();
        request.expected_target_version += 1;
        assert_eq!(collect(&w, &request).len(), 5);
    }
    let counterpart = w.view().unwrap().transactions[1].id.clone();
    w.match_transfer(
        &request.target_id,
        &counterpart,
        "Synthetic target pair",
        w.revision().unwrap(),
    )
    .unwrap();
    request.expected_target_version += 1;
    let candidates = collect(&w, &request);
    assert_eq!(candidates.len(), 5);
    assert!(candidates.iter().any(
        |row| row.id == counterpart && row.transfer_peer.as_deref() == Some(&request.target_id)
    ));
    // Listing a row is never authorization or a promise that matching will pass.
    assert!(w
        .match_transfer(
            &request.target_id,
            &candidates[2].id,
            "Synthetic invalid match",
            w.revision().unwrap()
        )
        .is_err());
}
#[test]
fn target_and_returned_candidate_identity_and_original_corruption_fail_explicitly() {
    let (_temp, mut w, mut request) = workspace();
    let target = w.view().unwrap().transactions[0].clone();
    request.filter.account = Some("missing".into());
    let path = w.root.join("originals").join(target.anchor.evidence_id());
    let moved = w.root.join("temporary-original");
    fs::rename(&path, &moved).unwrap();
    assert!(
        w.page_transfer_candidates(&request, w.revision().unwrap())
            .is_err(),
        "empty result hid missing target original"
    );
    fs::rename(&moved, &path).unwrap();
    assert_eq!(collect(&w, &request).len(), 0);
    for (field, value) in [
        ("id", json!("different")),
        ("amount", json!("NaN")),
        ("version", json!(0)),
    ] {
        let mut body = serde_json::to_value(&target).unwrap();
        body[field] = value;
        w.conn
            .execute(
                "UPDATE records SET body=? WHERE kind='transaction' AND id=?",
                params![body.to_string(), target.id],
            )
            .unwrap();
        assert!(
            w.page_transfer_candidates(&request, w.revision().unwrap())
                .is_err(),
            "target {field}"
        );
    }
    put(&w.conn, "transaction", &target.id, &target).unwrap();
    let source=w.import("separate-candidate.csv",b"account,date,description,amount,currency\n0099,2025-01-01,Separate candidate,1.00,NZD\n").unwrap();
    let candidate = w.view().unwrap().transactions.pop().unwrap();
    w.review_transaction(
        &candidate.id,
        ReviewState::Accepted,
        "Synthetic separate candidate",
        w.revision().unwrap(),
    )
    .unwrap();
    let candidate: Transaction = get(&w.conn, "transaction", &candidate.id).unwrap();
    request.filter.account = Some("0099".into());
    assert_eq!(collect(&w, &request).len(), 1);
    for (field, value) in [
        ("id", json!("wrong-key")),
        ("amount", json!("NaN")),
        ("version", json!(0)),
        ("description", json!(null)),
    ] {
        let mut body = serde_json::to_value(&candidate).unwrap();
        body[field] = value;
        w.conn
            .execute(
                "UPDATE records SET body=? WHERE kind='transaction' AND id=?",
                params![body.to_string(), candidate.id],
            )
            .unwrap();
        assert!(
            w.page_transfer_candidates(&request, w.revision().unwrap())
                .is_err(),
            "candidate {field}"
        );
    }
    put(&w.conn, "transaction", &candidate.id, &candidate).unwrap();
    let path = w.root.join("originals").join(&source);
    fs::rename(&path, &moved).unwrap();
    assert!(
        w.page_transfer_candidates(&request, w.revision().unwrap())
            .is_err(),
        "missing distinct candidate original passed"
    );
    fs::rename(&moved, &path).unwrap();
    let original: Evidence = get(&w.conn, "evidence", &source).unwrap();
    let other: Evidence = get(&w.conn, "evidence", target.anchor.evidence_id()).unwrap();
    let mut retargeted = original.clone();
    retargeted.sha256 = other.sha256;
    retargeted.bytes = other.bytes;
    put(&w.conn, "evidence", &source, &retargeted).unwrap();
    assert!(
        w.page_transfer_candidates(&request, w.revision().unwrap())
            .is_err(),
        "candidate original was retargeted"
    );
    put(&w.conn, "evidence", &source, &original).unwrap();
    assert_eq!(collect(&w, &request).len(), 1);
    assert!(w.conn.is_autocommit());
}
#[test]
fn byte_shortened_pages_never_skip_rows_and_oversized_target_or_candidate_fail() {
    let (_temp, w, mut request) = workspace();
    let rows = w.view().unwrap().transactions;
    for original in &rows[1..3] {
        let mut row = original.clone();
        row.description = "S".repeat(1_100_000);
        put(&w.conn, "transaction", &row.id, &row).unwrap();
    }
    request.page_size = 200;
    let first = w
        .page_transfer_candidates(&request, w.revision().unwrap())
        .unwrap();
    assert_eq!(first.page.rows.len(), 1);
    assert!(first.page.next_cursor.is_some());
    let actual = collect(&w, &request);
    let expected: Vec<_> = rows[1..6].iter().map(|row| row.id.as_str()).collect();
    assert_eq!(
        actual.iter().map(|row| row.id.as_str()).collect::<Vec<_>>(),
        expected
    );
    let mut huge = rows[1].clone();
    huge.description = "S".repeat(crate::transaction_page::MAX_PAGE_BODY_BYTES);
    put(&w.conn, "transaction", &huge.id, &huge).unwrap();
    assert!(w
        .page_transfer_candidates(&request, w.revision().unwrap())
        .is_err());
    put(&w.conn, "transaction", &rows[1].id, &rows[1]).unwrap();
    let mut target = rows[0].clone();
    target.description = "S".repeat(crate::transaction_page::MAX_PAGE_BODY_BYTES);
    put(&w.conn, "transaction", &target.id, &target).unwrap();
    request.filter.account = Some("missing".into());
    assert!(w
        .page_transfer_candidates(&request, w.revision().unwrap())
        .is_err());
    assert!(w.conn.is_autocommit());
}
#[test]
fn request_bounds_and_unknown_fields_reject_before_canonical_work() {
    let (_temp, w, good) = workspace();
    let revision = w.revision().unwrap();
    for mode in 0..12 {
        let mut request = good.clone();
        match mode {
            0 => request.target_id.clear(),
            1 => request.target_id = "x".repeat(257),
            2 => request.target_id = "bad\0id".into(),
            3 => request.expected_target_version = 0,
            4 => request.page_size = 0,
            5 => request.page_size = 201,
            6 => request.cursor = Some("x".repeat(2049)),
            7 => request.query = "x".repeat(1025),
            8 => request.filter.account = Some("x".repeat(4001)),
            9 => request.filter.currency = Some("aud".into()),
            10 => request.filter.date_from = Some("2025-02-30".into()),
            _ => request.filter.account = Some("bad\taccount".into()),
        }
        assert!(
            w.page_transfer_candidates(&request, revision).is_err(),
            "mode {mode}"
        );
        assert!(w.conn.is_autocommit());
    }
    let mut json = serde_json::to_value(&good).unwrap();
    json["filter"]["review"] = json!("pending");
    assert!(serde_json::from_value::<TransferCandidatesRequest>(json).is_err());
    assert_eq!(w.revision().unwrap(), revision);
}
#[test]
fn unicode_query_scope_and_equivalent_casing_share_existing_matcher_semantics() {
    let (_temp, mut w, mut request) = workspace();
    let source=w.import("unicode.csv","account,date,description,amount,currency\nunicode,2025-01-01,ΟΣ İ CAFÉ Café 𐐀 STRAẞE,1.00,AUD\n".as_bytes()).unwrap();
    let row = w
        .view()
        .unwrap()
        .transactions
        .into_iter()
        .find(|row| row.anchor.evidence_id() == source)
        .unwrap();
    w.review_transaction(
        &row.id,
        ReviewState::Accepted,
        "Synthetic Unicode",
        w.revision().unwrap(),
    )
    .unwrap();
    request.filter.account = Some("unicode".into());
    for (query, count) in [
        ("ος", 1),
        ("οσ", 0),
        ("i\u{307}", 1),
        ("café", 1),
        ("café", 1),
        ("𐐨", 1),
        ("straße", 1),
        ("STRASSE", 0),
        ("%", 0),
    ] {
        request.query = query.into();
        assert_eq!(collect(&w, &request).len(), count, "{query}");
    }
    request.filter.account = None;
    request.query = "repeated".into();
    request.page_size = 1;
    let first = w
        .page_transfer_candidates(&request, w.revision().unwrap())
        .unwrap();
    request.cursor = first.page.next_cursor;
    request.query = "REPEATED".into();
    let continuation = w
        .page_transfer_candidates(&request, w.revision().unwrap())
        .unwrap();
    assert_eq!(continuation.page.query_sha256, first.page.query_sha256);
    assert_eq!(continuation.page.rows.len(), 1);
    // A malformed/oversized nonmatching field within literal-search scope fails,
    // including a pending row before accepted selection. Same-account rows are outside scope.
    let pending = w.view().unwrap().transactions[7].clone();
    let mut corrupt = pending.clone();
    corrupt.description = "X".repeat(4001);
    put(&w.conn, "transaction", &corrupt.id, &corrupt).unwrap();
    request.cursor = None;
    request.query = "will not match".into();
    assert!(w
        .page_transfer_candidates(&request, w.revision().unwrap())
        .is_err());
    put(&w.conn, "transaction", &pending.id, &pending).unwrap();
    let same = w.view().unwrap().transactions[6].clone();
    let mut excluded = same;
    excluded.description = "X".repeat(4001);
    put(&w.conn, "transaction", &excluded.id, &excluded).unwrap();
    assert_eq!(collect(&w, &request).len(), 0);
}
#[test]
fn real_concurrent_candidate_correction_keeps_target_counts_and_rows_in_one_snapshot() {
    use rusqlite::hooks::{AuthAction, AuthContext, Authorization};
    use std::sync::{Arc, Mutex};
    let (_temp, w, request) = workspace();
    let revision = w.revision().unwrap();
    let candidate = w.view().unwrap().transactions[1].clone();
    let mut writer = Workspace::open(&w.root).unwrap();
    w.conn.pragma_update(None, "journal_mode", "WAL").unwrap();
    let state = Arc::new(Mutex::new(false));
    let observed = state.clone();
    w.conn.authorizer(Some(move |ctx: AuthContext<'_>| {
        if matches!(
            ctx.action,
            AuthAction::Read {
                table_name: "records",
                ..
            }
        ) {
            let mut done = observed.lock().unwrap();
            if !*done {
                writer
                    .correct_transaction(
                        &candidate.id,
                        "11.00",
                        "Concurrent synthetic candidate correction",
                        revision,
                    )
                    .unwrap();
                *done = true;
            }
        }
        Authorization::Allow
    }));
    let old = w.page_transfer_candidates(&request, revision).unwrap();
    assert!(*state.lock().unwrap());
    assert_eq!(old.page.workspace_revision, revision);
    assert_eq!(old.target_version, request.expected_target_version);
    assert_eq!(old.page.selected_count, 5);
    assert_eq!(old.page.review_counts.pending, 1);
    assert_eq!(old.page.rows[0].amount, "10.00");
    assert!(w.page_transfer_candidates(&request, revision).is_err());
    let current = w.page_transfer_candidates(&request, revision + 1).unwrap();
    assert_eq!(current.page.selected_count, 4);
    assert_eq!(current.page.review_counts.pending, 2);
    assert!(w.conn.is_autocommit());
}
#[test]
fn direct_dispatch_and_existing_page_search_schemas_remain_independent() {
    let (_temp, mut w, request) = workspace();
    let revision = w.revision().unwrap();
    let ledger = request.page_request();
    let search = crate::transaction_search::TransactionSearchRequest {
        query: "Repeated".into(),
        page: ledger.clone(),
    };
    let before_page =
        serde_json::to_value(w.page_transactions(&ledger, revision).unwrap()).unwrap();
    let before_search =
        serde_json::to_value(w.search_transactions(&search, revision).unwrap()).unwrap();
    let command = Command::PageTransferCandidates {
        request,
        expected_revision: revision,
    };
    let result = w.dispatch(command.clone()).unwrap();
    assert_eq!(result, w.dispatch_presentation(command.clone()).unwrap());
    assert_eq!(result, w.dispatch_summary(command).unwrap());
    assert!(result.get("workspace").is_none());
    assert_eq!(
        serde_json::to_value(w.page_transactions(&ledger, revision).unwrap()).unwrap(),
        before_page
    );
    assert_eq!(
        serde_json::to_value(w.search_transactions(&search, revision).unwrap()).unwrap(),
        before_search
    );
    assert_eq!(w.revision().unwrap(), revision);
}

#[test]
fn malformed_account_cannot_silently_disappear_from_other_account_scope() {
    let (_temp, w, mut request) = workspace();
    let candidate = w.view().unwrap().transactions[1].clone();
    request.page_size = 1;
    for value in [
        json!(null),
        json!(1),
        json!(["0001"]),
        json!({"account":"0001"}),
    ] {
        let mut body = serde_json::to_value(&candidate).unwrap();
        body["account"] = value;
        w.conn
            .execute(
                "UPDATE records SET body=? WHERE kind='transaction' AND id=?",
                params![body.to_string(), candidate.id],
            )
            .unwrap();
        for query in ["", "not a match"] {
            request.query = query.into();
            assert!(w
                .page_transfer_candidates(&request, w.revision().unwrap())
                .is_err());
            assert!(w.conn.is_autocommit());
        }
    }
}
