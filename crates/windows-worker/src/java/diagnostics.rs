//! Fixed diagnostic fields only; worker-controlled raw logs never enter receipts.
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};

#[derive(Debug, Default, Serialize)]
pub(crate) struct FailureDiagnostics {
    pub captured_after_termination: bool,
    pub scratch: Option<TreeCounts>,
    pub profile: Option<TreeCounts>,
    pub fatal_header: Option<FatalHeader>,
    pub worker_checkpoint: Option<JavaCheckpoint>,
    pub thread_sample: Option<ThreadSample>,
    pub worker_failure: Option<WorkerFailure>,
}
#[derive(Debug, Deserialize, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub(crate) enum JavaCheckpoint {
    FileWorkerEntered,
    MetadataRead,
    RequestDecoded,
    ParserSelected,
    SearchSelected,
    WorkerReturned,
    PdfLoadStarted,
    PdfLoaded,
    PdfStripperStarted,
    PdfStripperReady,
    PdfTextStarted,
    PdfTextFinished,
    SearchRequestValidated,
    SearchIndexValidated,
    SearchDirectoryStarted,
    SearchDirectoryReady,
    SearchManifestRead,
    SearchWriterStarted,
    SearchWriterReady,
    SearchIndexCommitted,
}
#[derive(Debug, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct WorkerFailure {
    category: FailureCategory,
}
#[derive(Debug, Deserialize, Serialize)]
#[serde(rename_all = "snake_case")]
enum FailureCategory {
    AccessDenied,
    MissingFile,
    FileExists,
    Filesystem,
    Io,
    Security,
    Other,
}
pub(crate) fn worker_failure(bytes: &[u8]) -> Option<WorkerFailure> {
    if bytes.len() > 128 {
        return None;
    }
    serde_json::from_slice(bytes).ok()
}
#[derive(Debug, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct ThreadSample {
    state: SampleState,
    frame_count: u8,
    font_provider: bool,
    font_directory_walk: bool,
    font_decode: bool,
    pdf_text: bool,
    class_loading: bool,
    file_io: bool,
}
#[derive(Debug, Deserialize, Serialize)]
#[serde(rename_all = "SCREAMING_SNAKE_CASE")]
enum SampleState {
    New,
    Runnable,
    Blocked,
    Waiting,
    TimedWaiting,
    Terminated,
}
pub(crate) fn thread_sample(bytes: &[u8]) -> Option<ThreadSample> {
    if bytes.len() > 512 {
        return None;
    }
    let sample: ThreadSample = serde_json::from_slice(bytes).ok()?;
    (sample.frame_count <= 64).then_some(sample)
}
#[derive(Debug, Default, Serialize)]
pub(crate) struct TreeCounts {
    pub entries: usize,
    pub file_bytes: u64,
    pub largest_file_bytes: u64,
    pub minidump_named_files: usize,
    pub fixed_error_file_seen: bool,
    pub fallback_error_files: usize,
}
#[derive(Debug, Default, PartialEq, Eq, Serialize)]
pub(crate) struct FatalHeader {
    pub exception_code: Option<u32>,
    pub frame_module: Option<&'static str>,
    pub internal_error: bool,
    pub out_of_memory: bool,
    pub source_component: Option<&'static str>,
    pub source_line: Option<u32>,
    pub source_basename_sha256: Option<String>,
}
pub(crate) fn fallback_name(name: &str) -> bool {
    name.strip_prefix("hs_err_pid")
        .and_then(|s| s.strip_suffix(".log"))
        .is_some_and(|digits| {
            !digits.is_empty() && digits.len() <= 10 && digits.bytes().all(|b| b.is_ascii_digit())
        })
}
// HotSpot's product build prints a basename and line. Reject paths instead of
// hashing them: a fingerprint must never encode arbitrary source path material.
fn source_location(content: &str) -> Option<(&str, u32)> {
    let location = content.strip_prefix("Internal Error (")?.split_once(')')?.0;
    if location.len() > 104 {
        return None;
    }
    let (basename, line) = location.split_once(':')?;
    if basename.is_empty()
        || basename.len() > 96
        || !basename.as_bytes()[0].is_ascii_alphabetic()
        || !basename
            .bytes()
            .all(|byte| byte.is_ascii_alphanumeric() || byte == b'_' || byte == b'.')
        || basename.contains("..")
        || !(basename.ends_with(".cpp")
            || basename.ends_with(".hpp")
            || basename.ends_with(".c")
            || basename.ends_with(".h"))
        || line.is_empty()
        || line.len() > 6
        || !line.bytes().all(|byte| byte.is_ascii_digit())
    {
        return None;
    }
    let line = line.parse().ok()?;
    (line > 0).then_some((basename, line))
}

pub(crate) fn fatal_header(bytes: &[u8]) -> Option<FatalHeader> {
    // Only the first 32 lines of an already bounded file are inspected. Emit
    // neither the source text, addresses, PID, thread IDs, paths nor environment.
    if bytes.len() > 256 * 1024 {
        return None;
    }
    let text = std::str::from_utf8(bytes).ok()?;
    let lines: Vec<_> = text.lines().take(32).collect();
    if !lines.contains(&"# A fatal error has been detected by the Java Runtime Environment:")
        && !lines.contains(
            &"# There is insufficient memory for the Java Runtime Environment to continue.",
        )
    {
        return None;
    }
    let mut header = FatalHeader::default();
    for (i, line) in lines.iter().enumerate() {
        let content = line.strip_prefix('#').unwrap_or("").trim_start();
        header.internal_error |= content.starts_with("Internal Error (");
        header.out_of_memory |= content.starts_with("Out of Memory Error (")
            || content.starts_with("Native memory allocation (")
            || content
                == "There is insufficient memory for the Java Runtime Environment to continue.";
        if let Some((basename, line)) = source_location(content) {
            header.source_basename_sha256 = Some(format!("{:x}", Sha256::digest(basename)));
            header.source_line = Some(line);
            header.source_component = [
                "os_windows.cpp",
                "os_windows_x86.cpp",
                "perfMemory_windows.cpp",
                "os.cpp",
                "thread.cpp",
                "javaThread.cpp",
                "allocation.cpp",
                "arena.cpp",
                "virtualspace.cpp",
                "vm_version_x86.cpp",
                "universe.cpp",
                "javaClasses.cpp",
                "classFileParser.cpp",
                "exceptions.cpp",
                "debug.cpp",
            ]
            .into_iter()
            .find(|component| *component == basename);
        }
        if content.starts_with("EXCEPTION_") || content.starts_with("Internal Error (0x") {
            if let Some((_, code)) = line.split_once("(0x") {
                if let Some((code, _)) = code.split_once(')') {
                    if code.len() == 8 && code.bytes().all(|b| b.is_ascii_hexdigit()) {
                        header.exception_code = u32::from_str_radix(code, 16).ok();
                    }
                }
            }
        }
        if *line == "# Problematic frame:" {
            if let Some(frame) = lines.get(i + 1) {
                for module in [
                    "ntdll.dll",
                    "KERNELBASE.dll",
                    "KERNEL32.dll",
                    "jvm.dll",
                    "java.dll",
                    "nio.dll",
                    "net.dll",
                    "zip.dll",
                    "ucrtbase.dll",
                ] {
                    if frame.contains(&format!("[{module}+0x")) {
                        header.frame_module = Some(module);
                    }
                }
            }
        }
    }
    Some(header)
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn failure_categories_reject_arbitrary_fields_names_and_oversize() {
        for category in [
            "access_denied",
            "missing_file",
            "file_exists",
            "filesystem",
            "io",
            "security",
            "other",
        ] {
            let bytes = format!(r#"{{"category":"{category}"}}"#);
            let result = worker_failure(bytes.as_bytes()).unwrap();
            assert_eq!(serde_json::to_string(&result).unwrap(), bytes);
        }
        for bytes in [
            br#"{"category":"PRIVATE/path"}"#.as_slice(),
            br#"{"category":"other","message":"PRIVATE"}"#.as_slice(),
            br#"{"category":"other","category":"io"}"#.as_slice(),
            br#"{"category":"other"} {}"#.as_slice(),
            br#"{"category":null}"#.as_slice(),
        ] {
            assert!(worker_failure(bytes).is_none());
        }
        assert!(worker_failure(&[b' '; 129]).is_none());
    }
    #[test]
    fn fallback_names_are_only_bounded_numeric_jvm_log_names() {
        assert!(fallback_name("hs_err_pid1234.log"));
        for name in [
            "hs_err_pid.log",
            "hs_err_pid12345678901.log",
            "hs_err_pidprivate.log",
            "hs_err_pid12.mdmp",
            "hs_err_pid1.log:stream",
            "../hs_err_pid12.log",
            "hs_err_pid1/2.log",
        ] {
            assert!(!fallback_name(name));
        }
    }
    #[test]
    fn fatal_hints_emit_only_fixed_modules_and_numeric_exception_codes() {
        let text = b"#\n# A fatal error has been detected by the Java Runtime Environment:\n#\n#  EXCEPTION_INVALID_HANDLE (0xc0000008) at pc=0xPRIVATE, pid=PRIVATE\n# Problematic frame:\n# C  [ntdll.dll+0xPRIVATE] private_name\n# PRIVATE_PATH\n";
        let header = fatal_header(text).unwrap();
        assert_eq!(header.exception_code, Some(0xc0000008));
        assert_eq!(header.frame_module, Some("ntdll.dll"));
        assert!(!serde_json::to_string(&header).unwrap().contains("PRIVATE"));
        let unknown = String::from_utf8(text.to_vec())
            .unwrap()
            .replace("ntdll.dll", "private_module.dll");
        assert_eq!(fatal_header(unknown.as_bytes()).unwrap().frame_module, None);
    }
    #[test]
    fn fatal_hints_require_header_and_do_not_scan_later_log_contents() {
        assert!(fatal_header(b"EXCEPTION_INVALID_HANDLE (0xc0000008)").is_none());
        assert!(fatal_header(b"\xff").is_none());
        let text = format!(
            "{}# A fatal error has been detected by the Java Runtime Environment:\n",
            "#\n".repeat(32)
        );
        assert!(fatal_header(text.as_bytes()).is_none());
        let text = b"# A fatal error has been detected by the Java Runtime Environment:\n#  EXCEPTION_INVALID_HANDLE (0xprivate)\n";
        assert_eq!(fatal_header(text).unwrap().exception_code, None);
    }
    #[test]
    fn internal_and_memory_hints_keep_only_fixed_categories_and_source_line() {
        let text = b"# A fatal error has been detected by the Java Runtime Environment:\n# Internal Error (os_windows.cpp:123), pid=PRIVATE\n# arbitrary private reason\n";
        let header = fatal_header(text).unwrap();
        assert!(header.internal_error);
        assert!(!header.out_of_memory);
        assert_eq!(header.source_component, Some("os_windows.cpp"));
        assert_eq!(header.source_line, Some(123));
        assert!(!serde_json::to_string(&header).unwrap().contains("PRIVATE"));
        let text = b"# There is insufficient memory for the Java Runtime Environment to continue.\n# Native memory allocation (malloc) failed\n# Out of Memory Error (PRIVATE/private.cpp:123)\n";
        let header = fatal_header(text).unwrap();
        assert!(header.out_of_memory);
        assert_eq!(header.source_component, None);
        assert_eq!(header.source_line, None);
        let text = b"# A fatal error has been detected by the Java Runtime Environment:\n# Internal Error (0xc0000008), pid=PRIVATE\n";
        assert_eq!(fatal_header(text).unwrap().exception_code, Some(0xc0000008));
    }
    #[test]
    fn source_fingerprint_accepts_only_bounded_internal_error_basenames() {
        let prefix = "# A fatal error has been detected by the Java Runtime Environment:\n# ";
        let header = fatal_header(
            format!("{prefix}Internal Error (classLoaderData.hpp:314), pid=PRIVATE").as_bytes(),
        )
        .unwrap();
        assert_eq!(header.source_line, Some(314));
        assert_eq!(
            header.source_basename_sha256,
            Some(format!("{:x}", Sha256::digest(b"classLoaderData.hpp")))
        );
        assert_eq!(header.source_component, None);
        let json = serde_json::to_string(&header).unwrap();
        assert!(!json.contains("classLoaderData"));
        assert!(!json.contains("PRIVATE"));
        for location in [
            "private/classLoaderData.hpp:314",
            r"private\classLoaderData.hpp:314",
            "C:classLoaderData.hpp:314",
            "../file.cpp:1",
            "file.cpp:0",
            "file.cpp:-1",
            "file.cpp:1x",
            "file.cpp:1000000",
            "file.cpp:",
            "file.cpp:1:2",
            "file..cpp:1",
            ".cpp:1",
            "file.txt:1",
            "fi le.cpp:1",
            "file.cpp:１",
            "file.cpp:1\0",
            "file.cpp:1\n",
            "file.cpp:1(no-close",
        ] {
            let value =
                fatal_header(format!("{prefix}Internal Error ({location})").as_bytes()).unwrap();
            assert_eq!(
                value.source_basename_sha256, None,
                "accepted invalid synthetic location"
            );
            assert_eq!(value.source_line, None);
        }
        let location = format!("{}.cpp:1", "a".repeat(97));
        let value =
            fatal_header(format!("{prefix}Internal Error ({location})").as_bytes()).unwrap();
        assert_eq!(value.source_basename_sha256, None);
        let oversized = format!(
            "{prefix}Internal Error (file.cpp:1){}",
            " ".repeat(256 * 1024)
        );
        assert!(fatal_header(oversized.as_bytes()).is_none());
    }
    #[test]
    fn adapter_checkpoint_hints_are_fixed_and_fit_the_existing_read_bound() {
        for value in [
            JavaCheckpoint::PdfLoadStarted,
            JavaCheckpoint::PdfLoaded,
            JavaCheckpoint::PdfStripperStarted,
            JavaCheckpoint::PdfStripperReady,
            JavaCheckpoint::PdfTextStarted,
            JavaCheckpoint::PdfTextFinished,
            JavaCheckpoint::SearchRequestValidated,
            JavaCheckpoint::SearchIndexValidated,
            JavaCheckpoint::SearchDirectoryStarted,
            JavaCheckpoint::SearchDirectoryReady,
            JavaCheckpoint::SearchManifestRead,
            JavaCheckpoint::SearchWriterStarted,
            JavaCheckpoint::SearchWriterReady,
            JavaCheckpoint::SearchIndexCommitted,
        ] {
            let bytes = serde_json::to_vec(&value).unwrap();
            assert!(bytes.len() <= 64);
            assert_eq!(
                serde_json::from_slice::<JavaCheckpoint>(&bytes).unwrap(),
                value
            );
        }
    }
    #[test]
    fn thread_samples_are_closed_bounded_and_not_arbitrary_stack_output() {
        let value = serde_json::json!({"state":"RUNNABLE","frame_count":64,
            "font_provider":true,"font_directory_walk":false,"font_decode":true,
            "pdf_text":true,"class_loading":false,"file_io":false});
        let bytes = serde_json::to_vec(&value).unwrap();
        assert!(thread_sample(&bytes).is_some());
        for (key, invalid) in [
            ("state", serde_json::json!("private-state")),
            ("frame_count", serde_json::json!(65)),
            ("frame_count", serde_json::json!(-1)),
            ("font_provider", serde_json::json!("true")),
            ("raw_stack", serde_json::json!("private")),
        ] {
            let mut bad = value.clone();
            bad[key] = invalid;
            assert!(thread_sample(&serde_json::to_vec(&bad).unwrap()).is_none());
        }
        let mut duplicate = bytes.clone();
        duplicate.pop();
        duplicate.extend(br#", "frame_count":1}"#);
        assert!(thread_sample(&duplicate).is_none());
        let mut trailing = bytes.clone();
        trailing.extend(b" {}");
        assert!(thread_sample(&trailing).is_none());
        assert!(thread_sample(&vec![b' '; 513]).is_none());
        assert!(thread_sample(b"{}").is_none());
        assert!(thread_sample(b"\xff").is_none());
    }
    #[test]
    fn java_checkpoint_is_a_closed_diagnostic_hint() {
        assert_eq!(
            serde_json::from_slice::<JavaCheckpoint>(br#""metadata_read""#).unwrap(),
            JavaCheckpoint::MetadataRead
        );
        for bytes in [
            br#""private_worker_text""#.as_slice(),
            br#""metadata_read" {}"#,
            br#"{"metadata_read":true}"#,
        ] {
            assert!(serde_json::from_slice::<JavaCheckpoint>(bytes).is_err());
        }
    }
}
