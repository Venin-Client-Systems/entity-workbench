use super::*;
use crate::engines::{search_lifecycle::INTENT, Runtime};
fn fixture() -> (tempfile::TempDir, Workspace) {
    let root = tempfile::tempdir_in(std::env::temp_dir().canonicalize().unwrap()).unwrap();
    let mut workspace = Workspace::open(root.path()).unwrap();
    workspace.attach_runtime(Runtime {
        root: root.path().join("not-executed"),
    });
    (root, workspace)
}
fn empty() -> crate::engines::SearchResults {
    crate::engines::SearchResults {
        workspace_revision: "1".into(),
        hits: vec![],
        total: 0,
    }
}
#[test]
fn search_lifecycle_workspace_owner_cannot_cross_workspaces_or_release_quarantine() {
    let (_a, mut a) = fixture();
    let (_b, mut b) = fixture();
    let owner = a.collection_ownership().unwrap();
    assert!(b
        .search_owned_with(&owner, "needle", |_, _, _, _, _| panic!("no launch"))
        .is_err());
    owner.quarantine();
    assert!(a
        .search_owned_with(&owner, "needle", |_, _, _, _, _| panic!("no launch"))
        .is_err());
    assert!(owner.publication_held());
    assert!(owner.release().is_err());
}
#[test]
fn search_lifecycle_standalone_cannot_reacquire_current_coordinator_owner() {
    let (_root, mut workspace) = fixture();
    let owner = workspace.collection_ownership().unwrap();
    assert!(workspace
        .dispatch(Command::Search {
            query: "needle".into()
        })
        .is_err());
    assert!(owner.held());
    owner.release().unwrap();
}
#[test]
fn search_lifecycle_bootstrap_intent_quarantines_owner_but_preserves_reads_and_revision() {
    for bytes in [b"".as_slice(), b"{", br#"{"format_version":999}"#] {
        let (root, mut workspace) = fixture();
        let before = workspace.revision().unwrap();
        let cache = root.path().join("indexes/lucene");
        fs::create_dir_all(&cache).unwrap();
        fs::write(cache.join(INTENT), bytes).unwrap();
        let owner = workspace.collection_ownership().unwrap();
        assert!(!owner.held());
        assert!(owner.publication_held());
        assert!(workspace
            .search_owned_with(&owner, "needle", |_, _, _, _, _| panic!("no launch"))
            .is_err());
        assert!(workspace.view().is_ok());
        assert_eq!(workspace.revision().unwrap(), before);
        assert_eq!(fs::read(cache.join(INTENT)).unwrap(), bytes);
    }
}
#[test]
fn search_lifecycle_existing_processing_uncertainty_blocks_before_callback() {
    let (_root, mut workspace) = fixture();
    // Synthetic retained minimal body exercises the durable execution gate, not claim parsing.
    put(
        &workspace.conn,
        "processing_job",
        "synthetic",
        &json!({"failure":"worker_exit_unverified"}),
    )
    .unwrap();
    let before = workspace.revision().unwrap();
    let owner = workspace.collection_ownership().unwrap();
    assert!(workspace
        .search_owned_with(&owner, "needle", |_, _, _, _, _| panic!("no launch"))
        .is_err());
    assert!(!owner.held());
    assert_eq!(workspace.revision().unwrap(), before);
}
#[test]
fn search_lifecycle_standalone_refuses_orphaned_running_or_legacy_interrupted_claims() {
    for (kind, body) in [
        ("processing_job", json!({"state":"running"})),
        (
            "processing_job",
            json!({"failure":"interrupted","lease":null}),
        ),
        ("collection_run", json!({"checkpoint":{"state":"running"}})),
    ] {
        let (root, mut workspace) = fixture();
        let before = workspace.revision().unwrap();
        put(&workspace.conn, kind, "synthetic", &body).unwrap();
        assert!(matches!(
            workspace.dispatch(Command::Search {
                query: "needle".into()
            }),
            Err(Error::Blocked(_))
        ));
        assert_eq!(workspace.revision().unwrap(), before);
        assert!(!root.path().join("indexes").exists());
        assert!(workspace.lock_processing().is_err());
    }
}
#[test]
fn search_lifecycle_collection_recovery_refuses_search_without_starting_runtime() {
    let (_root, mut workspace) = fixture();
    put(
        &workspace.conn,
        "collection_run",
        "synthetic",
        &json!({"checkpoint":{"state":"recovery_required"}}),
    )
    .unwrap();
    let owner = workspace.collection_ownership().unwrap();
    assert!(matches!(
        workspace.search_owned_with(&owner, "needle", |_, _, _, _, _| panic!("no launch")),
        Err(Error::Blocked(_))
    ));
    assert!(!owner.held());
}
#[test]
fn search_lifecycle_typed_disposition_quarantines_even_when_no_marker_can_be_read() {
    let (root, mut workspace) = fixture();
    let before = workspace.revision().unwrap();
    let owner = workspace.collection_ownership().unwrap();
    let outcome = workspace.search_owned_with(&owner, "needle", |_, _, _, _, _| Completion {
        result: Err(Error::TerminationUnverified(
            "synthetic exact primary".into(),
        )),
        disposition: Disposition::RecoveryRequired,
    });
    assert!(
        matches!(outcome, Err(Error::TerminationUnverified(ref value)) if value == "synthetic exact primary")
    );
    assert!(!root.path().join("indexes").exists());
    assert!(!owner.held());
    assert_eq!(workspace.revision().unwrap(), before);
    assert!(workspace
        .search_owned_with(&owner, "needle", |_, _, _, _, _| panic!("no launch"))
        .is_err());
}
#[test]
fn search_lifecycle_safe_completion_keeps_owner_and_panic_quarantines() {
    let (_root, mut workspace) = fixture();
    let before = workspace.revision().unwrap();
    let owner = workspace.collection_ownership().unwrap();
    assert!(workspace
        .search_owned_with(&owner, "needle", |_, _, _, _, _| Completion::released(Ok(
            empty()
        )))
        .is_ok());
    assert!(owner.held());
    let failed = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
        workspace.search_owned_with(&owner, "needle", |_, _, _, _, _| panic!("synthetic panic"))
    }));
    assert!(failed.is_err());
    assert!(!owner.held());
    assert_eq!(workspace.revision().unwrap(), before);
}
#[cfg(unix)]
#[test]
fn search_lifecycle_unreadable_marker_and_linked_ancestor_require_recovery() {
    use std::os::unix::fs::{symlink, PermissionsExt};
    let (root, workspace) = fixture();
    let cache = root.path().join("indexes/lucene");
    fs::create_dir_all(&cache).unwrap();
    fs::write(cache.join(INTENT), b"retained").unwrap();
    fs::set_permissions(cache.join(INTENT), fs::Permissions::from_mode(0o0)).unwrap();
    let owner = workspace.collection_ownership().unwrap();
    assert!(!owner.held());
    assert!(owner.publication_held());
    fs::set_permissions(cache.join(INTENT), fs::Permissions::from_mode(0o600)).unwrap();
    let (root, workspace) = fixture();
    let outside = root.path().join("outside");
    fs::create_dir(&outside).unwrap();
    symlink(&outside, root.path().join("indexes")).unwrap();
    assert!(!workspace.collection_ownership().unwrap().held());
    assert!(!outside.join("lucene").exists());
}

#[test]
fn search_lifecycle_real_orphan_claim_remains_unmodified_and_cannot_launch_search() {
    let (root, mut workspace) = fixture();
    let evidence_id = workspace
        .import("synthetic.source", b"Unaccepted synthetic source")
        .unwrap();
    workspace
        .dispatch(Command::QueueDocumentParse {
            evidence_id,
            request_key: id(),
        })
        .unwrap();
    let owner = workspace.collection_ownership().unwrap();
    let prepared = workspace.claim_processing_job().unwrap().unwrap();
    let job_id = prepared.ticket.job_id.clone();
    // The source fixture models process loss by releasing its OS owner without publication.
    // It does not create or claim termination of a native process.
    owner.release().unwrap();
    let before: String = workspace
        .conn
        .query_row(
            "SELECT body FROM records WHERE kind='processing_job' AND id=?",
            [&job_id],
            |row| row.get(0),
        )
        .unwrap();
    let revision = workspace.revision().unwrap();
    assert!(matches!(
        workspace.dispatch(Command::Search {
            query: "needle".into()
        }),
        Err(Error::Blocked(_))
    ));
    let after: String = workspace
        .conn
        .query_row(
            "SELECT body FROM records WHERE kind='processing_job' AND id=?",
            [&job_id],
            |row| row.get(0),
        )
        .unwrap();
    assert_eq!(before, after);
    assert_eq!(workspace.revision().unwrap(), revision);
    assert!(!root.path().join("indexes").exists());
}
