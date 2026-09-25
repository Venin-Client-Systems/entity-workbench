use super::*;
use crate::transaction_page::TransactionPageRequest;
use rusqlite::hooks::{AuthAction, AuthContext, Authorization};
use std::sync::{Arc, Mutex};

fn workspace() -> (tempfile::TempDir, Workspace) {
    let temp = tempfile::tempdir_in(std::env::temp_dir().canonicalize().unwrap()).unwrap();
    let workspace = Workspace::open(temp.path().join("case")).unwrap();
    (temp, workspace)
}
fn fixture(w: &mut Workspace) {
    let csv = b"account,date,description,amount,currency,balance\n0001,2025-01-01,Opening,100.00,AUD,100.00\n0001,2025-01-01,Repeated purchase,-0.10000001,AUD,\n0001,2025-01-01,Repeated purchase,-0.10000001,AUD,\n0001,2025-01-02,Closing,0.00,AUD,99.79999998\n0001,2025-01-03,Rejected movement,-2.00,AUD,\n0001,2025-01-04,Deferred closing,0.00,AUD,98.00\n0001,2025-01-05,Transfer out,-12.00,AUD,\n0002,2025-01-05,Transfer in,12.00,AUD,\n0003,2025-01-05,Foreign credit,5.25,USD,\n";
    let mapping = crate::statements::sample(csv, crate::statements::Delimiter::Comma)
        .unwrap()
        .suggested_mapping;
    let preview = w.preview_statement("summary.csv", csv, &mapping).unwrap();
    w.import_statement(
        "summary.csv",
        csv,
        mapping,
        &preview.preview_token,
        Some("Synthetic summary mapping"),
        w.revision().unwrap(),
    )
    .unwrap();
    let rows = w.view().unwrap().transactions;
    for (row, state) in rows.iter().zip([
        ReviewState::Accepted,
        ReviewState::Accepted,
        ReviewState::Accepted,
        ReviewState::Pending,
        ReviewState::Rejected,
        ReviewState::Deferred,
        ReviewState::Accepted,
        ReviewState::Accepted,
        ReviewState::Accepted,
    ]) {
        w.review_transaction(&row.id, state, "Synthetic review", w.revision().unwrap())
            .unwrap();
    }
    w.match_transfer(
        &rows[6].id,
        &rows[7].id,
        "Synthetic transfer",
        w.revision().unwrap(),
    )
    .unwrap();
    // Same account/currency in a different source must start a separate balance window.
    w.import("second.csv", b"account,date,description,amount,currency,balance\n0001,2025-01-06,Separate opening,1.00,AUD,500.00\n").unwrap();
    w.save_report().unwrap();
}
fn assert_projection(w: &Workspace) {
    let view = w.presentation().unwrap();
    let legacy = analytics::analyse(&view.transactions).unwrap();
    let summary = w.desktop_summary().unwrap();
    assert_eq!(summary.schema_version, 1);
    let mut expected = serde_json::to_value(&view).unwrap();
    let object = expected.as_object_mut().unwrap();
    object.remove("transactions");
    object.remove("decisions");
    object.insert("review_decision_count".into(), json!(view.decisions.len()));
    assert_eq!(serde_json::to_value(&summary.workspace).unwrap(), expected);
    assert_eq!(
        summary.analysis.transaction_count,
        view.transactions.len() as u64
    );
    assert_eq!(
        summary.analysis.review_counts.pending,
        legacy.pending as u64
    );
    assert_eq!(
        summary.analysis.duplicate_candidate_row_count,
        legacy.duplicate_candidates as u64
    );
    assert_eq!(
        summary.analysis.balance_check_count,
        legacy.balance_checks.len() as u64
    );
    assert_eq!(
        summary.analysis.balance_discrepancy_count,
        legacy
            .balance_checks
            .iter()
            .filter(|row| !row.reconciled)
            .count() as u64
    );
    assert_eq!(summary.analysis.totals.len(), legacy.totals.len());
    for (actual, expected) in summary.analysis.totals.iter().zip(&legacy.totals) {
        assert_eq!(
            (
                &actual.currency,
                &actual.credits,
                &actual.debits,
                &actual.net
            ),
            (
                &expected.currency,
                &expected.credits,
                &expected.debits,
                &expected.net
            )
        );
        assert_eq!(actual.included_count, expected.transaction_ids.len() as u64);
        assert_eq!(
            actual.excluded_transfer_count,
            expected.excluded_transfer_ids.len() as u64
        );
    }
    let wire = serde_json::to_value(&summary).unwrap();
    assert!(wire["workspace"].get("transactions").is_none());
    assert!(wire["workspace"].get("decisions").is_none());
    assert!(wire["analysis"].get("balance_checks").is_none());
    for total in wire["analysis"]["totals"].as_array().unwrap() {
        assert!(total.get("transaction_ids").is_none());
        assert!(total.get("excluded_transfer_ids").is_none());
    }
    let decoded: DesktopSummaryResponse = serde_json::from_value(wire.clone()).unwrap();
    assert_eq!(serde_json::to_value(decoded).unwrap(), wire);
    let mut unexpected = wire;
    unexpected["workspace"]["transactions"] = json!([]);
    assert!(serde_json::from_value::<DesktopSummaryResponse>(unexpected).is_err());
}
#[test]
fn summary_moves_retained_fields_and_preserves_difficult_calculations_exactly() {
    let (_temp, mut w) = workspace();
    fixture(&mut w);
    assert_projection(&w);
    let s = w.desktop_summary().unwrap();
    assert_eq!(s.analysis.transaction_count, 10);
    assert_eq!(
        s.analysis.review_counts,
        TransactionReviewCounts {
            accepted: 6,
            pending: 2,
            rejected: 1,
            deferred: 1,
        }
    );
    assert_eq!(s.analysis.duplicate_candidate_row_count, 2);
    assert_eq!(s.analysis.balance_check_count, 2);
    assert_eq!(s.analysis.balance_discrepancy_count, 1);
    assert_eq!(
        s.analysis.totals[0],
        ReviewedCurrencySummary {
            currency: "AUD".into(),
            credits: "100.00".into(),
            debits: "0.20000002".into(),
            net: "99.79999998".into(),
            included_count: 3,
            excluded_transfer_count: 2,
        }
    );
    assert_eq!(s.analysis.totals[1].currency, "USD");
    assert_eq!(s.analysis.totals[1].net, "5.25");
    assert_eq!(s.workspace.review_decision_count, 10);
    assert!(s.workspace.evidence[0].text.is_some());
    assert_eq!(s.workspace.statement_imports[0].transaction_ids.len(), 9);
    assert_eq!(s.workspace.statement_profiles.len(), 1);
    assert_eq!(s.workspace.reports.len(), 1);
}
#[test]
fn empty_and_pending_only_summaries_keep_different_denominators() {
    let (_temp, mut w) = workspace();
    assert_projection(&w);
    assert_eq!(w.desktop_summary().unwrap().analysis.transaction_count, 0);
    w.import(
        "pending.csv",
        b"account,date,description,amount,currency\nA,2025-01-01,Pending,1.00,AUD\n",
    )
    .unwrap();
    assert_projection(&w);
    let s = w.desktop_summary().unwrap();
    assert_eq!(s.analysis.transaction_count, 1);
    assert_eq!(s.analysis.review_counts.pending, 1);
    assert!(s.analysis.totals.is_empty());
    assert_eq!(s.workspace.review_decision_count, 0);
}
#[test]
fn invalid_and_unrepresentable_money_fail_as_legacy_without_writing_or_leaking_snapshot() {
    let (_temp, mut w) = workspace();
    w.import("invalid.csv", b"account,date,description,amount,currency\nA,2025-01-01,First,1.00,AUD\nA,2025-01-01,Second,1.00,AUD\n").unwrap();
    let rows = w.view().unwrap().transactions;
    for row in &rows {
        w.review_transaction(
            &row.id,
            ReviewState::Accepted,
            "Synthetic review",
            w.revision().unwrap(),
        )
        .unwrap();
    }
    let before = w.revision().unwrap();
    for (field, value) in [
        ("amount", "NaN"),
        ("date", "2025-02-30"),
        ("amount", "79228162514264337593543950335"),
    ] {
        let mut changed: Transaction = get(&w.conn, "transaction", &rows[0].id).unwrap();
        if field == "amount" {
            changed.amount = value.into();
        } else {
            changed.date = value.into();
        }
        put(&w.conn, "transaction", &changed.id, &changed).unwrap();
        let expected = w.dispatch(Command::View {}).unwrap_err().to_string();
        assert_eq!(
            w.dispatch_presentation(Command::View {})
                .unwrap_err()
                .to_string(),
            expected
        );
        assert_eq!(
            w.dispatch_summary(Command::View {})
                .unwrap_err()
                .to_string(),
            expected
        );
        assert_eq!(w.revision().unwrap(), before);
        assert!(w.conn.is_autocommit());
        changed.amount = "1.00".into();
        changed.date = "2025-01-01".into();
        put(&w.conn, "transaction", &changed.id, &changed).unwrap();
    }
    w.import("after-error.txt", b"Synthetic after rejected calculation")
        .unwrap();
    assert_eq!(w.desktop_summary().unwrap().workspace.revision, before + 1);
    // Decode failure occurs inside the SQL snapshot, not just during calculation.
    w.conn
        .execute(
            "INSERT INTO records(kind,id,body) VALUES('entity','broken','{}')",
            [],
        )
        .unwrap();
    assert!(w.desktop_summary().is_err());
    assert!(w.conn.is_autocommit());
}
#[test]
fn concurrent_canonical_review_cannot_mix_summary_counts_totals_or_revision() {
    let (_temp, mut reader) = workspace();
    reader
        .import(
            "concurrent.csv",
            b"account,date,description,amount,currency\nA,2025-01-01,Concurrent,7.25,AUD\n",
        )
        .unwrap();
    let before = reader.desktop_summary().unwrap();
    let row = reader.view().unwrap().transactions.remove(0);
    let writer = Workspace::open(&reader.root).unwrap();
    reader
        .conn
        .pragma_update(None, "journal_mode", "WAL")
        .unwrap();
    let shared = Arc::new(Mutex::new((writer, false)));
    let callback = shared.clone();
    reader.conn.authorizer(Some(move |ctx: AuthContext<'_>| {
        if matches!(
            ctx.action,
            AuthAction::Read {
                table_name: "records",
                ..
            }
        ) {
            let mut state = callback.lock().unwrap();
            if !state.1 {
                let revision = state.0.revision().unwrap();
                state
                    .0
                    .review_transaction(
                        &row.id,
                        ReviewState::Accepted,
                        "Concurrent synthetic review",
                        revision,
                    )
                    .unwrap();
                state.1 = true;
            }
        }
        Authorization::Allow
    }));
    let captured = reader.desktop_summary().unwrap();
    assert!(shared.lock().unwrap().1, "positive writer did not commit");
    assert_eq!(
        serde_json::to_value(captured).unwrap(),
        serde_json::to_value(before).unwrap()
    );
    let later = reader.desktop_summary().unwrap();
    assert_eq!(later.workspace.revision, 2);
    assert_eq!(later.workspace.review_decision_count, 1);
    assert_eq!(later.analysis.review_counts.accepted, 1);
    assert_eq!(later.analysis.review_counts.pending, 0);
    assert_eq!(later.analysis.totals[0].net, "7.25");
}
#[test]
fn summary_dispatch_mutates_once_and_keeps_legacy_and_direct_reader_responses() {
    let (_temp, mut w) = workspace();
    fixture(&mut w);
    let legacy = w.dispatch(Command::View {}).unwrap();
    let presentation = w.dispatch_presentation(Command::View {}).unwrap();
    let expected = serde_json::to_value(w.desktop_summary().unwrap()).unwrap();
    assert_eq!(w.dispatch_summary(Command::View {}).unwrap(), expected);
    assert_eq!(w.dispatch(Command::View {}).unwrap(), legacy);
    assert_eq!(
        w.dispatch_presentation(Command::View {}).unwrap(),
        presentation
    );
    let row = w.view().unwrap().transactions.remove(0);
    let revision = w.revision().unwrap();
    let events: u64 = w
        .conn
        .query_row("SELECT count(*) FROM events", [], |r| r.get(0))
        .unwrap();
    let command = Command::ReviewTransaction {
        id: row.id.clone(),
        state: ReviewState::Deferred,
        reason: "Summary mutation exactly once".into(),
        expected_revision: revision,
    };
    let refreshed = w.dispatch_summary(command.clone()).unwrap();
    assert_eq!(refreshed["workspace"]["revision"], revision + 1);
    assert!(w.dispatch_summary(command).is_err());
    let after: u64 = w
        .conn
        .query_row("SELECT count(*) FROM events", [], |r| r.get(0))
        .unwrap();
    assert_eq!(after, events + 1);
    assert_eq!(w.view().unwrap().transactions[0].version, row.version + 1);
    let report = w.view().unwrap().reports.remove(0);
    for command in [
        Command::PageTransactions {
            request: TransactionPageRequest::default(),
            expected_revision: revision + 1,
        },
        Command::InspectReportSnapshot {
            report_id: report.id,
            expected_sha256: report.sha256,
        },
        Command::InspectSource { anchor: row.anchor },
        Command::ListProcessingJobs {},
    ] {
        let full = w.dispatch(command.clone()).unwrap();
        assert_eq!(w.dispatch_presentation(command.clone()).unwrap(), full);
        assert_eq!(w.dispatch_summary(command).unwrap(), full);
    }
}
#[test]
fn queued_job_response_replay_and_cancellation_are_not_summary_wrapped() {
    let (_temp, mut w) = workspace();
    let source = w.import("job.txt", b"Synthetic job input").unwrap();
    let command = Command::QueueDocumentParse {
        evidence_id: source,
        request_key: Uuid::new_v4().to_string(),
    };
    let job = w.dispatch_summary(command.clone()).unwrap();
    assert_eq!(job["state"], "queued");
    assert!(job.get("workspace").is_none());
    let revision = w.revision().unwrap();
    assert_eq!(w.dispatch(command.clone()).unwrap(), job);
    assert_eq!(w.dispatch_presentation(command).unwrap(), job);
    assert_eq!(w.revision().unwrap(), revision);
    let cancelled = w
        .dispatch_summary(Command::CancelProcessingJob {
            job_id: job["id"].as_str().unwrap().into(),
            expected_attempt: 1,
        })
        .unwrap();
    assert_eq!(cancelled["state"], "cancelled");
    assert!(cancelled.get("workspace").is_none());
}

#[test]
fn failed_post_mutation_refresh_does_not_repeat_or_undo_a_committed_write() {
    let (_temp, mut w) = workspace();
    w.import(
        "broken.csv",
        b"account,date,description,amount,currency\nA,2025-01-01,First,1.00,AUD\n",
    )
    .unwrap();
    let mut row = w.view().unwrap().transactions.remove(0);
    row.amount = "invalid".into();
    put(&w.conn, "transaction", &row.id, &row).unwrap();
    let before = w.revision().unwrap();
    let command = Command::ReviewTransaction {
        id: row.id.clone(),
        state: ReviewState::Deferred,
        reason: "Synthetic mutation before failed refresh".into(),
        expected_revision: before,
    };
    assert!(w.dispatch_summary(command.clone()).is_err());
    assert_eq!(w.revision().unwrap(), before + 1);
    let committed: Transaction = get(&w.conn, "transaction", &row.id).unwrap();
    assert_eq!(committed.review, ReviewState::Deferred);
    assert_eq!(committed.version, row.version + 1);
    assert!(matches!(
        w.dispatch_summary(command),
        Err(Error::Conflict(_))
    ));
    assert_eq!(w.revision().unwrap(), before + 1);
    // A direct reader must not implicitly invoke the failed aggregate refresh.
    let jobs = w.dispatch_summary(Command::ListProcessingJobs {}).unwrap();
    assert_eq!(jobs["total"], 0);
    assert!(w.conn.is_autocommit());
}
