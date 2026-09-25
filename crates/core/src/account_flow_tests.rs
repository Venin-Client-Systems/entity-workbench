use super::*;

fn row(id: &str, account: &str, amount: &str) -> Transaction {
    Transaction {
        id: id.into(),
        account: account.into(),
        date: "2024-01-31".into(),
        posting_date: None,
        description: "Synthetic transaction".into(),
        amount: amount.into(),
        currency: "AUD".into(),
        balance: None,
        anchor: SourceAnchor::Cell {
            evidence_id: "synthetic-source".into(),
            sheet: "CSV".into(),
            row: 2,
            column: "amount".into(),
        },
        review: ReviewState::Accepted,
        duplicate_candidates: vec![],
        transfer_peer: None,
        merchant: None,
        version: 1,
    }
}
fn pair(a: &mut Transaction, b: &mut Transaction) {
    a.transfer_peer = Some(b.id.clone());
    b.transfer_peer = Some(a.id.clone());
}
fn ledger() -> Vec<Transaction> {
    let mut rows = vec![
        row("a", "0001", "-10.00"),
        row("b", "0002", "10.00"),
        row("c", "0001", "-10.00"),
        row("d", "0002", "10.00"),
        row("e", "0001", "-5.00"),
        row("f", "0001", "-5.00"),
        row("refund", "0001", "2.00"),
        row("near", "0001", "-9.00"),
        row("near-credit", "0002", "8.00"),
        row("pending", "0001", "-99.00"),
        row("rejected", "0001", "6.00"),
        row("deferred", "0002", "-7.00"),
        row("usd-a", "0001", "-3.00"),
        row("usd-b", "0002", "3.00"),
        row("zero-a", "0001", "0"),
        row("zero-b", "0002", "0"),
    ];
    for (a, b) in [(0, 1), (2, 3), (7, 8), (12, 13), (14, 15)] {
        rows[a].transfer_peer = Some(rows[b].id.clone());
        rows[b].transfer_peer = Some(rows[a].id.clone());
    }
    rows[1].date = "2024-02-01".into();
    rows[4].duplicate_candidates = vec!["f".into()];
    rows[5].duplicate_candidates = vec!["e".into()];
    rows[6].description = "REFUND synthetic purchase".into();
    rows[9].review = ReviewState::Pending;
    rows[9].transfer_peer = Some("missing".into());
    rows[10].review = ReviewState::Rejected;
    rows[11].review = ReviewState::Deferred;
    rows[12].currency = "USD".into();
    rows[13].currency = "USD".into();
    rows
}
fn node<'a>(result: &'a AccountFlows, account: &str, currency: &str) -> &'a AccountFlowNode {
    result
        .nodes
        .iter()
        .find(|n| n.account == account && n.currency == currency)
        .unwrap()
}
fn partition(result: &AccountFlows) {
    let selected: BTreeSet<_> = result
        .sources
        .iter()
        .filter(|r| r.in_scope)
        .map(|r| r.transaction_id.as_str())
        .collect();
    let support: BTreeSet<_> = result
        .sources
        .iter()
        .filter(|r| !r.in_scope)
        .map(|r| r.transaction_id.as_str())
        .collect();
    let mut all = Vec::new();
    let mut supporting = Vec::new();
    for n in &result.nodes {
        let ids: Vec<_> = [
            &n.accepted_ledger.transaction_ids,
            &n.pending_ids,
            &n.rejected_ids,
            &n.deferred_ids,
        ]
        .into_iter()
        .flatten()
        .map(String::as_str)
        .collect();
        assert_eq!(ids.len(), n.scope_count);
        assert_eq!(
            n.review_counts.accepted
                + n.review_counts.pending
                + n.review_counts.rejected
                + n.review_counts.deferred,
            n.scope_count as u64
        );
        assert_eq!(n.support_only, n.scope_count == 0);
        assert_eq!(n.unverified_transfer_count, n.unverified_transfer_ids.len());
        assert_eq!(n.duplicate_candidate_count, n.duplicate_candidate_ids.len());
        all.extend(ids);
        supporting.extend(n.support_ids.iter().map(String::as_str));
    }
    assert_eq!(all.len(), selected.len());
    assert_eq!(all.into_iter().collect::<BTreeSet<_>>(), selected);
    assert_eq!(supporting.len(), support.len());
    assert_eq!(supporting.into_iter().collect::<BTreeSet<_>>(), support);
    assert_eq!(result.scope_transaction_count, selected.len());
    assert_eq!(result.support_transaction_count, support.len());
    let mut endpoints = BTreeSet::new();
    for edge in &result.edges {
        for p in &edge.pairs {
            assert!(p.debit_in_scope || p.credit_in_scope);
            assert!(endpoints.insert(p.debit.transaction_id.as_str()));
            assert!(endpoints.insert(p.credit.transaction_id.as_str()));
            assert_eq!(
                p.debit_in_scope,
                selected.contains(p.debit.transaction_id.as_str())
            );
            assert_eq!(
                p.credit_in_scope,
                selected.contains(p.credit.transaction_id.as_str())
            );
        }
    }
}

#[test]
fn independently_calculated_flows_keep_repeats_reviews_refunds_and_currencies() {
    let result = analyze(&ledger(), 42, &AccountFlowRequest::default()).unwrap();
    partition(&result);
    assert_eq!(
        (
            result.workspace_revision,
            result.workspace_transaction_count,
            result.nodes.len(),
            result.edges.len()
        ),
        (42, 16, 4, 2)
    );
    let a = node(&result, "0001", "AUD");
    let b = node(&result, "0002", "AUD");
    assert_eq!(
        (
            a.scope_count,
            a.accepted_ledger.credits.as_str(),
            a.accepted_ledger.debits.as_str(),
            a.accepted_ledger.net.as_str()
        ),
        (9, "2.00", "39.00", "-37.00")
    );
    assert_eq!(
        (
            a.accepted_unmapped.credits.as_str(),
            a.accepted_unmapped.debits.as_str()
        ),
        ("2.00", "19.00")
    );
    assert_eq!(
        (
            b.scope_count,
            b.accepted_ledger.credits.as_str(),
            b.accepted_unmapped.credits.as_str()
        ),
        (5, "28.00", "8.00")
    );
    assert_eq!(a.pending_ids, vec!["pending"]);
    assert_eq!(a.rejected_ids, vec!["rejected"]);
    assert_eq!(b.deferred_ids, vec!["deferred"]);
    assert_eq!(a.duplicate_candidate_ids, vec!["e", "f"]);
    assert_eq!(a.unverified_transfer_ids, vec!["near", "pending", "zero-a"]);
    assert_eq!(b.unverified_transfer_ids, vec!["near-credit", "zero-b"]);
    assert_eq!(
        (
            result.edges[0].currency.as_str(),
            result.edges[0].amount.as_str(),
            result.edges[0].pairs.len()
        ),
        ("AUD", "20.00", 2)
    );
    assert_eq!(
        (
            result.edges[1].currency.as_str(),
            result.edges[1].amount.as_str(),
            result.edges[1].pairs.len()
        ),
        ("USD", "3.00", 1)
    );
    assert_eq!(result.edges[0].debit_node_id, a.id);
    assert_eq!(result.edges[0].credit_node_id, b.id);
}

#[test]
fn asymmetric_scope_retains_both_versions_dates_and_zero_support_activity() {
    let mut rows = ledger();
    rows[0].version = 4;
    rows[1].version = 5;
    let request = AccountFlowRequest {
        account: Some("0001".into()),
        currency: Some("AUD".into()),
        ..Default::default()
    };
    let result = analyze(&rows, 7, &request).unwrap();
    partition(&result);
    assert_eq!(
        (
            result.scope_transaction_count,
            result.support_transaction_count,
            result.edges.len()
        ),
        (9, 2, 1)
    );
    let support = node(&result, "0002", "AUD");
    assert!(support.support_only);
    assert_eq!(support.accepted_ledger.net, "0");
    assert_eq!(support.support_ids, vec!["b", "d"]);
    let p = result.edges[0]
        .pairs
        .iter()
        .find(|p| p.debit.transaction_id == "a")
        .unwrap();
    assert_eq!(
        (
            p.debit.version,
            p.credit.version,
            p.debit_date.as_str(),
            p.credit_date.as_str()
        ),
        (4, 5, "2024-01-31", "2024-02-01")
    );
    assert!(p.debit_in_scope);
    assert!(!p.credit_in_scope);
    let reverse = analyze(
        &rows,
        7,
        &AccountFlowRequest {
            account: Some("0002".into()),
            date_from: Some("2024-02-01".into()),
            date_to: Some("2024-02-01".into()),
            ..Default::default()
        },
    )
    .unwrap();
    partition(&reverse);
    assert_eq!(
        (
            reverse.scope_transaction_count,
            reverse.support_transaction_count
        ),
        (1, 1)
    );
    let p = &reverse.edges[0].pairs[0];
    assert!(!p.debit_in_scope);
    assert!(p.credit_in_scope);
    assert_eq!(
        p.id,
        result.edges[0]
            .pairs
            .iter()
            .find(|p| p.debit.transaction_id == "a")
            .unwrap()
            .id
    );
}

#[test]
fn pair_verification_never_guesses_and_dates_are_inclusive() {
    for variation in 0..6 {
        let mut a = row("a", "0001", "-1");
        let mut b = row("b", "0002", "1");
        pair(&mut a, &mut b);
        match variation {
            0 => b.transfer_peer = None,
            1 => b.review = ReviewState::Pending,
            2 => b.account = a.account.clone(),
            3 => b.currency = "USD".into(),
            4 => b.amount = "0.99".into(),
            _ => {
                a.amount = "0".into();
                b.amount = "0".into();
            }
        }
        assert!(analyze(&[a, b], 0, &AccountFlowRequest::default())
            .unwrap()
            .edges
            .is_empty());
    }
    let mut a = row("a", "A", "-1");
    a.date = "2024-02-29".into();
    a.posting_date = Some("2025-01-01".into());
    let mut b = row("b", "B", "1");
    b.date = "2024-03-01".into();
    pair(&mut a, &mut b);
    let result = analyze(
        &[a, b],
        0,
        &AccountFlowRequest {
            date_from: Some("2024-02-29".into()),
            date_to: Some("2024-02-29".into()),
            ..Default::default()
        },
    )
    .unwrap();
    assert_eq!(
        (
            result.scope_transaction_count,
            result.support_transaction_count
        ),
        (1, 1)
    );
}

#[test]
fn empty_scope_and_deterministic_input_permutations() {
    let mut rows = ledger();
    let result = analyze(&rows, 8, &AccountFlowRequest::default()).unwrap();
    rows.reverse();
    assert_eq!(
        serde_json::to_value(&result).unwrap(),
        serde_json::to_value(analyze(&rows, 8, &AccountFlowRequest::default()).unwrap()).unwrap()
    );
    let empty = analyze(
        &rows,
        8,
        &AccountFlowRequest {
            account: Some("absent".into()),
            ..Default::default()
        },
    )
    .unwrap();
    assert_eq!(empty.workspace_transaction_count, 16);
    assert!(empty.sources.is_empty() && empty.nodes.is_empty() && empty.edges.is_empty());
    partition(&empty);
    assert!(analyze(&[], 0, &AccountFlowRequest::default()).is_ok());
}

#[test]
fn exact_totals_fail_instead_of_rounding_and_preserve_small_values() {
    for values in [
        vec!["10000000000000000000000000000", "0.01"],
        vec!["79228162514264337593543950335", "1"],
    ] {
        let rows: Vec<_> = values
            .iter()
            .enumerate()
            .map(|(i, a)| row(&format!("r{i}"), "A", a))
            .collect();
        assert!(analyze(&rows, 0, &AccountFlowRequest::default()).is_err());
    }
    let result = analyze(
        &[row("a", "A", "0.00000001"), row("b", "A", "-0.00000002")],
        0,
        &AccountFlowRequest::default(),
    )
    .unwrap();
    assert_eq!(result.nodes[0].accepted_ledger.net, "-0.00000001");
    let mut rows = vec![
        row("a", "A", "-10000000000000000000000000000"),
        row("b", "B", "10000000000000000000000000000"),
        row("c", "A", "-0.01"),
        row("d", "B", "0.01"),
    ];
    rows[0].transfer_peer = Some("b".into());
    rows[1].transfer_peer = Some("a".into());
    rows[2].transfer_peer = Some("d".into());
    rows[3].transfer_peer = Some("c".into());
    assert!(analyze(&rows, 0, &AccountFlowRequest::default()).is_err());
}

#[test]
fn malformed_rows_scopes_and_identifiers_are_refused() {
    for date in ["2025-01- 1", "2024-02-30", "+2025-01-1"] {
        let mut a = row("a", "A", "1");
        a.date = date.into();
        assert!(analyze(&[a], 0, &AccountFlowRequest::default()).is_err());
        assert!(AccountFlowRequest {
            date_from: Some(date.into()),
            ..Default::default()
        }
        .validate()
        .is_err());
    }
    for variation in 0..5 {
        let mut a = row("a", "A", "1");
        match variation {
            0 => a.version = 0,
            1 => a.id = "x".repeat(257),
            2 => a.account = "x".repeat(4001),
            3 => a.transfer_peer = Some("bad\n".into()),
            _ => {
                a.anchor = SourceAnchor::Capture {
                    evidence_id: "source".into(),
                    selector: "x".repeat(MAX_ANCHOR_BYTES),
                }
            }
        };
        assert!(analyze(&[a], 0, &AccountFlowRequest::default()).is_err());
    }
    let a = row("a", "A", "1");
    assert!(analyze(&[a.clone(), a], 0, &AccountFlowRequest::default()).is_err());
    assert!(AccountFlowRequest {
        date_from: Some("2025-02-01".into()),
        date_to: Some("2025-01-01".into()),
        ..Default::default()
    }
    .validate()
    .is_err());
}

#[test]
fn whole_output_limits_fail_without_truncation_and_count_actual_escaping() {
    let escaped = "\n\"\\";
    let bytes = serde_json::to_vec(escaped).unwrap().len();
    assert_eq!(bounded_size(&escaped, bytes).unwrap(), bytes);
    assert!(bounded_size(&escaped, bytes - 1).is_err());
    let nodes: Vec<_> = (0..=MAX_FLOW_NODES)
        .map(|i| row(&format!("r{i}"), &format!("a{i}"), "1"))
        .collect();
    assert!(analyze(&nodes, 0, &AccountFlowRequest::default())
        .unwrap_err()
        .to_string()
        .contains("node bound"));
    let mut rows = vec![];
    for i in 0..=MAX_FLOW_EDGES {
        let mut a = row(&format!("d{i}"), &format!("A{}", i / 101), "-1");
        let mut b = row(&format!("c{i}"), &format!("B{}", i % 101), "1");
        pair(&mut a, &mut b);
        rows.extend([a, b]);
    }
    assert!(analyze(&rows, 0, &AccountFlowRequest::default())
        .unwrap_err()
        .to_string()
        .contains("edge bound"));
    let account = "a".repeat(4000);
    let rows: Vec<_> = (0..4200)
        .map(|i| row(&format!("r{i}"), &account, "1"))
        .collect();
    assert!(analyze(&rows, 0, &AccountFlowRequest::default())
        .unwrap_err()
        .to_string()
        .contains("byte bound"));
}
