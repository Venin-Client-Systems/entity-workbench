//! Source-only bootstrap checks. No candidate interpreter, shell or worker is executed.
use super::*;
use crate::{
    graph_api::{GraphAvailability, GraphJobPage, GraphJobPageRequest},
    processing::{ProcessingFailure, ProcessingJob, ProcessingState},
    store::graph_analysis::probe_fixture::specimen,
};
use std::{fs, time::Instant};

fn availability(c: &JobCoordinator) -> GraphAvailability {
    let page: GraphJobPage = serde_json::from_value(
        c.dispatch(Command::PageGraphJobs {
            request: GraphJobPageRequest {
                page_size: 25,
                cursor: None,
            },
            expected_revision: None,
        })
        .unwrap(),
    )
    .unwrap();
    page.availability
}

fn wait_terminal(c: &JobCoordinator, key: &str) -> ProcessingJob {
    let deadline = Instant::now() + Duration::from_secs(10);
    let mut w = c.shared.workspace.lock().unwrap();
    loop {
        let job = w.processing_job(key).unwrap();
        if !matches!(
            job.state,
            ProcessingState::Queued | ProcessingState::Running
        ) {
            return job;
        }
        assert!(Instant::now() < deadline, "synthetic bootstrap deadline");
        w = c
            .shared
            .wake
            .wait_timeout(w, Duration::from_millis(20))
            .unwrap()
            .0;
    }
}

#[test]
fn fixed_resource_child_is_the_only_verification_input() {
    let (root, _, _) = specimen();
    let token = CancellationToken::default();
    let result = resolve_graph(root.path(), &token, |path, seen_token| {
        assert_eq!(path, root.path().join("engines"));
        assert!(std::ptr::eq(seen_token, &token));
        Err(Error::Blocked("Synthetic invalid pinned prefix".into()))
    })
    .unwrap();
    assert!(matches!(result, graph::GraphExecution::Unavailable));
    assert!(matches!(
        resolve_graph(Path::new("relative-resources"), &token, |_, _| panic!(
            "no CWD discovery"
        ))
        .unwrap(),
        graph::GraphExecution::Unavailable
    ));
}

#[test]
fn configuration_cancellation_precedes_verification_and_unavailable_fallback() {
    let (root, _, _) = specimen();
    let cancelled = CancellationToken::default();
    cancelled.cancel();
    assert!(matches!(
        resolve_graph(root.path(), &cancelled, |_, _| panic!(
            "cancelled preflight"
        )),
        Err(Error::Interrupted(_))
    ));
    let during = CancellationToken::default();
    assert!(matches!(
        resolve_graph(root.path(), &during, |_, token| {
            token.cancel();
            Err(Error::Blocked("Concurrent missing runtime".into()))
        }),
        Err(Error::Interrupted(_))
    ));
    assert!(
        matches!(resolve_graph(root.path(), &CancellationToken::default(), |_, _| {
        Err(Error::Interrupted("Verifier cancellation".into()))
    }), Err(Error::Interrupted(message)) if message == "Verifier cancellation")
    );
}

#[test]
fn unexpected_preflight_errors_are_not_relabelled_as_optional_absence() {
    let (root, _, _) = specimen();
    assert!(matches!(
        resolve_graph(root.path(), &CancellationToken::default(), |_, _| {
            Err(Error::Cleanup("Synthetic failure".into()))
        }),
        Err(Error::Cleanup(_))
    ));
}

#[test]
fn cancelled_public_start_does_not_acquire_or_mutate_workspace() {
    let (_root, w, _) = specimen();
    let revision = w.revision().unwrap();
    let path = w.processing_scratch().parent().unwrap().to_path_buf();
    let cancel = CancellationToken::default();
    cancel.cancel();
    assert!(matches!(
        JobCoordinator::start_with_development_app_resources(w, 1, &path, &cancel),
        Err(Error::Interrupted(_))
    ));
    let reopened = Workspace::open(path).unwrap();
    assert_eq!(reopened.revision().unwrap(), revision);
    assert!(reopened.collection_ownership().unwrap().held());
}

#[test]
fn default_missing_and_invalid_resources_leave_review_and_document_lane_usable() {
    for mode in ["default", "missing", "invalid"] {
        let (root, mut w, evidence) = specimen();
        let document = w
            .queue_document_parse(&evidence, &uuid::Uuid::new_v4().to_string())
            .unwrap();
        let graph = w
            .queue_graph_path(
                w.revision().unwrap(),
                "a",
                "c",
                &uuid::Uuid::new_v4().to_string(),
            )
            .unwrap();
        let resources = root.path().join("resources");
        if mode == "invalid" {
            fs::create_dir_all(resources.join("engines/python")).unwrap();
            fs::write(resources.join("engines/python/manifest.json"), b"{}\n").unwrap();
        }
        let c = if mode == "default" {
            JobCoordinator::start(w, 1).unwrap()
        } else {
            JobCoordinator::start_with_development_app_resources(
                w,
                1,
                &resources,
                &CancellationToken::default(),
            )
            .unwrap()
        };
        assert!(c.dispatch(Command::View {}).is_ok());
        assert_eq!(availability(&c), GraphAvailability::RuntimeUnavailable);
        let graph = wait_terminal(&c, &graph.id);
        assert_eq!(graph.state, ProcessingState::Blocked);
        assert_eq!(
            graph.failure,
            Some(ProcessingFailure::SchedulingOrRuntimeUnavailable)
        );
        assert!(graph.started_at.is_none() && graph.lease.is_none());
        let document = wait_terminal(&c, &document.id);
        // The original document executor remains available for admission. With no
        // attached document runtime this exact fixture returns its existing refusal.
        assert_eq!(document.state, ProcessingState::Blocked);
        assert_eq!(
            document.failure,
            Some(ProcessingFailure::RuntimeUnavailable)
        );
        assert!(document.started_at.is_some());
        assert!(c.shared.ownership.held());
        assert!(c.shared.active.lock().unwrap().tokens.is_empty());
        c.shutdown().unwrap();
    }
}

#[test]
fn retained_intent_quarantine_dominates_development_start_and_executor_availability() {
    for synthetic in [false, true] {
        let (root, w, _) = specimen();
        let revision = w.revision().unwrap();
        let cache = root.path().join("case/indexes/lucene");
        fs::create_dir_all(&cache).unwrap();
        let marker = cache.join(crate::engines::search_lifecycle::INTENT);
        fs::write(&marker, b"synthetic retained uncertainty").unwrap();
        let c = if synthetic {
            // Tests the shared startup gate with an available source-only executor;
            // it does not fabricate or claim a verified Python runtime capability.
            JobCoordinator::with_graph_execution(
                w,
                1,
                document_executor(),
                None,
                graph::GraphExecution::Synthetic(Arc::new(|_, _| {
                    panic!("quarantined graph execution")
                })),
            )
            .unwrap()
        } else {
            JobCoordinator::start_with_development_app_resources(
                w,
                1,
                root.path(),
                &CancellationToken::default(),
            )
            .unwrap()
        };
        assert_eq!(availability(&c), GraphAvailability::RecoveryRequired);
        assert!(!c.shared.ownership.held());
        assert!(c.dispatch(Command::View {}).is_ok());
        assert!(c.shutdown().is_err());
        assert_eq!(
            c.shared.workspace.lock().unwrap().revision().unwrap(),
            revision
        );
        assert_eq!(fs::read(marker).unwrap(), b"synthetic retained uncertainty");
    }
}

#[cfg(not(all(target_os = "macos", target_arch = "aarch64")))]
#[test]
fn unsupported_platform_never_produces_a_configured_capability() {
    let (root, w, _) = specimen();
    let c = JobCoordinator::start_with_development_app_resources(
        w,
        1,
        root.path(),
        &CancellationToken::default(),
    )
    .unwrap();
    assert!(matches!(
        c.shared.graph_executor,
        graph::GraphExecution::Unavailable
    ));
    assert_eq!(availability(&c), GraphAvailability::RuntimeUnavailable);
    c.shutdown().unwrap();
}

#[cfg(unix)]
#[test]
fn linked_resource_prefix_is_unavailable_and_workspace_scratch_stays_private() {
    use std::os::unix::fs::{symlink, MetadataExt, PermissionsExt};
    let (root, w, _) = specimen();
    let resources = root.path().join("resources");
    fs::create_dir_all(resources.join("engines")).unwrap();
    let outside = root.path().join("outside");
    fs::create_dir(&outside).unwrap();
    fs::write(outside.join("manifest.json"), b"{}\n").unwrap();
    symlink(&outside, resources.join("engines/python")).unwrap();
    let scratch = w.processing_scratch();
    let c = JobCoordinator::start_with_development_app_resources(
        w,
        1,
        &resources,
        &CancellationToken::default(),
    )
    .unwrap();
    assert_eq!(availability(&c), GraphAvailability::RuntimeUnavailable);
    assert_eq!(
        c.shared.workspace.lock().unwrap().processing_scratch(),
        scratch
    );
    let info = fs::symlink_metadata(scratch).unwrap();
    assert!(info.is_dir());
    assert_eq!(info.permissions().mode() & 0o7777, 0o700);
    assert_eq!(info.uid(), unsafe { libc::geteuid() });
    assert_eq!(fs::read(outside.join("manifest.json")).unwrap(), b"{}\n");
    c.shutdown().unwrap();
}
