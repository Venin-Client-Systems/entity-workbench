use std::{collections::BTreeSet, fs};
use workbench_core::{domain::*, store::Workspace, transaction_analysis::*, Error};
const SOURCE: &[u8] = include_bytes!("../../../fixtures/transactions/patterns.csv");
fn ledger() -> Vec<Transaction> {
    let temp = tempfile::tempdir_in(std::env::temp_dir().canonicalize().unwrap()).unwrap();
    let mut workspace = Workspace::open(temp.path()).unwrap();
    workspace.import("synthetic-patterns.csv", SOURCE).unwrap();
    let mut rows = workspace.view().unwrap().transactions;
    for t in &mut rows {
        t.review = ReviewState::Accepted;
    }
    rows[10].review = ReviewState::Pending;
    rows[11].review = ReviewState::Rejected;
    rows[12].review = ReviewState::Deferred;
    rows[13].transfer_peer = Some(rows[14].id.clone());
    rows[14].transfer_peer = Some(rows[13].id.clone());
    rows[3].duplicate_candidates = vec![rows[4].id.clone()];
    rows[4].duplicate_candidates = vec![rows[3].id.clone()];
    rows
}
fn row(id: &str, date: &str, value: &str, description: &str) -> Transaction {
    Transaction {
        id: id.into(),
        account: "0001".into(),
        date: date.into(),
        posting_date: None,
        description: description.into(),
        amount: value.into(),
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
#[test]
fn independent_totals_keep_currencies_denominators_and_repeated_purchases() {
    let rows = ledger();
    let analysis = analyze(&rows, 7, &TransactionAnalysisRequest::default()).unwrap();
    assert_eq!(
        (
            analysis.workspace_revision,
            analysis.scope_transaction_count,
            analysis.rows.len()
        ),
        (7, 20, 20)
    );
    let aud = &analysis.currencies[0];
    let usd = &analysis.currencies[1];
    assert_eq!(
        (
            aud.currency.as_str(),
            aud.scope_count,
            aud.total.credits.as_str(),
            aud.total.debits.as_str(),
            aud.total.net.as_str()
        ),
        ("AUD", 17, "125.00", "127.00", "-2.00")
    );
    assert_eq!(aud.total.transaction_ids.len(), 14);
    assert_eq!(
        (
            aud.pending_ids.as_slice(),
            aud.rejected_ids.as_slice(),
            aud.deferred_ids.as_slice()
        ),
        (
            &[rows[10].id.clone()][..],
            &[rows[11].id.clone()][..],
            &[rows[12].id.clone()][..]
        )
    );
    assert!(aud.excluded_transfer_ids.is_empty());
    assert_eq!(
        (usd.currency.as_str(), usd.total.net.as_str()),
        ("USD", "-36.00")
    );
    assert_eq!(aud.cash_candidates.debits, "50.00");
    assert_eq!(
        aud.cash_candidates.transaction_ids,
        vec![rows[6].id.clone()]
    );
    assert_eq!(aud.refund_candidates.credits, "5.00");
    assert_eq!(
        aud.refund_candidates.transaction_ids,
        vec![rows[7].id.clone(), rows[8].id.clone()]
    );
    let club = analysis
        .merchant_groups
        .iter()
        .find(|g| g.currency == "AUD" && g.description_group == "ACME CLUB")
        .unwrap();
    assert_eq!(club.total.debits, "40.00");
    assert_eq!(club.accounts, vec!["0001", "0002"]);
    let cafe = analysis
        .merchant_groups
        .iter()
        .find(|g| g.description_group == "SYNTHETIC CAFE")
        .unwrap();
    assert_eq!(cafe.total.debits, "12.00");
    assert_eq!(cafe.total.transaction_ids.len(), 3);
    assert_eq!(
        analysis
            .rows
            .iter()
            .filter(|r| r.has_duplicate_candidates)
            .count(),
        2
    );
    assert_eq!(analysis.recurring_candidates.len(), 2);
    assert!(analysis
        .recurring_candidates
        .iter()
        .all(|c| c.cadence == Cadence::Weekly && c.account == "0001"));
    assert_eq!(analysis.recurrence_eligible_ids.len(), 13);
    assert_eq!(analysis.recurrence_unclassified_ids.len(), 7);
    // Every scope row appears once in the review/exclusion partition, including non-accepted rows.
    let partition: Vec<_> = analysis
        .currencies
        .iter()
        .flat_map(|c| {
            [
                &c.total.transaction_ids,
                &c.pending_ids,
                &c.rejected_ids,
                &c.deferred_ids,
                &c.excluded_transfer_ids,
            ]
        })
        .flatten()
        .collect();
    assert_eq!(partition.len(), 20);
    assert_eq!(partition.into_iter().collect::<BTreeSet<_>>().len(), 20);
}
#[test]
fn explicit_transfer_exclusion_checks_peers_outside_selected_scope() {
    let rows = ledger();
    let mut request = TransactionAnalysisRequest {
        transfers: TransferTreatment::ExcludeReviewedPairs,
        ..Default::default()
    };
    let result = analyze(&rows, 3, &request).unwrap();
    let aud = &result.currencies[0];
    assert_eq!(
        (&aud.total.credits, &aud.total.debits, &aud.total.net),
        (&"105.00".into(), &"107.00".into(), &"-2.00".into())
    );
    assert_eq!(
        aud.excluded_transfer_ids,
        vec![rows[13].id.clone(), rows[14].id.clone()]
    );
    request.account = Some("0001".into());
    request.currency = Some("AUD".into());
    request.date_from = Some("2025-01-07".into());
    request.date_to = Some("2025-01-07".into());
    let result = analyze(&rows, 3, &request).unwrap();
    assert_eq!(result.scope_transaction_count, 1);
    assert_eq!(
        result.rows[0].verified_transfer_peer,
        Some(rows[14].id.clone())
    );
    assert_eq!(
        result.currencies[0].excluded_transfer_ids,
        vec![rows[13].id.clone()]
    );
    assert!(result.currencies[0].total.transaction_ids.is_empty());
}
#[test]
fn invalid_transfer_links_are_visible_and_not_implicitly_excluded() {
    let request = TransactionAnalysisRequest {
        transfers: TransferTreatment::ExcludeReviewedPairs,
        ..Default::default()
    };
    for kind in 0..5 {
        let mut rows = ledger();
        match kind {
            0 => rows[14].transfer_peer = None,
            1 => rows[14].review = ReviewState::Pending,
            2 => rows[14].currency = "USD".into(),
            3 => rows[14].account = "0001".into(),
            _ => rows[14].amount = "19.99".into(),
        }
        let result = analyze(&rows, 0, &request).unwrap();
        let first = result
            .rows
            .iter()
            .find(|r| r.transaction_id == rows[13].id)
            .unwrap();
        assert!(first.unverified_transfer_match);
        assert_eq!(first.disposition, RowDisposition::Included);
        assert!(first.verified_transfer_peer.is_none());
        assert!(result
            .currencies
            .iter()
            .all(|c| c.excluded_transfer_ids.is_empty()));
    }
}
#[test]
fn calendar_month_ends_and_exact_amount_spread_are_explainable() {
    let rows = vec![
        row("a", "2024-01-31", "-10.00", "Synthetic Monthly"),
        row("b", "2024-02-29", "-10.20", "synthetic monthly"),
        row("c", "2024-03-31", "-10.10", "Synthetic Monthly"),
    ];
    let mut request = TransactionAnalysisRequest {
        date_tolerance_days: 0,
        amount_tolerance: "0.20".into(),
        ..Default::default()
    };
    let result = analyze(&rows, 0, &request).unwrap();
    let candidate = &result.recurring_candidates[0];
    assert_eq!(candidate.cadence, Cadence::Monthly);
    assert_eq!(
        candidate.expected_dates,
        vec!["2024-01-31", "2024-02-29", "2024-03-31"]
    );
    assert_eq!(candidate.deviations_days, vec![0, 0, 0]);
    assert_eq!(candidate.total.debits, "30.30");
    assert_eq!(
        (&candidate.minimum_debit, &candidate.maximum_debit),
        (&"10.00".into(), &"10.20".into())
    );
    request.amount_tolerance = "0.19".into();
    assert!(analyze(&rows, 0, &request)
        .unwrap()
        .recurring_candidates
        .is_empty());
}
#[test]
fn recurrence_does_not_hide_same_day_rows_missing_periods_or_gradual_drift() {
    for dates in [
        ["2025-01-01", "2025-01-10", "2025-01-19"],
        ["2025-01-01", "2025-01-01", "2025-01-08"],
        ["2025-01-01", "2025-02-01", "2025-04-01"],
    ] {
        let rows: Vec<_> = dates
            .into_iter()
            .enumerate()
            .map(|(i, date)| row(&i.to_string(), date, "-5.00", "Synthetic Repeated"))
            .collect();
        let result = analyze(&rows, 0, &TransactionAnalysisRequest::default()).unwrap();
        assert!(result.recurring_candidates.is_empty());
        assert_eq!(result.recurrence_unclassified_ids.len(), 3);
        assert_eq!(result.currencies[0].total.debits, "15.00");
    }
    let rows = vec![
        row("a", "2025-01-01", "-5", "A"),
        row("b", "2025-01-15", "-5", "A"),
        row("c", "2025-01-29", "-5", "A"),
    ];
    assert_eq!(
        analyze(&rows, 0, &Default::default())
            .unwrap()
            .recurring_candidates[0]
            .cadence,
        Cadence::Fortnightly
    );
}
#[test]
fn classification_respects_direction_whole_tokens_and_original_description_boundaries() {
    let rows = vec![
        row("a", "2025-01-01", "-5", "CASHMERE ATMOSPHERE"),
        row("b", "2025-01-02", "-5", "REFUND"),
        row("c", "2025-01-03", "5", "ATM"),
        row("d", "2025-01-04", "-5", "CASH WITHDRAWAL"),
        row("e", "2025-01-05", "0", "REFUND ATM"),
        row("f", "2025-01-06", "-5", "STORE 001"),
        row("g", "2025-01-07", "-5", "STORE 002"),
        row("h", "2025-01-08", "-5", "STORE-001"),
    ];
    let result = analyze(&rows, 0, &Default::default()).unwrap();
    assert_eq!(
        result.currencies[0].cash_candidates.transaction_ids,
        vec!["d"]
    );
    assert!(result.currencies[0]
        .refund_candidates
        .transaction_ids
        .is_empty());
    assert_eq!(result.merchant_groups.len(), 8);
    assert_eq!(
        result.rows[3].cash_rule,
        Some(CashRule::DebitWithCashWithdrawalPhrase)
    );
}
#[test]
fn invalid_parameters_duplicate_ids_and_overflow_fail_without_partial_totals() {
    let rows = vec![
        row("a", "2025-01-01", "79228162514264337593543950335", "A"),
        row("b", "2025-01-02", "1", "B"),
    ];
    assert!(matches!(
        analyze(&rows, 0, &Default::default()),
        Err(Error::Validation(_))
    ));
    assert!(analyze(
        &[
            row("same", "2025-01-01", "1", "A"),
            row("same", "2025-01-02", "1", "B")
        ],
        0,
        &Default::default()
    )
    .is_err());
    for request in [
        TransactionAnalysisRequest {
            minimum_occurrences: 2,
            ..Default::default()
        },
        TransactionAnalysisRequest {
            date_tolerance_days: 4,
            ..Default::default()
        },
        TransactionAnalysisRequest {
            amount_tolerance: "-1".into(),
            ..Default::default()
        },
        TransactionAnalysisRequest {
            date_from: Some("2025-02-01".into()),
            date_to: Some("2025-01-01".into()),
            ..Default::default()
        },
        TransactionAnalysisRequest {
            currency: Some("aud".into()),
            ..Default::default()
        },
    ] {
        assert!(analyze(&[], 0, &request).is_err());
    }
    let request = serde_json::to_value(TransactionAnalysisRequest::default()).unwrap();
    let mut extra = request.clone();
    extra["sql"] = serde_json::json!("SELECT anything");
    assert!(serde_json::from_value::<TransactionAnalysisRequest>(extra).is_err());
    let too_many = vec![row("a", "2025-01-01", "1", "A"); MAX_ANALYSIS_ROWS + 1];
    assert!(analyze(&too_many, 0, &Default::default()).is_err());
}
#[test]
fn canonical_analysis_is_read_only_fresh_source_bound_and_updates_after_correction() {
    let temp = tempfile::tempdir_in(std::env::temp_dir().canonicalize().unwrap()).unwrap();
    let mut workspace = Workspace::open(temp.path().join("case")).unwrap();
    let evidence = workspace.import("synthetic-patterns.csv", SOURCE).unwrap();
    let ids: Vec<_> = workspace
        .view()
        .unwrap()
        .transactions
        .into_iter()
        .map(|t| t.id)
        .collect();
    for id in &ids {
        workspace
            .review_transaction(
                id,
                ReviewState::Accepted,
                "Synthetic analysis fixture",
                workspace.revision().unwrap(),
            )
            .unwrap();
    }
    workspace.dispatch(Command::SaveReport {}).unwrap();
    let saved = workspace.view().unwrap().reports[0].clone();
    let revision = workspace.revision().unwrap();
    let request = TransactionAnalysisRequest::default();
    let result = workspace.analyze_transactions(&request, revision).unwrap();
    assert_eq!(workspace.revision().unwrap(), revision);
    let encoded = workspace
        .dispatch(Command::AnalyzeTransactions {
            request: request.clone(),
            expected_revision: revision,
        })
        .unwrap();
    assert_eq!(
        serde_json::from_value::<TransactionAnalysis>(encoded).unwrap(),
        result
    );
    assert_eq!(
        workspace
            .inspect_source(&workspace.view().unwrap().transactions[0].anchor)
            .unwrap()
            .quote,
        "-10.00"
    );
    workspace
        .correct_transaction(&ids[0], "-11.00", "Synthetic correction", revision)
        .unwrap();
    assert!(matches!(
        workspace.analyze_transactions(&request, revision),
        Err(Error::Conflict(_))
    ));
    let updated = workspace
        .analyze_transactions(&request, workspace.revision().unwrap())
        .unwrap();
    assert!(updated.currencies[0].pending_ids.contains(&ids[0]));
    assert!(!updated.currencies[0]
        .total
        .transaction_ids
        .contains(&ids[0]));
    let after = workspace.view().unwrap();
    assert_eq!(after.reports[0].html, saved.html);
    assert_eq!(after.reports[0].sha256, saved.sha256);
    let original = temp.path().join("case/originals").join(evidence);
    assert_eq!(fs::read(&original).unwrap(), SOURCE);
    corrupt_original(&original);
    assert!(workspace
        .analyze_transactions(&request, workspace.revision().unwrap())
        .is_err());
}

fn corrupt_original(path: &std::path::Path) {
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        fs::set_permissions(path, fs::Permissions::from_mode(0o600)).unwrap();
    }
    #[cfg(windows)]
    {
        let mut permissions = fs::metadata(path).unwrap().permissions();
        permissions.set_readonly(false);
        fs::set_permissions(path, permissions).unwrap();
    }
    fs::write(path, b"synthetic corruption").unwrap();
}
#[test]
fn excluded_transfer_counterpart_original_is_checked_outside_the_filter() {
    let temp = tempfile::tempdir_in(std::env::temp_dir().canonicalize().unwrap()).unwrap();
    let mut workspace = Workspace::open(temp.path()).unwrap();
    workspace
        .import(
            "synthetic-out.csv",
            b"account,date,description,amount,currency\n0001,2025-01-01,OUT,-50.00,AUD\n",
        )
        .unwrap();
    let peer_evidence = workspace
        .import(
            "synthetic-in.csv",
            b"account,date,description,amount,currency\n0002,2025-01-02,IN,50.00,AUD\n",
        )
        .unwrap();
    let rows = workspace.view().unwrap().transactions;
    for row in &rows {
        workspace
            .review_transaction(
                &row.id,
                ReviewState::Accepted,
                "Synthetic peer review",
                workspace.revision().unwrap(),
            )
            .unwrap();
    }
    workspace
        .match_transfer(
            &rows[0].id,
            &rows[1].id,
            "Synthetic confirmed pair",
            workspace.revision().unwrap(),
        )
        .unwrap();
    let request = TransactionAnalysisRequest {
        account: Some("0001".into()),
        transfers: TransferTreatment::ExcludeReviewedPairs,
        ..Default::default()
    };
    assert_eq!(
        workspace
            .analyze_transactions(&request, workspace.revision().unwrap())
            .unwrap()
            .currencies[0]
            .excluded_transfer_ids
            .len(),
        1
    );
    corrupt_original(&temp.path().join("originals").join(peer_evidence));
    assert!(workspace
        .analyze_transactions(&request, workspace.revision().unwrap())
        .is_err());
}

#[test]
fn decimal_parsing_and_shared_arithmetic_never_round_away_source_value() {
    use workbench_core::analytics::{amount, exact_add, exact_sub};
    for text in [
        "9999999999999999999999999999.9",
        "7922816251426433759354395033.6",
    ] {
        assert!(amount(text).is_err(), "Rounded parse accepted: {text}");
    }
    let huge = amount("10000000000000000000000000000").unwrap();
    let cent = amount("0.01").unwrap();
    for (left, right) in [(huge, cent), (cent, huge), (-huge, -cent)] {
        assert!(exact_add(left, right).is_err());
    }
    assert!(exact_sub(huge, cent).is_err());
    assert!(exact_sub(cent, huge).is_err());
    assert_eq!(exact_add(huge, -huge).unwrap().to_string(), "0");
    assert_eq!(exact_sub(huge, huge).unwrap().to_string(), "0");
    assert_eq!(
        exact_add(
            amount("7922816251426433759354395033.5").unwrap(),
            amount("0.5").unwrap()
        )
        .unwrap()
        .to_string(),
        "7922816251426433759354395034"
    );
    assert_eq!(
        exact_add(amount("1.00").unwrap(), cent)
            .unwrap()
            .to_string(),
        "1.01"
    );
    assert_eq!(
        exact_sub(amount("1.00").unwrap(), cent)
            .unwrap()
            .to_string(),
        "0.99"
    );
    for values in [
        ["10000000000000000000000000000", "0.01"],
        ["10000000000000000000000000000", "-0.01"],
        ["-10000000000000000000000000000", "-0.01"],
    ] {
        let rows = vec![
            row("a", "2025-01-01", values[0], "A"),
            row("b", "2025-01-02", values[1], "B"),
        ];
        assert!(analyze(&rows, 0, &Default::default()).is_err());
        assert!(workbench_core::analytics::analyse(&rows).is_err());
    }
    // Reconciliation must not claim a zero difference after silently losing a cent.
    let mut first = row("a", "2025-01-01", "0", "A");
    first.review = ReviewState::Pending;
    first.balance = Some("0".into());
    let mut second = row("b", "2025-01-02", "10000000000000000000000000000", "B");
    second.review = ReviewState::Pending;
    let mut third = row("c", "2025-01-03", "0.01", "C");
    third.review = ReviewState::Pending;
    third.balance = Some("10000000000000000000000000000".into());
    assert!(workbench_core::analytics::analyse(&[first, second, third]).is_err());
    // Exact opposite transfers remain valid even at the supported extreme.
    let mut rows = vec![
        row("a", "2025-01-01", "10000000000000000000000000000", "A"),
        row("b", "2025-01-01", "-10000000000000000000000000000", "B"),
    ];
    rows[0].transfer_peer = Some("b".into());
    rows[1].transfer_peer = Some("a".into());
    rows[1].account = "0002".into();
    let request = TransactionAnalysisRequest {
        transfers: TransferTreatment::ExcludeReviewedPairs,
        ..Default::default()
    };
    assert_eq!(
        analyze(&rows, 0, &request).unwrap().currencies[0]
            .excluded_transfer_ids
            .len(),
        2
    );
}
#[test]
fn noncanonical_dates_fail_before_filtering_or_cadence_comparison() {
    for invalid in [
        "2025-01- 1",
        "2025- 1-01",
        "+2025-01-1",
        "2025-1-001",
        "2025-02-29",
    ] {
        assert!(
            workbench_core::analytics::date(invalid).is_err(),
            "{invalid}"
        );
        let rows = vec![row("a", invalid, "-1", "A")];
        assert!(analyze(&rows, 0, &Default::default()).is_err());
        let request = TransactionAnalysisRequest {
            date_from: Some(invalid.into()),
            ..Default::default()
        };
        assert!(request.validate().is_err());
    }
    for valid in ["2024-02-29", "2025-01-01", "0000-01-01", "9999-12-31"] {
        assert!(workbench_core::analytics::date(valid).is_ok());
    }
}
