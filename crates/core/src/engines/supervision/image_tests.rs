use super::*;
#[test]
fn image_profile_adds_only_its_assigned_raster_output() {
    let profile = worker_profile(
        Path::new("/runtime/java/bin/java"),
        Path::new("/runtime"),
        Path::new("/job"),
        "image",
        None,
    )
    .unwrap();
    assert!(profile.contains("(allow file-write* (literal \"/job/raster.pgm\"))"));
    assert!(!profile.contains("/runtime/parser"));
    assert!(!profile.contains("/runtime/search"));
    assert!(!profile.contains("/runtime/ocr"));
    let parser = worker_profile(
        Path::new("/runtime/java/bin/java"),
        Path::new("/runtime"),
        Path::new("/job"),
        "parser",
        None,
    )
    .unwrap();
    assert!(!parser.contains("raster.pgm"));
}
#[test]
#[ignore = "requires staged image worker and hostile fixture"]
fn native_image_worker_denies_outside_io_network_and_verifies_cancel_and_output_bound() {
    use std::net::TcpListener;
    let runtime = std::path::PathBuf::from(
        std::env::var_os("WORKBENCH_TEST_IMAGE_RUNTIME").expect("Image runtime required"),
    );
    let root = tempfile::tempdir_in(std::env::temp_dir().canonicalize().unwrap()).unwrap();
    let other = root.path().join("outside");
    let original = root.path().join("original");
    let index = root.path().join("index");
    fs::create_dir(&index).unwrap();
    fs::write(&other, "outside sentinel").unwrap();
    fs::write(&original, "original sentinel").unwrap();
    fs::write(index.join("data"), "index sentinel").unwrap();
    let listener = TcpListener::bind("127.0.0.1:0").unwrap();
    listener.set_nonblocking(true).unwrap();
    let make_job = || {
        let job = tempfile::tempdir_in(root.path()).unwrap();
        fs::write(job.path().join("input.json"), "synthetic input").unwrap();
        fs::write(job.path().join("request.json"), "{}").unwrap();
        job
    };
    let job = make_job();
    run_image_java(
        &runtime,
        job.path(),
        "workbench.HostileProbe",
        &[
            "probe".into(),
            other.display().to_string(),
            original.display().to_string(),
            listener.local_addr().unwrap().port().to_string(),
            index.join("data").display().to_string(),
            index.display().to_string(),
        ],
        Duration::from_secs(10),
        &super::super::CancellationToken::default(),
    )
    .unwrap();
    let result: serde_json::Value =
        serde_json::from_slice(&read_result(&job.path().join("result.json"), 4096).unwrap())
            .unwrap();
    assert_eq!(
        result,
        serde_json::json!({"other_workspace_read":false,"data_volume_alias_read":false,"original_write":false,"direct_network":false,"input_read":true,"input_write":false,"scratch_write":true,"index_write":false,"sibling_job_read":false,"child_process":false,"caller_environment":false,"profile_read":false})
    );
    assert_eq!(fs::read_to_string(original).unwrap(), "original sentinel");
    assert_eq!(fs::read_to_string(other).unwrap(), "outside sentinel");
    assert_eq!(
        listener.accept().unwrap_err().kind(),
        std::io::ErrorKind::WouldBlock
    );
    finish_job(job, Ok(())).unwrap();
    let job = make_job();
    let token = super::super::CancellationToken::default();
    let worker_token = token.clone();
    let worker_runtime = runtime.clone();
    let worker_path = job.path().to_owned();
    let worker = std::thread::spawn(move || {
        run_image_java(
            &worker_runtime,
            &worker_path,
            "workbench.HostileProbe",
            &["identify_timeout".into()],
            Duration::from_secs(10),
            &worker_token,
        )
    });
    let started = Instant::now();
    let result = job.path().join("result.json");
    while !result.exists() || fs::metadata(&result).unwrap().len() == 0 {
        assert!(started.elapsed() < Duration::from_secs(5));
        std::thread::sleep(Duration::from_millis(10));
    }
    let pid: i32 = fs::read_to_string(result).unwrap().parse().unwrap();
    token.cancel();
    assert!(matches!(worker.join().unwrap(), Err(Error::Blocked(_))));
    assert_eq!(unsafe { libc::kill(pid, 0) }, -1);
    finish_job(job, Ok(())).unwrap();
    let job = make_job();
    let overflow = run_image_java(
        &runtime,
        job.path(),
        "workbench.HostileProbe",
        &["oversize".into()],
        Duration::from_secs(5),
        &super::super::CancellationToken::default(),
    );
    assert!(overflow.is_err());
    assert!(
        fs::metadata(job.path().join("scratch/oversize.bin"))
            .unwrap()
            .len()
            <= (super::super::ocr::MAX_PIXELS + 32) as u64
    );
    finish_job(job, Ok(())).unwrap();
}
