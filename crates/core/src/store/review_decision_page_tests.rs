use super::*;

fn workspace() -> (tempfile::TempDir, Workspace, Vec<Transaction>) {
    let temp = tempfile::tempdir_in(std::env::temp_dir().canonicalize().unwrap()).unwrap();
    let mut workspace = Workspace::open(temp.path().join("case")).unwrap();
    workspace.import("synthetic.csv", b"account,date,description,amount,currency\n0001,2025-01-01,First,-1.00,AUD\n0002,2025-01-02,Second,1.00,AUD\n").unwrap();
    let rows = workspace.view().unwrap().transactions;
    (temp, workspace, rows)
}
fn request(target: &str, size: u32) -> ReviewDecisionPageRequest {
    ReviewDecisionPageRequest {
        target_id: target.into(),
        page_size: size,
        cursor: None,
    }
}
fn review(workspace: &mut Workspace, target: &str, why: &str) {
    workspace
        .review_transaction(
            target,
            ReviewState::Accepted,
            why,
            workspace.revision().unwrap(),
        )
        .unwrap();
}
fn history(workspace: &Workspace, target: &str) -> Vec<ReviewDecision> {
    workspace
        .view()
        .unwrap()
        .decisions
        .into_iter()
        .filter(|d| d.target_id == target)
        .collect()
}

#[test]
fn sequence_pages_preserve_all_decisions_once_with_tied_legacy_dates_and_verbatim_text() {
    let (_temp, mut workspace, targets) = workspace();
    for index in 0..57 {
        review(
            &mut workspace,
            &targets[0].id,
            &format!("  Synthetic review {index}\n<script>inert text</script>\t"),
        );
        if index % 9 == 0 {
            review(&mut workspace, &targets[1].id, "Different target");
        }
    }
    workspace
        .conn
        .execute(
            "UPDATE records SET body=json_set(body,'$.at',?) WHERE kind='decision'",
            ["Legacy date spelling retained / same timestamp"],
        )
        .unwrap();
    let expected = history(&workspace, &targets[0].id);
    let revision = workspace.revision().unwrap();
    for size in [1, 2, 7, 25, 50] {
        let mut req = request(&targets[0].id, size);
        let mut collected = Vec::new();
        loop {
            let page = workspace.page_review_decisions(&req, revision).unwrap();
            assert_eq!(page.schema_version, 1);
            assert_eq!(page.workspace_revision, revision);
            assert_eq!(page.scope_count, 57);
            assert_eq!(
                page.resolved_target_kind,
                ReviewDecisionTargetKind::Transaction
            );
            assert!(page.rows.len() <= size as usize);
            collected.extend(page.rows);
            req.cursor = page.next_cursor;
            if req.cursor.is_none() {
                break;
            }
            assert!(collected.len() < 57);
        }
        assert_eq!(
            serde_json::to_value(collected).unwrap(),
            serde_json::to_value(&expected).unwrap()
        );
    }
    assert_eq!(workspace.revision().unwrap(), revision);
    assert!(workspace.conn.is_autocommit());
}

#[test]
fn supported_writer_targets_resolve_without_adding_kind_to_legacy_decisions() {
    let (_temp, mut w, rows) = workspace();
    let entity = w
        .add_entity(
            EntityInput {
                name: "Synthetic one".into(),
                kind: EntityKind::Person,
                identifiers: vec![],
            },
            "Created",
            w.revision().unwrap(),
        )
        .unwrap();
    let other = w
        .add_entity(
            EntityInput {
                name: "Synthetic two".into(),
                kind: EntityKind::Person,
                identifiers: vec![],
            },
            "Created",
            w.revision().unwrap(),
        )
        .unwrap();
    let observation = w
        .add_observation(
            ObservationInput {
                entity_id: entity.clone(),
                field: "description".into(),
                value: "Synthetic source".into(),
                anchor: rows[0].anchor.clone(),
            },
            "Observed",
            w.revision().unwrap(),
        )
        .unwrap();
    let question = w
        .save_question(
            None,
            HypothesisInput {
                question: "Synthetic question?".into(),
                proposition: "Synthetic possibility".into(),
                alternatives: vec![],
                gaps: vec![],
            },
            "Question saved",
            w.revision().unwrap(),
        )
        .unwrap();
    let finding = w
        .add_finding(
            FindingInput {
                title: "Synthetic finding".into(),
                assessment: "Unreviewed synthetic assessment".into(),
                supporting_ids: vec![rows[0].anchor.evidence_id().into()],
                contradicting_ids: vec![],
                limitations: "Synthetic only".into(),
                hypothesis_ids: vec![question.clone()],
            },
            w.revision().unwrap(),
        )
        .unwrap();
    let empty = w
        .page_review_decisions(&request(&finding, 50), w.revision().unwrap())
        .unwrap();
    assert_eq!(
        empty.resolved_target_kind,
        ReviewDecisionTargetKind::Finding
    );
    assert_eq!(empty.scope_count, 0);
    assert!(empty.rows.is_empty() && empty.next_cursor.is_none());
    w.review_finding(&finding, "Reviewed", w.revision().unwrap())
        .unwrap();
    w.merge(&entity, &other, "Synthetic merge", w.revision().unwrap())
        .unwrap();
    let merge = w.view().unwrap().merges[0].id.clone();
    w.reverse_merge(&merge, "Synthetic reversal", w.revision().unwrap())
        .unwrap();
    review(&mut w, &rows[0].id, "Transaction review");
    let job = w
        .queue_document_parse(rows[0].anchor.evidence_id(), &Uuid::new_v4().to_string())
        .unwrap();
    w.cancel_processing_job(&job.id, job.attempt).unwrap();
    w.retry_processing_job(&job.id, job.attempt, "Synthetic manual retry")
        .unwrap();
    let revision = w.revision().unwrap();
    for (target, kind) in [
        (&entity, ReviewDecisionTargetKind::Entity),
        (&observation, ReviewDecisionTargetKind::Observation),
        (&question, ReviewDecisionTargetKind::Hypothesis),
        (&finding, ReviewDecisionTargetKind::Finding),
        (&rows[0].id, ReviewDecisionTargetKind::Transaction),
        (&job.id, ReviewDecisionTargetKind::ProcessingJob),
        (&merge, ReviewDecisionTargetKind::Merge),
    ] {
        let page = w
            .page_review_decisions(&request(target, 50), revision)
            .unwrap();
        assert_eq!(page.resolved_target_kind, kind);
        assert!(page.scope_count > 0);
        assert_eq!(
            serde_json::to_value(page.rows).unwrap(),
            serde_json::to_value(history(&w, target)).unwrap()
        );
    }
}

#[test]
fn missing_unsupported_ambiguous_and_corrupt_target_identity_are_explicit_failures() {
    let (_temp, w, rows) = workspace();
    let revision = w.revision().unwrap();
    for missing in ["missing", rows[0].anchor.evidence_id()] {
        assert!(w
            .page_review_decisions(&request(missing, 50), revision)
            .unwrap_err()
            .to_string()
            .contains("unavailable or unsupported"));
    }
    let duplicate = Entity {
        id: rows[0].id.clone(),
        name: "Synthetic collision".into(),
        kind: EntityKind::Person,
        identifiers: vec![],
        merged_into: None,
    };
    put(&w.conn, "entity", &duplicate.id, &duplicate).unwrap();
    assert!(w
        .page_review_decisions(&request(&rows[0].id, 50), revision)
        .unwrap_err()
        .to_string()
        .contains("ambiguous"));
    w.conn
        .execute("DELETE FROM records WHERE kind='entity'", [])
        .unwrap();
    w.conn.execute("UPDATE records SET body=json_set(body,'$.id','wrong') WHERE kind='transaction' AND id=?", [&rows[0].id]).unwrap();
    assert!(w
        .page_review_decisions(&request(&rows[0].id, 50), revision)
        .unwrap_err()
        .to_string()
        .contains("body identity"));
    // Target identity is a bounded SQL projection; no full target deserialization
    // or claim to validate the target's entire unrelated business record.
    let target = serde_json::json!({"id":rows[1].id,"synthetic_padding":"x".repeat(2*1024*1024)});
    w.conn
        .execute(
            "UPDATE records SET body=? WHERE kind='transaction' AND id=?",
            params![target.to_string(), rows[1].id],
        )
        .unwrap();
    assert_eq!(
        w.page_review_decisions(&request(&rows[1].id, 50), revision)
            .unwrap()
            .scope_count,
        0
    );
    assert!(w.conn.is_autocommit());
}

#[test]
fn cursor_reuse_requires_exact_target_kind_revision_size_and_existing_position() {
    let (_temp, mut w, rows) = workspace();
    for _ in 0..3 {
        review(&mut w, &rows[0].id, "Synthetic review");
    }
    let revision = w.revision().unwrap();
    let first = w
        .page_review_decisions(&request(&rows[0].id, 1), revision)
        .unwrap();
    let mut req = request(&rows[0].id, 1);
    req.cursor = first.next_cursor;
    for changed in [
        ReviewDecisionPageRequest {
            page_size: 2,
            ..req.clone()
        },
        ReviewDecisionPageRequest {
            target_id: rows[1].id.clone(),
            ..req.clone()
        },
    ] {
        assert!(w.page_review_decisions(&changed, revision).is_err());
    }
    let mut cursor = Cursor::decode(req.cursor.as_ref().unwrap(), &first.query_sha256).unwrap();
    for invalid in [
        json!({"schema_version":2,"query_sha256":first.query_sha256,"sequence":cursor.sequence}),
        json!({"schema_version":1,"query_sha256":first.query_sha256,"sequence":cursor.sequence,"unknown":true}),
    ] {
        let mut malformed = req.clone();
        malformed.cursor = Some(
            serde_json::to_vec(&invalid)
                .unwrap()
                .iter()
                .map(|byte| format!("{byte:02x}"))
                .collect(),
        );
        assert!(w.page_review_decisions(&malformed, revision).is_err());
    }
    cursor.sequence = i64::MAX;
    let mut forged = req.clone();
    forged.cursor = Some(cursor.encode().unwrap());
    assert!(w
        .page_review_decisions(&forged, revision)
        .unwrap_err()
        .to_string()
        .contains("target scope"));
    let wrong_kind_hash = query_hash(&req, ReviewDecisionTargetKind::Finding, revision).unwrap();
    cursor.sequence = 1;
    cursor.query_sha256 = wrong_kind_hash;
    forged.cursor = Some(cursor.encode().unwrap());
    assert!(w.page_review_decisions(&forged, revision).is_err());
    let corrected = w.correct_transaction(&rows[0].id, "-2.00", "Synthetic correction", revision);
    corrected.unwrap();
    assert!(matches!(
        w.page_review_decisions(&req, revision),
        Err(Error::Conflict(_))
    ));
    assert!(w.page_review_decisions(&req, revision + 1).is_err());
    assert_eq!(
        w.page_review_decisions(&request(&rows[0].id, 50), revision + 1)
            .unwrap()
            .scope_count,
        4
    );
    assert!(w.conn.is_autocommit());
}

#[test]
fn full_page_body_budget_precedes_decode_and_never_returns_a_partial_prefix() {
    let (_temp, mut w, rows) = workspace();
    for _ in 0..2 {
        review(&mut w, &rows[0].id, "Synthetic review");
    }
    let mut decisions = history(&w, &rows[0].id);
    let revision = w.revision().unwrap();
    // An invalid first typed record plus oversized second record must reject on
    // metadata bounds before trying to decode even the first body.
    w.conn
        .execute(
            "UPDATE records SET body=? WHERE kind='decision' AND id=?",
            params![json!({"target_id":rows[0].id}).to_string(), decisions[0].id],
        )
        .unwrap();
    decisions[1].reason = "x".repeat(MAX_DECISION_PAGE_BODY_BYTES as usize);
    put(&w.conn, "decision", &decisions[1].id, &decisions[1]).unwrap();
    assert!(w
        .page_review_decisions(&request(&rows[0].id, 50), revision)
        .unwrap_err()
        .to_string()
        .contains("1 MiB"));
    // Even a one-row page fails if that complete retained record exceeds the
    // bound. There is no text truncation or implicit skipped decision.
    decisions[0].reason = "x".repeat(MAX_DECISION_PAGE_BODY_BYTES as usize);
    put(&w.conn, "decision", &decisions[0].id, &decisions[0]).unwrap();
    assert!(w
        .page_review_decisions(&request(&rows[0].id, 1), revision)
        .unwrap_err()
        .to_string()
        .contains("1 MiB"));
    for d in &mut decisions {
        d.reason = "r".repeat(600_000);
        put(&w.conn, "decision", &d.id, d).unwrap();
    }
    assert!(w
        .page_review_decisions(&request(&rows[0].id, 50), revision)
        .unwrap_err()
        .to_string()
        .contains("no partial page"));
    let mut req = request(&rows[0].id, 1);
    let first = w.page_review_decisions(&req, revision).unwrap();
    assert_eq!(first.rows[0].reason, decisions[0].reason);
    assert!(first.next_cursor.is_some());
    req.cursor = first.next_cursor;
    let second = w.page_review_decisions(&req, revision).unwrap();
    assert_eq!(second.rows[0].id, decisions[1].id);
    assert!(second.next_cursor.is_none());
    assert!(w.conn.is_autocommit());
}

#[test]
fn malformed_decision_key_state_and_unknown_fields_reject_the_page() {
    let (_temp, mut w, rows) = workspace();
    for _ in 0..2 {
        review(&mut w, &rows[0].id, "Synthetic review");
    }
    let decisions = history(&w, &rows[0].id);
    let revision = w.revision().unwrap();
    for field in ["id", "state", "unknown", "reason", "at"] {
        let mut value = serde_json::to_value(&decisions[1]).unwrap();
        value[field] = if field == "reason" || field == "at" {
            json!(7)
        } else {
            json!("invalid")
        };
        w.conn
            .execute(
                "UPDATE records SET body=? WHERE kind='decision' AND id=?",
                params![value.to_string(), decisions[1].id],
            )
            .unwrap();
        assert!(w
            .page_review_decisions(&request(&rows[0].id, 50), revision)
            .is_err());
        assert!(w.conn.is_autocommit());
    }
    put(&w.conn, "decision", &decisions[1].id, &decisions[1]).unwrap();
    assert_eq!(
        w.page_review_decisions(&request(&rows[0].id, 50), revision)
            .unwrap()
            .rows
            .len(),
        2
    );
}

#[test]
fn hostile_requests_and_dispatch_are_read_only_with_legacy_views_unchanged() {
    let (_temp, mut w, rows) = workspace();
    review(&mut w, &rows[0].id, "Synthetic review");
    let revision = w.revision().unwrap();
    for mode in 0..7 {
        let mut req = request(&rows[0].id, 50);
        match mode {
            0 => req.target_id.clear(),
            1 => req.target_id = "a".repeat(257),
            2 => req.target_id = "bad\0target".into(),
            3 => req.page_size = 0,
            4 => req.page_size = 51,
            5 => req.cursor = Some("x".into()),
            _ => req.cursor = Some("a".repeat(MAX_DECISION_CURSOR_BYTES + 1)),
        }
        assert!(w.page_review_decisions(&req, revision).is_err());
    }
    assert!(w
        .page_review_decisions(&request("' OR 1=1 --", 50), revision)
        .is_err());
    let mut value = serde_json::to_value(request(&rows[0].id, 50)).unwrap();
    value["sql"] = json!("SELECT * FROM records");
    assert!(serde_json::from_value::<ReviewDecisionPageRequest>(value).is_err());
    let before = serde_json::to_value(w.view().unwrap()).unwrap();
    let command = Command::PageReviewDecisions {
        request: request(&rows[0].id, 50),
        expected_revision: revision,
    };
    let full = w.dispatch(command.clone()).unwrap();
    let presentation = w.dispatch_presentation(command).unwrap();
    assert_eq!(full, presentation);
    assert!(full.get("workspace").is_none());
    assert_eq!(serde_json::to_value(w.view().unwrap()).unwrap(), before);
    assert!(w.conn.is_autocommit());
}

#[test]
fn target_count_and_rows_remain_one_snapshot_while_a_real_writer_adds_a_decision() {
    use rusqlite::hooks::{AuthAction, AuthContext, Authorization};
    use std::sync::{Arc, Mutex};
    let (_temp, mut w, rows) = workspace();
    review(&mut w, &rows[0].id, "First synthetic review");
    let expected = history(&w, &rows[0].id);
    let mut writer = Workspace::open(&w.root).unwrap();
    w.conn.pragma_update(None, "journal_mode", "WAL").unwrap();
    let revision = w.revision().unwrap();
    let target = rows[0].id.clone();
    let changed = Arc::new(Mutex::new(false));
    let observed = changed.clone();
    w.conn.authorizer(Some(move |context: AuthContext<'_>| {
        if matches!(
            context.action,
            AuthAction::Read {
                table_name: "records",
                ..
            }
        ) {
            let mut done = observed.lock().unwrap();
            if !*done {
                writer
                    .review_transaction(
                        &target,
                        ReviewState::Deferred,
                        "Concurrent synthetic review",
                        revision,
                    )
                    .unwrap();
                *done = true;
            }
        }
        Authorization::Allow
    }));
    let page = w
        .page_review_decisions(&request(&rows[0].id, 50), revision)
        .unwrap();
    assert!(*changed.lock().unwrap());
    assert_eq!(page.workspace_revision, revision);
    assert_eq!(page.scope_count, 1);
    assert_eq!(
        serde_json::to_value(page.rows).unwrap(),
        serde_json::to_value(expected).unwrap()
    );
    assert_eq!(w.revision().unwrap(), revision + 1);
    assert!(matches!(
        w.page_review_decisions(&request(&rows[0].id, 50), revision),
        Err(Error::Conflict(_))
    ));
    assert_eq!(
        w.page_review_decisions(&request(&rows[0].id, 50), revision + 1)
            .unwrap()
            .scope_count,
        2
    );
    assert!(w.conn.is_autocommit());
}
