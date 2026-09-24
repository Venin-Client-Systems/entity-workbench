use super::*;
use std::{
    net::TcpListener,
    os::unix::{fs::symlink, io::AsRawFd},
    path::PathBuf,
};

#[test]
fn result_rejects_links_and_oversize_and_special_files() {
    let root = tempfile::tempdir().unwrap();
    let path = root.path().join("result");
    fs::write(&path, b"valid").unwrap();
    assert_eq!(read_result(&path, 5).unwrap(), b"valid");
    assert!(read_result(&path, 4).is_err());
    fs::hard_link(&path, root.path().join("hard")).unwrap();
    assert!(read_result(&path, 5).is_err());
    fs::remove_file(root.path().join("hard")).unwrap();
    symlink(&path, root.path().join("link")).unwrap();
    assert!(read_result(&root.path().join("link"), 5).is_err());
    assert!(read_result(root.path(), 5).is_err());
}
#[test]
fn index_rejects_links_and_tree_budget_overruns() {
    let root = tempfile::tempdir().unwrap();
    let index = root.path().join("index");
    fs::create_dir(&index).unwrap();
    let outside = root.path().join("outside");
    fs::write(&outside, b"sentinel").unwrap();
    symlink(&outside, index.join("link")).unwrap();
    assert!(validate_index(&index).is_err());
    fs::remove_file(index.join("link")).unwrap();
    File::create(index.join("huge"))
        .unwrap()
        .set_len(FILE_BYTES + 1)
        .unwrap();
    assert!(validate_index(&index).is_err());
    fs::remove_file(index.join("huge")).unwrap();
    for i in 0..=TREE_FILES {
        File::create(index.join(i.to_string())).unwrap();
    }
    assert!(validate_index(&index).is_err());
}
#[test]
fn profile_has_distinct_read_and_write_access() {
    let read = profile(
        Path::new("/runtime/java/bin/java"),
        Path::new("/runtime"),
        Path::new("/job"),
        Path::new("/index"),
        false,
    )
    .unwrap();
    assert!(read.contains("(deny process-fork)"));
    assert!(!read.contains("(allow process-fork)"));
    assert!(!read.contains("(allow file-write* (subpath \"/index\"))"));
    assert!(profile(
        Path::new("/runtime/java/bin/java"),
        Path::new("/runtime"),
        Path::new("/job"),
        Path::new("/index"),
        true
    )
    .unwrap()
    .contains("(allow file-write* (subpath \"/index\"))"));
    assert!(quote(Path::new("/path\n(injection)")).is_err());
}
#[test]
fn process_setup_closes_inheritable_descriptor_and_reaps_timeout_group() {
    let root = tempfile::tempdir().unwrap();
    let sentinel = File::create(root.path().join("sentinel")).unwrap();
    // Deliberately create a descriptor without CLOEXEC, as a native library might.
    let leaked = unsafe { libc::fcntl(sentinel.as_raw_fd(), libc::F_DUPFD, 100) };
    assert!(leaked >= 100);
    let mut command = Command::new("/bin/sh");
    command.arg("-c").arg(format!("test ! -e /dev/fd/{leaked}"));
    configure_process(&mut command).unwrap();
    let status = command.status().unwrap();
    unsafe {
        libc::close(leaked);
    }
    assert!(status.success());
    let index = root.path().join("index");
    fs::create_dir(&index).unwrap();
    let job = root.path().join("job");
    fs::create_dir(&job).unwrap();
    let pidfile = root.path().join("child.pid");
    let mut command = Command::new("/bin/sh");
    command
        .arg("-c")
        .arg("sleep 60 & echo $! > \"$1\"; wait")
        .arg("probe")
        .arg(&pidfile);
    configure_process(&mut command).unwrap();
    let before = Instant::now();
    assert!(wait(
        command.spawn().unwrap(),
        &job,
        &index,
        Duration::from_millis(400)
    )
    .is_err());
    assert!(before.elapsed() < Duration::from_secs(3));
    let descendant = fs::read_to_string(pidfile)
        .unwrap()
        .trim()
        .parse::<i32>()
        .unwrap();
    // A killed child may briefly remain a zombie awaiting launchd reaping.
    let status = Command::new("/bin/ps")
        .args(["-o", "stat=", "-p", &descendant.to_string()])
        .output()
        .unwrap();
    let state = String::from_utf8(status.stdout).unwrap();
    assert!(
        state.trim().is_empty() || state.trim().starts_with('Z'),
        "descendant survived: {state}"
    );
}
fn runtime() -> PathBuf {
    PathBuf::from(
        std::env::var_os("WORKBENCH_TEST_RUNTIME")
            .expect("Set WORKBENCH_TEST_RUNTIME to the staged synthetic probe runtime"),
    )
}
fn prepare(root: &Path) -> (PathBuf, PathBuf) {
    let job = root.join("job");
    let index = root.join("index");
    fs::create_dir(&job).unwrap();
    fs::create_dir(&index).unwrap();
    super::super::write_new(&job.join("input.json"), b"synthetic input").unwrap();
    super::super::write_new(&job.join("request.json"), b"{}").unwrap();
    (job, index)
}
#[test]
#[ignore = "requires staged Java 21 and current probe JAR; run scripts/test_macos_confinement.py"]
fn native_java_hostile_and_benign_boundaries() {
    let runtime = runtime();
    for writable in [false, true] {
        let root = tempfile::tempdir().unwrap();
        let root = root.path().canonicalize().unwrap();
        let (job, index) = prepare(&root);
        let other = root.join("other.txt");
        fs::write(&other, "sentinel").unwrap();
        let original = root.join("original.txt");
        fs::write(&original, "retained").unwrap();
        let sibling = root.join("sibling.txt");
        fs::write(&sibling, "private job").unwrap();
        let listener = TcpListener::bind("127.0.0.1:0").unwrap();
        let args = vec![
            "probe".into(),
            other.display().to_string(),
            original.display().to_string(),
            listener.local_addr().unwrap().port().to_string(),
            sibling.display().to_string(),
        ];
        if !writable {
            fs::create_dir(job.join("scratch")).unwrap();
            fs::write(job.join("worker.sb"), "synthetic baseline profile sentinel").unwrap();
            let classpath = format!(
                "{}:{}",
                runtime.join("search/workers-0.1.0.jar").display(),
                runtime.join("search/lib/*").display()
            );
            let baseline = Command::new(runtime.join("java/bin/java"))
                .args(["-Xmx256m", "-XX:-UsePerfData"])
                .arg(format!("-Dworkbench.index={}", index.display()))
                .args(["-cp", &classpath, "workbench.HostileProbe"])
                .args(&args)
                .current_dir(&job)
                .env("WORKBENCH_TEST_SECRET", "synthetic")
                .stdout(Stdio::null())
                .stderr(Stdio::null())
                .status()
                .unwrap();
            assert!(baseline.success());
            let result: serde_json::Value =
                serde_json::from_slice(&fs::read(job.join("result.json")).unwrap()).unwrap();
            assert!(result
                .as_object()
                .unwrap()
                .values()
                .all(|value| value == true));
            fs::remove_dir_all(job.join("scratch")).unwrap();
            fs::remove_file(job.join("worker.sb")).unwrap();
            fs::remove_file(job.join("result.json")).unwrap();
            fs::remove_file(index.join("probe.txt")).unwrap();
            fs::write(&original, "retained").unwrap();
            fs::write(job.join("input.json"), "synthetic input").unwrap();
        }
        run_java(
            &runtime,
            &job,
            &index,
            writable,
            "workbench.HostileProbe",
            &args,
            Duration::from_secs(15),
        )
        .unwrap();
        let result: serde_json::Value =
            serde_json::from_slice(&read_result(&job.join("result.json"), 4096).unwrap()).unwrap();
        assert_eq!(
            result,
            serde_json::json!({"other_workspace_read":false,"data_volume_alias_read":false,"original_write":false,"direct_network":false,"input_read":true,"input_write":false,"scratch_write":true,"index_write":writable,"sibling_job_read":false,"child_process":false,"caller_environment":false,"profile_read":false})
        );
        assert_eq!(fs::read_to_string(original).unwrap(), "retained");
    }
    for mode in ["timeout", "oversize"] {
        let root = tempfile::tempdir().unwrap();
        let root = root.path().canonicalize().unwrap();
        let (job, index) = prepare(&root);
        let started = Instant::now();
        assert!(run_java(
            &runtime,
            &job,
            &index,
            false,
            "workbench.HostileProbe",
            &[mode.into()],
            Duration::from_secs(2)
        )
        .is_err());
        assert!(started.elapsed() < Duration::from_secs(5));
        if mode == "oversize" {
            assert!(
                fs::metadata(job.join("scratch/oversize.bin"))
                    .unwrap()
                    .len()
                    <= FILE_BYTES
            );
        }
    }
}
#[test]
#[ignore = "requires staged Java 21 and current worker JAR; run scripts/test_macos_confinement.py"]
fn native_lucene_uses_separate_jobs_and_read_only_search_index() {
    let root = tempfile::tempdir().unwrap();
    let runtime = super::super::Runtime { root: runtime() };
    // Use the actual Rust production path, preserving revision and evidence validation.
    let evidence: crate::domain::Evidence = serde_json::from_value(serde_json::json!({
        "id":"synthetic-a", "origin_group":"synthetic-origin", "extraction_status":"complete", "sha256":"0".repeat(64), "name":"Synthetic notice", "media_type":"text/plain", "bytes":32, "imported_at":"2026-09-24T00:00:00Z", "text":"Rowan Ellis synthetic cooperative"
    })).unwrap();
    let cache = root.path().join("cache");
    for query in [
        "\"Rowan Ellis\"",
        "Rowan AND Ellis",
        "\"Rowan cooperative\"~10",
        "Rowen~1",
        "name:notice",
    ] {
        let results = runtime
            .search(&cache, 7, std::slice::from_ref(&evidence), query)
            .unwrap_or_else(|error| panic!("query {query}: {error}"));
        assert_eq!(results.workspace_revision, "7");
        assert_eq!(results.hits.len(), 1);
    }
    // Failed requests remove their private job and staged input too.
    assert!(runtime
        .search(&cache, 7, std::slice::from_ref(&evidence), "(")
        .is_err());
    let missing_runtime = super::super::Runtime {
        root: root.path().join("missing-runtime"),
    };
    let missing = missing_runtime.search(&cache, 8, &[evidence], "Rowan");
    assert!(missing.is_err());
    assert!(!root.path().join("lucene-revision.txt").exists());
    assert!(fs::read_dir(&cache).unwrap().all(|entry| {
        let name = entry.unwrap().file_name();
        name == "index" || name == "coordinator.lock"
    }));
}
