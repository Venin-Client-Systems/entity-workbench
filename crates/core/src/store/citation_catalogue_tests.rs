use super::*;

fn workspace() -> (tempfile::TempDir, Workspace) {
    let temp = tempfile::tempdir_in(std::env::temp_dir().canonicalize().unwrap()).unwrap();
    let workspace = Workspace::open(temp.path().join("case")).unwrap();
    (temp, workspace)
}
fn mixed(observations: usize) -> (tempfile::TempDir, Workspace) {
    let (temp, mut w) = workspace();
    let source = w
        .import(
            "SYNTHETIC notices.txt",
            "Synthetic source ΟΣ <script>inert</script>\n".as_bytes(),
        )
        .unwrap();
    w.import("Ledger.csv", b"account,date,description,amount,currency\n0042,2025-01-01,Merchant %_ literal,-001.20,AUD\n0007,2025-01-02,Refund  X,2.000,USD\n").unwrap();
    let entity = w
        .add_entity(
            EntityInput {
                name: "ΟΣ Synthetic person".into(),
                kind: EntityKind::Person,
                identifiers: vec![],
            },
            "Synthetic entity",
            w.revision().unwrap(),
        )
        .unwrap();
    for index in 0..observations {
        w.add_observation(
            ObservationInput {
                entity_id: entity.clone(),
                field: "reference".into(),
                value: format!("Value {index:04} <b>inert</b>"),
                anchor: SourceAnchor::Text {
                    evidence_id: source.clone(),
                    line_start: 1,
                    line_end: 1,
                },
            },
            "Synthetic observation",
            w.revision().unwrap(),
        )
        .unwrap();
    }
    let v = w.view().unwrap();
    for (row, state) in v
        .transactions
        .iter()
        .zip([ReviewState::Accepted, ReviewState::Deferred])
    {
        w.review_transaction(&row.id, state, "Synthetic state", w.revision().unwrap())
            .unwrap();
    }
    if let Some(row) = v.observations.first() {
        w.review_observation(
            &row.id,
            ReviewState::Rejected,
            "Synthetic rejected observation",
            w.revision().unwrap(),
        )
        .unwrap();
    }
    (temp, w)
}
fn request(query: &str, page_size: u32) -> CitationCatalogueRequest {
    CitationCatalogueRequest {
        query: query.into(),
        page_size,
        ..Default::default()
    }
}
fn selected(w: &Workspace, ids: &[String]) -> Result<CitationSelections> {
    w.read_citation_selections(
        &CitationSelectionsRequest { ids: ids.to_vec() },
        w.revision().unwrap(),
    )
}
fn summary_ids(rows: &[CitationSummary]) -> Vec<&str> {
    rows.iter().map(CitationSummary::id).collect()
}
fn ordered_ids(w: &Workspace) -> Vec<String> {
    let v = w.view().unwrap();
    v.observations
        .into_iter()
        .map(|v| v.id)
        .chain(v.transactions.into_iter().map(|v| v.id))
        .chain(v.evidence.into_iter().map(|v| v.id))
        .collect()
}
#[test]
fn pages_and_selected_sets_preserve_kind_sequence_order_and_every_review_state() {
    let (_temp, w) = mixed(57);
    let revision = w.revision().unwrap();
    let expected = ordered_ids(&w);
    for size in [1, 7, 50] {
        let mut req = request("", size);
        let mut all = Vec::new();
        loop {
            let page = w.page_citation_catalogue(&req, revision).unwrap();
            assert_eq!(page.scope_count, expected.len() as u64);
            assert_eq!(page.workspace_revision, revision);
            assert!(page.rows.len() <= size as usize);
            all.extend(page.rows.into_iter().map(|v| v.id().to_owned()));
            req.cursor = page.next_cursor;
            if req.cursor.is_none() {
                break;
            }
            assert!(all.len() < expected.len());
        }
        assert_eq!(all, expected);
    }
    let reversed = expected.iter().rev().cloned().collect::<Vec<_>>();
    let resolved = selected(&w, &reversed).unwrap();
    assert_eq!(
        summary_ids(&resolved.rows),
        expected.iter().map(String::as_str).collect::<Vec<_>>()
    );
    let v = w.view().unwrap();
    for transaction in &v.transactions {
        let row = resolved
            .rows
            .iter()
            .find(|r| r.id() == transaction.id)
            .unwrap();
        if let CitationSummary::Transaction {
            amount,
            account,
            review,
            version,
            anchor,
            ..
        } = row
        {
            assert_eq!(amount, &transaction.amount);
            assert_eq!(account, &transaction.account);
            assert_eq!(review, &transaction.review);
            assert_eq!(*version, transaction.version);
            assert_eq!(
                serde_json::to_value(anchor).unwrap(),
                serde_json::to_value(&transaction.anchor).unwrap()
            );
        } else {
            panic!("transaction changed kind");
        }
    }
    assert!(matches!(
        &resolved.rows[0],
        CitationSummary::Observation {
            review: ReviewState::Rejected,
            ..
        }
    ));
    assert_eq!(w.revision().unwrap(), revision);
}
#[test]
fn literal_search_preserves_display_fields_unicode_whitespace_and_selected_exclusions() {
    let (_temp, w) = mixed(2);
    let revision = w.revision().unwrap();
    let v = w.view().unwrap();
    for (query, count) in [
        ("ΟΣ", 2),
        ("οσ", 0),
        ("account 0042", 1),
        ("%_", 1),
        ("refund  x", 1),
        ("refund x", 0),
        ("-1.20 aud", 1),
        ("rejected", 1),
        ("ledger.csv", 3),
        ("whole source", 2),
        ("<b>inert</b>", 2),
        ("' OR 1=1 --", 0),
        ("no match", 0),
    ] {
        let page = w
            .page_citation_catalogue(&request(query, 50), revision)
            .unwrap();
        assert_eq!(page.scope_count, count, "{query}");
        assert_eq!(page.rows.len() as u64, count);
        assert!(page.next_cursor.is_none());
        assert_eq!(page.matching, LiteralMatching::default());
    }
    let mut req = request("ledger.csv", 1);
    req.excluded_ids = vec![v.transactions[0].id.clone()];
    let page = w.page_citation_catalogue(&req, revision).unwrap();
    assert_eq!(page.scope_count, 2);
    assert_eq!(page.rows[0].id(), v.transactions[1].id);
    let selection = selected(&w, &req.excluded_ids).unwrap();
    assert_eq!(selection.rows[0].id(), v.transactions[0].id);
}
#[test]
fn large_evidence_blobs_are_omitted_and_reads_leave_originals_records_and_old_reports_unchanged() {
    let (_temp, mut w) = workspace();
    let original = "SYNTHETIC omitted text\n".repeat(110_000);
    let source = w.import("large.txt", original.as_bytes()).unwrap();
    let finding = w
        .add_finding(
            FindingInput {
                title: "Synthetic finding".into(),
                assessment: "Whole source only".into(),
                limitations: "No source-independence conclusion".into(),
                supporting_ids: vec![source.clone()],
                contradicting_ids: vec![],
                hypothesis_ids: vec![],
            },
            w.revision().unwrap(),
        )
        .unwrap();
    w.review_finding(&finding, "Reviewed original", w.revision().unwrap())
        .unwrap();
    let report_id = w.save_report().unwrap();
    let before = serde_json::to_value(w.view().unwrap()).unwrap();
    let report = w.view().unwrap().reports[0].clone();
    let result = selected(&w, std::slice::from_ref(&source)).unwrap();
    let bytes = serde_json::to_vec(&result).unwrap();
    assert!(bytes.len() < 1500);
    let value = serde_json::to_value(&result.rows[0]).unwrap();
    assert!(value["source"].get("text").is_none());
    assert!(value["source"].get("acquisitions").is_none());
    assert!(value.get("review").is_none());
    assert_eq!(value["source"]["origin_group"], source);
    w.page_citation_catalogue(&request("", 50), w.revision().unwrap())
        .unwrap();
    assert_eq!(serde_json::to_value(w.view().unwrap()).unwrap(), before);
    assert_eq!(
        fs::read(w.root.join("originals").join(&source)).unwrap(),
        original.as_bytes()
    );
    assert_eq!(
        w.inspect_report_snapshot(&report_id, &report.sha256)
            .unwrap()
            .html,
        report.html
    );
    assert!(w.conn.is_autocommit());
}
#[test]
fn missing_colliding_and_corrupt_citations_fail_without_partial_selection() {
    let (_temp, w) = mixed(1);
    let all = ordered_ids(&w);
    let revision = w.revision().unwrap();
    let mut ids = vec![all[0].clone(), "missing".into()];
    assert!(selected(&w, &ids).is_err());
    ids[1] = w.view().unwrap().entities[0].id.clone();
    assert!(selected(&w, &ids).is_err());
    let target = w.view().unwrap().transactions[0].clone();
    let other = w.view().unwrap().observations[0].clone();
    let mut collision = serde_json::to_value(&other).unwrap();
    collision["id"] = json!(target.id);
    w.conn
        .execute(
            "INSERT INTO records(kind,id,body) VALUES('observation',?,?)",
            params![target.id, collision.to_string()],
        )
        .unwrap();
    assert!(selected(&w, std::slice::from_ref(&target.id))
        .unwrap_err()
        .to_string()
        .contains("ambiguous"));
    assert!(w
        .page_citation_catalogue(&request("", 50), revision)
        .is_err());
    w.conn
        .execute(
            "DELETE FROM records WHERE kind='observation' AND id=?",
            [&target.id],
        )
        .unwrap();
    for (field, value) in [
        ("id", json!("wrong")),
        ("amount", json!([1])),
        ("amount", json!("7922816251426433759354395033.6")),
        ("date", json!("2025-01- 1")),
        ("version", json!(0)),
        ("review", json!({"state":"accepted"})),
    ] {
        let mut corrupt = serde_json::to_value(&target).unwrap();
        corrupt[field] = value;
        w.conn
            .execute(
                "UPDATE records SET body=? WHERE kind='transaction' AND id=?",
                params![corrupt.to_string(), target.id],
            )
            .unwrap();
        assert!(
            selected(&w, &[other.id.clone(), target.id.clone()]).is_err(),
            "{field}"
        );
        assert!(w.conn.is_autocommit());
    }
    w.conn
        .execute(
            "UPDATE records SET body=? WHERE kind='transaction' AND id=?",
            params![serde_json::to_string(&target).unwrap(), target.id],
        )
        .unwrap();
    w.conn
        .execute(
            "UPDATE records SET body=json_set(body,'$.id','wrong') WHERE kind='evidence' AND id=?",
            [target.anchor.evidence_id()],
        )
        .unwrap();
    assert!(selected(&w, std::slice::from_ref(&target.id)).is_err());
}
#[test]
fn selected_preflight_is_complete_and_byte_short_catalogue_pages_do_not_omit_rows() {
    let (_temp, mut w) = workspace();
    let a = w.import("a.txt", b"Synthetic A").unwrap();
    let b = w.import("b.txt", b"Synthetic B").unwrap();
    let long = "x".repeat(MAX_CITATION_PROJECTION_BYTES / 2 + 100);
    w.conn
        .execute(
            "UPDATE records SET body=json_set(body,'$.name',?) WHERE kind='evidence'",
            [&long],
        )
        .unwrap();
    let revision = w.revision().unwrap();
    let mut req = request("", 50);
    let page = w.page_citation_catalogue(&req, revision).unwrap();
    assert_eq!(page.scope_count, 2);
    assert_eq!(summary_ids(&page.rows), vec![a.as_str()]);
    req.cursor = page.next_cursor;
    let tail = w.page_citation_catalogue(&req, revision).unwrap();
    assert_eq!(summary_ids(&tail.rows), vec![b.as_str()]);
    assert!(tail.next_cursor.is_none());
    // An invalid first typed body must not be decoded before aggregate preflight.
    w.conn.execute("UPDATE records SET body=json_set(body,'$.bytes','invalid') WHERE kind='evidence' AND id=?",[&a]).unwrap();
    let error = selected(&w, &[a.clone(), b]).unwrap_err().to_string();
    assert!(error.contains("Selected citations exceed"), "{error}");
    w.conn
        .execute(
            "UPDATE records SET body=json_set(body,'$.name',?) WHERE kind='evidence' AND id=?",
            params!["x".repeat(MAX_CITATION_PROJECTION_BYTES + 1), a],
        )
        .unwrap();
    assert!(w
        .page_citation_catalogue(&request("", 50), revision)
        .unwrap_err()
        .to_string()
        .contains("projection exceeds"));
}
#[test]
fn hostile_requests_stale_queries_and_forged_cursor_positions_fail() {
    let (_temp, mut w) = mixed(3);
    let revision = w.revision().unwrap();
    let base = request("", 1);
    let first = w.page_citation_catalogue(&base, revision).unwrap();
    let mut req = base.clone();
    req.cursor = first.next_cursor;
    for mode in 0..4 {
        let mut changed = req.clone();
        match mode {
            0 => changed.query = "reference".into(),
            1 => changed.page_size = 2,
            2 => changed.excluded_ids.push(first.rows[0].id().into()),
            _ => changed.cursor = Some("a".repeat(2049)),
        }
        assert!(w.page_citation_catalogue(&changed, revision).is_err());
    }
    let mut cursor = Cursor::decode(req.cursor.as_ref().unwrap(), &first.query_sha256).unwrap();
    cursor.position.sequence = i64::MAX;
    req.cursor = Some(cursor.encode().unwrap());
    assert!(w.page_citation_catalogue(&req, revision).is_err());
    for mode in 0..5 {
        let mut bad = base.clone();
        match mode {
            0 => bad.query = "é".repeat(129),
            1 => bad.page_size = 0,
            2 => bad.page_size = 51,
            3 => bad.excluded_ids = vec!["duplicate".into(); 2],
            _ => bad.excluded_ids = vec!["bad\0id".into()],
        }
        assert!(w.page_citation_catalogue(&bad, revision).is_err());
    }
    for ids in [
        vec![],
        vec!["same".into(); 2],
        (0..101).map(|i| format!("id{i}")).collect(),
        vec!["x".repeat(257)],
    ] {
        assert!(selected(&w, &ids).is_err());
    }
    let mut unknown = serde_json::to_value(&base).unwrap();
    unknown["sql"] = json!("SELECT body FROM records");
    assert!(serde_json::from_value::<CitationCatalogueRequest>(unknown).is_err());
    w.import("later.txt", b"Synthetic next revision").unwrap();
    assert!(w.page_citation_catalogue(&base, revision).is_err());
    assert!(w
        .read_citation_selections(
            &CitationSelectionsRequest {
                ids: vec![first.rows[0].id().into()]
            },
            revision
        )
        .is_err());
    w.conn
        .execute(
            "UPDATE records SET sequence=0 WHERE kind='observation' AND id=?",
            [first.rows[0].id()],
        )
        .unwrap();
    assert!(w
        .page_citation_catalogue(&base, w.revision().unwrap())
        .unwrap_err()
        .to_string()
        .contains("sequence is invalid"));
}
#[test]
fn source_groups_and_legacy_metadata_are_preserved_without_accepting_new_anchor_kinds() {
    let (_temp, w) = mixed(2);
    let v = w.view().unwrap();
    let ids = vec![
        v.observations[1].id.clone(),
        v.evidence[0].id.clone(),
        v.observations[0].id.clone(),
    ];
    let result = selected(&w, &ids).unwrap();
    let groups = result
        .rows
        .iter()
        .map(|r| &r.source().origin_group)
        .collect::<BTreeSet<_>>();
    assert_eq!(groups.len(), 1);
    w.conn
        .execute("DELETE FROM records WHERE kind='entity'", [])
        .unwrap();
    let result = selected(&w, std::slice::from_ref(&v.observations[0].id)).unwrap();
    assert!(matches!(
        &result.rows[0],
        CitationSummary::Observation {
            entity_name: None,
            ..
        }
    ));
    let page_anchor = json!({"kind":"page","evidence_id":v.evidence[0].id,"page":1,"region":null});
    w.conn.execute("UPDATE records SET body=json_set(body,'$.anchor',json(?)) WHERE kind='observation' AND id=?",params![page_anchor.to_string(),v.observations[0].id]).unwrap();
    let result = selected(&w, std::slice::from_ref(&v.observations[0].id)).unwrap();
    let CitationSummary::Observation { anchor, .. } = &result.rows[0] else {
        panic!("kind")
    };
    assert!(w.inspect_source(anchor).is_err());
    w.conn.execute("UPDATE records SET body=json_set(body,'$.extraction_status','acquisition_only') WHERE kind='evidence'",[]).unwrap();
    assert_eq!(
        w.page_citation_catalogue(&request("acquisition_only", 50), w.revision().unwrap())
            .unwrap()
            .scope_count,
        2
    );
}
#[test]
fn catalogue_and_selected_resolution_remain_one_snapshot_with_a_real_concurrent_writer() {
    use rusqlite::hooks::{AuthAction, AuthContext, Authorization};
    use std::sync::{Arc, Mutex};
    for selected_read in [false, true] {
        let (_temp, mut w) = workspace();
        let source = w.import("before.csv", b"account,date,description,amount,currency\n0042,2025-01-01,Synthetic before,-1.00,AUD\n").unwrap();
        let target = w.view().unwrap().transactions[0].clone();
        let writer_target = target.id.clone();
        let mut writer = Workspace::open(&w.root).unwrap();
        // Workspace::open selects DELETE mode; enable WAL after both opens.
        w.conn.pragma_update(None, "journal_mode", "WAL").unwrap();
        let revision = w.revision().unwrap();
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
                    if selected_read {
                        writer
                            .review_transaction(
                                &writer_target,
                                ReviewState::Deferred,
                                "Concurrent synthetic review",
                                revision,
                            )
                            .unwrap();
                    } else {
                        writer
                            .import("after.txt", b"Synthetic concurrent writer")
                            .unwrap();
                    }
                    *done = true;
                }
            }
            Authorization::Allow
        }));
        if selected_read {
            let result = w
                .read_citation_selections(
                    &CitationSelectionsRequest {
                        ids: vec![target.id.clone()],
                    },
                    revision,
                )
                .unwrap();
            assert_eq!(result.workspace_revision, revision);
            assert_eq!(summary_ids(&result.rows), vec![target.id.as_str()]);
            assert!(matches!(
                &result.rows[0],
                CitationSummary::Transaction {
                    review: ReviewState::Pending,
                    ..
                }
            ));
            assert_eq!(
                w.view().unwrap().transactions[0].review,
                ReviewState::Deferred
            );
        } else {
            let result = w
                .page_citation_catalogue(&request("", 50), revision)
                .unwrap();
            assert_eq!(result.workspace_revision, revision);
            assert_eq!(result.scope_count, 2);
            assert_eq!(
                summary_ids(&result.rows),
                vec![target.id.as_str(), source.as_str()]
            );
        }
        assert!(*changed.lock().unwrap());
        assert_eq!(w.revision().unwrap(), revision + 1);
        assert!(w.conn.is_autocommit());
    }
}

#[test]
fn source_metadata_rejects_digest_and_size_retargeted_to_another_intact_original() {
    let (_temp, w) = mixed(1);
    let v = w.view().unwrap();
    let a = &v.evidence[0];
    let b = &v.evidence[1];
    w.conn.execute("UPDATE records SET body=json_set(body,'$.sha256',?1,'$.bytes',?2) WHERE kind='evidence' AND id=?3",
        params![b.sha256,b.bytes,a.id]).unwrap();
    assert!(selected(&w, std::slice::from_ref(&a.id))
        .unwrap_err()
        .to_string()
        .contains("content-addressed"));
    assert!(selected(&w, std::slice::from_ref(&v.observations[0].id)).is_err());
    assert!(w
        .page_citation_catalogue(&request("", 50), w.revision().unwrap())
        .is_err());
    assert_eq!(
        hash(&fs::read(w.root.join("originals").join(&a.sha256)).unwrap()),
        a.sha256
    );
    assert_eq!(
        hash(&fs::read(w.root.join("originals").join(&b.sha256)).unwrap()),
        b.sha256
    );
}

#[test]
fn maximum_selected_set_is_complete_and_excluded_order_is_only_a_set() {
    let (_temp, w) = mixed(97);
    let ids = ordered_ids(&w);
    let revision = w.revision().unwrap();
    assert_eq!(selected(&w, &ids[..100]).unwrap().rows.len(), 100);
    let mut req = request("", 1);
    req.excluded_ids = ids[..2].to_vec();
    let first = w.page_citation_catalogue(&req, revision).unwrap();
    req.cursor = first.next_cursor;
    req.excluded_ids.reverse();
    let second = w.page_citation_catalogue(&req, revision).unwrap();
    assert_eq!(second.scope_count, 99);
    assert_eq!(second.rows[0].id(), ids[3]);
    assert_eq!(
        w.page_citation_catalogue(&request(&"é".repeat(128), 50), revision)
            .unwrap()
            .scope_count,
        0
    );
}

#[test]
fn public_commands_return_only_the_pinned_projection_in_both_response_modes() {
    let (_temp, mut w) = mixed(1);
    let revision = w.revision().unwrap();
    let before = serde_json::to_value(w.view().unwrap()).unwrap();
    let catalogue = Command::PageCitationCatalogue {
        request: request("", 50),
        expected_revision: revision,
    };
    let value = w.dispatch(catalogue.clone()).unwrap();
    assert_eq!(value, w.dispatch_presentation(catalogue).unwrap());
    let page: CitationCataloguePage = serde_json::from_value(value.clone()).unwrap();
    assert_eq!(page.workspace_revision, revision);
    assert_eq!(page.scope_count, 5);
    assert!(value.get("workspace").is_none());
    assert!(value.get("analysis").is_none());
    let selection = Command::ReadCitationSelections {
        request: CitationSelectionsRequest {
            ids: vec![page.rows[4].id().into(), page.rows[0].id().into()],
        },
        expected_revision: revision,
    };
    let value = w.dispatch(selection.clone()).unwrap();
    assert_eq!(value, w.dispatch_presentation(selection).unwrap());
    let selected: CitationSelections = serde_json::from_value(value.clone()).unwrap();
    assert_eq!(selected.workspace_revision, revision);
    assert_eq!(
        summary_ids(&selected.rows),
        vec![page.rows[0].id(), page.rows[4].id()]
    );
    assert!(value.get("workspace").is_none());
    assert!(value.get("analysis").is_none());
    assert_eq!(serde_json::to_value(w.view().unwrap()).unwrap(), before);
}
