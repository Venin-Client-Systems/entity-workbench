use super::*;

fn workspace() -> tempfile::TempDir {
    let job = tempfile::tempdir_in(std::env::temp_dir().canonicalize().unwrap()).unwrap();
    fs::create_dir(job.path().join("scratch")).unwrap();
    job
}

fn top(job: &Path, index: usize, time: u64) {
    fs::write(
        job.join(format!("scratch/import-{index}.json")),
        serde_json::to_vec(&serde_json::json!({
            "module":IMPORTS[index / 2],"boundary":(["before","after"][index % 2]),
            "elapsed_ms":time,"process_cpu_ms":time,
        }))
        .unwrap(),
    )
    .unwrap();
}

fn attempt(job: &Path, index: usize, module: &str, time: u64) {
    fs::write(
        job.join(format!("scratch/import-attempt-{index}.json")),
        serde_json::to_vec(&serde_json::json!({
            "module":module,"ordinal":index,"elapsed_ms":time,"process_cpu_ms":time,
        }))
        .unwrap(),
    )
    .unwrap();
}

#[test]
fn empty_attempts_are_unknown_and_do_not_prevent_complete_top_level_sequence() {
    let job = workspace();
    let empty = collect(job.path());
    assert!(empty.valid);
    assert!(!empty.complete());
    assert!(empty.last().is_none());
    for index in 0..12 {
        top(job.path(), index, 1); // Millisecond truncation permits equal clocks.
    }
    let observed = collect(job.path());
    assert!(observed.complete());
    assert!(observed.attempts.is_empty());
    assert_eq!(observed.last().unwrap().module, "pyarrow");
    for (index, module) in ATTEMPTS.iter().enumerate() {
        attempt(job.path(), index, module, index as u64);
    }
    let observed = collect(job.path());
    assert!(observed.complete());
    assert_eq!(observed.attempts.len(), 16);
}

#[test]
fn invalid_later_data_retains_validated_prefix_and_never_copies_arbitrary_strings() {
    let job = workspace();
    top(job.path(), 0, 10);
    attempt(job.path(), 0, "thinc", 12);
    for invalid in [
        br#"{"module":"private/path","boundary":"after","elapsed_ms":12,"process_cpu_ms":12}"#.as_slice(),
        br#"{"module":"duckdb","boundary":"after","elapsed_ms":12,"process_cpu_ms":12,"args":"private/path"}"#,
        br#"{"module":"duckdb","boundary":"after","elapsed_ms":12,"elapsed_ms":12,"process_cpu_ms":12}"#,
        br#"{"module":"duckdb","boundary":"after","elapsed_ms":9,"process_cpu_ms":12}"#,
        br#"{"module":"duckdb","boundary":"after","elapsed_ms":12,"process_cpu_ms":9}"#,
        br#"{"module":"duckdb","boundary":"after","elapsed_ms":true,"process_cpu_ms":12}"#,
        br#"{"module":"duckdb","boundary":"after","elapsed_ms":120001,"process_cpu_ms":12}"#,
        br#"{"module":"duckdb","boundary":"after","elapsed_ms":12.0,"process_cpu_ms":12}"#,
        br#"{"module":"duckdb","boundary":"after","elapsed_ms":-1,"process_cpu_ms":12}"#,
        br#"{"module":"duckdb","boundary":"after"}"#,
    ] {
        fs::write(job.path().join("scratch/import-1.json"), invalid).unwrap();
        let observed = collect(job.path());
        assert!(!observed.valid);
        assert!(!observed.complete());
        assert_eq!(observed.checkpoints.len(), 1);
        assert_eq!(observed.attempts.len(), 1);
        assert!(!serde_json::to_string(&observed).unwrap().contains("private/path"));
    }
    fs::write(job.path().join("scratch/import-1.json"), [b' '; 513]).unwrap();
    assert!(!collect(job.path()).valid);
}

#[test]
fn attempt_duplicates_ordinals_clocks_and_extra_records_fail_closed() {
    let job = workspace();
    attempt(job.path(), 0, "numpy", 10);
    for (module, time) in [("numpy", 11), ("arbitrary", 11), ("blis", 9)] {
        attempt(job.path(), 1, module, time);
        let observed = collect(job.path());
        assert!(!observed.valid);
        assert_eq!(observed.attempts.len(), 1);
    }
    fs::write(
        job.path().join("scratch/import-attempt-1.json"),
        br#"{"module":"blis","ordinal":2,"elapsed_ms":12,"process_cpu_ms":12}"#,
    )
    .unwrap();
    assert!(!collect(job.path()).valid);
    fs::remove_file(job.path().join("scratch/import-attempt-1.json")).unwrap();
    attempt(job.path(), 2, "blis", 12);
    assert!(!collect(job.path()).valid);
    fs::remove_file(job.path().join("scratch/import-attempt-2.json")).unwrap();
    for extra in [
        "import-12.json",
        "import-attempt-16.json",
        "import-unreviewed.json",
    ] {
        let path = job.path().join("scratch").join(extra);
        fs::write(&path, b"{}").unwrap();
        assert!(!collect(job.path()).valid);
        fs::remove_file(path).unwrap();
    }
    assert!(collect(job.path()).valid);
}

#[test]
fn gaps_links_hardlinks_and_partial_files_never_complete_diagnostics() {
    let job = workspace();
    top(job.path(), 0, 1);
    top(job.path(), 2, 2);
    assert!(!collect(job.path()).valid);
    fs::remove_file(job.path().join("scratch/import-2.json")).unwrap();
    let path = job.path().join("scratch/import-1.json");
    fs::write(&path, b"{\"module\":").unwrap();
    assert_eq!(collect(job.path()).checkpoints.len(), 1);
    fs::remove_file(&path).unwrap();
    std::os::unix::fs::symlink(job.path().join("scratch/import-0.json"), &path).unwrap();
    assert!(!collect(job.path()).valid);
    fs::remove_file(&path).unwrap();
    fs::hard_link(job.path().join("scratch/import-0.json"), &path).unwrap();
    assert!(!collect(job.path()).valid);
    let alias = job.path().join("alias");
    std::os::unix::fs::symlink(job.path(), &alias).unwrap();
    assert!(!collect(&alias).valid);
}
