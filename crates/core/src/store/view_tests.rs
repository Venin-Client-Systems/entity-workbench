//! Interleave an actual second canonical writer at SQLite's statement boundary.
//! Hooks are a dev-only dependency; production reads have no injected callbacks.
use super::*;
use rusqlite::hooks::{AuthAction, AuthContext, Authorization};
use std::sync::{Arc, Mutex};

struct Interleaving {
    writer: Workspace,
    attempted: bool,
    committed: bool,
}
fn workspace(wal: bool) -> (tempfile::TempDir, Workspace, Arc<Mutex<Interleaving>>) {
    let temp = tempfile::tempdir_in(std::env::temp_dir().canonicalize().unwrap()).unwrap();
    let root = temp.path().join("case");
    let mut reader = Workspace::open(&root).unwrap();
    reader
        .import("before.txt", b"Synthetic snapshot before interleaving")
        .unwrap();
    let writer = Workspace::open(&root).unwrap();
    writer.conn.busy_timeout(std::time::Duration::ZERO).unwrap();
    if wal {
        reader
            .conn
            .pragma_update(None, "journal_mode", "WAL")
            .unwrap();
    }
    let state = Arc::new(Mutex::new(Interleaving {
        writer,
        attempted: false,
        committed: false,
    }));
    let callback = Arc::clone(&state);
    // The revision SELECT has completed before the first records SELECT is
    // prepared. A second real connection now attempts a canonical import.
    reader.conn.authorizer(Some(move |ctx: AuthContext<'_>| {
        if matches!(
            ctx.action,
            AuthAction::Read {
                table_name: "records",
                ..
            }
        ) {
            let mut state = callback.lock().unwrap();
            if !state.attempted {
                state.attempted = true;
                match state
                    .writer
                    .import("after.txt", b"Synthetic concurrent source")
                {
                    Ok(_) => state.committed = true,
                    Err(Error::Database(rusqlite::Error::SqliteFailure(error, _)))
                        if error.code == rusqlite::ErrorCode::DatabaseBusy => {}
                    Err(error) => panic!("unexpected second-writer result: {error}"),
                }
            }
        }
        Authorization::Allow
    }));
    (temp, reader, state)
}
#[test]
fn view_excludes_a_committed_concurrent_revision_in_wal_mode() {
    let (_temp, reader, state) = workspace(true);
    let before = reader.revision().unwrap();
    let view = reader.view().unwrap();
    let state = state.lock().unwrap();
    assert!(
        state.attempted && state.committed,
        "positive concurrent writer did not commit"
    );
    assert_eq!(view.revision, before);
    assert_eq!(
        view.evidence.len(),
        1,
        "view mixed a later source into an earlier revision"
    );
    assert_eq!(view.evidence[0].name, "before.txt");
    assert_eq!(reader.revision().unwrap(), before + 1);
    drop(state);
    assert_eq!(reader.view().unwrap().evidence.len(), 2);
}
#[test]
fn delete_journal_view_holds_writer_until_the_snapshot_is_released() {
    let (_temp, reader, state) = workspace(false);
    let before = reader.revision().unwrap();
    let view = reader.view().unwrap();
    let mut state = state.lock().unwrap();
    assert!(
        state.attempted && !state.committed,
        "writer passed the active read snapshot"
    );
    assert_eq!(view.revision, before);
    assert_eq!(view.evidence.len(), 1);
    state
        .writer
        .import("after.txt", b"Synthetic concurrent source")
        .unwrap();
    assert_eq!(state.writer.revision().unwrap(), before + 1);
    drop(state);
    assert_eq!(reader.view().unwrap().evidence.len(), 2);
}
#[test]
fn saved_report_uses_one_snapshot_when_a_second_connection_commits() {
    let (_temp, mut reader, state) = workspace(true);
    let before = reader.revision().unwrap();
    let id = reader.save_report().unwrap();
    assert!(state.lock().unwrap().committed);
    let report: ReportSnapshot = get(&reader.conn, "report", &id).unwrap();
    assert_eq!(report.workspace_revision, before);
    assert!(report.html.contains("before.txt"));
    assert!(!report.html.contains("after.txt"));
    assert_eq!(hash(report.html.as_bytes()), report.sha256);
    assert_eq!(reader.revision().unwrap(), before + 2);
    assert_eq!(reader.view().unwrap().evidence.len(), 2);
}

#[test]
fn failed_view_releases_its_snapshot_before_later_canonical_work() {
    let temp = tempfile::tempdir_in(std::env::temp_dir().canonicalize().unwrap()).unwrap();
    let root = temp.path().join("case");
    let reader = Workspace::open(&root).unwrap();
    // A valid JSON object with an invalid entity shape exercises decode failure
    // after the read transaction has begun, without bypassing SQL constraints.
    reader
        .conn
        .execute(
            "INSERT INTO records(kind,id,body) VALUES('entity','broken','{}')",
            [],
        )
        .unwrap();
    assert!(reader.view().is_err());
    assert!(reader.conn.is_autocommit());
    let mut writer = Workspace::open(&root).unwrap();
    writer
        .import("after-error.txt", b"Synthetic source after rejected view")
        .unwrap();
    assert_eq!(writer.revision().unwrap(), 1);
}
