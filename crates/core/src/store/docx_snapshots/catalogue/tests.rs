use super::*;
use crate::coordinator::JobCoordinator;

fn workspace() -> (tempfile::TempDir, Workspace) {
    let temp = tempfile::tempdir_in(std::env::temp_dir().canonicalize().unwrap()).unwrap();
    let workspace = Workspace::open(temp.path().join("case")).unwrap();
    (temp, workspace)
}
fn publish(workspace: &mut Workspace) -> DocxSnapshotRecord {
    workspace
        .save_docx_snapshot(&id(), workspace.revision().unwrap())
        .unwrap()
}
fn request(size: u32) -> DocxSnapshotPageRequest {
    DocxSnapshotPageRequest {
        page_size: size,
        cursor: None,
    }
}
fn page(workspace: &Workspace, request: &DocxSnapshotPageRequest) -> Result<DocxSnapshotPage> {
    workspace.page_docx_snapshots(request, workspace.revision().unwrap())
}

#[test]
fn direct_commands_recover_uncertain_publication_without_default_refresh_or_duplicate_writes() {
    type Dispatch = fn(&mut Workspace, Command) -> Result<Value>;
    for dispatch in [
        Workspace::dispatch as Dispatch,
        Workspace::dispatch_presentation,
        Workspace::dispatch_summary,
    ] {
        let (_temp, mut workspace) = workspace();
        let key = id();
        let command = Command::SaveDocxSnapshot {
            request_id: key.clone(),
            expected_revision: 0,
        };
        let first = dispatch(&mut workspace, command.clone()).unwrap();
        assert!(first.get("workspace").is_none());
        let record: DocxSnapshotRecord = serde_json::from_value(first.clone()).unwrap();
        assert_eq!(record.id, key);
        assert_eq!(record.workspace_revision, 0);
        assert_eq!(workspace.revision().unwrap(), 1);
        let refreshed = dispatch(&mut workspace, Command::View {}).unwrap();
        assert_eq!(refreshed["workspace"]["revision"], 1);
        assert!(refreshed["workspace"].get("docx_snapshots").is_none());
        assert_eq!(refreshed["workspace"]["reports"], json!([]));
        workspace
            .import(
                "later.txt",
                b"Later canonical source after uncertain acknowledgement",
            )
            .unwrap();
        let revision = workspace.revision().unwrap();
        assert_eq!(dispatch(&mut workspace, command).unwrap(), first);
        assert_eq!(workspace.revision().unwrap(), revision);
        let catalogue = dispatch(
            &mut workspace,
            Command::PageDocxSnapshots {
                request: request(10),
                expected_revision: revision,
            },
        )
        .unwrap();
        assert_eq!(catalogue["total_count"], 1);
        assert_eq!(catalogue["rows"][0], first);
        let inspect_command = Command::InspectDocxSnapshot {
            report_id: key,
            expected_document_sha256: record.document.sha256,
            expected_docx_sha256: record.docx.sha256,
        };
        let inspected = dispatch(&mut workspace, inspect_command).unwrap();
        assert_eq!(inspected["snapshot"], first);
        assert_eq!(inspected["document"]["workspace_revision"], 0);
        assert!(inspected.get("workspace").is_none());
        assert!(dispatch(
            &mut workspace,
            Command::SaveDocxSnapshot {
                request_id: id(),
                expected_revision: 0
            }
        )
        .is_err());
    }
}

#[test]
fn canonical_sequence_pages_cover_more_than_fifty_reports_without_html_or_duplicates() {
    let (_temp, mut workspace) = workspace();
    workspace.save_report().unwrap();
    let mut expected = Vec::new();
    for _ in 0..53 {
        expected.push(publish(&mut workspace).id);
    }
    expected.reverse();
    let revision = workspace.revision().unwrap();
    let mut next = request(17);
    let mut ids = Vec::new();
    let mut sizes = Vec::new();
    loop {
        let result = workspace.page_docx_snapshots(&next, revision).unwrap();
        assert_eq!(result.total_count, 53);
        assert_eq!(result.workspace_revision, revision);
        sizes.push(result.rows.len());
        ids.extend(result.rows.into_iter().map(|r| r.id));
        next.cursor = result.next_cursor;
        if next.cursor.is_none() {
            break;
        }
    }
    assert_eq!(sizes, [17, 17, 17, 2]);
    assert_eq!(ids, expected);
    assert_eq!(workspace.revision().unwrap(), revision);
    assert_eq!(workspace.view().unwrap().reports.len(), 1);
}

#[test]
fn catalogue_cursor_scope_bounds_and_stale_revision_are_explicit() {
    let (_temp, mut workspace) = workspace();
    let empty = page(&workspace, &request(50)).unwrap();
    assert_eq!(empty.total_count, 0);
    assert!(empty.rows.is_empty() && empty.next_cursor.is_none());
    for _ in 0..3 {
        publish(&mut workspace);
    }
    let first = page(&workspace, &request(1)).unwrap();
    let cursor = first.next_cursor.unwrap();
    assert!(page(
        &workspace,
        &DocxSnapshotPageRequest {
            page_size: 2,
            cursor: Some(cursor.clone())
        }
    )
    .is_err());
    for bad in [
        String::new(),
        "../outside".into(),
        "f".repeat(MAX_DOCX_CURSOR_BYTES + 1),
        "00".into(),
    ] {
        assert!(page(
            &workspace,
            &DocxSnapshotPageRequest {
                page_size: 1,
                cursor: Some(bad)
            }
        )
        .is_err());
    }
    for size in [0, 51, u32::MAX] {
        assert!(page(&workspace, &request(size)).is_err());
    }
    for sequence in [0, i64::MAX] {
        let forged = Cursor {
            schema_version: 1,
            query_sha256: first.query_sha256.clone(),
            sequence,
        }
        .encode()
        .unwrap();
        assert!(page(
            &workspace,
            &DocxSnapshotPageRequest {
                page_size: 1,
                cursor: Some(forged)
            }
        )
        .is_err());
    }
    let original_revision = workspace.revision().unwrap();
    workspace.import("later.txt", b"Changed revision").unwrap();
    assert!(matches!(
        workspace.page_docx_snapshots(&request(1), original_revision),
        Err(Error::Conflict(_))
    ));
    assert!(page(
        &workspace,
        &DocxSnapshotPageRequest {
            page_size: 1,
            cursor: Some(cursor)
        }
    )
    .is_err());
    assert!(serde_json::from_value::<Command>(json!({"action":"save_docx_snapshot","request_id":id(),"expected_revision":0,"bytes":"untrusted"})).is_err());
    assert!(serde_json::from_value::<DocxSnapshotPageRequest>(
        json!({"page_size":1,"cursor":null,"sql":"DROP TABLE records"})
    )
    .is_err());
}

#[test]
fn catalogue_validates_only_requested_metadata_and_never_claims_artifact_integrity() {
    let (_temp, mut workspace) = workspace();
    let old = publish(&mut workspace);
    let recent = publish(&mut workspace);
    // The older malformed row is outside the first requested page; it must fail
    // when reached, not be silently omitted from the denominator or continuation.
    for body in [
        "{}".into(),
        format!(
            "{}{}",
            serde_json::to_string(&old).unwrap(),
            " ".repeat(MAX_RECORD_BYTES as usize)
        ),
    ] {
        workspace
            .conn
            .execute(
                "UPDATE records SET body=? WHERE kind=? AND id=?",
                params![body, KIND, old.id],
            )
            .unwrap();
        let first = page(&workspace, &request(1)).unwrap();
        assert_eq!(first.rows[0], recent);
        assert_eq!(first.total_count, 2);
        assert!(first.next_cursor.is_some());
        assert!(page(
            &workspace,
            &DocxSnapshotPageRequest {
                page_size: 1,
                cursor: first.next_cursor
            }
        )
        .is_err());
    }
    put(&workspace.conn, KIND, &old.id, &old).unwrap();
    let path = workspace
        .root
        .join("derivatives/objects")
        .join(&recent.docx.sha256);
    fs::remove_file(path).unwrap();
    assert_eq!(page(&workspace, &request(1)).unwrap().rows[0], recent);
    assert!(workspace
        .inspect_docx_snapshot(&recent.id, &recent.document.sha256, &recent.docx.sha256)
        .is_err());
    workspace
        .conn
        .execute(
            "UPDATE derivative_objects SET bytes=bytes+1 WHERE sha256=?",
            [&recent.docx.sha256],
        )
        .unwrap();
    assert!(page(&workspace, &request(1)).is_err());
}

#[test]
fn requested_corrupt_oversized_and_mismatched_metadata_never_returns_a_partial_page() {
    let (_temp, mut workspace) = workspace();
    let record = publish(&mut workspace);
    let body = serde_json::to_string(&record).unwrap();
    for altered in [
        format!("{body}{}", " ".repeat(MAX_RECORD_BYTES as usize)),
        "{}".into(),
    ] {
        workspace
            .conn
            .execute(
                "UPDATE records SET body=? WHERE kind=? AND id=?",
                params![altered, KIND, record.id],
            )
            .unwrap();
        assert!(page(&workspace, &request(50)).is_err());
        assert!(workspace.conn.is_autocommit());
    }
    let mut changed = record.clone();
    changed.id = id();
    put(&workspace.conn, KIND, &record.id, &changed).unwrap();
    assert!(page(&workspace, &request(50)).is_err());
    changed = record.clone();
    changed.workspace_revision = workspace.revision().unwrap();
    put(&workspace.conn, KIND, &record.id, &changed).unwrap();
    assert!(page(&workspace, &request(50)).is_err());
    put(&workspace.conn, KIND, &record.id, &record).unwrap();
    workspace
        .conn
        .execute(
            "UPDATE records SET id=? WHERE kind=?",
            params!["x".repeat(20_000), KIND],
        )
        .unwrap();
    assert!(page(&workspace, &request(50)).is_err());
}

#[test]
fn count_rows_and_cursor_remain_in_one_snapshot_during_a_real_publication() {
    use rusqlite::hooks::{AuthAction, AuthContext, Authorization};
    use std::sync::{Arc, Mutex};
    let (temp, mut reader) = workspace();
    let first = publish(&mut reader);
    let writer = Workspace::open(temp.path().join("case")).unwrap();
    reader
        .conn
        .pragma_update(None, "journal_mode", "WAL")
        .unwrap();
    let revision = reader.revision().unwrap();
    let state = Arc::new(Mutex::new((writer, false)));
    let callback = state.clone();
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
                publish(&mut state.0);
                state.1 = true;
            }
        }
        Authorization::Allow
    }));
    let result = reader.page_docx_snapshots(&request(1), revision).unwrap();
    assert!(state.lock().unwrap().1);
    assert_eq!(result.workspace_revision, revision);
    assert_eq!(result.total_count, 1);
    assert_eq!(result.rows, [first]);
    assert!(result.next_cursor.is_none());
    assert_eq!(reader.revision().unwrap(), revision + 1);
}

#[test]
fn coordinator_dispatch_preserves_direct_receipt_and_single_publication() {
    let (_temp, workspace) = workspace();
    let coordinator = JobCoordinator::start(workspace, 1).unwrap();
    let command = Command::SaveDocxSnapshot {
        request_id: id(),
        expected_revision: 0,
    };
    let first = coordinator.dispatch_summary(command.clone()).unwrap();
    assert_eq!(coordinator.dispatch_summary(command).unwrap(), first);
    assert_eq!(
        coordinator.dispatch_summary(Command::View {}).unwrap()["workspace"]["revision"],
        1
    );
    coordinator.shutdown().unwrap();
}
