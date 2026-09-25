use std::{collections::BTreeSet, fs, path::Path};
use workbench_core::{
    domain::*,
    store::Workspace,
    transaction_analysis::{RowDisposition, TransferTreatment, MAX_ANALYSIS_ROWS},
    transaction_comparison::*,
    Error,
};

fn row(id: &str, date: &str, amount: &str) -> Transaction {
    Transaction {
        id: id.into(),
        account: "0001".into(),
        date: date.into(),
        posting_date: None,
        description: "Synthetic purchase".into(),
        amount: amount.into(),
        currency: "AUD".into(),
        balance: None,
        anchor: SourceAnchor::Text {
            evidence_id: "synthetic-source".into(),
            line_start: 1,
            line_end: 1,
        },
        review: ReviewState::Accepted,
        duplicate_candidates: vec![],
        transfer_peer: None,
        merchant: None,
        version: 1,
    }
}
fn period(from: &str, through: &str) -> DatePeriod {
    DatePeriod {
        from: from.into(),
        through: through.into(),
    }
}
fn request() -> TransactionComparisonRequest {
    TransactionComparisonRequest {
        baseline: period("2025-01-01", "2025-01-31"),
        comparison: period("2025-02-01", "2025-02-28"),
        account: None,
        currency: None,
        transfers: TransferTreatment::Include,
    }
}
fn assert_partition(total: &PeriodAccountTotal) {
    let ids: Vec<_> = [
        &total.total.transaction_ids,
        &total.pending_ids,
        &total.rejected_ids,
        &total.deferred_ids,
        &total.excluded_transfer_ids,
    ]
    .into_iter()
    .flatten()
    .collect();
    assert_eq!(ids.len(), total.scope_count);
    assert_eq!(
        ids.into_iter().collect::<BTreeSet<_>>(),
        total.rows.iter().map(|r| &r.transaction_id).collect()
    );
    assert_eq!(total.rows.len(), total.scope_count);
}

#[test]
fn inclusive_leap_year_boundaries_gaps_and_posting_dates_are_explicit() {
    let mut input = vec![
        row("before", "2023-12-30", "9"),
        row("a", "2023-12-31", "1.00"),
        row("b", "2024-01-01", "2.00"),
        row("gap", "2024-01-02", "7"),
        row("c", "2024-02-29", "4.00"),
        row("d", "2024-03-01", "5.00"),
        row("after", "2024-03-02", "8"),
    ];
    input[2].posting_date = Some("2024-03-01".into());
    input[4].version = 4;
    let request = TransactionComparisonRequest {
        baseline: period("2023-12-31", "2024-01-01"),
        comparison: period("2024-02-29", "2024-03-01"),
        ..request()
    };
    let result = compare(&input, 41, &request).unwrap();
    assert_eq!(
        (
            result.workspace_revision,
            result.baseline_days,
            result.comparison_days,
            result.gap_days
        ),
        (41, 2, 2, 58)
    );
    assert!(!result.unequal_duration);
    assert_eq!(
        (
            result.baseline_transaction_count,
            result.comparison_transaction_count
        ),
        (2, 2)
    );
    assert_eq!(
        result
            .outside_period_rows
            .iter()
            .map(|r| r.transaction_id.as_str())
            .collect::<Vec<_>>(),
        ["before", "gap", "after"]
    );
    let group = &result.groups[0];
    assert_eq!(group.baseline.total.transaction_ids, ["a", "b"]);
    assert_eq!(group.comparison.total.transaction_ids, ["c", "d"]);
    assert_eq!(group.comparison.rows[0].version, 4);
    assert_eq!(
        (
            &group.credits.baseline,
            &group.credits.comparison,
            &group.credits.delta
        ),
        (&"3.00".into(), &"9.00".into(), &"6.00".into())
    );
    assert_eq!(
        group.credits.relative_change,
        RelativeChange::Defined {
            numerator: "6.00".into(),
            denominator: "3.00".into()
        }
    );
}

#[test]
fn account_currency_review_denominators_and_duplicate_purchases_are_preserved() {
    let mut rows = vec![
        row("a", "2025-01-01", "-10.25"),
        row("b", "2025-01-01", "-10.25"),
        row("pending", "2025-01-02", "900"),
        row("rejected", "2025-02-02", "800"),
        row("deferred", "2025-02-03", "700"),
        row("other-account", "2025-02-04", "12.00"),
        row("other-currency", "2025-01-10", "30.00"),
        row("accepted", "2025-02-05", "-11.00"),
    ];
    rows[0].duplicate_candidates = vec!["b".into()];
    rows[1].duplicate_candidates = vec!["a".into()];
    rows[2].review = ReviewState::Pending;
    rows[3].review = ReviewState::Rejected;
    rows[4].review = ReviewState::Deferred;
    rows[5].account = "1".into();
    rows[6].currency = "USD".into();
    let result = compare(&rows, 8, &request()).unwrap();
    assert!(result.unequal_duration);
    assert_eq!(
        (
            result.baseline_days,
            result.comparison_days,
            result.gap_days
        ),
        (31, 28, 0)
    );
    assert_eq!(result.groups.len(), 3);
    assert_eq!(
        (
            result.workspace_transaction_count,
            result.account_currency_transaction_count,
            result.outside_account_currency_count
        ),
        (8, 8, 0)
    );
    let aud = &result.groups[0];
    assert_eq!(
        (aud.account.as_str(), aud.currency.as_str()),
        ("0001", "AUD")
    );
    assert_eq!(aud.baseline.total.debits, "20.50");
    assert_eq!(aud.baseline.pending_ids, ["pending"]);
    assert_eq!(aud.comparison.rejected_ids, ["rejected"]);
    assert_eq!(aud.comparison.deferred_ids, ["deferred"]);
    assert!(aud.baseline.rows[..2]
        .iter()
        .all(|row| row.has_duplicate_candidates));
    assert_eq!(aud.debits.delta, "-9.50");
    assert_eq!(aud.net.delta, "9.50");
    assert_eq!(aud.net.relative_change, RelativeChange::NegativeBaseline);
    for group in &result.groups {
        assert_partition(&group.baseline);
        assert_partition(&group.comparison);
    }
    let filtered = compare(
        &rows,
        8,
        &TransactionComparisonRequest {
            account: Some("0001".into()),
            currency: Some("AUD".into()),
            ..request()
        },
    )
    .unwrap();
    assert_eq!(
        (
            filtered.groups.len(),
            filtered.account_currency_transaction_count,
            filtered.outside_account_currency_count
        ),
        (1, 6, 2)
    );
}

#[test]
fn zero_missing_unaccepted_and_signed_baselines_never_invent_percentages() {
    let mut rows = vec![
        row("pending", "2025-01-01", "50"),
        row("new", "2025-02-01", "3"),
        row("one-sided", "2025-02-02", "2"),
    ];
    rows[0].review = ReviewState::Pending;
    rows[2].account = "0002".into();
    let result = compare(&rows, 0, &request()).unwrap();
    let first = &result.groups[0];
    let second = &result.groups[1];
    assert_eq!(first.baseline.scope_count, 1);
    assert_eq!(second.baseline.scope_count, 0);
    assert_eq!(
        first.credits.relative_change,
        RelativeChange::ZeroBaseline {
            comparison_is_zero: false
        }
    );
    assert_eq!(
        first.debits.relative_change,
        RelativeChange::ZeroBaseline {
            comparison_is_zero: true
        }
    );
    assert_eq!(
        second.credits.relative_change,
        RelativeChange::ZeroBaseline {
            comparison_is_zero: false
        }
    );
    let thirds = compare(
        &[row("a", "2025-01-01", "3"), row("b", "2025-02-01", "4")],
        0,
        &request(),
    )
    .unwrap();
    assert_eq!(
        thirds.groups[0].credits.relative_change,
        RelativeChange::Defined {
            numerator: "1".into(),
            denominator: "3".into()
        }
    );
    let empty = compare(&[], 0, &request()).unwrap();
    assert!(empty.groups.is_empty());
    assert_eq!(empty.baseline_transaction_count, 0);
    assert_eq!(empty.comparison_transaction_count, 0);
}

#[test]
fn selected_direction_is_preserved_when_baseline_is_later() {
    let mut req = request();
    std::mem::swap(&mut req.baseline, &mut req.comparison);
    let result = compare(
        &[row("a", "2025-01-01", "3"), row("b", "2025-02-01", "4")],
        0,
        &req,
    )
    .unwrap();
    assert_eq!(result.groups[0].credits.delta, "-1");
    assert_eq!(
        result.groups[0].credits.relative_change,
        RelativeChange::Defined {
            numerator: "-1".into(),
            denominator: "4".into()
        }
    );
    assert_eq!(
        (
            result.baseline_days,
            result.comparison_days,
            result.gap_days
        ),
        (28, 31, 0)
    );
}

#[test]
fn only_verified_reviewed_transfers_are_excluded_including_outside_period_peers() {
    let mut rows = vec![
        row("out", "2025-01-01", "-50.00"),
        row("in", "2025-03-01", "50.00"),
    ];
    rows[1].account = "0002".into();
    rows[1].version = 3;
    rows[0].transfer_peer = Some("in".into());
    rows[1].transfer_peer = Some("out".into());
    let req = TransactionComparisonRequest {
        account: Some("0001".into()),
        transfers: TransferTreatment::ExcludeReviewedPairs,
        ..request()
    };
    let result = compare(&rows, 9, &req).unwrap();
    assert_eq!(result.groups[0].baseline.excluded_transfer_ids, ["out"]);
    assert_eq!(result.groups[0].baseline.total.debits, "0");
    assert_eq!(
        result.verified_transfer_peers,
        [TransactionVersion {
            transaction_id: "in".into(),
            version: 3
        }]
    );
    assert_eq!(result.outside_account_currency_count, 1);
    assert_partition(&result.groups[0].baseline);
    let included = compare(
        &rows,
        9,
        &TransactionComparisonRequest {
            transfers: TransferTreatment::Include,
            ..req.clone()
        },
    )
    .unwrap();
    assert_eq!(included.groups[0].baseline.total.debits, "50.00");
    for kind in 0..5 {
        let mut invalid = rows.clone();
        match kind {
            0 => invalid[1].transfer_peer = None,
            1 => invalid[1].review = ReviewState::Pending,
            2 => invalid[1].amount = "49.99".into(),
            3 => invalid[1].currency = "USD".into(),
            _ => invalid[1].account = "0001".into(),
        }
        let result = compare(&invalid, 9, &req).unwrap();
        assert!(result.verified_transfer_peers.is_empty());
        let first = &result.groups[0].baseline;
        assert_eq!(first.total.debits, "50.00");
        assert_eq!(first.rows[0].disposition, RowDisposition::Included);
        assert!(first.rows[0].unverified_transfer_match);
    }
}

#[test]
fn comparison_precision_never_rounds_deltas_ratios_or_totals() {
    let exact = compare(
        &[
            row("a", "2025-01-01", "0.10000001"),
            row("b", "2025-02-01", "0.20000002"),
        ],
        0,
        &request(),
    )
    .unwrap();
    assert_eq!(exact.groups[0].credits.delta, "0.10000001");
    assert_eq!(
        exact.groups[0].credits.relative_change,
        RelativeChange::Defined {
            numerator: "0.10000001".into(),
            denominator: "0.10000001".into()
        }
    );
    for rows in [
        vec![
            row("a", "2025-01-01", "0.01"),
            row("b", "2025-02-01", "10000000000000000000000000000"),
        ],
        vec![
            row("a", "2025-01-01", "-10000000000000000000000000000"),
            row("b", "2025-02-01", "0.01"),
        ],
        vec![
            row("a", "2025-01-01", "10000000000000000000000000000"),
            row("b", "2025-01-02", "0.01"),
        ],
        vec![row("a", "2025-01-01", "9999999999999999999999999999.9")],
        vec![row("a", "2025-01-01", "7922816251426433759354395033.6")],
    ] {
        assert!(compare(&rows, 0, &request()).is_err());
    }
    // Exact generated debit/net totals may exceed the source field's byte-length cap.
    let large = compare(
        &[
            row("a", "2025-01-01", "-7922816251426433759354395033"),
            row("b", "2025-01-02", "-0.5"),
        ],
        0,
        &request(),
    )
    .unwrap();
    assert_eq!(
        large.groups[0].net.baseline,
        "-7922816251426433759354395033.5"
    );
    assert_eq!(large.groups[0].net.delta, "7922816251426433759354395033.5");
}

#[test]
fn invalid_dates_overlap_order_filters_and_unknown_fields_are_rejected() {
    for dates in [
        ("2025-01-01", "2025-01-31", "2025-01-31", "2025-02-28"),
        ("2025-01-01", "2025-01-31", "2025-01-10", "2025-01-11"),
        ("2025-01-31", "2025-01-01", "2025-02-01", "2025-02-28"),
        ("2025-01-01", "2025-01-31", "2025-02-29", "2025-03-01"),
        ("2025-01- 1", "2025-01-31", "2025-02-01", "2025-02-28"),
        ("2025- 1-01", "2025-01-31", "2025-02-01", "2025-02-28"),
        ("+2025-01-1", "2025-01-31", "2025-02-01", "2025-02-28"),
    ] {
        let req = TransactionComparisonRequest {
            baseline: period(dates.0, dates.1),
            comparison: period(dates.2, dates.3),
            ..request()
        };
        assert!(compare(&[], 0, &req).is_err());
    }
    for req in [
        TransactionComparisonRequest {
            account: Some(" ".into()),
            ..request()
        },
        TransactionComparisonRequest {
            currency: Some("aud".into()),
            ..request()
        },
    ] {
        assert!(compare(&[], 0, &req).is_err());
    }
    for date in ["2025-01- 1", "2025- 1-01", "+2025-01-1", "2025-02-29"] {
        assert!(compare(&[row("a", date, "1")], 0, &request()).is_err());
    }
    let mut extra = serde_json::to_value(request()).unwrap();
    extra["sql"] = "untrusted".into();
    assert!(serde_json::from_value::<TransactionComparisonRequest>(extra).is_err());
    let mut extra = serde_json::to_value(request()).unwrap();
    extra["baseline"]["timezone"] = "untrusted".into();
    assert!(serde_json::from_value::<TransactionComparisonRequest>(extra).is_err());
    let single_days = TransactionComparisonRequest {
        baseline: period("2024-02-29", "2024-02-29"),
        comparison: period("2024-03-01", "2024-03-01"),
        ..request()
    };
    assert_eq!(compare(&[], 0, &single_days).unwrap().gap_days, 0);
}

#[test]
fn shared_ledger_validation_applies_even_to_filtered_out_rows() {
    let duplicate = vec![
        row("same", "2025-01-01", "1"),
        row("same", "2024-01-01", "1"),
    ];
    assert!(compare(&duplicate, 0, &request()).is_err());
    assert!(compare(&[row("bad", "2024-02-30", "1")], 0, &request()).is_err());
    let oversized = vec![row("a", "2025-01-01", "1"); MAX_ANALYSIS_ROWS + 1];
    assert!(compare(&oversized, 0, &request()).is_err());
}

fn temp() -> tempfile::TempDir {
    tempfile::tempdir_in(std::env::temp_dir().canonicalize().unwrap()).unwrap()
}
fn accepted(workspace: &mut Workspace) {
    for row in workspace.view().unwrap().transactions {
        workspace
            .review_transaction(
                &row.id,
                ReviewState::Accepted,
                "Synthetic fixture review",
                workspace.revision().unwrap(),
            )
            .unwrap();
    }
}
fn corrupt_original(path: &Path) {
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        fs::set_permissions(path, fs::Permissions::from_mode(0o600)).unwrap();
    }
    #[cfg(windows)]
    #[allow(clippy::permissions_set_readonly_false)]
    {
        // Windows-only mutation of an owned synthetic corruption fixture. Unix
        // uses explicit owner-only mode above; this never broadens Unix access.
        let mut permissions = fs::metadata(path).unwrap().permissions();
        permissions.set_readonly(false);
        fs::set_permissions(path, permissions).unwrap();
    }
    fs::write(path, b"synthetic corruption").unwrap();
}

#[test]
fn canonical_comparison_is_revision_bound_read_only_and_recomputes_after_correction() {
    let temp = temp();
    let mut workspace = Workspace::open(temp.path()).unwrap();
    workspace.import("synthetic-periods.csv", b"account,date,description,amount,currency\n0001,2025-01-31,Synthetic baseline,-10.00,AUD\n0001,2025-02-01,Synthetic comparison,-20.00,AUD\n").unwrap();
    accepted(&mut workspace);
    workspace.dispatch(Command::SaveReport {}).unwrap();
    let before = workspace.view().unwrap();
    let revision = workspace.revision().unwrap();
    let result = workspace
        .compare_transaction_periods(&request(), revision)
        .unwrap();
    assert_eq!(result.workspace_revision, revision);
    let dispatched = workspace
        .dispatch(Command::CompareTransactionPeriods {
            request: request(),
            expected_revision: revision,
        })
        .unwrap();
    assert_eq!(
        serde_json::from_value::<TransactionComparison>(dispatched).unwrap(),
        result
    );
    assert_eq!(result.groups[0].debits.delta, "10.00");
    assert_eq!(
        serde_json::to_value(workspace.view().unwrap()).unwrap(),
        serde_json::to_value(&before).unwrap()
    );
    let id = &before.transactions[1].id;
    workspace
        .correct_transaction(id, "-21.00", "Synthetic correction", revision)
        .unwrap();
    assert!(matches!(
        workspace.compare_transaction_periods(&request(), revision),
        Err(Error::Conflict(_))
    ));
    let corrected = workspace
        .compare_transaction_periods(&request(), workspace.revision().unwrap())
        .unwrap();
    assert_eq!(
        corrected.groups[0].comparison.pending_ids.as_slice(),
        std::slice::from_ref(id)
    );
    assert_eq!(
        corrected.groups[0].comparison.rows[0].version,
        before.transactions[1].version + 1
    );
    assert!(corrected.groups[0]
        .comparison
        .total
        .transaction_ids
        .is_empty());
    workspace
        .review_transaction(
            id,
            ReviewState::Accepted,
            "Synthetic re-review",
            workspace.revision().unwrap(),
        )
        .unwrap();
    let reviewed = workspace
        .compare_transaction_periods(&request(), workspace.revision().unwrap())
        .unwrap();
    assert_eq!(reviewed.groups[0].debits.delta, "11.00");
    assert_eq!(
        workspace
            .inspect_source(&before.transactions[1].anchor)
            .unwrap()
            .quote,
        "-20.00"
    );
    let after = workspace.view().unwrap();
    assert_eq!(after.reports[0].sha256, before.reports[0].sha256);
    assert_eq!(after.reports[0].html, before.reports[0].html);
}

#[test]
fn both_period_originals_and_filtered_out_transfer_peer_are_verified() {
    for target in 0..3 {
        let temp = temp();
        let mut workspace = Workspace::open(temp.path()).unwrap();
        let inputs: [&[u8]; 3] = [
            b"account,date,description,amount,currency\n0001,2025-01-01,Synthetic baseline,-50.00,AUD\n",
            b"account,date,description,amount,currency\n0001,2025-02-01,Synthetic comparison,-25.00,AUD\n",
            b"account,date,description,amount,currency\n0002,2025-03-01,Synthetic peer,50.00,AUD\n",
        ];
        let evidence: Vec<_> = inputs
            .iter()
            .enumerate()
            .map(|(index, bytes)| {
                workspace
                    .import(&format!("synthetic-{index}.csv"), bytes)
                    .unwrap()
            })
            .collect();
        accepted(&mut workspace);
        let rows = workspace.view().unwrap().transactions;
        workspace
            .match_transfer(
                &rows[0].id,
                &rows[2].id,
                "Synthetic reviewed transfer",
                workspace.revision().unwrap(),
            )
            .unwrap();
        let req = TransactionComparisonRequest {
            account: Some("0001".into()),
            transfers: TransferTreatment::ExcludeReviewedPairs,
            ..request()
        };
        let result = workspace
            .compare_transaction_periods(&req, workspace.revision().unwrap())
            .unwrap();
        assert_eq!(
            result.groups[0].baseline.excluded_transfer_ids,
            [rows[0].id.clone()]
        );
        corrupt_original(&temp.path().join("originals").join(&evidence[target]));
        assert!(
            workspace
                .compare_transaction_periods(&req, workspace.revision().unwrap())
                .is_err(),
            "Corrupt source {target} was not verified"
        );
    }
}
