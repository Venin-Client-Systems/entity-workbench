use super::*;
#[test]
fn ocr_profile_excludes_other_workers_and_original_writes() {
    let text = profile(
        Path::new("/bundle/ocr/bin/tesseract"),
        Path::new("/bundle/ocr"),
        Path::new("/job"),
        Recipe::Text,
    )
    .unwrap();
    assert!(text.contains("(deny process-fork)"));
    assert!(!text.contains("allow network"));
    assert!(!text.contains("java"));
    assert!(!text.contains("search"));
    assert!(text.contains("(literal \"/job/input.pgm\")"));
    assert!(!text.contains("(allow file-write* (literal \"/job/input.pgm\")"));
}

#[test]
#[ignore = "requires compiled synthetic native OCR probe"]
fn native_ocr_profile_denies_outside_io_network_fork_and_inheritance() {
    native_recipe_boundaries(Recipe::Text);
}

pub(super) fn native_recipe_boundaries(recipe: Recipe) {
    let filename = match recipe {
        Recipe::Text => "result.txt",
        Recipe::WordRegions => "result.tsv",
    };
    use std::{net::TcpListener, os::fd::AsRawFd};
    let root = tempfile::tempdir_in(std::env::temp_dir().canonicalize().unwrap()).unwrap();
    let runtime = root.path().join("runtime");
    fs::create_dir_all(runtime.join("bin")).unwrap();
    fs::create_dir(runtime.join("lib")).unwrap();
    fs::create_dir(runtime.join("tessdata")).unwrap();
    fs::copy(
        std::env::var_os("WORKBENCH_TEST_OCR_PROBE").expect("Native probe required"),
        runtime.join("bin/tesseract"),
    )
    .unwrap();
    fs::write(runtime.join("tessdata/eng.traineddata"), b"synthetic").unwrap();
    let outside = root.path().join("outside");
    let index = root.path().join("index");
    fs::write(&outside, b"outside sentinel").unwrap();
    fs::write(&index, b"index sentinel").unwrap();
    let file = File::open(&outside).unwrap();
    let descriptor = unsafe { libc::fcntl(file.as_raw_fd(), libc::F_DUPFD, 300) };
    assert!(descriptor >= 300);
    let listener = TcpListener::bind("127.0.0.1:0").unwrap();
    listener.set_nonblocking(true).unwrap();
    let job = tempfile::tempdir_in(root.path()).unwrap();
    let input = format!(
        "probe\n{}\n{}\n{} {}\n",
        outside.display(),
        index.display(),
        listener.local_addr().unwrap().port(),
        descriptor
    );
    fs::write(job.path().join("input.pgm"), &input).unwrap();
    let result = run_recipe(
        &runtime,
        job.path(),
        Duration::from_secs(5),
        &CancellationToken::default(),
        recipe,
    );
    unsafe {
        libc::close(descriptor);
    }
    result.unwrap();
    let mut expected = "outside_read=0 outside_write=0 index_read=0 index_write=0 input_write=0 network=0 fork=0 inherited=0 environment=0".to_owned();
    if matches!(recipe, Recipe::WordRegions) {
        expected.push_str("\nunexpected=0");
    }
    assert_eq!(
        fs::read_to_string(job.path().join(filename))
            .unwrap()
            .trim(),
        expected
    );
    assert_eq!(
        fs::read_to_string(job.path().join("input.pgm")).unwrap(),
        input
    );
    assert_eq!(fs::read(&outside).unwrap(), b"outside sentinel");
    assert_eq!(fs::read(&index).unwrap(), b"index sentinel");
    assert_eq!(
        listener.accept().unwrap_err().kind(),
        std::io::ErrorKind::WouldBlock
    );
    finish_job(job, Ok(())).unwrap();
    for mode in ["sleep\n", "overflow\n"] {
        let job = tempfile::tempdir_in(root.path()).unwrap();
        fs::write(job.path().join("input.pgm"), mode).unwrap();
        let token = CancellationToken::default();
        let worker_token = token.clone();
        let worker_runtime = runtime.clone();
        let path = job.path().to_path_buf();
        let started = Instant::now();
        let worker = std::thread::spawn(move || {
            run_recipe(
                &worker_runtime,
                &path,
                Duration::from_secs(5),
                &worker_token,
                recipe,
            )
        });
        if mode.starts_with("sleep") {
            let result = job.path().join(filename);
            while !result.exists() || fs::metadata(&result).unwrap().len() == 0 {
                assert!(started.elapsed() < Duration::from_secs(5));
                std::thread::sleep(Duration::from_millis(5));
            }
            let pid: i32 = fs::read_to_string(result).unwrap().trim().parse().unwrap();
            token.cancel();
            assert!(matches!(worker.join().unwrap(), Err(Error::Blocked(_))));
            assert_eq!(
                unsafe { libc::kill(pid, 0) },
                -1,
                "Cancelled native worker was not reaped"
            );
        } else {
            assert!(worker.join().unwrap().is_err());
            assert!(fs::metadata(job.path().join(filename)).unwrap().len() <= recipe.limit());
        }
        assert!(started.elapsed() < Duration::from_secs(5));
        finish_job(job, Ok(())).unwrap();
    }
}
