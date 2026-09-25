use super::*;

fn temporary() -> tempfile::TempDir {
    tempfile::Builder::new()
        .permissions(fs::Permissions::from_mode(0o700))
        .tempdir_in(std::env::temp_dir().canonicalize().unwrap())
        .unwrap()
}

#[test]
fn opened_runtime_file_must_still_be_the_final_named_file() {
    let root = temporary();
    let path = root.path().join("asset");
    fs::write(&path, vec![b'x'; 128 * 1024]).unwrap();
    let mut replaced = false;
    let result = hash_verified(
        &path,
        128 * 1024,
        &CancellationToken::default(),
        |path, count| {
            if count > 0 && !replaced {
                replaced = true;
                fs::rename(path, path.with_extension("old")).unwrap();
                fs::write(path, vec![b'x'; 128 * 1024]).unwrap();
            }
        },
    );
    assert!(replaced && result.is_err());
    assert_eq!(fs::read(&path).unwrap().len(), 128 * 1024);
}

#[test]
fn runtime_swap_to_fifo_between_stat_and_open_cannot_block() {
    use std::{ffi::CString, os::unix::ffi::OsStrExt};
    let root = temporary();
    let path = root.path().join("asset");
    fs::write(&path, b"fixed").unwrap();
    let mut replaced = false;
    let started = Instant::now();
    let result = hash_verified(&path, 64, &CancellationToken::default(), |path, count| {
        if count == 0 {
            replaced = true;
            fs::remove_file(path).unwrap();
            let name = CString::new(path.as_os_str().as_bytes()).unwrap();
            // A private synthetic path; no reader or writer is connected to it.
            assert_eq!(unsafe { libc::mkfifo(name.as_ptr(), 0o600) }, 0);
        }
    });
    assert!(replaced && result.is_err());
    assert!(started.elapsed() < Duration::from_secs(2));
}

#[test]
fn hardlinks_and_cancellation_are_checked_during_bounded_hashing() {
    let root = temporary();
    let path = root.path().join("asset");
    fs::write(&path, vec![b'x'; 128 * 1024]).unwrap();
    let cancel = CancellationToken::default();
    let mut read = 0;
    let result = hash_verified(&path, 128 * 1024, &cancel, |_, count| {
        read = count;
        if count > 0 {
            cancel.cancel();
        }
    });
    assert!(matches!(result, Err(Error::Interrupted(_))));
    assert_eq!(read, 64 * 1024);
    fs::hard_link(&path, root.path().join("alias")).unwrap();
    assert!(read_verified(&path, 128 * 1024, &CancellationToken::default()).is_err());
}

#[test]
fn per_asset_bounds_preserve_legacy_64k_and_allow_fixed_app_1m() {
    let root = temporary();
    fs::create_dir(root.path().join("input")).unwrap();
    let raw = vec![b'x'; 64 * 1024 + 1];
    let cancel = CancellationToken::default();
    assert!(stage(root.path(), &[("input/request", &raw, 64 * 1024)], &cancel).is_err());
    assert!(!root.path().join("input/request").exists());
    let assigned = stage(
        root.path(),
        &[("input/request", &raw, 1024 * 1024)],
        &cancel,
    )
    .unwrap();
    verify_assigned(root.path(), &assigned, &cancel).unwrap();
    let mut replaced = raw;
    replaced[0] = b'y';
    fs::write(root.path().join("input/request"), replaced).unwrap();
    assert!(verify_assigned(root.path(), &assigned, &cancel).is_err());
    cancel.cancel();
    assert!(stage(root.path(), &[("input/cancelled", b"fixed", 64)], &cancel).is_err());
    assert!(!root.path().join("input/cancelled").exists());
}

#[test]
fn uncertain_termination_skips_all_post_reads_and_retains_assignment() {
    let job = temporary();
    let path = job.path().to_owned();
    fs::write(path.join("retained"), b"unconfirmed worker assignment").unwrap();
    let status: Result<ExitStatus> = Err(Error::TerminationUnverified("synthetic".into()));
    let outcome: Result<()> =
        after_confirmed(status, |_| panic!("post-termination read must not run"));
    assert!(matches!(
        finish_job(job, outcome),
        Err(Error::TerminationUnverified(_))
    ));
    assert_eq!(
        fs::read(path.join("retained")).unwrap(),
        b"unconfirmed worker assignment"
    );
    fs::remove_dir_all(path).unwrap();
}

#[test]
fn cancellation_skips_post_reads_and_cleanup_failure_keeps_precedence() {
    let outer = temporary();
    let job = tempfile::tempdir_in(outer.path()).unwrap();
    let status: Result<ExitStatus> = Err(Error::Interrupted("synthetic cancellation".into()));
    let outcome: Result<()> = after_confirmed(status, |_| {
        panic!("cancelled wait must not read outputs or prefix")
    });
    let path = job.path().to_owned();
    assert!(matches!(
        finish_job(job, outcome),
        Err(Error::Interrupted(_))
    ));
    assert!(!path.exists());

    let job = tempfile::tempdir_in(outer.path()).unwrap();
    // The owned parent denies deletion of the assignment itself. Restore its
    // mode immediately after the call so the test cannot leak a fixture tree.
    fs::set_permissions(outer.path(), fs::Permissions::from_mode(0o500)).unwrap();
    let outcome: Result<()> = finish_job(
        job,
        Err(Error::Interrupted("synthetic cancellation".into())),
    );
    fs::set_permissions(outer.path(), fs::Permissions::from_mode(0o700)).unwrap();
    assert!(matches!(outcome, Err(Error::Cleanup(_))));
}

#[test]
fn application_scratch_is_explicit_private_owned_and_unlinked() {
    let root = temporary();
    validate_scratch_root(root.path()).unwrap();
    fs::set_permissions(root.path(), fs::Permissions::from_mode(0o755)).unwrap();
    assert!(validate_scratch_root(root.path()).is_err());
    fs::set_permissions(root.path(), fs::Permissions::from_mode(0o700)).unwrap();
    let holder = temporary();
    let alias = holder.path().join("alias");
    std::os::unix::fs::symlink(root.path(), &alias).unwrap();
    assert!(validate_scratch_root(&alias).is_err());
}

#[test]
fn command_keeps_fixed_flags_profile_and_no_recipe_argument_for_application() {
    // Does not spawn any process or read a runtime prefix.
    let root = temporary();
    fs::create_dir(root.path().join("scratch")).unwrap();
    let command = command(
        Path::new("/bundle/python"),
        root.path(),
        "graph_worker.py",
        None,
    )
    .unwrap();
    let args: Vec<_> = command
        .get_args()
        .map(|x| x.to_string_lossy().into_owned())
        .collect();
    assert_eq!(command.get_program(), "/usr/bin/sandbox-exec");
    assert_eq!(&args[3..6], ["-I", "-S", "-B"]);
    assert!(args.last().unwrap().ends_with("code/graph_worker.py"));
    assert_eq!(args.len(), 7);
    let profile = profile(Path::new("/bundle/python"), root.path()).unwrap();
    assert!(!profile.contains("allow network"));
    assert!(profile.contains("deny process-fork"));
    assert_eq!(profile.matches("allow file-write").count(), 1);
}
