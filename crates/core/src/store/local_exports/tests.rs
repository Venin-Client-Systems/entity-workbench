use super::*;
use crate::{
    coordinator::JobCoordinator,
    literal_search::LiteralMatching,
    transaction_export::TransactionExportRequest,
    transaction_page::{TransactionPageFilter, TransactionPageOrder},
};
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
    assert!(exports
        .prepare(|| workspace.native_export_content(request(0)))
        .is_err());
    assert!(exports
        .commit(&prepared.ticket, &"0".repeat(64), prepared.artifact.bytes())
        .is_err());
    let saved = commit(&exports, &prepared).unwrap();
    assert_eq!(fs::read(&saved.location).unwrap(), expected.as_bytes());
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
    fs::write(&outside, content.as_bytes()).unwrap();
    let target = exports.session.output(&prepared.artifact);
    fs::hard_link(&outside, &target).unwrap();
    assert!(commit(&exports, &prepared).is_err());
    assert_eq!(fs::read(&outside).unwrap(), content.as_bytes());
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
