use tempfile::TempDir;
use workbench_core::{domain::*, store::Workspace};
fn workspace() -> (TempDir, Workspace) {
    let temp = TempDir::new_in(std::env::temp_dir().canonicalize().unwrap()).unwrap();
    let w = Workspace::open(temp.path().join("case")).unwrap();
    (temp, w)
}
fn question() -> HypothesisInput {
    HypothesisInput {
        question: "Do these records describe one person?".into(),
        proposition: "Identity remains unresolved".into(),
        alternatives: vec!["Namesakes".into(), "Source error".into()],
        gaps: vec!["Independent confirming record".into()],
    }
}
fn finding(source: &str) -> FindingInput {
    FindingInput {
        title: "Conflicting source records".into(),
        assessment: "Different birth years require further review".into(),
        supporting_ids: vec![source.into()],
        contradicting_ids: vec![],
        limitations: "Source origin does not establish independence".into(),
        hypothesis_ids: vec![],
    }
}
#[test]
fn questions_findings_review_and_edits_preserve_previous_snapshots() {
    let (temp, mut w) = workspace();
    let source = w
        .import("source.txt", b"Synthetic source <script>unsafe()</script>")
        .unwrap();
    let q = w
        .save_question(
            None,
            question(),
            "Record investigation scope",
            w.revision().unwrap(),
        )
        .unwrap();
    let mut input = finding(&source);
    input.hypothesis_ids.push(q.clone());
    let id = w.add_finding(input.clone(), w.revision().unwrap()).unwrap();
    assert!(w.view().unwrap().findings[0].needs_review);
    w.review_finding(
        &id,
        "Inspected whole source and retained limitations",
        w.revision().unwrap(),
    )
    .unwrap();
    assert!(!w.view().unwrap().findings[0].needs_review);
    assert!(w
        .review_finding(&id, "Repeated review", w.revision().unwrap())
        .is_err());
    w.save_report().unwrap();
    let report = w.view().unwrap().reports[0].clone();
    assert!(report.html.contains("Whole source: source.txt"));
    assert!(report.html.contains(&format!("href=\"#{q}\"")));
    assert!(report.html.contains("&lt;script&gt;"));
    assert!(!report.html.contains("<script>"));
    input.assessment = "Revised assessment".into();
    w.update_finding(&id, input, "Explain source conflict", w.revision().unwrap())
        .unwrap();
    assert!(w.view().unwrap().findings[0].needs_review);
    w.review_finding(&id, "Reviewed revision", w.revision().unwrap())
        .unwrap();
    let mut updated = question();
    updated.gaps.push("Check date transcription".into());
    w.save_question(
        Some(&q),
        updated,
        "Additional enquiry",
        w.revision().unwrap(),
    )
    .unwrap();
    let v = w.view().unwrap();
    assert!(v.findings[0].needs_review);
    assert_eq!(v.reports[0].html, report.html);
    assert_eq!(v.reports[0].sha256, report.sha256);
    assert_eq!(v.decisions.iter().filter(|d| d.target_id == id).count(), 3);
    let backup = w.backup().unwrap();
    let restored = Workspace::restore(&backup, &temp.path().join("restored")).unwrap();
    assert_eq!(restored.view().unwrap().reports[0].html, report.html);
    assert_eq!(
        std::fs::read(temp.path().join("restored/originals").join(source)).unwrap(),
        b"Synthetic source <script>unsafe()</script>"
    );
}
#[test]
fn mixed_support_and_contradictions_require_accepted_records_and_valid_originals() {
    let (_temp, mut w) = workspace();
    w.seed_demo().unwrap();
    let v = w.view().unwrap();
    let o = &v.observations[0];
    let t = &v.transactions[0];
    let mut input = finding(&o.id);
    input.contradicting_ids.push(t.id.clone());
    let id = w.add_finding(input, w.revision().unwrap()).unwrap();
    let before = w.revision().unwrap();
    assert!(w.review_finding(&id, "Sources inspected", before).is_err());
    assert_eq!(w.revision().unwrap(), before);
    w.review_observation(&o.id, ReviewState::Accepted, "Checked source line", before)
        .unwrap();
    w.dispatch(Command::ReviewTransaction {
        id: t.id.clone(),
        state: ReviewState::Accepted,
        reason: "Checked statement row".into(),
        expected_revision: w.revision().unwrap(),
    })
    .unwrap();
    w.review_finding(&id, "Both cited records checked", w.revision().unwrap())
        .unwrap();
    w.correct_observation(
        &o.id,
        "1985",
        o.anchor.clone(),
        "Corrected source interpretation",
        w.revision().unwrap(),
    )
    .unwrap();
    assert!(w.view().unwrap().findings.last().unwrap().needs_review);
    assert!(w
        .review_finding(
            &id,
            "Cannot bypass pending correction",
            w.revision().unwrap()
        )
        .is_err());
}
#[test]
fn invalid_and_stale_assessment_writes_are_atomic() {
    let (_temp, mut w) = workspace();
    let source = w.import("source.txt", b"Synthetic source").unwrap();
    let rev = w.revision().unwrap();
    let mut invalid = finding(&source);
    invalid.contradicting_ids.push(source.clone());
    assert!(w.add_finding(invalid, rev).is_err());
    let mut invalid = finding(&source);
    invalid.hypothesis_ids.push("missing".into());
    assert!(w.add_finding(invalid, rev).is_err());
    assert!(w.add_finding(finding("missing"), rev).is_err());
    let mut invalid = finding(&source);
    invalid.title = "x".repeat(301);
    assert!(w.add_finding(invalid, rev).is_err());
    let mut invalid = question();
    invalid.alternatives.push("Namesakes".into());
    assert!(w.save_question(None, invalid, "Fixture", rev).is_err());
    assert!(w.save_question(None, question(), "", rev).is_err());
    assert_eq!(w.revision().unwrap(), rev);
    let id = w.add_finding(finding(&source), rev).unwrap();
    assert!(w
        .update_finding(&id, finding(&source), "Stale change", rev)
        .is_err());
    assert!(w.review_finding(&id, "Stale review", rev).is_err());
    assert!(w.review_finding(&id, "", rev + 1).is_err());
    assert_eq!(w.revision().unwrap(), rev + 1);
    assert!(w.view().unwrap().decisions.is_empty());
}
#[test]
fn review_refuses_tampered_retained_evidence() {
    let (temp, mut w) = workspace();
    let source = w.import("source.txt", b"Synthetic source").unwrap();
    let id = w
        .add_finding(finding(&source), w.revision().unwrap())
        .unwrap();
    let rev = w.revision().unwrap();
    let original = temp.path().join("case/originals").join(source);
    std::fs::remove_file(&original).unwrap();
    std::fs::write(original, b"Tampered").unwrap();
    assert!(w
        .review_finding(&id, "Cannot review altered original", rev)
        .is_err());
    assert_eq!(w.revision().unwrap(), rev);
    assert!(w.view().unwrap().findings[0].needs_review);
}
#[test]
fn schema_two_upgrade_reopens_legacy_findings_and_preserves_report_and_originals() {
    let (temp, mut w) = workspace();
    let original = b"Synthetic legacy original";
    let source = w.import("source.txt", original).unwrap();
    let id = w
        .add_finding(finding(&source), w.revision().unwrap())
        .unwrap();
    w.review_finding(&id, "Legacy review", w.revision().unwrap())
        .unwrap();
    w.save_report().unwrap();
    let report = w.view().unwrap().reports[0].clone();
    let rev = w.revision().unwrap();
    drop(w);
    let db = temp.path().join("case/workspace.db");
    let conn = rusqlite::Connection::open(&db).unwrap();
    conn.pragma_update(None, "user_version", 2).unwrap();
    conn.execute(
        "UPDATE records SET body=json_remove(body,'$.hypothesis_ids') WHERE kind='finding'",
        [],
    )
    .unwrap();
    drop(conn);
    let w = Workspace::open(temp.path().join("case")).unwrap();
    let v = w.view().unwrap();
    assert_eq!(v.schema_version, 3);
    assert_eq!(v.revision, rev + 1);
    assert!(v.findings[0].needs_review);
    assert!(v.findings[0].hypothesis_ids.is_empty());
    assert_eq!(v.reports[0].html, report.html);
    let backup = std::fs::read_dir(temp.path().join("case/backups"))
        .unwrap()
        .next()
        .unwrap()
        .unwrap()
        .path();
    let conn = rusqlite::Connection::open(backup.join("workspace.db")).unwrap();
    assert_eq!(
        conn.pragma_query_value::<u32, _>(None, "user_version", |r| r.get(0))
            .unwrap(),
        2
    );
    let body: String = conn
        .query_row("SELECT body FROM records WHERE kind='finding'", [], |r| {
            r.get(0)
        })
        .unwrap();
    assert!(!body.contains("hypothesis_ids"));
    assert!(body.contains("\"needs_review\":false"));
    drop(conn);
    let restored = Workspace::restore(&backup, &temp.path().join("restored")).unwrap();
    assert_eq!(restored.view().unwrap().reports[0].sha256, report.sha256);
    assert_eq!(
        std::fs::read(temp.path().join("restored/originals").join(source)).unwrap(),
        original
    );
}

#[test]
fn failed_schema_two_upgrade_rolls_back_finding_review_and_recovers_from_backup() {
    let (temp, mut w) = workspace();
    let source = w.import("legacy.txt", b"Synthetic legacy source").unwrap();
    let id = w
        .add_finding(finding(&source), w.revision().unwrap())
        .unwrap();
    w.review_finding(&id, "Reviewed retained source", w.revision().unwrap())
        .unwrap();
    let rev = w.revision().unwrap();
    drop(w);
    let db = temp.path().join("case/workspace.db");
    let conn = rusqlite::Connection::open(&db).unwrap();
    conn.pragma_update(None, "user_version", 2).unwrap();
    conn.execute_batch("CREATE TRIGGER fail_upgrade BEFORE INSERT ON events WHEN NEW.action='workspace.schema_v3' BEGIN SELECT RAISE(ABORT,'synthetic failure'); END;").unwrap();
    drop(conn);
    assert!(Workspace::open(temp.path().join("case")).is_err());
    let conn = rusqlite::Connection::open(&db).unwrap();
    assert_eq!(
        conn.pragma_query_value::<u32, _>(None, "user_version", |r| r.get(0))
            .unwrap(),
        2
    );
    assert_eq!(
        conn.query_row::<u64, _, _>("SELECT revision FROM meta", [], |r| r.get(0))
            .unwrap(),
        rev
    );
    let body: String = conn
        .query_row("SELECT body FROM records WHERE kind='finding'", [], |r| {
            r.get(0)
        })
        .unwrap();
    assert!(!serde_json::from_str::<Finding>(&body).unwrap().needs_review);
    let backup = std::fs::read_dir(temp.path().join("case/backups"))
        .unwrap()
        .next()
        .unwrap()
        .unwrap()
        .path();
    assert_eq!(
        std::fs::read(backup.join("originals").join(source)).unwrap(),
        b"Synthetic legacy source"
    );
    // The injected trigger models an environmental migration failure. Remove it
    // from the original database, retry, then verify a fresh recovery copy.
    conn.execute_batch("DROP TRIGGER fail_upgrade").unwrap();
    drop(conn);
    let mut recovered = Workspace::open(temp.path().join("case")).unwrap();
    assert!(recovered.view().unwrap().findings[0].needs_review);
    let clean = recovered.backup().unwrap();
    let restored = Workspace::restore(&clean, &temp.path().join("restored")).unwrap();
    assert_eq!(restored.view().unwrap().findings[0].id, id);
    assert_eq!(restored.revision().unwrap(), rev + 1);
}
