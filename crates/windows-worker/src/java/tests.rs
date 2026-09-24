use super::*;
use std::fs;
fn index() -> Job {
    Job::index(
        7,
        vec![Document {
            id: "synthetic-document".into(),
            name: "Synthetic source".into(),
            text: "Aster Harbour".into(),
        }],
    )
    .unwrap()
}
fn files() -> Vec<IndexFile> {
    vec![IndexFile {
        name: "segments_1".into(),
        bytes: b"synthetic index bytes".to_vec(),
        sha256: format!("{:x}", Sha256::digest(b"synthetic index bytes")),
    }]
}
fn snapshot() -> IndexSnapshot {
    accept(
        &index(),
        br#"{"indexed":1,"workspace_revision":7,"directory_policy":"lucene-10.5.1-bytebuffers-v1"}"#.to_vec(),
        files(),
    )
    .unwrap()
    .index
    .unwrap()
}
#[test]
fn recipes_have_fixed_paths_classes_and_jvm_options() {
    let snap = snapshot();
    for job in [
        Job::parse(b"synthetic".to_vec()).unwrap(),
        index(),
        Job::search(&snap, "Aster").unwrap(),
    ] {
        let prepared = prepare(Path::new("Q:/runtime"), Path::new("Q:/jobs"), &job).unwrap();
        assert_eq!(prepared.request.executable, Path::new("java/bin/java.exe"));
        assert!(prepared
            .request
            .arguments
            .contains(&"workbench.FileWorker".into()));
        assert_eq!(prepared.request.arguments.last().unwrap(), "$EW_REQUEST");
        assert!(prepared
            .request
            .arguments
            .contains(&r"-XX:ErrorFile=$EW_SCRATCH\jvm-error.log".into()));
        assert!(prepared
            .request
            .arguments
            .contains(&"-XX:-CreateCoredumpOnCrash".into()));
        for flag in ["-XX:-UsePerfData", "-XX:+DisableAttachMechanism"] {
            assert!(prepared.request.arguments.iter().any(|arg| arg == flag));
        }
        assert!(prepared
            .request
            .arguments
            .iter()
            .all(|arg| !arg.contains('/')));
        assert_eq!(
            prepared
                .request
                .arguments
                .iter()
                .any(|arg| arg == "-Dworkbench.fontPolicy=liberation-sans-2.1.5-extraction-v1"),
            job.role() == Role::Parser
        );
        assert_eq!(
            prepared
                .request
                .arguments
                .iter()
                .any(|arg| arg == &format!("-Dworkbench.directoryPolicy={DIRECTORY_POLICY}")),
            job.role() == Role::Search
        );
        assert_eq!(prepared.build_index(), job.operation_name() == "index");
        assert_eq!(
            prepared.snapshot().is_some(),
            job.operation_name() == "search"
        );
        assert_eq!(
            prepared.output_limit(),
            if job.role() == Role::Parser {
                PARSE_BYTES
            } else {
                1024 * 1024
            }
        );
        let metadata: serde_json::Value = serde_json::from_slice(&prepared.metadata).unwrap();
        assert_eq!(metadata["inputs"], serde_json::json!(["input.json"]));
        assert_eq!(metadata["output"], "result.json");
    }
}
#[cfg(windows)]
#[test]
fn recipe_suffixes_resolve_real_files_with_verbatim_windows_roots() {
    use std::os::windows::ffi::OsStrExt;
    use windows_sys::Win32::Storage::FileSystem::{GetFileAttributesW, INVALID_FILE_ATTRIBUTES};
    fn exists_native(value: &str) -> bool {
        let name: Vec<u16> = std::ffi::OsStr::new(value)
            .encode_wide()
            .chain(Some(0))
            .collect();
        // Read-only Win32 query of a live, nul-terminated synthetic path.
        unsafe { GetFileAttributesW(name.as_ptr()) != INVALID_FILE_ATTRIBUTES }
    }
    let temp = tempfile::tempdir().unwrap();
    let root = temp.path().canonicalize().unwrap();
    let text = root.to_str().unwrap();
    assert!(text.starts_with(r"\\?\"));
    fs::write(root.join("jvm-error.log"), b"synthetic").unwrap();
    fs::write(root.join("worker.jar"), b"synthetic").unwrap();
    fs::create_dir(root.join("lib")).unwrap();
    fs::create_dir(root.join("index")).unwrap();
    let job = index();
    let prepared = prepare(&root, &root, &job).unwrap();
    let arguments: Vec<_> = prepared
        .request
        .arguments
        .iter()
        .map(|arg| {
            arg.replace("$EW_RUNTIME", text)
                .replace("$EW_SCRATCH", text)
        })
        .collect();
    let error_file = arguments
        .iter()
        .find_map(|a| a.strip_prefix("-XX:ErrorFile="))
        .unwrap();
    let index = arguments
        .iter()
        .find_map(|a| a.strip_prefix("-Dworkbench.index="))
        .unwrap();
    let classpath = &arguments[arguments.iter().position(|a| a == "-cp").unwrap() + 1];
    assert!(exists_native(error_file));
    assert!(exists_native(index));
    for asset in classpath.split(';') {
        assert!(exists_native(asset.strip_suffix(r"\*").unwrap_or(asset)));
    }
    // This is the old emitted form, not a PathBuf operation that might repair it.
    assert!(!exists_native(&format!("{text}/jvm-error.log")));
}
#[test]
fn result_acknowledgements_cannot_publish_wrong_or_ambiguous_index() {
    for bytes in [
        br#"{"indexed":0,"workspace_revision":7,"directory_policy":"lucene-10.5.1-bytebuffers-v1"}"#.as_slice(),
        br#"{"indexed":1,"workspace_revision":8,"directory_policy":"lucene-10.5.1-bytebuffers-v1"}"#,
        br#"{"indexed":1,"workspace_revision":7,"directory_policy":"lucene-10.5.1-bytebuffers-v1","extra":true}"#,
        br#"{"indexed":1,"indexed":1,"workspace_revision":7,"directory_policy":"lucene-10.5.1-bytebuffers-v1"}"#,
        br#"{"indexed":1,"workspace_revision":7,"directory_policy":"lucene-10.5.1-bytebuffers-v1"} {}"#,
    ] {
        assert!(matches!(accept(&index(), bytes.to_vec(), files()), Err(Error::InvalidResult(_))));
    }
    assert!(accept(
        &index(),
        br#"{"indexed":1,"workspace_revision":7,"directory_policy":"lucene-10.5.1-bytebuffers-v1"}"#.to_vec(),
        Vec::new()
    )
    .is_err());
}
#[test]
fn snapshots_reject_path_stream_device_collision_and_hash_poison() {
    for name in [
        "../escape",
        "a/b",
        "a\\b",
        "file:stream",
        "NUL.txt",
        "CON",
        "COM1",
        "LPT9",
        "bad.",
        ".hidden",
    ] {
        let mut value = snapshot();
        value.files[0].name = name.into();
        assert!(validate_snapshot(&value).is_err());
    }
    let mut value = snapshot();
    value.files[0].bytes[0] ^= 1;
    assert!(validate_snapshot(&value).is_err());
    let mut value = snapshot();
    let mut collision = value.files[0].clone();
    collision.name = collision.name.to_ascii_uppercase();
    value.files.push(collision);
    assert!(validate_snapshot(&value).is_err());
    let mut value = snapshot();
    value.files[0].bytes = vec![0; INDEX_FILE_BYTES + 1];
    value.files[0].sha256 = format!("{:x}", Sha256::digest(&value.files[0].bytes));
    assert!(validate_snapshot(&value).is_err());
}
#[test]
fn search_results_require_matching_revision_and_known_unique_sources() {
    let job = Job::search(&snapshot(), "Aster").unwrap();
    let valid = serde_json::json!({"directory_policy":DIRECTORY_POLICY,"workspace_revision":"7","hits":[{"id":"synthetic-document","name":"Synthetic source","score":1.0}],"total":1});
    assert!(accept(&job, serde_json::to_vec(&valid).unwrap(), Vec::new()).is_ok());
    for mode in 0..5 {
        let mut value = valid.clone();
        match mode {
            0 => value["workspace_revision"] = serde_json::json!("8"),
            1 => value["hits"][0]["id"] = serde_json::json!("unknown"),
            2 => value["hits"][0]["name"] = serde_json::json!("wrong"),
            3 => value["total"] = serde_json::json!(0),
            _ => value["extra"] = serde_json::json!(true),
        }
        assert!(accept(&job, serde_json::to_vec(&value).unwrap(), Vec::new()).is_err());
    }
}
fn fake_runtime(root: &Path) {
    fs::create_dir_all(root.join("java/bin")).unwrap();
    fs::create_dir(root.join("lib")).unwrap();
    let mut files = serde_json::Map::new();
    for name in [
        "java/bin/java.exe",
        "worker.jar",
        "lib/lucene-core-10.5.1.jar",
        "lib/lucene-analysis-common-10.5.1.jar",
        "lib/lucene-queryparser-10.5.1.jar",
        "lib/jackson-databind-2.22.3.jar",
    ] {
        fs::write(root.join(name), b"synthetic asset").unwrap();
        files.insert(name.into(),serde_json::json!({"bytes":15,"sha256":format!("{:x}",Sha256::digest(b"synthetic asset"))}));
    }
    fs::write(root.join("manifest.json"),serde_json::to_vec(&serde_json::json!({"schema_version":1,"development_only":true,"role":"search","java_version":"21.0.synthetic","files":files,"directories":["java","java/bin","lib"]})).unwrap()).unwrap();
}
#[test]
fn runtime_inventory_binds_role_assets_and_every_file_and_directory() {
    let temporary = tempfile::tempdir().unwrap();
    let root = temporary.path().canonicalize().unwrap();
    fake_runtime(&root);
    runtime::verify(&root, Role::Search).unwrap();
    prepare(&root, &root, &index())
        .unwrap()
        .verify_runtime(&root)
        .unwrap();
    assert!(runtime::verify(&root, Role::Parser).is_err());
    fs::write(root.join("lib/unlisted.jar"), b"unlisted").unwrap();
    assert!(runtime::verify(&root, Role::Search).is_err());
    fs::remove_file(root.join("lib/unlisted.jar")).unwrap();
    fs::create_dir(root.join("empty-extra")).unwrap();
    assert!(runtime::verify(&root, Role::Search).is_err());
    fs::remove_dir(root.join("empty-extra")).unwrap();
    fs::write(root.join("worker.jar"), b"synthetic changed asset").unwrap();
    assert!(runtime::verify(&root, Role::Search).is_err());
}

fn assert_runtime_unavailable<T>(result: Result<T>) {
    assert!(matches!(
        result,
        Err(Error::Blocked("trusted Java runtime is unavailable"))
    ));
}

#[test]
fn trusted_runtime_absence_is_blocked_before_copy_and_during_copied_verification() {
    let temporary = tempfile::tempdir().unwrap();
    let parent = temporary.path().canonicalize().unwrap();
    let root = parent.join("absent-runtime");
    let scratch = parent.join("absent-scratch");
    for present_directory in [false, true] {
        if present_directory {
            fs::create_dir(&root).unwrap();
        }
        for job in [Job::parse(b"synthetic".to_vec()).unwrap(), index()] {
            assert_runtime_unavailable(execute(&root, &scratch, &job, || false));
            let prepared = prepare(&root, &scratch, &job).unwrap();
            // These are the same entrypoints used by copied-runtime verification
            // and the fixed native control. This fixture never launches a worker.
            assert_runtime_unavailable(prepared.verify_runtime_with_cancel(&root, &|| false));
            assert_runtime_unavailable(prepared.verify_runtime(&root));
            assert!(matches!(
                prepared.verify_runtime_with_cancel(&root, &|| true),
                Err(Error::Cancelled)
            ));
        }
        assert!(!scratch.exists());
    }
}

#[test]
#[cfg(any(unix, windows))]
fn trusted_runtime_access_failure_is_blocked_without_hiding_cancellation() {
    let temporary = tempfile::tempdir().unwrap();
    let root = temporary.path().canonicalize().unwrap();
    fake_runtime(&root);
    let manifest = root.join("manifest.json");
    #[cfg(unix)]
    let original_permissions = {
        use std::os::unix::fs::PermissionsExt;
        let original = fs::metadata(&manifest).unwrap().permissions();
        fs::set_permissions(&manifest, fs::Permissions::from_mode(0o000)).unwrap();
        original
    };
    #[cfg(windows)]
    let exclusive = {
        use std::os::windows::fs::OpenOptionsExt;
        // The verifier must honor this real Windows sharing refusal. No ACL
        // modification or privileged launch is needed for the fixture.
        fs::OpenOptions::new()
            .read(true)
            .share_mode(0)
            .open(&manifest)
            .unwrap()
    };
    let job = index();
    let prepared = prepare(&root, &root, &job).unwrap();
    let raw_read = runtime::read_bounded(&manifest, 1024 * 1024);
    let verification = prepared.verify_runtime_with_cancel(&root, &|| false);
    let execution = execute(&root, &root, &job, || false);
    let cancellation = prepared.verify_runtime_with_cancel(&root, &|| true);
    #[cfg(unix)]
    fs::set_permissions(&manifest, original_permissions).unwrap();
    #[cfg(windows)]
    drop(exclusive);
    assert!(matches!(
        raw_read,
        Err(Error::Io(std::io::ErrorKind::PermissionDenied))
    ));
    assert_runtime_unavailable(verification);
    assert_runtime_unavailable(execution);
    assert!(matches!(cancellation, Err(Error::Cancelled)));
    prepared.verify_runtime(&root).unwrap();
}

#[test]
fn runtime_verification_does_not_reclassify_scratch_or_generic_io() {
    let temporary = tempfile::tempdir().unwrap();
    let root = temporary.path().canonicalize().unwrap();
    fake_runtime(&root);
    let scratch = root.join("absent-scratch");
    assert!(matches!(
        execute(&root, &scratch, &index(), || false),
        Err(Error::Io(std::io::ErrorKind::NotFound))
    ));
    assert!(!scratch.exists());
    assert!(matches!(
        runtime::read_bounded(&root.join("absent-result.json"), 100),
        Err(Error::Io(std::io::ErrorKind::NotFound))
    ));
    assert!(matches!(
        Error::from(std::io::Error::from(std::io::ErrorKind::PermissionDenied)),
        Error::Io(std::io::ErrorKind::PermissionDenied)
    ));
}

#[test]
fn copied_runtime_verification_failure_keeps_cleanup_and_termination_precedence() {
    use crate::outcomes::{after_termination, finish_assignment};
    let temporary = tempfile::tempdir().unwrap();
    let parent = temporary.path().canonicalize().unwrap();
    let absent = parent.join("absent-copied-runtime");
    let job = index();
    let prepared = prepare(&absent, &parent, &job).unwrap();
    let verification = || prepared.verify_runtime_with_cancel(&absent, &|| false);
    let cleanup = finish_assignment(true, verification(), || {
        Err(Error::Io(std::io::ErrorKind::PermissionDenied))
    });
    assert!(matches!(
        cleanup,
        Err(Error::Cleanup { prior: Some(prior) })
            if matches!(*prior, Error::Blocked("trusted Java runtime is unavailable"))
    ));
    let uncertain = after_termination(
        verification(),
        Err(Error::Api {
            operation: "SyntheticTerminationFailure",
            code: 5,
        }),
    );
    let retained = finish_assignment(false, uncertain, || panic!("must retain assignment"));
    assert!(matches!(
        retained,
        Err(Error::TerminationUnverified { cause, prior: Some(prior) })
            if matches!(*cause, Error::Api { code: 5, .. })
                && matches!(*prior, Error::Blocked("trusted Java runtime is unavailable"))
    ));
}

#[test]
fn index_input_is_bounded_before_copies_and_during_json_escaping() {
    let document = |id: &str, text: String| Document {
        id: id.into(),
        name: "synthetic".into(),
        text,
    };
    assert!(Job::index(1, vec![document("one", "x".repeat(INPUT_BYTES + 1))]).is_err());
    assert!(Job::index(
        1,
        vec![
            document("one", "x".repeat(INPUT_BYTES / 2)),
            document("two", "x".repeat(INPUT_BYTES / 2))
        ]
    )
    .is_err());
    assert!(Job::index(
        1,
        vec![document("one", "\u{0001}".repeat(INPUT_BYTES / 6 + 1))]
    )
    .is_err());
    let accepted = Job::index(1, vec![document("one", "\u{0001}".repeat(1000))]).unwrap();
    let decoded: serde_json::Value = serde_json::from_slice(&accepted.input).unwrap();
    assert_eq!(decoded["documents"][0]["text"], "\u{0001}".repeat(1000));
    assert!(accepted.input.len() <= INPUT_BYTES);
}

#[test]
fn parser_results_are_bound_to_assigned_job_and_original_bytes() {
    let job = Job::parse(b"synthetic original".to_vec()).unwrap();
    let valid = serde_json::json!({"protocol_version":1,"job_id":job.id,"content_sha256":format!("{:x}",Sha256::digest(&job.input)),
        "source_bytes":job.input.len(),"parser":"utf8-v1","media_type":"text/plain","status":"complete",
        "text":"synthetic original","metadata":{},"limitations":["no_source_anchors"],"error":null});
    let bytes = serde_json::to_vec(&valid).unwrap();
    let output = accept(&job, bytes.clone(), Vec::new()).unwrap();
    assert_eq!(
        output.output_sha256,
        format!("{:x}", Sha256::digest(&bytes))
    );
    for field in ["job_id", "content_sha256", "source_bytes", "unexpected"] {
        let mut altered = valid.clone();
        altered[field] = match field {
            "job_id" => serde_json::json!(Uuid::new_v4()),
            "content_sha256" => serde_json::json!("0".repeat(64)),
            _ => serde_json::json!(0),
        };
        assert!(matches!(
            accept(&job, serde_json::to_vec(&altered).unwrap(), Vec::new()),
            Err(Error::InvalidResult(_))
        ));
    }
}

#[test]
fn pre_cancelled_java_execution_never_requires_runtime_or_scratch() {
    let tree = tempfile::tempdir().unwrap();
    let absent = tree.path().join("absent");
    let job = Job::parse(b"synthetic".to_vec()).unwrap();
    assert!(matches!(
        execute(&absent, &absent, &job, || true),
        Err(Error::Cancelled)
    ));
    assert!(!absent.exists());
}

#[test]
fn runtime_inventory_cancellation_is_distinct_from_invalid_runtime() {
    use std::cell::Cell;
    let tree = tempfile::tempdir().unwrap();
    let root = tree.path().canonicalize().unwrap();
    fake_runtime(&root);
    let job = index();
    let prepared = prepare(&root, &root, &job).unwrap();
    prepared
        .verify_runtime_with_cancel(&root, &|| false)
        .unwrap();
    let checks = Cell::new(0);
    let result = prepared.verify_runtime_with_cancel(&root, &|| {
        checks.set(checks.get() + 1);
        checks.get() >= 8
    });
    assert!(matches!(result, Err(Error::Cancelled)));
    assert_eq!(checks.get(), 8);
    prepared
        .verify_runtime_with_cancel(&root, &|| false)
        .unwrap();
    fs::write(root.join("worker.jar"), b"altered").unwrap();
    assert!(matches!(
        prepared.verify_runtime_with_cancel(&root, &|| false),
        Err(Error::Blocked(_))
    ));
}

#[test]
fn malformed_reply_and_output_quota_remain_different_outcomes() {
    let job = Job::parse(b"synthetic".to_vec()).unwrap();
    for bytes in [b"{".as_slice(), b"{} {}", b"{\"unknown\":true}"] {
        assert!(matches!(
            accept(&job, bytes.to_vec(), vec![]),
            Err(Error::InvalidResult(_))
        ));
    }
    assert!(matches!(
        accept(&job, vec![0; PARSE_BYTES as usize + 1], vec![]),
        Err(Error::ResourceLimit(ResourceLimit::OutputBytes))
    ));
}

#[test]
fn runtime_rejects_an_outside_hardlink_even_when_content_matches() {
    let tree = tempfile::tempdir().unwrap();
    let root = tree.path().canonicalize().unwrap().join("runtime");
    fs::create_dir(&root).unwrap();
    fake_runtime(&root);
    runtime::verify(&root, Role::Search).unwrap();
    let outside = tree.path().join("outside-copy.jar");
    fs::hard_link(root.join("worker.jar"), &outside).unwrap();
    assert!(runtime::verify(&root, Role::Search).is_err());
    assert_eq!(fs::read(outside).unwrap(), b"synthetic asset");
}

#[test]
#[cfg(not(windows))]
fn fixed_java_recipes_have_no_unconfined_platform_fallback() {
    let tree = tempfile::tempdir().unwrap();
    let root = tree.path().canonicalize().unwrap();
    fake_runtime(&root);
    assert!(matches!(
        execute(&root, &root, &index(), || false),
        Err(Error::Blocked(
            "Windows Java recipe requires native AppContainer"
        ))
    ));
}

#[test]
fn pdf_results_cannot_silently_use_old_policy_or_claim_complete_coverage() {
    let job = Job::parse(b"%PDF-synthetic".to_vec()).unwrap();
    let mut result = serde_json::json!({"protocol_version":1,"job_id":job.id,"content_sha256":format!("{:x}",Sha256::digest(&job.input)),"source_bytes":job.input.len(),"parser":PDF_FONT_PARSER,"media_type":"application/pdf","status":"partial","text":"synthetic","metadata":{},"limitations":["font_substituted","font_coverage_unverified"],"error":null});
    assert!(accept(&job, serde_json::to_vec(&result).unwrap(), vec![]).is_ok());
    for (field, value) in [
        ("parser", serde_json::json!("pdfbox-3.0.8")),
        ("status", serde_json::json!("complete")),
        ("limitations", serde_json::json!(["font_substituted"])),
    ] {
        let mut invalid = result.clone();
        invalid[field] = value;
        assert!(accept(&job, serde_json::to_vec(&invalid).unwrap(), vec![]).is_err());
    }
    result["limitations"] = serde_json::json!([]);
    assert!(accept(&job, serde_json::to_vec(&result).unwrap(), vec![]).is_ok());
}

#[test]
fn directory_policy_is_required_in_acknowledgements_snapshots_and_queries() {
    for policy in [None, Some("unknown"), Some("")] {
        let mut reply = serde_json::json!({"indexed":1,"workspace_revision":7});
        if let Some(policy) = policy {
            reply["directory_policy"] = serde_json::json!(policy);
        }
        assert!(accept(&index(), serde_json::to_vec(&reply).unwrap(), files()).is_err());
        let mut snap = snapshot();
        snap.policy = policy.unwrap_or_default().to_string();
        assert!(Job::search(&snap, "Aster").is_err());
    }
    let snap = snapshot();
    let job = Job::search(&snap, "Aster").unwrap();
    assert_eq!(
        serde_json::from_slice::<serde_json::Value>(&job.input).unwrap(),
        serde_json::json!({"query":"Aster","workspace_revision":"7","directory_policy":DIRECTORY_POLICY})
    );
    for policy in [None, Some("unknown")] {
        let mut reply = serde_json::json!({"workspace_revision":"7","hits":[],"total":0});
        if let Some(policy) = policy {
            reply["directory_policy"] = serde_json::json!(policy);
        }
        assert!(accept(&job, serde_json::to_vec(&reply).unwrap(), Vec::new()).is_err());
    }
}
