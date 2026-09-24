//! Native tests only. Phase observations are thread-local, never worker IPC or
//! a production launcher option. The child stays suspended in the final case.
use super::*;
use std::{cell::Cell, time::Duration};

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub(super) enum Stage {
    Copy,
    Create,
    Resume,
}
thread_local! { static STAGE: Cell<Option<Stage>> = const { Cell::new(None) }; }
pub(super) fn observe(stage: Stage) {
    STAGE.set(Some(stage));
}
struct Reset;
impl Drop for Reset {
    fn drop(&mut self) {
        STAGE.set(None);
    }
}

#[test]
fn cancellation_at_assignment_stages_never_accepts_and_cleans_the_tree() {
    for stage in [Stage::Copy, Stage::Create, Stage::Resume] {
        let _reset = Reset;
        STAGE.set(None);
        let temporary = tempfile::tempdir().unwrap();
        let root = temporary.path().canonicalize().unwrap();
        let runtime = root.join("runtime");
        let jobs = root.join("jobs");
        fs::create_dir(&runtime).unwrap();
        fs::create_dir(&jobs).unwrap();
        // Static-CRT native unit-test binary: cancellation must occur before it
        // can execute. No alternative or unconfined launch is attempted.
        fs::copy(std::env::current_exe().unwrap(), runtime.join("worker.exe")).unwrap();
        let request = Request {
            runtime,
            executable: "worker.exe".into(),
            arguments: vec!["--list".into()],
            input: b"{}".to_vec(),
            scratch_parent: jobs.clone(),
            wall_time: Duration::from_secs(3),
            memory_bytes: 768 * 1024 * 1024,
        };
        assert!(
            matches!(
                run(&request, || STAGE.get() == Some(stage)),
                Err(Error::Cancelled)
            ),
            "cancellation phase {stage:?}"
        );
        assert_eq!(STAGE.get(), Some(stage));
        assert!(fs::read_dir(jobs).unwrap().next().is_none());
    }
}

#[test]
fn bounded_copy_cancels_mid_asset_and_never_overwrites_existing_files() {
    use crate::java::runtime::copy_asset;
    let tree = tempfile::tempdir().unwrap();
    let root = tree.path().canonicalize().unwrap();
    let source = root.join("source.bin");
    let destination = root.join("copy.bin");
    let original = vec![7; 131072];
    fs::write(&source, &original).unwrap();
    let cancelled = || fs::metadata(&destination).is_ok_and(|metadata| metadata.len() >= 65536);
    assert!(matches!(
        copy_asset(&source, &destination, &cancelled),
        Err(Error::Cancelled)
    ));
    assert_eq!(fs::metadata(&destination).unwrap().len(), 65536);
    assert_eq!(fs::read(&source).unwrap(), original);
    assert!(matches!(
        copy_asset(&source, &destination, &|| false),
        Err(Error::Io(std::io::ErrorKind::AlreadyExists))
    ));
    assert_eq!(fs::metadata(&destination).unwrap().len(), 65536);
    fs::remove_file(&destination).unwrap();
    fs::hard_link(&source, root.join("outside-link.bin")).unwrap();
    assert!(matches!(
        copy_asset(&source, &destination, &|| false),
        Err(Error::Blocked("runtime linked file rejected"))
    ));
    assert!(!destination.exists());
}

#[test]
fn partially_initialized_profile_is_explicitly_closed_on_setup_error() {
    let mut folder = PathBuf::new();
    let result = Profile::create_initialized(|profile| {
        profile.locate_folder()?;
        folder = profile.folder.clone();
        Err(Error::Api {
            operation: "SyntheticProfileSetup",
            code: 5,
        })
    });
    assert!(matches!(
        result,
        Err(Error::Api {
            operation: "SyntheticProfileSetup",
            code: 5
        })
    ));
    assert!(!folder.as_os_str().is_empty());
    assert!(
        matches!(fs::symlink_metadata(folder), Err(error) if error.kind() == std::io::ErrorKind::NotFound)
    );
}
