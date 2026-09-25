use super::*;
use crate::{
    coordinator::JobCoordinator,
    literal_search::LiteralMatching,
    transaction_csv::{NonAcceptedCsvPolicy, TransactionCsvFormat, TransactionCsvRequest},
    transaction_export::TransactionExportRequest,
    transaction_page::{TransactionPageFilter, TransactionPageOrder},
};

fn csv_request(revision: u64, count: u64, policy: NonAcceptedCsvPolicy) -> NativeExportRequest {
    NativeExportRequest::TransactionCsv {
        request: TransactionCsvRequest {
            selection: TransactionExportRequest {
                query: String::new(),
                filter: TransactionPageFilter::default(),
                order: TransactionPageOrder::DateAscending,
            },
            non_accepted: policy,
        },
        expected_revision: revision,
        expected_row_count: count,
        expected_matching: LiteralMatching::default(),
        expected_format: TransactionCsvFormat::TypedLiteralV1,
    }
}

#[test]
fn typed_csv_native_save_keeps_prepared_bytes_after_correction_and_retries_same_ticket() {
    let (_temp, mut workspace) = fixture();
    workspace
        .import(
            "synthetic.csv",
            b"account,date,description,amount,currency\n000042,2024-02-29,=1+1,-0.10000001,AUD\n",
        )
        .unwrap();
    let before = workspace.view().unwrap();
    let request = csv_request(before.revision, 1, NonAcceptedCsvPolicy::AllowSelected);
    let expected = workspace.native_export_content(request.clone()).unwrap();
    let exports = workspace.start_native_exports().unwrap();
    let prepared = exports
        .prepare(|| workspace.native_export_content(request))
        .unwrap();
    assert!(
        matches!(&prepared.artifact, ExportArtifact::TransactionCsv { workspace_revision, row_count: 1, .. } if *workspace_revision == before.revision)
    );
    assert_eq!(
        serde_json::to_value(&prepared.artifact).unwrap(),
        serde_json::to_value(&expected.0).unwrap()
    );
    let metadata = serde_json::to_value(&prepared).unwrap();
    assert_eq!(prepared.schema_version, 2);
    assert!(metadata["artifact"].get("csv").is_none());
    workspace
        .correct_transaction(
            &before.transactions[0].id,
            "-9.00000001",
            "Synthetic correction after CSV preparation",
            before.revision,
        )
        .unwrap();
    let revision = workspace.revision().unwrap();
    let saved = commit(&exports, &prepared).unwrap();
    assert_eq!(saved.schema_version, 2);
    assert!(saved.filename.ends_with(".csv"));
    assert!(saved
        .filename
        .starts_with(&format!("transactions-typed-v1-r{}-", before.revision)));
    let bytes = fs::read(&saved.location).unwrap();
    assert_eq!(bytes, expected.1);
    let text = String::from_utf8(bytes).unwrap();
    assert!(text.starts_with('\u{feff}'));
    assert!(
        text.contains("text:000042")
            && text.contains("text:=1+1")
            && text.contains("decimal:-0.10000001")
    );
    assert_eq!(
        serde_json::to_value(&saved).unwrap(),
        serde_json::to_value(commit(&exports, &prepared).unwrap()).unwrap()
    );
    let current = exports
        .prepare(|| {
            workspace.native_export_content(csv_request(
                revision,
                1,
                NonAcceptedCsvPolicy::AllowSelected,
            ))
        })
        .unwrap();
    let changed = commit(&exports, &current).unwrap();
    assert_ne!(changed.filename, saved.filename);
    assert_eq!(fs::read(&saved.location).unwrap(), expected.1);
    assert_eq!(workspace.revision().unwrap(), revision);
    exports.shutdown().unwrap();
}

#[test]
fn typed_csv_native_refuses_nonaccepted_stale_and_wrong_scope_expectations_without_stage() {
    let (_temp, mut workspace) = fixture();
    workspace
        .import(
            "synthetic.csv",
            b"account,date,description,amount,currency\n000042,2024-02-29,Synthetic,-1.01,AUD\n",
        )
        .unwrap();
    let revision = workspace.revision().unwrap();
    let exports = workspace.start_native_exports().unwrap();
    let mut wrong_matching = csv_request(revision, 1, NonAcceptedCsvPolicy::AllowSelected);
    if let NativeExportRequest::TransactionCsv {
        expected_matching, ..
    } = &mut wrong_matching
    {
        expected_matching.unicode_version = [0, 0, 0];
    }
    for request in [
        csv_request(revision, 1, NonAcceptedCsvPolicy::Reject),
        csv_request(revision - 1, 1, NonAcceptedCsvPolicy::AllowSelected),
        csv_request(revision, 2, NonAcceptedCsvPolicy::AllowSelected),
        wrong_matching,
    ] {
        assert!(exports
            .prepare(|| workspace.native_export_content(request))
            .is_err());
        assert!(exports.session.registry.lock().unwrap().stage.is_none());
        assert_eq!(fs::read_dir(exports.session.staging()).unwrap().count(), 0);
        assert_eq!(fs::read_dir(exports.session.exports()).unwrap().count(), 0);
    }
    let mut invalid = serde_json::to_value(csv_request(
        revision,
        1,
        NonAcceptedCsvPolicy::AllowSelected,
    ))
    .unwrap();
    invalid["expected_format"] = serde_json::json!("raw_unquoted");
    assert!(serde_json::from_value::<NativeExportRequest>(invalid).is_err());
    assert_eq!(workspace.revision().unwrap(), revision);
    exports.shutdown().unwrap();
}

#[test]
fn typed_csv_native_same_content_reuses_file_but_corrupt_target_never_gets_receipt() {
    let (_temp, workspace) = fixture();
    let exports = workspace.start_native_exports().unwrap();
    let make = || workspace.native_export_content(csv_request(0, 0, NonAcceptedCsvPolicy::Reject));
    let prepared = exports.prepare(make).unwrap();
    let saved = commit(&exports, &prepared).unwrap();
    let second = exports.prepare(make).unwrap();
    assert_eq!(commit(&exports, &second).unwrap().location, saved.location);
    assert_eq!(fs::read_dir(exports.session.exports()).unwrap().count(), 1);
    let third = exports.prepare(make).unwrap();
    fs::write(&saved.location, b"unrelated prior file").unwrap();
    assert!(commit(&exports, &third).is_err());
    assert_eq!(fs::read(&saved.location).unwrap(), b"unrelated prior file");
    assert!(!exports
        .session
        .registry
        .lock()
        .unwrap()
        .completed
        .iter()
        .any(|r| r.ticket == third.ticket));
    exports.discard(&third.ticket).unwrap();
    exports.shutdown().unwrap();
}
fn fixture() -> (tempfile::TempDir, Workspace) {
    let temp = tempfile::tempdir_in(std::env::temp_dir().canonicalize().unwrap()).unwrap();
    let workspace = Workspace::open(temp.path().join("case")).unwrap();
    (temp, workspace)
}
fn request(revision: u64) -> NativeExportRequest {
    NativeExportRequest::Transactions {
        request: TransactionExportRequest {
            query: String::new(),
            filter: TransactionPageFilter::default(),
            order: TransactionPageOrder::DateAscending,
        },
        expected_revision: revision,
        expected_row_count: 0,
        expected_matching: LiteralMatching::default(),
    }
}
fn prepare(exports: &NativeExports, workspace: &Workspace) -> PreparedExport {
    exports
        .prepare(|| workspace.native_export_content(request(workspace.revision().unwrap())))
        .unwrap()
}
fn commit(exports: &NativeExports, prepared: &PreparedExport) -> Result<SavedExportReceipt> {
    exports.commit(
        &prepared.ticket,
        prepared.artifact.sha256(),
        prepared.artifact.bytes(),
    )
}
#[test]
fn transaction_identity_exact_bytes_no_clobber_and_lost_ack_recommit() {
    let (_temp, workspace) = fixture();
    let exports = workspace.start_native_exports().unwrap();
    let expected = workspace.native_export_content(request(0)).unwrap().1;
    let prepared = prepare(&exports, &workspace);
    assert_eq!(prepared.schema_version, 1);
    assert!(exports
        .prepare(|| workspace.native_export_content(request(0)))
        .is_err());
    assert!(exports
        .commit(&prepared.ticket, &"0".repeat(64), prepared.artifact.bytes())
        .is_err());
    let saved = commit(&exports, &prepared).unwrap();
    assert_eq!(saved.schema_version, 1);
    assert_eq!(fs::read(&saved.location).unwrap(), expected.as_slice());
    assert_eq!(
        serde_json::to_value(&saved).unwrap(),
        serde_json::to_value(commit(&exports, &prepared).unwrap()).unwrap()
    );
    assert!(matches!(
        exports.discard(&prepared.ticket).unwrap(),
        DiscardedExport::Saved { .. }
    ));
    let again = prepare(&exports, &workspace);
    assert_eq!(commit(&exports, &again).unwrap().location, saved.location);
    assert_eq!(fs::read_dir(exports.session.exports()).unwrap().count(), 1);
    assert_eq!(workspace.revision().unwrap(), 0);
    exports.shutdown().unwrap();
}
#[test]
fn immutable_html_uses_saved_identity_after_current_workspace_changes() {
    let (_temp, mut workspace) = fixture();
    let report_id = workspace.save_report().unwrap();
    let old = workspace.view().unwrap().reports.remove(0);
    workspace
        .import("later.txt", b"Synthetic later changes")
        .unwrap();
    let exports = workspace.start_native_exports().unwrap();
    let prepared = exports
        .prepare(|| {
            workspace.native_export_content(NativeExportRequest::HtmlReport {
                report_id: report_id.clone(),
                expected_sha256: old.sha256.clone(),
            })
        })
        .unwrap();
    let saved = commit(&exports, &prepared).unwrap();
    assert_eq!(fs::read_to_string(saved.location).unwrap(), old.html);
    assert!(
        matches!(prepared.artifact, ExportArtifact::HtmlReport { workspace_revision, .. } if workspace_revision == old.workspace_revision)
    );
    assert!(exports
        .prepare(
            || workspace.native_export_content(NativeExportRequest::HtmlReport {
                report_id,
                expected_sha256: "a".repeat(64)
            })
        )
        .is_err());
    assert!(exports
        .prepare(|| workspace.native_export_content(request(0)))
        .is_err());
    exports.shutdown().unwrap();
}
#[test]
fn stage_expiry_discard_and_completed_cache_are_bounded() {
    let (_temp, workspace) = fixture();
    let exports = workspace.start_native_exports().unwrap();
    let prepared = prepare(&exports, &workspace);
    exports
        .session
        .registry
        .lock()
        .unwrap()
        .stage
        .as_mut()
        .unwrap()
        .expires = Instant::now();
    assert!(commit(&exports, &prepared).is_err());
    assert!(!exports.session.stage_path(&prepared.ticket).exists());
    exports.discard(&prepared.ticket).unwrap();
    exports.discard(&prepared.ticket).unwrap();
    assert!(exports.commit("../target", "", 0).is_err());
    for _ in 0..10 {
        let value = prepare(&exports, &workspace);
        commit(&exports, &value).unwrap();
    }
    assert_eq!(exports.session.registry.lock().unwrap().completed.len(), 8);
    exports.shutdown().unwrap();
}
#[test]
fn staged_and_target_tampering_fail_without_overwrite_or_false_receipt() {
    let (_temp, workspace) = fixture();
    let exports = workspace.start_native_exports().unwrap();
    let prepared = prepare(&exports, &workspace);
    fs::write(exports.session.stage_path(&prepared.ticket), b"bad").unwrap();
    assert!(commit(&exports, &prepared).is_err());
    exports.discard(&prepared.ticket).unwrap();
    let prepared = prepare(&exports, &workspace);
    let target = exports.session.output(&prepared.artifact);
    fs::write(&target, b"prior synthetic target").unwrap();
    assert!(commit(&exports, &prepared).is_err());
    assert_eq!(fs::read(&target).unwrap(), b"prior synthetic target");
    assert!(exports
        .session
        .registry
        .lock()
        .unwrap()
        .completed
        .is_empty());
    exports.discard(&prepared.ticket).unwrap();
    fs::remove_file(&target).unwrap();
    let prepared = prepare(&exports, &workspace);
    let saved = commit(&exports, &prepared).unwrap();
    fs::write(&saved.location, b"edited").unwrap();
    assert!(commit(&exports, &prepared).is_err());
    exports.shutdown().unwrap();
}
#[test]
fn cleanup_failure_is_explicit_retains_stage_and_blocks_new_preparation() {
    let (_temp, workspace) = fixture();
    let exports = workspace.start_native_exports().unwrap();
    let prepared = prepare(&exports, &workspace);
    let pending = exports.session.stage_path(&prepared.ticket);
    fs::remove_file(&pending).unwrap();
    fs::create_dir(&pending).unwrap();
    assert!(matches!(
        exports.discard(&prepared.ticket),
        Err(Error::Cleanup(_))
    ));
    assert!(exports
        .prepare(|| workspace.native_export_content(request(0)))
        .is_err());
    assert!(matches!(exports.shutdown(), Err(Error::Cleanup(_))));
    fs::remove_dir(&pending).unwrap();
    exports.shutdown().unwrap();
}
#[cfg(unix)]
#[test]
fn links_never_touch_outside_sentinel_and_startup_refuses_unexpected_entries() {
    use std::os::unix::fs::symlink;
    let (temp, workspace) = fixture();
    let outside = temp.path().join("outside");
    fs::write(&outside, b"synthetic sentinel").unwrap();
    let exports = workspace.start_native_exports().unwrap();
    let prepared = prepare(&exports, &workspace);
    let pending = exports.session.stage_path(&prepared.ticket);
    fs::remove_file(&pending).unwrap();
    symlink(&outside, &pending).unwrap();
    assert!(commit(&exports, &prepared).is_err());
    exports.discard(&prepared.ticket).unwrap();
    let prepared = prepare(&exports, &workspace);
    let target = exports.session.output(&prepared.artifact);
    symlink(&outside, &target).unwrap();
    assert!(commit(&exports, &prepared).is_err());
    exports.discard(&prepared.ticket).unwrap();
    fs::remove_file(target).unwrap();
    let prepared = prepare(&exports, &workspace);
    fs::hard_link(
        exports.session.stage_path(&prepared.ticket),
        temp.path().join("extra-link"),
    )
    .unwrap();
    assert!(commit(&exports, &prepared).is_err());
    exports.discard(&prepared.ticket).unwrap();
    assert_eq!(fs::read(&outside).unwrap(), b"synthetic sentinel");
    exports.shutdown().unwrap();
    fs::write(exports.session.staging().join("unknown"), b"leave").unwrap();
    assert!(workspace.start_native_exports().is_err());
}
#[test]
fn shutdown_during_preparation_waits_then_rejects_and_cleans_without_deadlock() {
    use std::sync::mpsc;
    let (_temp, workspace) = fixture();
    let exports = Arc::new(workspace.start_native_exports().unwrap());
    let content = workspace.native_export_content(request(0)).unwrap();
    let (entered_tx, entered_rx) = mpsc::channel();
    let (release_tx, release_rx) = mpsc::channel();
    let preparing = exports.clone();
    let worker = thread::spawn(move || {
        preparing.prepare(|| {
            entered_tx.send(()).unwrap();
            release_rx.recv().unwrap();
            Ok(content)
        })
    });
    entered_rx.recv_timeout(Duration::from_secs(2)).unwrap();
    let stopping = exports.clone();
    let (stopped_tx, stopped_rx) = mpsc::channel();
    let shutdown = thread::spawn(move || {
        let result = stopping.shutdown();
        stopped_tx.send(()).unwrap();
        result
    });
    while !exports.session.stopping.load(Ordering::Acquire) {
        thread::yield_now();
    }
    assert!(stopped_rx.try_recv().is_err());
    release_tx.send(()).unwrap();
    assert!(worker.join().unwrap().is_err());
    shutdown.join().unwrap().unwrap();
    assert_eq!(fs::read_dir(exports.session.staging()).unwrap().count(), 0);
    assert!(exports
        .prepare(|| workspace.native_export_content(request(0)))
        .is_err());
}
#[test]
fn coordinator_shutdown_and_restart_clean_stages_without_canonical_mutation() {
    let (temp, workspace) = fixture();
    let coordinator = JobCoordinator::start(workspace, 1).unwrap();
    let prepared = coordinator.prepare_native_export(request(0)).unwrap();
    coordinator.shutdown().unwrap();
    assert!(coordinator
        .commit_native_export(
            &prepared.ticket,
            prepared.artifact.sha256(),
            prepared.artifact.bytes()
        )
        .is_err());
    let workspace = Workspace::open(temp.path().join("case")).unwrap();
    let exports = workspace.start_native_exports().unwrap();
    assert_eq!(workspace.revision().unwrap(), 0);
    assert_eq!(fs::read_dir(exports.session.staging()).unwrap().count(), 0);
    exports.shutdown().unwrap();
}

#[cfg(unix)]
#[test]
fn verification_refuses_path_replacement_and_fifo_swap_without_blocking() {
    use std::os::unix::ffi::OsStrExt;
    let (_temp, workspace) = fixture();
    let exports = workspace.start_native_exports().unwrap();
    let prepared = prepare(&exports, &workspace);
    let path = exports.session.stage_path(&prepared.ticket);
    let original = fs::read(&path).unwrap();
    let changed = verify_checked(
        &path,
        &prepared.artifact,
        1,
        |_| Ok(()),
        |path| {
            fs::remove_file(path)?;
            fs::write(path, &original)?;
            Ok(())
        },
    );
    assert!(changed.is_err());
    let started = Instant::now();
    let special = verify_checked(
        &path,
        &prepared.artifact,
        1,
        |path| {
            fs::remove_file(path)?;
            let name = std::ffi::CString::new(path.as_os_str().as_bytes()).unwrap();
            // Synthetic named FIFO in a disposable test workspace. The C string is live.
            assert_eq!(unsafe { libc::mkfifo(name.as_ptr(), 0o600) }, 0);
            Ok(())
        },
        |_| panic!("special file must be refused before reading"),
    );
    assert!(special.is_err());
    assert!(started.elapsed() < Duration::from_secs(2));
    exports.discard(&prepared.ticket).unwrap();
    exports.shutdown().unwrap();
}

#[test]
fn identical_external_hardlink_target_is_rejected_for_publication_and_reuse() {
    let (temp, workspace) = fixture();
    let exports = workspace.start_native_exports().unwrap();
    let prepared = prepare(&exports, &workspace);
    let content = workspace.native_export_content(request(0)).unwrap().1;
    let outside = temp.path().join("same-content-outside");
    fs::write(&outside, content.as_slice()).unwrap();
    let target = exports.session.output(&prepared.artifact);
    fs::hard_link(&outside, &target).unwrap();
    assert!(commit(&exports, &prepared).is_err());
    assert_eq!(fs::read(&outside).unwrap(), content.as_slice());
    exports.discard(&prepared.ticket).unwrap();
    fs::remove_file(&target).unwrap();
    let prepared = prepare(&exports, &workspace);
    let saved = commit(&exports, &prepared).unwrap();
    fs::hard_link(&saved.location, temp.path().join("later-external-link")).unwrap();
    assert!(commit(&exports, &prepared).is_err());
    let new = prepare(&exports, &workspace);
    assert!(commit(&exports, &new).is_err());
    exports.discard(&new.ticket).unwrap();
    exports.shutdown().unwrap();
}

#[test]
fn startup_recovers_owned_publication_orphan_but_rejects_noncanonical_names() {
    let (_temp, workspace) = fixture();
    let exports = workspace.start_native_exports().unwrap();
    let prepared = prepare(&exports, &workspace);
    let pending = exports.session.stage_path(&prepared.ticket);
    let target = exports.session.output(&prepared.artifact);
    fs::hard_link(&pending, &target).unwrap();
    // Model a crash after no-clobber publication: no in-memory session owns it.
    exports.session.registry.lock().unwrap().stage = None;
    exports.shutdown().unwrap();
    let restarted = workspace.start_native_exports().unwrap();
    assert!(!pending.exists());
    verify(&target, &prepared.artifact, 1).unwrap();
    restarted.shutdown().unwrap();
    let invalid = restarted
        .session
        .staging()
        .join(format!("{}.pending", Uuid::new_v4().simple()));
    fs::write(&invalid, b"unknown").unwrap();
    assert!(workspace.start_native_exports().is_err());
    assert!(invalid.exists());
}

#[cfg(windows)]
#[test]
fn open_export_denies_write_and_delete_sharing() {
    let (_temp, workspace) = fixture();
    let exports = workspace.start_native_exports().unwrap();
    let prepared = prepare(&exports, &workspace);
    let path = exports.session.stage_path(&prepared.ticket);
    verify_checked(
        &path,
        &prepared.artifact,
        1,
        |_| Ok(()),
        |path| {
            require(
                OpenOptions::new().write(true).open(path).is_err(),
                "Conflicting export write allowed",
            )?;
            require(
                fs::remove_file(path).is_err(),
                "Conflicting export deletion allowed",
            )
        },
    )
    .unwrap();
    exports.shutdown().unwrap();
}

fn docx_request(record: &crate::docx_snapshot::DocxSnapshotRecord) -> NativeExportRequest {
    NativeExportRequest::DocxReport {
        report_id: record.id.clone(),
        expected_document_sha256: record.document.sha256.clone(),
        expected_docx_sha256: record.docx.sha256.clone(),
    }
}
fn publish_docx(workspace: &mut Workspace) -> crate::docx_snapshot::DocxSnapshotRecord {
    workspace
        .save_docx_snapshot(&id(), workspace.revision().unwrap())
        .unwrap()
}

#[test]
fn docx_native_save_preserves_binary_both_digests_and_historical_revision_through_lost_ack() {
    let (_temp, mut workspace) = fixture();
    workspace
        .import("source.txt", b"Synthetic frozen report source")
        .unwrap();
    let record = publish_docx(&mut workspace);
    let bytes = workspace
        .read_docx_snapshot(&record.id, &record.document.sha256, &record.docx.sha256)
        .unwrap();
    assert!(bytes.starts_with(b"PK"));
    assert!(
        std::str::from_utf8(&bytes).is_err(),
        "binary specimen unexpectedly behaves as text"
    );
    workspace
        .import("later.txt", b"Later source must not regenerate this DOCX")
        .unwrap();
    let revision = workspace.revision().unwrap();
    let exports = workspace.start_native_exports().unwrap();
    let prepared = exports
        .prepare(|| workspace.native_export_content(docx_request(&record)))
        .unwrap();
    assert!(
        matches!(&prepared.artifact,ExportArtifact::DocxReport{report_id,workspace_revision,document_sha256,sha256,bytes} if report_id==&record.id && workspace_revision==&record.workspace_revision && document_sha256==&record.document.sha256 && sha256==&record.docx.sha256 && bytes==&record.docx.bytes)
    );
    assert!(exports
        .prepare(|| workspace.native_export_content(docx_request(&record)))
        .is_err());
    let saved = commit(&exports, &prepared).unwrap();
    assert_eq!(
        saved.filename,
        format!("assessment-{}-{}.docx", record.id, record.docx.sha256)
    );
    assert_eq!(fs::read(&saved.location).unwrap(), bytes);
    assert_eq!(
        serde_json::to_value(commit(&exports, &prepared).unwrap()).unwrap(),
        serde_json::to_value(&saved).unwrap()
    );
    assert!(matches!(
        exports.discard(&prepared.ticket).unwrap(),
        DiscardedExport::Saved { .. }
    ));
    let again = exports
        .prepare(|| workspace.native_export_content(docx_request(&record)))
        .unwrap();
    assert_eq!(commit(&exports, &again).unwrap().location, saved.location);
    assert_eq!(fs::read_dir(exports.session.exports()).unwrap().count(), 1);
    assert_eq!(workspace.revision().unwrap(), revision);
    exports.shutdown().unwrap();
}

#[test]
fn docx_native_preparation_rejects_wrong_identity_and_frontend_bytes_without_a_stage() {
    let (_temp, mut workspace) = fixture();
    let record = publish_docx(&mut workspace);
    let exports = workspace.start_native_exports().unwrap();
    for document in [false, true] {
        let mut request = docx_request(&record);
        if let NativeExportRequest::DocxReport {
            expected_document_sha256,
            expected_docx_sha256,
            ..
        } = &mut request
        {
            if document {
                *expected_document_sha256 = "0".repeat(64);
            } else {
                *expected_docx_sha256 = "0".repeat(64);
            }
        }
        assert!(exports
            .prepare(|| workspace.native_export_content(request))
            .is_err());
        assert!(exports.session.registry.lock().unwrap().stage.is_none());
    }
    let mut forged = serde_json::to_value(docx_request(&record)).unwrap();
    forged["bytes"] = json!([1, 2, 3]);
    assert!(serde_json::from_value::<NativeExportRequest>(forged).is_err());
    let mut forged = serde_json::to_value(docx_request(&record)).unwrap();
    forged["path"] = json!("../outside.docx");
    assert!(serde_json::from_value::<NativeExportRequest>(forged).is_err());
    workspace.save_report().unwrap();
    let mut request = docx_request(&record);
    if let NativeExportRequest::DocxReport { report_id, .. } = &mut request {
        *report_id = workspace.view().unwrap().reports[0].id.clone();
    }
    assert!(exports
        .prepare(|| workspace.native_export_content(request))
        .is_err());
    assert_eq!(fs::read_dir(exports.session.staging()).unwrap().count(), 0);
    exports.shutdown().unwrap();
}

#[test]
fn docx_stage_expiry_tamper_and_coordinator_shutdown_retain_exact_lifecycle_boundaries() {
    let (temp, mut workspace) = fixture();
    let record = publish_docx(&mut workspace);
    let exports = workspace.start_native_exports().unwrap();
    let staged = exports
        .prepare(|| workspace.native_export_content(docx_request(&record)))
        .unwrap();
    exports
        .session
        .registry
        .lock()
        .unwrap()
        .stage
        .as_mut()
        .unwrap()
        .expires = Instant::now();
    assert!(commit(&exports, &staged).is_err());
    assert!(!exports.session.stage_path(&staged.ticket).exists());
    let staged = exports
        .prepare(|| workspace.native_export_content(docx_request(&record)))
        .unwrap();
    let path = exports.session.stage_path(&staged.ticket);
    let mut bytes = fs::read(&path).unwrap();
    bytes[0] ^= 1;
    fs::write(path, &bytes).unwrap();
    assert!(commit(&exports, &staged).is_err());
    exports.discard(&staged.ticket).unwrap();
    let staged = exports
        .prepare(|| workspace.native_export_content(docx_request(&record)))
        .unwrap();
    let target = exports.session.output(&staged.artifact);
    fs::write(&target, b"Preserve this unrelated target").unwrap();
    assert!(commit(&exports, &staged).is_err());
    exports.discard(&staged.ticket).unwrap();
    assert_eq!(
        fs::read(&target).unwrap(),
        b"Preserve this unrelated target"
    );
    fs::remove_file(target).unwrap();
    exports.shutdown().unwrap();
    let coordinator = JobCoordinator::start(workspace, 1).unwrap();
    let staged = coordinator
        .prepare_native_export(docx_request(&record))
        .unwrap();
    coordinator.shutdown().unwrap();
    assert!(coordinator
        .commit_native_export(
            &staged.ticket,
            staged.artifact.sha256(),
            staged.artifact.bytes()
        )
        .is_err());
    let workspace = Workspace::open(temp.path().join("case")).unwrap();
    assert_eq!(workspace.revision().unwrap(), record.workspace_revision + 1);
    let restarted = workspace.start_native_exports().unwrap();
    assert_eq!(
        fs::read_dir(restarted.session.staging()).unwrap().count(),
        0
    );
    restarted.shutdown().unwrap();
}

#[test]
fn docx_has_its_own_binary_limit_before_staging_even_if_other_export_kinds_allow_more() {
    let (_temp, workspace) = fixture();
    let exports = workspace.start_native_exports().unwrap();
    // Only the internal test closure can provide bytes; native IPC accepts typed source identities.
    let content = vec![0; crate::report_docx::MAX_DOCX_BYTES + 1];
    let artifact = ExportArtifact::DocxReport {
        report_id: id(),
        workspace_revision: 0,
        document_sha256: "0".repeat(64),
        bytes: content.len() as u64,
        sha256: hash(&content),
    };
    assert!(exports.prepare(|| Ok((artifact, content))).is_err());
    assert!(exports.session.registry.lock().unwrap().stage.is_none());
    assert_eq!(fs::read_dir(exports.session.staging()).unwrap().count(), 0);
    exports.shutdown().unwrap();
}

#[test]
fn housekeeping_cannot_falsely_reject_first_prepare_but_concurrent_generation_is_blocked() {
    use std::sync::mpsc;
    let (_temp, mut workspace) = fixture();
    let record = publish_docx(&mut workspace);
    let exports = Arc::new(workspace.start_native_exports().unwrap());
    let content = workspace
        .native_export_content(docx_request(&record))
        .unwrap();
    // Model exactly the registry lock held by the janitor, before any stage exists.
    let housekeeping = exports.session.registry.lock().unwrap();
    let preparing = exports.clone();
    let (generated_tx, generated_rx) = mpsc::channel();
    let (claimed_tx, claimed_rx) = mpsc::channel();
    let worker = thread::spawn(move || {
        preparing.prepare_claimed(
            || {
                generated_tx.send(()).unwrap();
                Ok(content)
            },
            || claimed_tx.send(()).unwrap(),
        )
    });
    claimed_rx.recv_timeout(Duration::from_secs(2)).unwrap();
    assert!(generated_rx.try_recv().is_err());
    assert!(exports
        .prepare(|| panic!("second generator must not start"))
        .is_err());
    drop(housekeeping);
    generated_rx.recv_timeout(Duration::from_secs(2)).unwrap();
    let prepared = worker.join().unwrap().unwrap();
    let saved = commit(&exports, &prepared).unwrap();
    assert_eq!(
        fs::read(saved.location).unwrap().len() as u64,
        record.docx.bytes
    );
    exports.shutdown().unwrap();
}
