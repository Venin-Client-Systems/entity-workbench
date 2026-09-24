//! Fixed diagnostic fields only; worker-controlled raw logs never enter receipts.
use serde::Serialize;

#[derive(Debug, Default, Serialize)]
pub(crate) struct FailureDiagnostics {
    pub captured_after_termination: bool,
    pub scratch: Option<TreeCounts>,
    pub profile: Option<TreeCounts>,
    pub fatal_header: Option<FatalHeader>,
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
}
pub(crate) fn fallback_name(name: &str) -> bool {
    name.strip_prefix("hs_err_pid")
        .and_then(|s| s.strip_suffix(".log"))
        .is_some_and(|digits| {
            !digits.is_empty() && digits.len() <= 10 && digits.bytes().all(|b| b.is_ascii_digit())
        })
}
pub(crate) fn fatal_header(bytes: &[u8]) -> Option<FatalHeader> {
    // Only the first 32 lines of an already bounded file are inspected. Emit
    // neither the source text, addresses, PID, thread IDs, paths nor environment.
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
        if content.starts_with("Internal Error (") || content.starts_with("Out of Memory Error (") {
            for component in [
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
            ] {
                if let Some((_, suffix)) = content.split_once(&format!("{component}:")) {
                    header.source_component = Some(component);
                    if let Some((line, _)) = suffix.split_once(')') {
                        if line.len() <= 6 && line.bytes().all(|b| b.is_ascii_digit()) {
                            header.source_line = line.parse().ok();
                        }
                    }
                }
            }
        }
        if content.starts_with("EXCEPTION_") {
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
        let text = b"# A fatal error has been detected by the Java Runtime Environment:\n# Internal Error (PRIVATE/os_windows.cpp:123), pid=PRIVATE\n# arbitrary private reason\n";
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
    }
}
