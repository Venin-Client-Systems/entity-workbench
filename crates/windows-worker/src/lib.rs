//! Isolated development launcher; the desktop does not enable it until native evidence exists.
use std::{path::PathBuf, time::Duration};
pub mod java;

#[derive(Debug, thiserror::Error)]
pub enum Error {
    #[error("Windows worker blocked: {0}")]
    Blocked(&'static str),
    #[error("Windows worker API {operation} failed (code {code})")]
    Api { operation: &'static str, code: u32 },
    #[error("Windows worker local I/O failed ({0:?})")]
    Io(std::io::ErrorKind),
    #[error("Windows worker exited unsuccessfully (code {0})")]
    Exit(u32),
    #[error("Windows worker cleanup failed; result rejected; preceding error: {prior:?}")]
    Cleanup { prior: Option<Box<Error>> },
}
impl From<std::io::Error> for Error {
    fn from(value: std::io::Error) -> Self {
        Self::Io(value.kind())
    }
}
pub type Result<T> = std::result::Result<T, Error>;

/// Synthetic probe hints only. Worker-written checkpoints never independently
/// prove isolation or change general worker acceptance; arbitrary text is rejected.
#[derive(Debug, Clone, Copy, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
#[serde(rename_all = "snake_case", deny_unknown_fields)]
pub enum ProbeCheckpoint {
    ChildEntered,
    InputRead,
    TokenOpen,
    TokenQuery,
    TokenClose,
    ConsoleQuery,
    OtherWorkspaceRead,
    OriginalWrite,
    OriginalDacl,
    InputDacl,
    RuntimeDacl,
    AssignedInputRead,
    AssignedInputWrite,
    RuntimeWrite,
    ScratchWrite,
    InheritedHandleSeek,
    InheritedHandleRead,
    ConfinedHandleRead,
    HandleReadReturned,
    RestrictedIdentityFailed { code: u32 },
    RestrictedDescriptorFailed { code: u32 },
    RestrictedDescriptorReady,
    RestrictedDirectoryCreate,
    RestrictedDirectoryCreateFailed { code: u32 },
    RestrictedDirectoryCreated,
    RestrictedOpenFailed { code: u32 },
    RestrictedDaclSet,
    RestrictedDaclSetFailed { code: u32 },
    RestrictedDaclVerifyFailed { code: u32 },
    RestrictedDaclVerified,
    RestrictedLabelVerifyFailed { code: u32 },
    RestrictedResultWrite,
    CallerEnvironment,
    TcpConnect,
    HttpConnect,
    UdpProbe,
    ChildSpawn,
    ResultWrite,
    Completed,
}
#[derive(Debug, Default, serde::Serialize)]
pub struct ProbeDiagnostics {
    /// None means no usable checkpoint was observed, not proof of pre-main exit.
    pub last_worker_checkpoint: Option<ProbeCheckpoint>,
}

/// Only the Rust coordinator may construct this from reviewed adapter definitions.
/// Runtime bytes are copied into a disposable tree; caller-owned ACLs are never changed.
pub struct Request {
    pub runtime: PathBuf,
    pub executable: PathBuf,
    /// `$EW_INPUT`, `$EW_SCRATCH` and `$EW_RUNTIME` are substituted, never shell evaluated.
    pub arguments: Vec<String>,
    pub input: Vec<u8>,
    pub scratch_parent: PathBuf,
    pub wall_time: Duration,
    pub memory_bytes: usize,
}
#[derive(Debug)]
pub struct Output {
    pub bytes: Vec<u8>,
}

fn validate(request: &Request) -> Result<()> {
    if request.input.len() > 16 * 1024 * 1024
        || request.wall_time.is_zero()
        || request.wall_time > Duration::from_secs(60)
        || !(64 * 1024 * 1024..=1024 * 1024 * 1024).contains(&request.memory_bytes)
        || request.arguments.len() > 64
        || request
            .arguments
            .iter()
            .any(|a| a.contains('\0') || a.len() > 4096)
        || request.executable.is_absolute()
        || request
            .executable
            .components()
            .any(|c| !matches!(c, std::path::Component::Normal(_)))
        || request.executable.as_os_str().is_empty()
    {
        return Err(Error::Blocked("invalid bounded request"));
    }
    Ok(())
}

/// Quote one argument using Windows CRT backslash/quote rules. No command shell is used.
#[cfg(any(windows, test))]
fn quote_argument(value: &str) -> String {
    let mut result = String::from("\"");
    let mut slashes = 0;
    for ch in value.chars() {
        if ch == '\\' {
            slashes += 1;
            continue;
        }
        result.extend(std::iter::repeat_n(
            '\\',
            if ch == '"' { slashes * 2 + 1 } else { slashes },
        ));
        result.push(ch);
        slashes = 0;
    }
    result.extend(std::iter::repeat_n('\\', slashes * 2));
    result.push('"');
    result
}

#[cfg(windows)]
mod windows;
#[cfg(windows)]
pub use windows::{
    host_injected_tree_probe, protect_private_tree, run, run_probe, HostInjectedTreeReceipt,
};
#[cfg(not(windows))]
pub fn run(request: &Request, _cancelled: impl Fn() -> bool) -> Result<Output> {
    validate(request)?;
    Err(Error::Blocked(
        "AppContainer is unavailable on this platform",
    ))
}
/// Development harness only: same launch policy, plus a bounded fixed-code hint.
#[cfg(not(windows))]
pub fn run_probe(
    request: &Request,
    cancelled: impl Fn() -> bool,
    diagnostics: &mut ProbeDiagnostics,
) -> Result<Output> {
    *diagnostics = ProbeDiagnostics::default();
    run(request, cancelled)
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn windows_arguments_are_not_shell_commands() {
        assert_eq!(quote_argument(""), "\"\"");
        assert_eq!(quote_argument("a b"), "\"a b\"");
        assert_eq!(quote_argument("end\\"), "\"end\\\\\"");
        assert_eq!(quote_argument("a\\\"b"), "\"a\\\\\\\"b\"");
        assert_eq!(quote_argument("$(x)&y"), "\"$(x)&y\"");
    }
    #[test]
    fn paths_and_limits_are_bounded_before_platform_launch() {
        let mut request = Request {
            runtime: "runtime".into(),
            executable: "../escape".into(),
            arguments: vec![],
            input: vec![],
            scratch_parent: "scratch".into(),
            wall_time: Duration::from_secs(1),
            memory_bytes: 128 * 1024 * 1024,
        };
        assert!(validate(&request).is_err());
        request.executable = "worker.exe".into();
        assert!(validate(&request).is_ok());
        request.arguments.push("hidden\0argument".into());
        assert!(validate(&request).is_err());
    }
    #[test]
    fn io_errors_do_not_publish_paths() {
        let error = Error::from(std::io::Error::new(
            std::io::ErrorKind::PermissionDenied,
            "sensitive-owned-path",
        ));
        assert!(!error.to_string().contains("sensitive-owned-path"));
        assert!(error.to_string().contains("PermissionDenied"));
    }
    #[test]
    fn probe_checkpoints_never_accept_arbitrary_worker_diagnostics() {
        assert_eq!(
            serde_json::from_slice::<ProbeCheckpoint>(b"\"token_query\"").unwrap(),
            ProbeCheckpoint::TokenQuery
        );
        for checkpoint in [
            ProbeCheckpoint::RestrictedIdentityFailed { code: u32::MAX },
            ProbeCheckpoint::RestrictedOpenFailed { code: u32::MAX },
            ProbeCheckpoint::RestrictedDaclSetFailed { code: u32::MAX },
            ProbeCheckpoint::RestrictedDaclVerifyFailed { code: u32::MAX },
            ProbeCheckpoint::RestrictedLabelVerifyFailed { code: u32::MAX },
            ProbeCheckpoint::RestrictedDescriptorFailed { code: u32::MAX },
            ProbeCheckpoint::RestrictedDirectoryCreateFailed { code: u32::MAX },
        ] {
            let bytes = serde_json::to_vec(&checkpoint).unwrap();
            assert!(bytes.len() <= 64);
            assert_eq!(
                serde_json::from_slice::<ProbeCheckpoint>(&bytes).unwrap(),
                checkpoint
            );
        }
        assert!(serde_json::from_slice::<ProbeCheckpoint>(
            br#"{"restricted_directory_create_failed":{"code":5,"path":"untrusted"}}"#
        )
        .is_err());
        for bytes in [
            b"\"private-path-or-document-content\"".as_slice(),
            b"{}",
            b"\"token_query\" {}",
        ] {
            assert!(serde_json::from_slice::<ProbeCheckpoint>(bytes).is_err());
        }
    }
    #[cfg(not(windows))]
    #[test]
    fn unsupported_platform_has_no_unconfined_fallback() {
        let request = Request {
            runtime: "unused".into(),
            executable: "worker.exe".into(),
            arguments: vec![],
            input: vec![],
            scratch_parent: "unused".into(),
            wall_time: Duration::from_secs(1),
            memory_bytes: 128 * 1024 * 1024,
        };
        assert!(matches!(
            run(&request, || false),
            Err(Error::Blocked(
                "AppContainer is unavailable on this platform"
            ))
        ));
    }
}
