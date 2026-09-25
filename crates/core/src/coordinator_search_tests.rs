//! Synthetic closures and deterministic barriers only; no worker processes.
use super::*;
use crate::engines::search_lifecycle::{Completion, Disposition, INTENT};
use std::{fs, sync::mpsc, time::Instant};
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
fn manual(workspace: Workspace) -> JobCoordinator {
    let ownership = Arc::new(workspace.collection_ownership().unwrap());
    let exports = workspace.start_native_exports().unwrap();
    JobCoordinator {
        exports,
        shared: Arc::new(Shared {
            workspace: Mutex::new(workspace),
            active: Mutex::new(graph::ProcessingActivity::default()),
            graph_executor: graph::GraphExecution::Unavailable,
            graph_wake: Condvar::new(),
            stopping: AtomicBool::new(false),
            wake: Condvar::new(),
            executor: Arc::new(|_, _, _, _, _| panic!("no document launch")),
            ownership,
            collection: None,
        }),
        workers: Mutex::new(vec![]),
    }
}
#[test]
fn search_lifecycle_coordinator_routes_search_using_its_exact_existing_owner() {
    let (_root, workspace) = fixture();
    let c = manual(workspace);
    let owner = c.shared.ownership.lifetime().to_owned();
    let result = c
        .dispatch_search_with(|workspace, supplied| {
            assert_eq!(supplied.lifetime(), owner);
            workspace.search_owned_with(supplied, "needle", |_, _, _, _, _| {
                Completion::released(Ok(empty()))
            })
        })
        .unwrap();
    assert_eq!(result["total"], 0);
    assert!(c.shared.ownership.held());
    c.shutdown().unwrap();
}
#[test]
fn search_lifecycle_coordinator_unknown_exit_quarantines_and_cancels_other_owned_tokens() {
    let (_root, workspace) = fixture();
    let c = manual(workspace);
    let token = CancellationToken::default();
    c.shared
        .active
        .lock()
        .unwrap()
        .tokens
        .insert("other-owned".into(), token.clone());
    let result = c.dispatch_search_with(|workspace, owner| {
        workspace.search_owned_with(owner, "needle", |_, _, _, _, _| Completion {
            result: Err(Error::TerminationUnverified(
                "synthetic exact primary".into(),
            )),
            disposition: Disposition::RecoveryRequired,
        })
    });
    assert!(matches!(result, Err(Error::TerminationUnverified(_))));
    assert!(token.is_cancelled());
    assert!(!c.shared.ownership.held());
    assert!(c
        .dispatch_search_with(|_, _| panic!("no new execution"))
        .is_err());
    assert!(c.dispatch(Command::View {}).is_ok());
    assert!(c.shutdown().is_err());
}
#[test]
fn search_lifecycle_bootstrap_quarantine_prevents_document_start_but_allows_view() {
    let (root, mut workspace) = fixture();
    let id = workspace.import("synthetic.source", b"unaccepted").unwrap();
    workspace
        .dispatch(Command::QueueDocumentParse {
            evidence_id: id,
            request_key: uuid::Uuid::new_v4().to_string(),
        })
        .unwrap();
    let revision = workspace.revision().unwrap();
    let cache = root.path().join("indexes/lucene");
    fs::create_dir_all(&cache).unwrap();
    fs::write(cache.join(INTENT), b"crash-left intent").unwrap();
    let c = JobCoordinator::with_executor(
        workspace,
        1,
        Arc::new(|_, _, _, _, _| panic!("no document launch")),
    )
    .unwrap();
    assert!(!c.shared.ownership.held());
    assert!(c.dispatch(Command::View {}).is_ok());
    assert!(c
        .dispatch(Command::Search {
            query: "needle".into()
        })
        .is_err());
    assert!(c.shutdown().is_err());
    assert_eq!(
        c.shared.workspace.lock().unwrap().revision().unwrap(),
        revision
    );
    assert_eq!(fs::read(cache.join(INTENT)).unwrap(), b"crash-left intent");
}
#[test]
fn search_lifecycle_shutdown_waits_for_admitted_dispatch_after_worker_registry_is_empty() {
    for uncertain in [false, true] {
        let (root, workspace) = fixture();
        let c = Arc::new(manual(workspace));
        let (entered_tx, entered_rx) = mpsc::channel();
        let (finish_tx, finish_rx) = mpsc::channel();
        let caller = c.clone();
        let search = thread::spawn(move || {
            caller.dispatch_search_with(|workspace, owner| {
                workspace.search_owned_with(owner, "needle", |_, _, _, _, _| {
                    entered_tx.send(()).unwrap();
                    finish_rx.recv_timeout(Duration::from_secs(5)).unwrap();
                    if uncertain {
                        Completion {
                            result: Err(Error::TerminationUnverified("synthetic".into())),
                            disposition: Disposition::RecoveryRequired,
                        }
                    } else {
                        Completion::released(Ok(empty()))
                    }
                })
            })
        });
        entered_rx.recv_timeout(Duration::from_secs(5)).unwrap();
        let stopped = c.clone();
        let (done_tx, done_rx) = mpsc::channel();
        let shutdown = thread::spawn(move || {
            let result = stopped.shutdown();
            done_tx.send(result.is_ok()).unwrap();
            result
        });
        // Observe the real shutdown admission state, not a sleep-based race assumption.
        let deadline = Instant::now() + Duration::from_secs(5);
        while !c.shared.stopping.load(Ordering::Acquire) {
            assert!(Instant::now() < deadline);
            thread::yield_now();
        }
        assert!(matches!(
            done_rx.recv_timeout(Duration::from_millis(50)),
            Err(mpsc::RecvTimeoutError::Timeout)
        ));
        assert!(c.shared.ownership.publication_held());
        let other = Workspace::open(root.path()).unwrap();
        assert!(other.lock_processing().is_err());
        finish_tx.send(()).unwrap();
        let result = search.join().unwrap();
        assert_eq!(result.is_ok(), !uncertain);
        assert_eq!(
            done_rx.recv_timeout(Duration::from_secs(5)).unwrap(),
            !uncertain
        );
        assert_eq!(shutdown.join().unwrap().is_ok(), !uncertain);
        assert_eq!(other.lock_processing().is_ok(), !uncertain);
        assert!(c
            .dispatch_search_with(|_, _| panic!("no post-stop search"))
            .is_err());
    }
}
#[test]
fn search_lifecycle_panicked_dispatch_retains_owner_and_shutdown_refuses_poisoned_barrier() {
    let (_root, workspace) = fixture();
    let c = manual(workspace);
    let panic = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
        c.dispatch_search_with(|workspace, owner| {
            workspace.search_owned_with(owner, "needle", |_, _, _, _, _| {
                panic!("synthetic search panic")
            })
        })
    }));
    assert!(panic.is_err());
    assert!(!c.shared.ownership.held());
    assert!(c.shared.ownership.publication_held());
    assert!(c.shutdown().is_err());
}
