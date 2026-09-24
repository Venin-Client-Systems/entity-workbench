use super::*;

fn workspace() -> (tempfile::TempDir, Workspace) {
    let temp = tempfile::tempdir_in(std::env::temp_dir().canonicalize().unwrap()).unwrap();
    let mut workspace = Workspace::open(temp.path().join("case")).unwrap();
    workspace.import("synthetic.csv", b"account,date,description,amount,currency\n0001,2025-01-03,First,-0.10000001,AUD\n0001,2025-01-01,Second,12.00,AUD\n0002,2025-01-03,Third,-12.00,USD\n0001,2025-01-03,Fourth,0.10000001,AUD\n0002,2025-01-02,Fifth,-1.00,USD\n0001,2025-01-03,Sixth,-12.00,AUD\n").unwrap();
    (temp, workspace)
}
fn collect(workspace: &Workspace, request: &TransactionPageRequest) -> Vec<Transaction> {
    let mut request = request.clone();
    let revision = workspace.revision().unwrap();
    let mut result = Vec::new();
    for _ in 0..100 {
        let page = workspace.page_transactions(&request, revision).unwrap();
        assert!(page.rows.len() <= request.page_size as usize);
        result.extend(page.rows);
        request.cursor = page.next_cursor;
        if request.cursor.is_none() {
            return result;
        }
    }
    panic!("Pagination failed to exhaust synthetic scope");
}
#[test]
fn pages_keep_every_tied_row_once_in_both_date_orders_and_preserve_exact_rows() {
    let (_temp, workspace) = workspace();
    let original = workspace.view().unwrap().transactions;
    for order in [
        TransactionPageOrder::DateAscending,
        TransactionPageOrder::DateDescending,
    ] {
        let mut expected = original.clone();
        // Stable sort retains canonical insertion sequence for equal dates.
        expected.sort_by(|a, b| match order {
            TransactionPageOrder::DateAscending => a.date.cmp(&b.date),
            TransactionPageOrder::DateDescending => b.date.cmp(&a.date),
        });
        for page_size in [1, 2, 3, 5, 6, 7, 200] {
            let actual = collect(
                &workspace,
                &TransactionPageRequest {
                    order,
                    page_size,
                    ..Default::default()
                },
            );
            assert_eq!(
                serde_json::to_value(actual).unwrap(),
                serde_json::to_value(&expected).unwrap()
            );
        }
    }
}
#[test]
fn scope_counts_keep_every_review_denominator_and_currency_account_dates_exact() {
    let (_temp, mut workspace) = workspace();
    let rows = workspace.view().unwrap().transactions;
    for (row, state) in rows.iter().zip([
        ReviewState::Accepted,
        ReviewState::Pending,
        ReviewState::Rejected,
        ReviewState::Deferred,
        ReviewState::Accepted,
        ReviewState::Pending,
    ]) {
        workspace
            .review_transaction(
                &row.id,
                state,
                "Synthetic review",
                workspace.revision().unwrap(),
            )
            .unwrap();
    }
    let revision = workspace.revision().unwrap();
    for currency in [None, Some("AUD"), Some("USD"), Some("EUR")] {
        for account in [None, Some("0001"), Some("0002"), Some("1")] {
            for review in [
                None,
                Some(ReviewState::Accepted),
                Some(ReviewState::Pending),
                Some(ReviewState::Rejected),
                Some(ReviewState::Deferred),
            ] {
                let request = TransactionPageRequest {
                    filter: TransactionPageFilter {
                        currency: currency.map(Into::into),
                        account: account.map(Into::into),
                        date_from: Some("2025-01-02".into()),
                        date_to: Some("2025-01-03".into()),
                        review: review.clone(),
                    },
                    page_size: 1,
                    ..Default::default()
                };
                let page = workspace.page_transactions(&request, revision).unwrap();
                let canonical = workspace.view().unwrap().transactions;
                let scope: Vec<_> = canonical
                    .iter()
                    .filter(|row| request.filter.scope().includes(row))
                    .collect();
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
                assert_eq!(
                    page.selected_count,
                    scope
                        .iter()
                        .filter(|r| review.as_ref().is_none_or(|state| r.review == *state))
                        .count() as u64
                );
                assert_eq!(
                    collect(&workspace, &request).len() as u64,
                    page.selected_count
                );
            }
        }
    }
    assert_eq!(workspace.revision().unwrap(), revision);
}
#[test]
fn pending_only_scope_is_distinct_from_empty_scope() {
    let (_temp, workspace) = workspace();
    let request = TransactionPageRequest {
        filter: TransactionPageFilter {
            review: Some(ReviewState::Accepted),
            ..Default::default()
        },
        ..Default::default()
    };
    let page = workspace
        .page_transactions(&request, workspace.revision().unwrap())
        .unwrap();
    assert_eq!(page.scope_count, 6);
    assert_eq!(page.review_counts.pending, 6);
    assert_eq!(page.selected_count, 0);
    assert!(page.rows.is_empty());
    assert!(page.next_cursor.is_none());
    let empty = TransactionPageRequest {
        filter: TransactionPageFilter {
            currency: Some("EUR".into()),
            ..request.filter
        },
        ..request
    };
    assert_eq!(
        workspace
            .page_transactions(&empty, workspace.revision().unwrap())
            .unwrap()
            .scope_count,
        0
    );
}
#[test]
fn cursors_reject_cross_query_reuse_stale_corrections_and_invalid_positions() {
    let (_temp, mut workspace) = workspace();
    let revision = workspace.revision().unwrap();
    let first = TransactionPageRequest {
        page_size: 1,
        ..Default::default()
    };
    let page = workspace.page_transactions(&first, revision).unwrap();
    let continued = TransactionPageRequest {
        cursor: page.next_cursor.clone(),
        ..first.clone()
    };
    for mode in 0..6 {
        let mut changed = continued.clone();
        match mode {
            0 => changed.page_size = 2,
            1 => changed.order = TransactionPageOrder::DateDescending,
            2 => changed.filter.currency = Some("AUD".into()),
            3 => changed.filter.account = Some("0001".into()),
            4 => changed.filter.review = Some(ReviewState::Pending),
            _ => changed.filter.date_from = Some("2025-01-01".into()),
        }
        assert!(workspace.page_transactions(&changed, revision).is_err());
        assert!(workspace.conn.is_autocommit());
    }
    let mut cursor =
        Cursor::decode(continued.cursor.as_ref().unwrap(), &page.query_sha256).unwrap();
    cursor.sequence = i64::MAX;
    assert!(workspace
        .page_transactions(
            &TransactionPageRequest {
                cursor: Some(cursor.encode().unwrap()),
                ..first.clone()
            },
            revision
        )
        .is_err());
    let row = &page.rows[0];
    workspace
        .correct_transaction(&row.id, "12.00000001", "Synthetic correction", revision)
        .unwrap();
    assert!(matches!(
        workspace.page_transactions(&continued, revision),
        Err(Error::Conflict(_))
    ));
    assert!(workspace
        .page_transactions(&continued, workspace.revision().unwrap())
        .is_err());
    let refreshed = workspace
        .page_transactions(&first, workspace.revision().unwrap())
        .unwrap();
    assert_eq!(refreshed.rows[0].amount, "12.00000001");
    assert_eq!(refreshed.rows[0].version, row.version + 1);
}
#[test]
fn byte_budget_shortens_pages_without_skipping_and_oversized_single_rows_fail() {
    let (_temp, mut workspace) = workspace();
    let mut rows = workspace.view().unwrap().transactions;
    for row in &mut rows {
        row.description = "S".repeat(800_000);
    }
    workspace
        .change(None, "synthetic.large_rows", false, |conn| {
            for row in &rows {
                put(conn, "transaction", &row.id, row)?;
            }
            Ok(())
        })
        .unwrap();
    let request = TransactionPageRequest {
        page_size: 200,
        ..Default::default()
    };
    let page = workspace
        .page_transactions(&request, workspace.revision().unwrap())
        .unwrap();
    assert_eq!(page.rows.len(), 2);
    assert!(page.next_cursor.is_some());
    let all = collect(&workspace, &request);
    assert_eq!(all.len(), 6);
    assert_eq!(all.iter().map(|r| &r.id).collect::<BTreeSet<_>>().len(), 6);
    let first = &all[0];
    workspace.conn.execute("UPDATE records SET body=json_set(body,'$.description',?) WHERE kind='transaction' AND id=?",params!["x".repeat(MAX_PAGE_BODY_BYTES+1),first.id]).unwrap();
    assert!(workspace
        .page_transactions(&request, workspace.revision().unwrap())
        .unwrap_err()
        .to_string()
        .contains("single transaction"));
    assert!(workspace.conn.is_autocommit());
}
#[test]
fn hostile_parameters_unknown_fields_and_corrupt_sources_fail_without_writes() {
    let (temp, workspace) = workspace();
    let revision = workspace.revision().unwrap();
    for mode in 0..9 {
        let mut request = TransactionPageRequest::default();
        match mode {
            0 => request.page_size = 0,
            1 => request.page_size = 201,
            2 => request.filter.currency = Some("aud".into()),
            3 => request.filter.date_from = Some("2025-02-30".into()),
            4 => {
                request.filter.date_from = Some("2025-03-01".into());
                request.filter.date_to = Some("2025-01-01".into());
            }
            5 => request.filter.account = Some("bad\0value".into()),
            6 => request.cursor = Some("a".into()),
            7 => request.cursor = Some("f".repeat(MAX_CURSOR_BYTES + 2)),
            _ => request.filter.account = Some("a".repeat(4001)),
        }
        assert!(workspace.page_transactions(&request, revision).is_err());
    }
    let request = TransactionPageRequest {
        filter: TransactionPageFilter {
            account: Some("' OR 1=1 --".into()),
            ..Default::default()
        },
        ..Default::default()
    };
    assert_eq!(
        workspace
            .page_transactions(&request, revision)
            .unwrap()
            .scope_count,
        0
    );
    let mut json = serde_json::to_value(&request).unwrap();
    json["sql"] = json!("DROP TABLE records");
    assert!(serde_json::from_value::<TransactionPageRequest>(json).is_err());
    let source = workspace.view().unwrap().evidence[0].sha256.clone();
    let path = temp.path().join("case/originals").join(source);
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        fs::set_permissions(&path, fs::Permissions::from_mode(0o600)).unwrap();
    }
    fs::write(path, b"corrupt").unwrap();
    assert!(workspace
        .page_transactions(&TransactionPageRequest::default(), revision)
        .is_err());
    assert_eq!(workspace.revision().unwrap(), revision);
    assert!(workspace.conn.is_autocommit());
}

#[test]
fn transaction_page_pins_counts_and_rows_while_another_writer_commits() {
    use rusqlite::hooks::{AuthAction, AuthContext, Authorization};
    use std::sync::{Arc, Mutex};
    let (_temp, workspace) = workspace();
    let mut writer = Workspace::open(&workspace.root).unwrap();
    workspace
        .conn
        .pragma_update(None, "journal_mode", "WAL")
        .unwrap();
    let first = workspace.view().unwrap().transactions[0].id.clone();
    let revision = workspace.revision().unwrap();
    let committed = Arc::new(Mutex::new(false));
    let observation = committed.clone();
    workspace
        .conn
        .authorizer(Some(move |context: AuthContext<'_>| {
            if matches!(
                context.action,
                AuthAction::Read {
                    table_name: "records",
                    ..
                }
            ) {
                let mut changed = observation.lock().unwrap();
                if !*changed {
                    writer
                        .review_transaction(
                            &first,
                            ReviewState::Accepted,
                            "Synthetic concurrent review",
                            revision,
                        )
                        .unwrap();
                    *changed = true;
                }
            }
            Authorization::Allow
        }));
    let page = workspace
        .page_transactions(&TransactionPageRequest::default(), revision)
        .unwrap();
    assert!(*committed.lock().unwrap());
    assert_eq!(page.workspace_revision, revision);
    assert_eq!(page.review_counts.pending, 6);
    assert_eq!(page.review_counts.accepted, 0);
    assert!(page
        .rows
        .iter()
        .all(|row| row.review == ReviewState::Pending));
    assert_eq!(workspace.revision().unwrap(), revision + 1);
    assert!(workspace.conn.is_autocommit());
    let next = workspace
        .page_transactions(&TransactionPageRequest::default(), revision + 1)
        .unwrap();
    assert_eq!(next.review_counts.accepted, 1);
}

#[test]
fn page_dispatch_returns_only_the_page_and_keeps_legacy_views_unchanged() {
    let (_temp, mut workspace) = workspace();
    let request = TransactionPageRequest {
        page_size: 1,
        ..Default::default()
    };
    let revision = workspace.revision().unwrap();
    let command = Command::PageTransactions {
        request,
        expected_revision: revision,
    };
    let full = workspace.dispatch(command.clone()).unwrap();
    let presentation = workspace.dispatch_presentation(command).unwrap();
    assert_eq!(full, presentation);
    assert_eq!(full["rows"].as_array().unwrap().len(), 1);
    assert!(full.get("workspace").is_none());
    assert_eq!(
        workspace.dispatch(Command::View {}).unwrap()["workspace"]["transactions"]
            .as_array()
            .unwrap()
            .len(),
        6
    );
    assert_eq!(workspace.revision().unwrap(), revision);
}

#[test]
fn empty_selection_still_rejects_a_cursor_for_an_excluded_existing_row() {
    let (_temp, workspace) = workspace();
    let revision = workspace.revision().unwrap();
    let row = &workspace.view().unwrap().transactions[0];
    let sequence: i64 = workspace
        .conn
        .query_row(
            "SELECT sequence FROM records WHERE kind='transaction' AND id=?",
            [&row.id],
            |record| record.get(0),
        )
        .unwrap();
    for filter in [
        TransactionPageFilter {
            review: Some(ReviewState::Accepted),
            ..Default::default()
        },
        TransactionPageFilter {
            currency: Some("EUR".into()),
            ..Default::default()
        },
    ] {
        let mut request = TransactionPageRequest {
            filter,
            ..Default::default()
        };
        let page = workspace.page_transactions(&request, revision).unwrap();
        assert_eq!(page.selected_count, 0);
        assert!(page.rows.is_empty());
        // Correct query/revision digest and real row position, but the row is
        // excluded by this selection. The optimization must still reject it.
        request.cursor = Some(
            Cursor {
                schema_version: 1,
                query_sha256: query_hash(&request, revision).unwrap(),
                date: row.date.clone(),
                sequence,
            }
            .encode()
            .unwrap(),
        );
        let error = workspace.page_transactions(&request, revision).unwrap_err();
        assert!(matches!(error, Error::Validation(_)));
        assert!(error.to_string().contains("selected scope"));
        assert!(workspace.conn.is_autocommit());
        assert_eq!(workspace.revision().unwrap(), revision);
    }
}
