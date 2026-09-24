//! Probe-only observations, collected after acknowledged process-tree termination.
use super::*;
use crate::java::diagnostics::{
    fallback_name, fatal_header, thread_sample, worker_failure, FailureDiagnostics, FatalHeader,
    TreeCounts,
};

fn tree_counts(root: &Path) -> Option<TreeCounts> {
    // Same no-follow/ADS/directory-pin validation and depth/member limits as
    // worker monitoring. Metadata only: no dump or arbitrary file is read.
    // A rejected/unreadable tree is null, never reported as an empty tree.
    let entries = walk(root, u64::MAX, 512).ok()?;
    let mut counts = TreeCounts {
        entries: entries.len(),
        ..Default::default()
    };
    for path in entries {
        let metadata = ordinary(&path).ok()?;
        if metadata.is_file() {
            counts.file_bytes = counts.file_bytes.checked_add(metadata.len())?;
            counts.largest_file_bytes = counts.largest_file_bytes.max(metadata.len());
            if path
                .extension()
                .and_then(|name| name.to_str())
                .is_some_and(|name| {
                    name.eq_ignore_ascii_case("mdmp") || name.eq_ignore_ascii_case("dmp")
                })
            {
                counts.minidump_named_files += 1;
            }
            counts.fixed_error_file_seen |= path == root.join("jvm-error.log");
            counts.fallback_error_files += usize::from(
                path.parent() == Some(root)
                    && path
                        .file_name()
                        .and_then(|name| name.to_str())
                        .is_some_and(fallback_name),
            );
        }
    }
    Some(counts)
}
fn read_fatal_header(scratch: &Path) -> Option<FatalHeader> {
    if let Ok(bytes) = read_output_bounded(&scratch.join("jvm-error.log"), 256 * 1024) {
        return fatal_header(&bytes);
    }
    // HotSpot documents a numeric hs_err_pid fallback in the current directory.
    // Inspect only this allowlist within the existing bounded, no-follow walk.
    // Multiple candidates are ambiguous. Never read a dump or arbitrary log.
    let candidates: Vec<_> = walk(scratch, u64::MAX, 512)
        .ok()?
        .into_iter()
        .filter(|path| {
            path.parent() == Some(scratch)
                && path
                    .file_name()
                    .and_then(|name| name.to_str())
                    .is_some_and(fallback_name)
        })
        .collect();
    if candidates.len() != 1 {
        return None;
    }
    read_output_bounded(&candidates[0], 256 * 1024)
        .ok()
        .and_then(|bytes| fatal_header(&bytes))
}
pub(super) fn capture(scratch: &Path, profile: &Path) -> FailureDiagnostics {
    let mut result = capture_control(scratch);
    result.profile = tree_counts(profile);
    result
}
pub(super) fn capture_control(scratch: &Path) -> FailureDiagnostics {
    FailureDiagnostics {
        captured_after_termination: true,
        scratch: tree_counts(scratch),
        profile: None,
        fatal_header: read_fatal_header(scratch),
        thread_sample: read_output_bounded(&scratch.join("java-sample.json"), 512)
            .ok()
            .and_then(|bytes| thread_sample(&bytes)),
        worker_failure: read_output_bounded(&scratch.join("java-failure.json"), 128)
            .ok()
            .and_then(|bytes| worker_failure(&bytes)),
        worker_checkpoint: read_output_bounded(&scratch.join("java-checkpoint.json"), 64)
            .ok()
            .and_then(|bytes| serde_json::from_slice(&bytes).ok()),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn failure_category_file_refuses_hardlinks_oversize_and_unsafe_fields() {
        let root = tempfile::tempdir().unwrap();
        let scratch = root.path().join("scratch");
        fs::create_dir(&scratch).unwrap();
        let source = root.path().join("source");
        fs::write(&source, br#"{"category":"access_denied"}"#).unwrap();
        let target = scratch.join("java-failure.json");
        fs::hard_link(&source, &target).unwrap();
        assert!(capture_control(&scratch).worker_failure.is_none());
        fs::remove_file(&target).unwrap();
        fs::write(&target, br#"{"category":"access_denied"}"#).unwrap();
        assert!(capture_control(&scratch).worker_failure.is_some());
        fs::write(&target, vec![b' '; 129]).unwrap();
        assert!(capture_control(&scratch).worker_failure.is_none());
        fs::write(&target, br#"{"category":"other","path":"PRIVATE"}"#).unwrap();
        assert!(capture_control(&scratch).worker_failure.is_none());
    }
    #[test]
    fn diagnostic_counts_do_not_read_dump_bytes_or_publish_names() {
        let temp = tempfile::tempdir().unwrap();
        let dump = fs::File::create(temp.path().join("private-worker-name.mdmp")).unwrap();
        dump.set_len(33 * 1024 * 1024).unwrap();
        drop(dump);
        let counts = tree_counts(temp.path()).unwrap();
        assert_eq!(counts.file_bytes, 33 * 1024 * 1024);
        assert_eq!(counts.minidump_named_files, 1);
        assert!(!serde_json::to_string(&counts)
            .unwrap()
            .contains("private-worker-name"));
        assert!(tree_counts(&temp.path().join("missing")).is_none());
    }
    #[test]
    fn fatal_diagnostic_refuses_hardlinks_and_oversized_files() {
        let temp = tempfile::tempdir().unwrap();
        let scratch = temp.path().join("scratch");
        let profile = temp.path().join("profile");
        fs::create_dir(&scratch).unwrap();
        fs::create_dir(&profile).unwrap();
        let source = temp.path().join("source");
        fs::write(
            &source,
            b"# A fatal error has been detected by the Java Runtime Environment:\n",
        )
        .unwrap();
        fs::hard_link(&source, scratch.join("jvm-error.log")).unwrap();
        assert!(capture(&scratch, &profile).fatal_header.is_none());
        fs::remove_file(scratch.join("jvm-error.log")).unwrap();
        fs::write(scratch.join("jvm-error.log"), vec![b'#'; 256 * 1024 + 1]).unwrap();
        assert!(capture(&scratch, &profile).fatal_header.is_none());
        fs::write(&source, br#""metadata_read""#).unwrap();
        fs::hard_link(&source, scratch.join("java-checkpoint.json")).unwrap();
        assert!(capture(&scratch, &profile).worker_checkpoint.is_none());
        fs::remove_file(scratch.join("java-checkpoint.json")).unwrap();
        fs::write(scratch.join("java-checkpoint.json"), vec![b' '; 65]).unwrap();
        assert!(capture(&scratch, &profile).worker_checkpoint.is_none());
    }
    #[test]
    fn thread_sample_file_rejects_links_and_oversize_after_quiescence() {
        let root = tempfile::tempdir().unwrap();
        let scratch = root.path().join("scratch");
        fs::create_dir(&scratch).unwrap();
        let source = root.path().join("source");
        let bytes=br#"{"state":"RUNNABLE","frame_count":1,"font_provider":true,"font_directory_walk":false,"font_decode":false,"pdf_text":true,"class_loading":false,"file_io":false}"#;
        fs::write(&source, bytes).unwrap();
        fs::hard_link(&source, scratch.join("java-sample.json")).unwrap();
        assert!(capture_control(&scratch).thread_sample.is_none());
        fs::remove_file(scratch.join("java-sample.json")).unwrap();
        fs::write(scratch.join("java-sample.json"), bytes).unwrap();
        assert!(capture_control(&scratch).thread_sample.is_some());
        fs::write(scratch.join("java-sample.json"), vec![b' '; 513]).unwrap();
        assert!(capture_control(&scratch).thread_sample.is_none());
    }
    #[test]
    fn fallback_diagnostic_requires_one_regular_bounded_numeric_log() {
        let temp = tempfile::tempdir().unwrap();
        let log = temp.path().join("hs_err_pid123.log");
        let text = b"# A fatal error has been detected by the Java Runtime Environment:\n#  EXCEPTION_INVALID_HANDLE (0xc0000008)\n";
        fs::write(&log, text).unwrap();
        assert_eq!(
            read_fatal_header(temp.path()).unwrap().exception_code,
            Some(0xc0000008)
        );
        let second = temp.path().join("hs_err_pid456.log");
        fs::write(&second, text).unwrap();
        assert!(read_fatal_header(temp.path()).is_none());
        fs::remove_file(second).unwrap();
        let linked = temp.path().join("outside-name");
        fs::hard_link(&log, &linked).unwrap();
        assert!(read_fatal_header(temp.path()).is_none());
        fs::remove_file(linked).unwrap();
        fs::write(log, vec![b'#'; 256 * 1024 + 1]).unwrap();
        assert!(read_fatal_header(temp.path()).is_none());
    }
}
