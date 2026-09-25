use super::*;

fn workspace() -> (tempfile::TempDir, Workspace) {
    let temp = tempfile::tempdir_in(std::env::temp_dir().canonicalize().unwrap()).unwrap();
    let mut workspace = Workspace::open(temp.path().join("case")).unwrap();
    workspace.import("synthetic.csv", b"account,date,description,amount,currency\n001,2025-03-01,Opening,100.00,AUD\n001,2025-03-02,Repeated purchase,-12.30000001,AUD\n001,2025-03-02,Repeated purchase,-12.30000001,AUD\n001,2025-03-03,Transfer out,-50.00,AUD\n002,2025-03-03,Transfer in,50.00,AUD\n001,2025-03-04,Foreign purchase,-7.10,USD\n001,2025-03-05,Unreviewed,1000.00,AUD\n").unwrap();
    let rows = workspace.view().unwrap().transactions;
    for row in &rows[..6] {
        workspace
            .review_transaction(
                &row.id,
                ReviewState::Accepted,
                "Synthetic review",
                workspace.revision().unwrap(),
            )
            .unwrap();
    }
    workspace
        .match_transfer(
            &rows[3].id,
            &rows[4].id,
            "Synthetic pair",
            workspace.revision().unwrap(),
        )
        .unwrap();
    (temp, workspace)
}
fn save(workspace: &mut Workspace) -> DocxSnapshotRecord {
    workspace
        .save_docx_snapshot(&id(), workspace.revision().unwrap())
        .unwrap()
}
fn read(workspace: &Workspace, record: &DocxSnapshotRecord) -> Result<Vec<u8>> {
    workspace.read_docx_snapshot(&record.id, &record.document.sha256, &record.docx.sha256)
}
fn inspect(workspace: &Workspace, record: &DocxSnapshotRecord) -> Result<DocxSnapshotInspection> {
    workspace.inspect_docx_snapshot(&record.id, &record.document.sha256, &record.docx.sha256)
}
fn object(workspace: &Workspace, sha: &str) -> PathBuf {
    workspace.root.join("derivatives/objects").join(sha)
}
fn overwrite(path: &Path, bytes: &[u8]) {
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        fs::set_permissions(path, fs::Permissions::from_mode(0o600)).unwrap();
    }
    #[cfg(windows)]
    {
        let mut permissions = fs::metadata(path).unwrap().permissions();
        // Corruption test changes only its owned disposable fixture.
        #[allow(clippy::permissions_set_readonly_false)]
        permissions.set_readonly(false);
        fs::set_permissions(path, permissions).unwrap();
    }
    fs::write(path, bytes).unwrap();
    private_file(path, 0o400).unwrap();
}
fn no_publication(workspace: &Workspace, revision: u64) {
    assert_eq!(workspace.revision().unwrap(), revision);
    assert!(workspace.docx_records().unwrap().is_empty());
    assert_eq!(
        workspace
            .conn
            .query_row::<u64, _, _>("SELECT count(*) FROM derivative_objects", [], |row| row
                .get(0))
            .unwrap(),
        0
    );
}

#[test]
fn recovery_direct_commands_verify_saved_identity_without_another_write_or_refresh() {
    type Dispatch = fn(&mut Workspace, Command) -> Result<Value>;
    for dispatch in [
        Workspace::dispatch as Dispatch,
        Workspace::dispatch_presentation,
        Workspace::dispatch_summary,
    ] {
        let (_temp, mut workspace) = workspace();
        let revision = workspace.revision().unwrap();
        let request = id();
        let command = Command::ResolveDocxCapture {
            request_id: request.clone(),
            captured_revision: revision,
        };
        let absent: DocxCaptureResolution =
            serde_json::from_value(dispatch(&mut workspace, command.clone()).unwrap()).unwrap();
        assert_eq!(absent.workspace_revision, revision);
        assert_eq!(absent.outcome, DocxCaptureOutcome::NotRecorded);
        let record = workspace.save_docx_snapshot(&request, revision).unwrap();
        workspace
            .import("later.txt", b"Synthetic later source")
            .unwrap();
        let current = workspace.revision().unwrap();
        let response = dispatch(&mut workspace, command).unwrap();
        assert!(response.get("workspace").is_none());
        let result: DocxCaptureResolution = serde_json::from_value(response).unwrap();
        assert_eq!(result.request_id, request);
        assert_eq!(result.captured_revision, revision);
        assert_eq!(result.workspace_revision, current);
        assert_eq!(
            result.outcome,
            DocxCaptureOutcome::Saved {
                snapshot: Box::new(record)
            }
        );
        assert_eq!(workspace.revision().unwrap(), current);
        assert_eq!(workspace.docx_records().unwrap().len(), 1);
    }
}

#[test]
fn recovery_absence_at_same_revision_cannot_preclude_an_already_sent_capture() {
    let (_temp, mut workspace) = workspace();
    let request = id();
    let revision = workspace.revision().unwrap();
    workspace
        .save_docx_snapshot_checked(&request, revision, |root| {
            let other = Workspace::open(root)?;
            let result = other.resolve_docx_capture(&request, revision)?;
            assert_eq!(result.outcome, DocxCaptureOutcome::NotRecorded);
            assert_eq!(result.workspace_revision, revision);
            Ok(())
        })
        .unwrap();
    assert!(matches!(
        workspace
            .resolve_docx_capture(&request, revision)
            .unwrap()
            .outcome,
        DocxCaptureOutcome::Saved { .. }
    ));
    let next = id();
    let next_revision = workspace.revision().unwrap();
    let result = workspace.save_docx_snapshot_checked(&next, next_revision, |root| {
        let mut other = Workspace::open(root)?;
        other.import(
            "changed.txt",
            b"Synthetic later revision rules out old publication",
        )?;
        let result = other.resolve_docx_capture(&next, next_revision)?;
        assert_eq!(result.outcome, DocxCaptureOutcome::NotRecorded);
        assert!(result.workspace_revision > next_revision);
        Ok(())
    });
    assert!(matches!(result, Err(Error::Conflict(_))));
    assert!(matches!(
        workspace.save_docx_snapshot(&next, next_revision),
        Err(Error::Conflict(_))
    ));
    assert_eq!(workspace.docx_records().unwrap().len(), 1);
}

#[test]
fn recovery_pins_revision_and_lookup_while_an_actual_writer_publishes() {
    let (_temp, workspace) = workspace();
    let mut writer = Workspace::open(&workspace.root).unwrap();
    workspace
        .conn
        .pragma_update(None, "journal_mode", "WAL")
        .unwrap();
    writer
        .conn
        .pragma_update(None, "journal_mode", "WAL")
        .unwrap();
    let request = id();
    let revision = workspace.revision().unwrap();
    let result = workspace
        .resolve_docx_capture_checked(&request, revision, || {
            writer.save_docx_snapshot(&request, revision)?;
            Ok(())
        })
        .unwrap();
    assert_eq!(result.workspace_revision, revision);
    assert_eq!(result.outcome, DocxCaptureOutcome::NotRecorded);
    let result = workspace.resolve_docx_capture(&request, revision).unwrap();
    assert_eq!(result.workspace_revision, revision + 1);
    assert!(matches!(result.outcome, DocxCaptureOutcome::Saved { .. }));
}

#[test]
fn recovery_never_labels_corrupt_conflicting_or_future_identity_as_absent() {
    let (_temp, mut workspace) = workspace();
    let record = save(&mut workspace);
    let revision = workspace.revision().unwrap();
    for request in [
        "".to_owned(),
        "a".repeat(100_000),
        format!("{{{}}}", record.id),
    ] {
        assert!(workspace.resolve_docx_capture(&request, revision).is_err());
    }
    assert!(workspace
        .resolve_docx_capture(&record.id, revision)
        .is_err());
    assert!(matches!(
        workspace.resolve_docx_capture(&id(), revision + 1),
        Err(Error::Conflict(_))
    ));
    let path = object(&workspace, &record.docx.sha256);
    overwrite(&path, b"Synthetic corrupt frozen DOCX");
    assert!(workspace
        .resolve_docx_capture(&record.id, record.workspace_revision)
        .is_err());
    put(&workspace.conn, KIND, &record.id, &json!({})).unwrap();
    assert!(workspace
        .resolve_docx_capture(&record.id, record.workspace_revision)
        .is_err());
}

#[test]
fn recovery_restored_copies_do_not_authorize_replacement_at_same_or_earlier_revision() {
    let (temp, mut workspace) = workspace();
    let initial_revision = workspace.revision().unwrap();
    let backup = workspace.backup().unwrap();
    workspace
        .import("after-backup.txt", b"Synthetic content after backup")
        .unwrap();
    let request = id();
    let later_revision = workspace.revision().unwrap();
    let restored = Workspace::restore(&backup, &temp.path().join("restored-recovery")).unwrap();
    assert!(matches!(
        restored.resolve_docx_capture(&request, later_revision),
        Err(Error::Conflict(_))
    ));
    let result = restored
        .resolve_docx_capture(&request, initial_revision)
        .unwrap();
    assert_eq!(result.workspace_revision, initial_revision);
    assert_eq!(result.outcome, DocxCaptureOutcome::NotRecorded);
    assert_eq!(workspace.revision().unwrap(), later_revision);
}

#[test]
fn frozen_money_sources_and_legacy_html_survive_corrections_replay_and_restore() {
    let (temp, mut workspace) = workspace();
    workspace.save_report().unwrap();
    let before = workspace.view().unwrap();
    let record = save(&mut workspace);
    assert_eq!(record.workspace_revision, before.revision);
    assert_eq!(workspace.revision().unwrap(), before.revision + 1);
    let inspected = inspect(&workspace, &record).unwrap();
    assert_eq!(
        serde_json::to_value(&inspected.document.content.transactions).unwrap(),
        serde_json::to_value(&before.transactions).unwrap()
    );
    assert_eq!(
        serde_json::to_value(&inspected.document.calculations.totals).unwrap(),
        serde_json::to_value(analytics::analyse(&before.transactions).unwrap().totals).unwrap()
    );
    assert_eq!(
        inspected
            .document
            .calculations
            .totals
            .iter()
            .find(|t| t.currency == "AUD")
            .unwrap()
            .net,
        "75.39999998"
    );
    assert_eq!(
        inspected
            .document
            .calculations
            .totals
            .iter()
            .find(|t| t.currency == "USD")
            .unwrap()
            .net,
        "-7.10"
    );
    assert_eq!(inspected.document.content.transactions[0].account, "001");
    let bytes = read(&workspace, &record).unwrap();
    assert_eq!(hash(&bytes), record.docx.sha256);
    assert_eq!(bytes, report_docx::render(&inspected.document).unwrap());
    let revision = workspace.revision().unwrap();
    assert_eq!(
        workspace
            .save_docx_snapshot(&record.id, before.revision)
            .unwrap(),
        record
    );
    assert_eq!(workspace.revision().unwrap(), revision);
    assert!(workspace.save_docx_snapshot(&record.id, revision).is_err());
    assert!(matches!(
        workspace.save_docx_snapshot(&id(), before.revision),
        Err(Error::Conflict(_))
    ));
    assert!(workspace
        .inspect_docx_snapshot(
            &before.reports[0].id,
            &record.document.sha256,
            &record.docx.sha256
        )
        .is_err());
    workspace
        .correct_transaction(
            &before.transactions[1].id,
            "-999.99",
            "Synthetic correction",
            revision,
        )
        .unwrap();
    workspace
        .import("later.txt", b"Later independent synthetic source")
        .unwrap();
    assert_eq!(read(&workspace, &record).unwrap(), bytes);
    assert_eq!(
        workspace.view().unwrap().reports[0].html,
        before.reports[0].html
    );
    let backup = workspace.backup().unwrap();
    let manifest: Value =
        serde_json::from_slice(&fs::read(backup.join("manifest.json")).unwrap()).unwrap();
    assert_eq!(manifest["format_version"], 3);
    assert_eq!(manifest["schema_version"], 5);
    let restored = Workspace::restore(&backup, &temp.path().join("restored")).unwrap();
    assert_eq!(read(&restored, &record).unwrap(), bytes);
    assert_eq!(
        restored.view().unwrap().reports[0].html,
        before.reports[0].html
    );
}

#[test]
fn concurrent_writer_after_file_preparation_leaves_only_unreferenced_objects() {
    let (temp, mut workspace) = workspace();
    let revision = workspace.revision().unwrap();
    let result = workspace.save_docx_snapshot_checked(&id(), revision, |root| {
        let mut other = Workspace::open(root)?;
        other.import("concurrent.txt", b"Synthetic concurrent writer")?;
        Ok(())
    });
    assert!(matches!(result, Err(Error::Conflict(_))));
    no_publication(&workspace, revision + 1);
    assert_eq!(
        fs::read_dir(workspace.root.join("derivatives/objects"))
            .unwrap()
            .count(),
        2
    );
    let backup = workspace.backup().unwrap();
    assert!(!backup.join("derivatives/objects").exists());
    Workspace::restore(&backup, &temp.path().join("restored")).unwrap();
    let record = save(&mut workspace);
    read(&workspace, &record).unwrap();
}

#[test]
fn failed_canonical_commit_rolls_back_both_catalog_entries_and_preserves_old_objects() {
    let (_temp, mut workspace) = workspace();
    let original = save(&mut workspace);
    let bytes = read(&workspace, &original).unwrap();
    let revision = workspace.revision().unwrap();
    workspace.conn.execute_batch("CREATE TRIGGER reject_docx BEFORE INSERT ON records WHEN NEW.kind='docx_snapshot' BEGIN SELECT RAISE(ABORT,'synthetic publication failure'); END;").unwrap();
    assert!(workspace.save_docx_snapshot(&id(), revision).is_err());
    assert_eq!(workspace.revision().unwrap(), revision);
    assert_eq!(
        workspace.docx_records().unwrap(),
        std::slice::from_ref(&original)
    );
    assert_eq!(
        workspace
            .conn
            .query_row::<u64, _, _>("SELECT count(*) FROM derivative_objects", [], |r| r.get(0))
            .unwrap(),
        2
    );
    assert_eq!(read(&workspace, &original).unwrap(), bytes);
    assert_eq!(
        fs::read_dir(workspace.root.join("derivatives/objects"))
            .unwrap()
            .count(),
        4
    );
    workspace.backup().unwrap();
}

#[test]
fn tampered_prepared_object_or_original_cannot_publish() {
    for original in [false, true] {
        let (_temp, mut workspace) = workspace();
        let revision = workspace.revision().unwrap();
        let result = workspace.save_docx_snapshot_checked(&id(), revision, |root| {
            let directory = root.join(if original {
                "originals"
            } else {
                "derivatives/objects"
            });
            let path = fs::read_dir(directory)?.next().unwrap()?.path();
            let mut bytes = fs::read(&path)?;
            bytes[0] ^= 1;
            overwrite(&path, &bytes);
            Ok(())
        });
        assert!(result.is_err());
        no_publication(&workspace, revision);
    }
}

#[test]
fn exact_read_rejects_metadata_artifact_catalog_and_original_corruption() {
    let (_temp, mut workspace) = workspace();
    let record = save(&mut workspace);
    let bytes = read(&workspace, &record).unwrap();
    assert!(workspace
        .read_docx_snapshot(&record.id, &record.document.sha256, &"0".repeat(64))
        .is_err());
    let mut changed = record.clone();
    changed.workspace_revision -= 1;
    put(&workspace.conn, KIND, &record.id, &changed).unwrap();
    assert!(read(&workspace, &record).is_err());
    put(&workspace.conn, KIND, &record.id, &record).unwrap();
    let path = object(&workspace, &record.docx.sha256);
    let mut corrupt = bytes.clone();
    corrupt[0] ^= 1;
    overwrite(&path, &corrupt);
    assert!(read(&workspace, &record).is_err());
    assert!(workspace.backup().is_err());
    overwrite(&path, &bytes);
    workspace
        .conn
        .execute(
            "UPDATE derivative_objects SET bytes=bytes+1 WHERE sha256=?",
            [&record.docx.sha256],
        )
        .unwrap();
    assert!(read(&workspace, &record).is_err());
    workspace
        .conn
        .execute(
            "UPDATE derivative_objects SET bytes=bytes-1 WHERE sha256=?",
            [&record.docx.sha256],
        )
        .unwrap();
    let source = workspace.view().unwrap().evidence[0].clone();
    let path = workspace.root.join("originals").join(source.sha256);
    let mut original = fs::read(&path).unwrap();
    original[0] ^= 1;
    overwrite(&path, &original);
    assert!(read(&workspace, &record).is_err());
    assert!(workspace
        .save_docx_snapshot(&record.id, record.workspace_revision)
        .is_err());
}

#[test]
fn frozen_model_and_rendered_package_must_agree_even_when_every_hash_is_valid() {
    let (_temp, mut workspace) = workspace();
    let record = save(&mut workspace);
    let mut document = inspect(&workspace, &record).unwrap().document;
    document.content.evidence[0].name = "different-synthetic-name.csv".into();
    let bytes = document.to_json().unwrap();
    let mut changed = record.clone();
    changed.document = reference(ReportArtifactKind::ReportDocumentJsonV1, &bytes).unwrap();
    let object = ObjectRef::Report(changed.document.clone());
    files::retain_object(&workspace.root, &object, &bytes).unwrap();
    files::catalog_object(&workspace.conn, &object).unwrap();
    put(&workspace.conn, KIND, &record.id, &changed).unwrap();
    assert!(read(&workspace, &changed)
        .unwrap_err()
        .to_string()
        .contains("Retained DOCX differs"));
}

#[test]
fn backup_copies_the_saved_snapshot_union_even_if_another_docx_is_published() {
    let (temp, mut workspace) = workspace();
    let first = save(&mut workspace);
    let revision = workspace.revision().unwrap();
    let root = workspace.root.clone();
    let backup = workspace
        .backup_snapshot(|| {
            let mut other = Workspace::open(&root)?;
            other.import(
                "concurrent.txt",
                b"A later original belongs to a later snapshot",
            )?;
            other.save_docx_snapshot(&id(), other.revision()?)?;
            Ok(())
        })
        .unwrap();
    assert_eq!(workspace.docx_records().unwrap().len(), 2);
    let restored = Workspace::restore(&backup, &temp.path().join("restored")).unwrap();
    assert_eq!(restored.revision().unwrap(), revision);
    assert_eq!(
        restored.docx_records().unwrap(),
        std::slice::from_ref(&first)
    );
    assert_eq!(restored.view().unwrap().evidence.len(), 1);
    read(&restored, &first).unwrap();
    assert_eq!(
        fs::read_dir(backup.join("derivatives/objects"))
            .unwrap()
            .count(),
        2
    );
}

#[test]
fn missing_backup_object_refuses_canonical_restore_and_older_schema_cannot_hide_reports() {
    let (temp, mut workspace) = workspace();
    let record = save(&mut workspace);
    let backup = workspace.backup().unwrap();
    let path = backup.join("derivatives/objects").join(&record.docx.sha256);
    overwrite(&path, b"missing artifact fixture");
    let destination = temp.path().join("failed-restore");
    assert!(Workspace::restore(&backup, &destination).is_err());
    assert!(!destination.join("workspace.db").exists());
    for version in [3, 4] {
        workspace
            .conn
            .pragma_update(None, "user_version", version)
            .unwrap();
        assert!(workspace.derivative_refs().is_err());
    }
}

#[test]
fn bounds_invalid_xml_dangling_references_and_canonical_uuid_fail_before_retention() {
    let (_temp, mut workspace) = workspace();
    let revision = workspace.revision().unwrap();
    for key in [
        Uuid::new_v4().simple().to_string(),
        id().to_uppercase(),
        "invalid".into(),
    ] {
        assert!(workspace.save_docx_snapshot(&key, revision).is_err());
    }
    let source = workspace.view().unwrap().evidence[0].clone();
    let mut modified = source.clone();
    for text in [
        "Invalid XML\u{0001}".into(),
        "x".repeat(report_document::MAX_TEXT_BYTES + 1),
    ] {
        modified.name = text;
        put(&workspace.conn, "evidence", &source.id, &modified).unwrap();
        assert!(workspace.save_docx_snapshot(&id(), revision).is_err());
        no_publication(&workspace, revision);
        assert!(!workspace.root.join("derivatives/objects").exists());
    }
    put(&workspace.conn, "evidence", &source.id, &source).unwrap();
    let mut transaction = workspace.view().unwrap().transactions[0].clone();
    transaction.anchor = SourceAnchor::Cell {
        evidence_id: "0".repeat(64),
        sheet: "Sheet1".into(),
        row: 2,
        column: "amount".into(),
    };
    put(
        &workspace.conn,
        "transaction",
        &transaction.id,
        &transaction,
    )
    .unwrap();
    assert!(workspace.save_docx_snapshot(&id(), revision).is_err());
    no_publication(&workspace, revision);
    for (kind, size) in [
        (
            ReportArtifactKind::ReportDocumentJsonV1,
            report_document::MAX_DOCUMENT_BYTES,
        ),
        (
            ReportArtifactKind::ReportDocxV1,
            report_docx::MAX_DOCX_BYTES,
        ),
    ] {
        assert!(ReportArtifactRef {
            kind,
            sha256: "0".repeat(64),
            bytes: size as u64 + 1
        }
        .validate()
        .is_err());
    }
}

#[test]
fn schema_four_migration_preserves_review_history_and_keeps_a_recoverable_backup() {
    for fail in [false, true] {
        let (temp, mut workspace) = workspace();
        let source = workspace.view().unwrap().evidence[0].id.clone();
        let finding = workspace
            .add_finding(
                FindingInput {
                    title: "Synthetic finding".into(),
                    assessment: "Review remains current across storage upgrade".into(),
                    supporting_ids: vec![source],
                    contradicting_ids: vec![],
                    limitations: "Synthetic only".into(),
                    hypothesis_ids: vec![],
                },
                workspace.revision().unwrap(),
            )
            .unwrap();
        workspace
            .review_finding(&finding, "Checked source", workspace.revision().unwrap())
            .unwrap();
        workspace.save_report().unwrap();
        let before = workspace.view().unwrap();
        workspace
            .conn
            .pragma_update(None, "user_version", 4)
            .unwrap();
        if fail {
            workspace.conn.execute_batch("CREATE TRIGGER reject_upgrade BEFORE INSERT ON events WHEN NEW.action='workspace.schema_v5' BEGIN SELECT RAISE(ABORT,'synthetic migration failure'); END;").unwrap();
        }
        drop(workspace);
        let opened = Workspace::open(temp.path().join("case"));
        assert_eq!(opened.is_err(), fail);
        if let Ok(workspace) = opened {
            let after = workspace.view().unwrap();
            assert_eq!(after.schema_version, 5);
            assert_eq!(after.revision, before.revision + 1);
            assert_eq!(
                serde_json::to_value(after.findings).unwrap(),
                serde_json::to_value(&before.findings).unwrap()
            );
            assert_eq!(
                serde_json::to_value(after.reports).unwrap(),
                serde_json::to_value(&before.reports).unwrap()
            );
        }
        let backup = fs::read_dir(temp.path().join("case/backups"))
            .unwrap()
            .next()
            .unwrap()
            .unwrap()
            .path();
        let manifest: Value =
            serde_json::from_slice(&fs::read(backup.join("manifest.json")).unwrap()).unwrap();
        assert_eq!(manifest["schema_version"], 4);
        assert_eq!(manifest["format_version"], 2);
        let connection = Connection::open(backup.join("workspace.db")).unwrap();
        assert_eq!(
            connection
                .query_row::<u64, _, _>("SELECT revision FROM meta", [], |r| r.get(0))
                .unwrap(),
            before.revision
        );
        // A persistent injected trigger is an environmental failure in both database copies.
        connection
            .execute_batch("DROP TRIGGER IF EXISTS reject_upgrade")
            .unwrap();
        drop(connection);
        let restored = Workspace::restore(&backup, &temp.path().join("restored")).unwrap();
        assert!(!restored.view().unwrap().findings[0].needs_review);
        assert_eq!(
            restored.view().unwrap().reports[0].html,
            before.reports[0].html
        );
        if fail {
            let live = Connection::open(temp.path().join("case/workspace.db")).unwrap();
            assert_eq!(
                live.pragma_query_value::<u32, _>(None, "user_version", |r| r.get(0))
                    .unwrap(),
                4
            );
            assert_eq!(
                live.query_row::<u64, _, _>("SELECT revision FROM meta", [], |r| r.get(0))
                    .unwrap(),
                before.revision
            );
        }
    }
}

#[test]
fn bounded_record_identity_and_newer_schema_refusal_are_explicit() {
    let (temp, mut workspace) = workspace();
    workspace.save_report().unwrap();
    let html = workspace.view().unwrap().reports[0].clone();
    let revision = workspace.revision().unwrap();
    assert!(workspace.save_docx_snapshot(&html.id, revision).is_err());
    no_publication(&workspace, revision);
    let record = save(&mut workspace);
    let mut modified = record.clone();
    modified.id = id();
    put(&workspace.conn, KIND, &record.id, &modified).unwrap();
    assert!(read(&workspace, &record).is_err());
    modified = record.clone();
    modified.generator_version = "x".repeat(MAX_RECORD_BYTES as usize + 1);
    put(&workspace.conn, KIND, &record.id, &modified).unwrap();
    assert!(read(&workspace, &record)
        .unwrap_err()
        .to_string()
        .contains("metadata exceeds"));
    put(&workspace.conn, KIND, &record.id, &record).unwrap();
    workspace
        .conn
        .pragma_update(None, "user_version", 6)
        .unwrap();
    drop(workspace);
    assert!(matches!(
        Workspace::open(temp.path().join("case")),
        Err(Error::Blocked(_))
    ));
}

#[test]
fn legacy_backups_accept_actual_omitted_or_format_two_manifests_but_reject_explicit_unknown_versions(
) {
    for schema in 1..=3 {
        let (temp, mut workspace) = workspace();
        workspace
            .conn
            .pragma_update(None, "user_version", schema)
            .unwrap();
        let backup = workspace.backup().unwrap();
        let path = backup.join("manifest.json");
        let valid: Value = serde_json::from_slice(&fs::read(&path).unwrap()).unwrap();
        for (index, format) in [
            None,
            Some(json!(2)),
            Some(json!(3)),
            Some(json!(99)),
            Some(json!(1)),
            Some(json!(null)),
            Some(json!("2")),
        ]
        .into_iter()
        .enumerate()
        {
            let mut manifest = valid.clone();
            if let Some(format) = format {
                manifest["format_version"] = format;
            } else {
                for name in ["format_version", "complete", "derivatives"] {
                    manifest.as_object_mut().unwrap().remove(name);
                }
            }
            fs::write(&path, serde_json::to_vec(&manifest).unwrap()).unwrap();
            let destination = temp.path().join(format!("restored-{index}"));
            let result = Workspace::restore(&backup, &destination);
            assert_eq!(
                result.is_ok(),
                index < 2,
                "schema {schema}, manifest {manifest}"
            );
            if index >= 2 {
                assert!(!destination.join("workspace.db").exists());
            }
        }
    }
}

#[test]
fn generator_one_snapshot_still_inspects_exports_and_restores_after_generator_two_capture() {
    use crate::local_export::NativeExportRequest;
    let temp = tempfile::tempdir_in(std::env::temp_dir().canonicalize().unwrap()).unwrap();
    let mut workspace = Workspace::open(temp.path().join("case")).unwrap();
    let document = ReportDocument::from_json(include_bytes!(
        "../../../tests/fixtures/report-generator1.json"
    ))
    .unwrap();
    let evidence = &document.content.evidence[0];
    workspace
        .import(&evidence.name, evidence.text.as_ref().unwrap().as_bytes())
        .unwrap();
    while workspace.revision().unwrap() < document.workspace_revision {
        let revision = workspace.revision().unwrap();
        workspace
            .import(
                &format!("synthetic-{revision}.txt"),
                format!("Synthetic later source {revision}").as_bytes(),
            )
            .unwrap();
    }
    let json = document.to_json().unwrap();
    let bytes = report_docx::render(&document).unwrap();
    assert_eq!(
        hash(&bytes),
        "c839120c7646195012e57119be34dcca7ad27e32a032ad734ca47680cfc1c75d"
    );
    let old = DocxSnapshotRecord {
        schema_version: 1,
        id: document.report_id.clone(),
        workspace_revision: document.workspace_revision,
        created_at: document.created_at.clone(),
        template_version: document.template_version.clone(),
        generator_version: document.generator_version.clone(),
        document: reference(ReportArtifactKind::ReportDocumentJsonV1, &json).unwrap(),
        docx: reference(ReportArtifactKind::ReportDocxV1, &bytes).unwrap(),
    };
    // Install a fixed historical fixture, using the same private CAS/catalogue
    // operations as publication. No public arbitrary-finish hook is introduced.
    for (reference, content) in refs(&old).iter().zip([json.as_slice(), bytes.as_slice()]) {
        files::retain_object(&workspace.root, reference, content).unwrap();
    }
    workspace
        .change(None, "test.historical_docx", false, |conn| {
            for reference in refs(&old) {
                files::catalog_object(conn, &reference)?;
            }
            put(conn, KIND, &old.id, &old)
        })
        .unwrap();
    assert_eq!(
        inspect(&workspace, &old)
            .unwrap()
            .document
            .to_json()
            .unwrap(),
        json
    );
    let new = save(&mut workspace);
    assert_eq!(new.generator_version, "ooxml-foundation-2");
    assert_eq!(read(&workspace, &old).unwrap(), bytes);
    assert_eq!(
        workspace
            .save_docx_snapshot(&old.id, old.workspace_revision)
            .unwrap(),
        old
    );
    let exports = workspace.start_native_exports().unwrap();
    let prepared = exports
        .prepare(|| {
            workspace.native_export_content(NativeExportRequest::DocxReport {
                report_id: old.id.clone(),
                expected_document_sha256: old.document.sha256.clone(),
                expected_docx_sha256: old.docx.sha256.clone(),
            })
        })
        .unwrap();
    let receipt = exports
        .commit(&prepared.ticket, &old.docx.sha256, old.docx.bytes)
        .unwrap();
    assert_eq!(fs::read(&receipt.location).unwrap(), bytes);
    exports.shutdown().unwrap();
    let backup = workspace.backup().unwrap();
    let restored = Workspace::restore(&backup, &temp.path().join("restored")).unwrap();
    assert_eq!(read(&restored, &old).unwrap(), bytes);
    assert_eq!(inspect(&restored, &new).unwrap().snapshot, new);
}
