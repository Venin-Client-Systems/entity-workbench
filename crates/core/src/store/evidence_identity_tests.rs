use super::*;
use crate::{
    transaction_analysis::TransactionAnalysisRequest,
    transaction_comparison::{DatePeriod, TransactionComparisonRequest},
};

fn workspace() -> (tempfile::TempDir, Workspace, String, String) {
    let temp = tempfile::tempdir_in(std::env::temp_dir().canonicalize().unwrap()).unwrap();
    let mut w = Workspace::open(temp.path().join("case")).unwrap();
    let a = w.import("a.txt", b"Synthetic source A\n").unwrap();
    let b = w.import("b.txt", b"Synthetic source B\n").unwrap();
    (temp, w, a, b)
}

fn substitute(conn: &Connection, a: &str, b: &str) {
    // Both files and B's complete canonical body are valid. Only the lookup
    // binding is hostile; a checksum of B alone cannot detect this substitution.
    conn.execute(
        "UPDATE records SET body=(SELECT body FROM records WHERE kind='evidence' AND id=?1) WHERE kind='evidence' AND id=?2",
        params![b, a],
    ).unwrap();
}

fn state(w: &Workspace) -> (u64, Vec<Vec<String>>) {
    let tables = [
        "SELECT json_array(sequence,kind,id,body) FROM records ORDER BY sequence",
        "SELECT json_array(sequence,kind,id,body,revision) FROM history ORDER BY sequence",
        "SELECT json_array(sequence,revision,action,at) FROM events ORDER BY sequence",
    ];
    let rows = tables
        .iter()
        .map(|sql| {
            w.conn
                .prepare(sql)
                .unwrap()
                .query_map([], |r| r.get(0))
                .unwrap()
                .collect::<std::result::Result<Vec<String>, _>>()
                .unwrap()
        })
        .collect();
    (w.revision().unwrap(), rows)
}

fn anchor(source: &str) -> SourceAnchor {
    SourceAnchor::Text {
        evidence_id: source.into(),
        line_start: 1,
        line_end: 1,
    }
}

fn entity(w: &mut Workspace, name: &str) -> String {
    w.add_entity(
        EntityInput {
            name: name.into(),
            kind: EntityKind::Person,
            identifiers: vec![],
        },
        "Synthetic source binding test",
        w.revision().unwrap(),
    )
    .unwrap()
}

fn finding(source: &str) -> FindingInput {
    FindingInput {
        title: "Synthetic source binding".into(),
        assessment: "A source was cited".into(),
        supporting_ids: vec![source.into()],
        contradicting_ids: vec![],
        limitations: "Synthetic only; no source independence claim".into(),
        hypothesis_ids: vec![],
    }
}

#[test]
fn valid_b_under_a_cannot_authorize_observation_or_finding_review() {
    let (_temp, mut w, a, b) = workspace();
    let person = entity(&mut w, "Synthetic person");
    let input = ObservationInput {
        entity_id: person,
        field: "name".into(),
        value: "Synthetic value".into(),
        anchor: anchor(&a),
    };
    let observation = w
        .add_observation(input.clone(), "Read source A", w.revision().unwrap())
        .unwrap();
    w.review_observation(
        &observation,
        ReviewState::Accepted,
        "Checked source A",
        w.revision().unwrap(),
    )
    .unwrap();
    let direct = w.add_finding(finding(&a), w.revision().unwrap()).unwrap();
    let anchored = w
        .add_finding(finding(&observation), w.revision().unwrap())
        .unwrap();
    substitute(&w.conn, &a, &b);
    let before = state(&w);
    assert!(identity::validate_anchor(&w.conn, &anchor(&a)).is_err());
    assert!(w
        .add_observation(input, "Must refuse substituted source", before.0)
        .is_err());
    assert!(w
        .correct_observation(
            &observation,
            "Changed value",
            anchor(&a),
            "Must refuse",
            before.0
        )
        .is_err());
    assert!(w
        .review_observation(&observation, ReviewState::Accepted, "Must refuse", before.0)
        .is_err());
    for key in [direct, anchored] {
        assert!(w
            .review_finding(&key, "Must refuse substituted source", before.0)
            .is_err());
    }
    assert_eq!(state(&w), before);
    assert!(w.conn.is_autocommit());
}

#[test]
fn valid_b_under_a_cannot_supply_analysis_or_comparison_originals() {
    let (_temp, mut w, _a, b) = workspace();
    let a = w
        .import(
            "statement.csv",
            b"account,date,description,amount,currency\n0042,2025-01-01,Synthetic item,-1.00,AUD\n",
        )
        .unwrap();
    let comparison = TransactionComparisonRequest {
        baseline: DatePeriod {
            from: "2025-01-01".into(),
            through: "2025-01-01".into(),
        },
        comparison: DatePeriod {
            from: "2025-02-01".into(),
            through: "2025-02-01".into(),
        },
        account: None,
        currency: None,
        transfers: crate::transaction_analysis::TransferTreatment::Include,
    };
    w.analyze_transactions(
        &TransactionAnalysisRequest::default(),
        w.revision().unwrap(),
    )
    .unwrap();
    w.compare_transaction_periods(&comparison, w.revision().unwrap())
        .unwrap();
    substitute(&w.conn, &a, &b);
    let before = state(&w);
    assert!(w
        .analyze_transactions(&TransactionAnalysisRequest::default(), before.0)
        .is_err());
    assert!(w
        .compare_transaction_periods(&comparison, before.0)
        .is_err());
    assert_eq!(state(&w), before);
    assert!(w.conn.is_autocommit());
}

#[test]
fn valid_b_under_a_cannot_be_returned_or_backed_up_as_a() {
    let (_temp, mut w, a, b) = workspace();
    let left = entity(&mut w, "Synthetic left");
    let right = entity(&mut w, "Synthetic right");
    let report_id = w.save_report().unwrap();
    let report: ReportSnapshot = get(&w.conn, "report", &report_id).unwrap();
    let export = w.root.join("exports").join(format!("{report_id}.html"));
    let exported = fs::read(&export).unwrap();
    substitute(&w.conn, &a, &b);
    let before = state(&w);
    assert!(w.view().is_err());
    assert!(w.presentation().is_err());
    assert!(w.desktop_summary().is_err());
    assert!(w.compare_entities(&left, &right).is_err());
    assert!(w.backup().is_err());
    assert!(w.save_report().is_err());
    assert_eq!(state(&w), before);
    assert_eq!(fs::read(export).unwrap(), exported);
    assert_eq!(
        get::<ReportSnapshot>(&w.conn, "report", &report_id)
            .unwrap()
            .html,
        report.html
    );
    assert_eq!(fs::read_dir(w.root.join("exports")).unwrap().count(), 1);
    assert_eq!(
        fs::read(w.root.join("originals").join(&a)).unwrap(),
        b"Synthetic source A\n"
    );
    assert_eq!(
        fs::read(w.root.join("originals").join(&b)).unwrap(),
        b"Synthetic source B\n"
    );
    assert!(fs::read_dir(w.root.join("backups"))
        .unwrap()
        .all(|entry| !entry.unwrap().path().join("manifest.json").exists()));
}

#[test]
fn valid_b_under_a_cannot_retarget_new_jobs_or_import_deduplication() {
    let (_temp, mut w, a, b) = workspace();
    substitute(&w.conn, &a, &b);
    let before = state(&w);
    assert!(w.queue_document_parse(&a, &id()).is_err());
    assert!(w.queue_image_ocr(&a, &id()).is_err());
    assert!(w.queue_image_ocr_regions(&a, &id()).is_err());
    assert!(w.queue_pdf_page_ocr(&a, &id(), 1, 144).is_err());
    assert!(w.import("a-again.txt", b"Synthetic source A\n").is_err());
    assert_eq!(state(&w), before);
}

#[test]
fn restore_refuses_substituted_body_even_when_manifest_and_both_files_exist() {
    let (temp, mut w, a, b) = workspace();
    let backup = w.backup().unwrap();
    let snapshot = Connection::open(backup.join("workspace.db")).unwrap();
    substitute(&snapshot, &a, &b);
    snapshot
        .execute("DELETE FROM records WHERE kind='evidence' AND id=?", [&b])
        .unwrap();
    drop(snapshot);
    // An adversary also replaces the manifest's evidence list to match the
    // body-only reader. A and B's intact original files remain in the backup.
    let manifest_path = backup.join("manifest.json");
    let mut manifest: Value = serde_json::from_slice(&fs::read(&manifest_path).unwrap()).unwrap();
    manifest["evidence"] = json!([b]);
    fs::write(manifest_path, serde_json::to_vec(&manifest).unwrap()).unwrap();
    let destination = temp.path().join("restored");
    assert!(Workspace::restore(&backup, &destination).is_err());
    assert!(!destination.join("workspace.db").exists());
}

#[test]
fn malformed_existing_evidence_is_not_treated_as_missing_by_import() {
    let (_temp, mut w, a, _b) = workspace();
    w.conn
        .execute(
            "UPDATE records SET body='{}' WHERE kind='evidence' AND id=?",
            [&a],
        )
        .unwrap();
    let before = state(&w);
    assert!(w.import("a-again.txt", b"Synthetic source A\n").is_err());
    assert_eq!(state(&w), before);
}

#[test]
fn metadata_readers_preserve_order_and_missing_semantics_without_hashing_files() {
    let (_temp, mut w, a, b) = workspace();
    assert!(find_evidence(&w.conn, "missing").unwrap().is_none());
    assert!(get_evidence(&w.conn, "missing")
        .unwrap_err()
        .to_string()
        .contains("Unknown evidence"));
    let before = state(&w);
    assert_eq!(
        w.import("duplicate.txt", b"Synthetic source A\n").unwrap(),
        a
    );
    assert_eq!(state(&w), before);
    let c = w.import("new.txt", b"Synthetic source C\n").unwrap();
    assert_eq!(
        all_evidence(&w.conn)
            .unwrap()
            .iter()
            .map(|e| e.id.clone())
            .collect::<Vec<_>>(),
        [a.clone(), b, c]
    );
    let original = get_evidence(&w.conn, &a).unwrap();
    w.verify_original(&original).unwrap();
    fs::remove_file(w.root.join("originals").join(&a)).unwrap();
    assert!(get_evidence(&w.conn, &a).is_ok());
    assert!(w.view().is_ok());
    assert!(w.verify_original(&original).is_err());
    w.conn
        .execute(
            "UPDATE records SET body='{}' WHERE kind='evidence' AND id=?",
            [&a],
        )
        .unwrap();
    assert!(find_evidence(&w.conn, &a).is_err());
    assert!(get_evidence(&w.conn, &a).is_err());
    assert!(all_evidence(&w.conn).is_err());
}

#[test]
fn metadata_readers_reject_body_digest_retargeting_and_invalid_digest_shape() {
    let (_temp, w, a, b) = workspace();
    let original = get_evidence(&w.conn, &a).unwrap();
    w.conn
        .execute(
            "UPDATE records SET body=json_set(body,'$.sha256',?1) WHERE kind='evidence' AND id=?2",
            params![b, a],
        )
        .unwrap();
    assert!(get_evidence(&w.conn, &a).is_err());
    assert!(all_evidence(&w.conn).is_err());
    let mut malformed = original;
    malformed.id = "not-a-digest".into();
    malformed.sha256 = malformed.id.clone();
    w.conn
        .execute("DELETE FROM records WHERE kind='evidence' AND id=?", [&a])
        .unwrap();
    put(&w.conn, "evidence", &malformed.id, &malformed).unwrap();
    assert!(get_evidence(&w.conn, &malformed.id).is_err());
    assert!(all_evidence(&w.conn).is_err());
}

#[test]
fn filtered_out_transfer_peer_still_requires_its_exact_evidence_key() {
    let (_temp, mut w, _a, substitute_id) = workspace();
    w.import("outgoing.csv", b"account,date,description,amount,currency\n0042,2025-01-01,Synthetic transfer,-10.00,AUD\n").unwrap();
    let peer_source = w.import("incoming.csv", b"account,date,description,amount,currency\n0043,2025-02-01,Synthetic transfer,10.00,AUD\n").unwrap();
    let rows = w.view().unwrap().transactions;
    for row in &rows {
        w.review_transaction(
            &row.id,
            ReviewState::Accepted,
            "Synthetic transfer source checked",
            w.revision().unwrap(),
        )
        .unwrap();
    }
    w.match_transfer(
        &rows[0].id,
        &rows[1].id,
        "Synthetic same-currency pair",
        w.revision().unwrap(),
    )
    .unwrap();
    let request = TransactionAnalysisRequest {
        account: Some("0042".into()),
        date_to: Some("2025-01-31".into()),
        transfers: crate::transaction_analysis::TransferTreatment::ExcludeReviewedPairs,
        ..Default::default()
    };
    let analysis = w
        .analyze_transactions(&request, w.revision().unwrap())
        .unwrap();
    assert_eq!(analysis.scope_transaction_count, 1);
    assert_eq!(
        analysis.currencies[0].excluded_transfer_ids,
        [rows[0].id.clone()]
    );
    substitute(&w.conn, &peer_source, &substitute_id);
    let before = state(&w);
    assert!(w.analyze_transactions(&request, before.0).is_err());
    assert_eq!(state(&w), before);
    assert!(w.conn.is_autocommit());
}
