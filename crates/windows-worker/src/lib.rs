//! Isolated development launcher; the desktop does not enable it until native evidence exists.
use std::{path::PathBuf, time::Duration};

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
pub use windows::{protect_private_tree, run};
#[cfg(not(windows))]
pub fn run(request: &Request, _cancelled: impl Fn() -> bool) -> Result<Output> {
    validate(request)?;
    Err(Error::Blocked(
        "AppContainer is unavailable on this platform",
    ))
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
