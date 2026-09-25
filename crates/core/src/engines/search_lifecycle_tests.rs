use super::*;
use crate::{engines::Runtime, policy::WorkerOperation};

fn temp() -> tempfile::TempDir {
    tempfile::tempdir_in(std::env::temp_dir().canonicalize().unwrap()).unwrap()
}
fn runtime(root: &Path) -> Runtime {
    Runtime {
        root: root.join("not-executed"),
    }
}
fn success(cache: &Path, operation: WorkerOperation, _: &str) -> Result<Vec<u8>> {
    assert!(cache.join(INTENT).is_file());
    match operation {
        WorkerOperation::Index => {
            fs::create_dir_all(cache.join("index"))?;
            fs::write(cache.join("index/segment"), b"retained-index")?;
            Ok(br#"{"indexed":0,"workspace_revision":7}"#.to_vec())
        }
        WorkerOperation::Search => {
            Ok(br#"{"workspace_revision":"7","hits":[],"total":0}"#.to_vec())
        }
        _ => panic!("unexpected operation"),
    }
}
#[test]
fn search_lifecycle_success_releases_both_stages_without_canonical_writes() {
    let root = temp();
    let cache = root.path().join("cache");
    let runtime = runtime(root.path());
    let mut operations = Vec::new();
    let outcome = runtime.search_with(&cache, 7, &[], "needle", |cache, operation, input| {
        operations.push(format!("{operation:?}"));
        success(cache, operation, input)
    });
    assert!(outcome.result.is_ok());
    assert_eq!(outcome.disposition, Disposition::Released);
    assert_eq!(operations, ["Index", "Search"]);
    assert!(!cache.join(INTENT).exists());
    assert!(fs::read_dir(&cache).unwrap().all(|entry| {
        let n = entry.unwrap().file_name();
        n == "coordinator.lock" || n == "index"
    }));
    assert!(runtime
        .search_with(&cache, 7, &[], "needle", success)
        .result
        .is_ok());
}
#[test]
fn search_lifecycle_unknown_exit_retains_every_input_and_never_attempts_cleanup() {
    for fail_index in [true, false] {
        let root = temp();
        let cache = root.path().join("cache");
        let runtime = runtime(root.path());
        let mut calls = 0;
        hooks::fail_at("input_cleanup");
        let outcome = runtime.search_with(&cache, 7, &[], "needle", |cache, operation, input| {
            calls += 1;
            if fail_index || matches!(operation, WorkerOperation::Search) {
                fs::create_dir_all(cache.join("job-synthetic"))?;
                fs::write(cache.join("job-synthetic/retained"), b"assignment")?;
                // If cleanup were attempted, this deliberate path replacement would fail too.
                fs::remove_file(cache.join(input))?;
                fs::create_dir(cache.join(input))?;
                return Err(Error::TerminationUnverified(
                    "synthetic leader ownership".into(),
                ));
            }
            success(cache, operation, input)
        });
        assert!(
            matches!(outcome.result, Err(Error::TerminationUnverified(ref text)) if text == "synthetic leader ownership")
        );
        assert_eq!(outcome.disposition, Disposition::RecoveryRequired);
        assert_eq!(calls, if fail_index { 1 } else { 2 });
        assert!(!hooks::take().contains(&"input_cleanup"));
        assert!(cache.join(INTENT).is_file());
        assert_eq!(
            fs::read(cache.join("job-synthetic/retained")).unwrap(),
            b"assignment"
        );
        let before: Vec<_> = fs::read_dir(&cache)
            .unwrap()
            .map(|entry| entry.unwrap().file_name())
            .collect();
        let second = runtime.search_with(&cache, 8, &[], "another", |_, _, _| {
            panic!("must not execute")
        });
        assert!(second.result.is_err());
        assert_eq!(second.disposition, Disposition::RecoveryRequired);
        let after: Vec<_> = fs::read_dir(&cache)
            .unwrap()
            .map(|entry| entry.unwrap().file_name())
            .collect();
        assert_eq!(before, after);
    }
}
#[test]
fn search_lifecycle_known_failure_cleans_and_preserves_primary_error() {
    let root = temp();
    let cache = root.path().join("cache");
    let outcome = runtime(root.path()).search_with(&cache, 7, &[], "needle", |_, _, _| {
        Err(Error::QuotaExhausted("synthetic quota".into()))
    });
    assert!(
        matches!(outcome.result, Err(Error::QuotaExhausted(ref value)) if value == "synthetic quota")
    );
    assert_eq!(outcome.disposition, Disposition::Released);
    assert!(!cache.join(INTENT).exists());
}
#[test]
fn search_lifecycle_cleanup_error_cannot_publish_or_release_intent() {
    let root = temp();
    let cache = root.path().join("cache");
    let outcome = runtime(root.path()).search_with(&cache, 7, &[], "needle", |_, _, _| {
        Err(Error::Cleanup("synthetic assignment cleanup".into()))
    });
    assert!(matches!(outcome.result, Err(Error::Cleanup(_))));
    assert_eq!(outcome.disposition, Disposition::RecoveryRequired);
    assert!(cache.join(INTENT).exists());
}
#[test]
fn search_lifecycle_write_and_sync_failures_prevent_any_launch() {
    for step in ["intent_write", "intent_sync", "input_write"] {
        let root = temp();
        let cache = root.path().join("cache");
        hooks::fail_at(step);
        let outcome = runtime(root.path()).search_with(&cache, 7, &[], "needle", |_, _, _| {
            panic!("must not execute")
        });
        assert!(matches!(outcome.result, Err(Error::Io(_))));
        assert_eq!(outcome.disposition, Disposition::Released);
        assert!(!cache.join(INTENT).exists());
        assert!(hooks::take().contains(&step));
    }
}
#[test]
fn search_lifecycle_finalization_failures_reject_success_and_retain_lock() {
    for step in [
        "input_cleanup",
        "intent_remove",
        "finish_sync",
        "lock_release",
    ] {
        let root = temp();
        let cache = root.path().join("cache");
        hooks::fail_at(step);
        let outcome = runtime(root.path()).search_with(&cache, 7, &[], "needle", success);
        assert!(matches!(outcome.result, Err(Error::Cleanup(_))));
        assert_eq!(outcome.disposition, Disposition::RecoveryRequired);
        assert!(hooks::take().contains(&step));
        assert!(runtime(root.path())
            .search_with(&cache, 7, &[], "needle", |_, _, _| panic!("no retry"))
            .result
            .is_err());
        if matches!(step, "input_cleanup" | "intent_remove") {
            assert!(cache.join(INTENT).exists());
        }
    }
}
#[test]
fn search_lifecycle_existing_invalid_and_crash_left_intents_block_before_index_change() {
    for bytes in [
        b"".as_slice(),
        b"{",
        br#"{"format_version":999}"#,
        br#"{"format_version":1,"attempt_id":"not-authority"}"#,
    ] {
        let root = temp();
        let cache = root.path().join("cache");
        fs::create_dir_all(cache.join("index")).unwrap();
        fs::write(cache.join("index/retained"), b"index").unwrap();
        fs::write(cache.join(INTENT), bytes).unwrap();
        // Represents a reopened workspace with no OS lock, including crash before a worker started.
        let outcome = runtime(root.path())
            .search_with(&cache, 7, &[], "needle", |_, _, _| panic!("no launch"));
        assert!(matches!(outcome.result, Err(Error::Blocked(_))));
        assert!(recovery_required(&cache));
        assert_eq!(fs::read(cache.join("index/retained")).unwrap(), b"index");
        assert_eq!(fs::read(cache.join(INTENT)).unwrap(), bytes);
    }
}
#[test]
fn search_lifecycle_create_collision_is_never_deleted_as_owned() {
    let root = temp();
    let cache = root.path().join("cache");
    let mut lease = match Lease::acquire(&cache) {
        Ok(value) => value,
        Err(_) => panic!("lease"),
    };
    fs::write(cache.join(INTENT), b"other intent").unwrap();
    let error = lease.prepare(7).unwrap_err();
    let outcome = lease.finish::<()>(Err(error));
    assert_eq!(outcome.disposition, Disposition::RecoveryRequired);
    assert_eq!(fs::read(cache.join(INTENT)).unwrap(), b"other intent");
}
#[test]
fn search_lifecycle_panic_retains_inputs_and_intent() {
    let root = temp();
    let cache = root.path().join("cache");
    let outcome = std::panic::catch_unwind(|| {
        runtime(root.path()).search_with(&cache, 7, &[], "needle", |cache, _, _| {
            fs::create_dir(cache.join("job-panic")).unwrap();
            fs::write(cache.join("job-panic/retained"), b"owned").unwrap();
            panic!("synthetic executor panic")
        })
    });
    assert!(outcome.is_err());
    assert!(cache.join(INTENT).exists());
    assert!(cache.join("job-panic/retained").exists());
    assert!(fs::read_dir(cache).unwrap().any(|entry| entry
        .unwrap()
        .file_name()
        .to_string_lossy()
        .starts_with("manifest-")));
}
#[test]
fn search_lifecycle_invalid_query_does_not_create_ownership() {
    let root = temp();
    let cache = root.path().join("cache");
    let outcome =
        runtime(root.path()).search_with(&cache, 7, &[], " ", |_, _, _| panic!("no launch"));
    assert_eq!(outcome.disposition, Disposition::Released);
    assert!(!cache.exists());
}
#[test]
fn search_lifecycle_control_hardlinks_fail_without_altering_outside_bytes() {
    for name in [INTENT, "coordinator.lock"] {
        let root = temp();
        let cache = root.path().join("cache");
        fs::create_dir(&cache).unwrap();
        let outside = root.path().join("outside");
        fs::write(&outside, b"outside").unwrap();
        fs::hard_link(&outside, cache.join(name)).unwrap();
        assert!(runtime(root.path())
            .search_with(&cache, 7, &[], "needle", |_, _, _| panic!("no launch"))
            .result
            .is_err());
        assert_eq!(fs::read(outside).unwrap(), b"outside");
    }
}
#[cfg(unix)]
#[test]
fn search_lifecycle_linked_control_and_parent_fail_without_following() {
    use std::os::unix::fs::symlink;
    for name in [INTENT, "coordinator.lock"] {
        let root = temp();
        let cache = root.path().join("cache");
        fs::create_dir(&cache).unwrap();
        let outside = root.path().join("outside");
        fs::write(&outside, b"outside").unwrap();
        symlink(&outside, cache.join(name)).unwrap();
        assert!(runtime(root.path())
            .search_with(&cache, 7, &[], "needle", |_, _, _| panic!("no launch"))
            .result
            .is_err());
        assert_eq!(fs::read(outside).unwrap(), b"outside");
    }
    let root = temp();
    let outside = root.path().join("outside");
    fs::create_dir(&outside).unwrap();
    symlink(&outside, root.path().join("link")).unwrap();
    assert!(runtime(root.path())
        .search_with(
            &root.path().join("link/cache"),
            7,
            &[],
            "needle",
            |_, _, _| panic!("no launch")
        )
        .result
        .is_err());
    assert!(!outside.join("cache").exists());
}
