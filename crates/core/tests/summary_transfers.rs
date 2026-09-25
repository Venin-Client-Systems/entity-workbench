use workbench_core::{analytics, domain::*};

fn row(id: &str, account: &str, amount: &str) -> Transaction {
    Transaction {
        id: id.into(),
        account: account.into(),
        amount: amount.into(),
        date: "2025-01-02".into(),
        posting_date: None,
        description: "Synthetic transfer validation".into(),
        currency: "AUD".into(),
        balance: None,
        anchor: SourceAnchor::Text {
            evidence_id: "synthetic".into(),
            line_start: 2,
            line_end: 2,
        },
        review: ReviewState::Accepted,
        duplicate_candidates: vec![],
        transfer_peer: None,
        merchant: None,
        version: 1,
    }
}
fn pair() -> Vec<Transaction> {
    let mut debit = row("debit", "00001", "-0.10000001");
    let mut credit = row("credit", "00002", "0.10000001");
    debit.transfer_peer = Some(credit.id.clone());
    credit.transfer_peer = Some(debit.id.clone());
    vec![debit, credit]
}

#[test]
fn summary_refuses_invalid_transfer_markers_instead_of_silently_excluding_money() {
    for case in 0..10 {
        let mut rows = pair();
        match case {
            0 => {
                rows.pop();
            }
            1 => rows[1].transfer_peer = None,
            2 => rows[1].currency = "USD".into(),
            3 => rows[1].account = rows[0].account.clone(),
            4 => rows[1].amount = "0.10000002".into(),
            5 => rows[0].transfer_peer = Some(rows[0].id.clone()),
            6 => rows[1].review = ReviewState::Pending,
            7 => rows[1].review = ReviewState::Rejected,
            8 => rows[1].review = ReviewState::Deferred,
            9 => {
                rows[0].amount = "0".into();
                rows[1].amount = "0".into();
            }
            _ => unreachable!(),
        }
        assert!(
            analytics::analyse(&rows).is_err(),
            "invalid pair case {case} was silently excluded"
        );
    }
}

#[test]
fn summary_preserves_valid_exact_pairs_repeated_purchases_and_review_denominators() {
    let mut rows = pair();
    rows.push(row("purchase1", "00001", "-0.10000001"));
    rows.push(row("purchase2", "00001", "-0.10000001"));
    let mut pending = row("pending", "00001", "99.00");
    pending.review = ReviewState::Pending;
    rows.push(pending);
    let result = analytics::analyse(&rows).unwrap();
    assert_eq!(result.pending, 1);
    let total = &result.totals[0];
    assert_eq!(total.currency, "AUD");
    assert_eq!(total.credits, "0");
    assert_eq!(total.debits, "0.20000002");
    assert_eq!(total.net, "-0.20000002");
    assert_eq!(total.excluded_transfer_ids, ["debit", "credit"]);
    assert_eq!(total.transaction_ids, ["purchase1", "purchase2"]);
}

#[test]
fn summary_refuses_ambiguous_duplicate_ids_before_resolving_transfer_peers() {
    let mut rows = pair();
    let mut duplicate = rows[1].clone();
    duplicate.amount = "90".into();
    rows.push(duplicate);
    assert!(analytics::analyse(&rows).is_err());
}
