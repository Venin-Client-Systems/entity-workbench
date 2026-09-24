use super::*;
use std::net::IpAddr;

#[derive(Debug)]
pub(crate) struct ResolvedCandidates {
    pub addresses: Vec<SocketAddr>,
    pub method: &'static str,
    /// Always false: an OS candidate snapshot does not prove a complete DNS RRset.
    pub authoritative_complete_set: bool,
}
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) struct ResolverUncertainty {
    pub method: &'static str,
    pub caller_context: CallerContextState,
}
#[derive(Debug)]
pub(super) enum ResolverFailure {
    Stopped(StopReason),
    /// Provider quiescence was not established. The caller context has a separate
    /// lifetime state; either variant quarantines the entire transport process.
    QuiescenceUnverified(ResolverUncertainty),
}
impl From<StopReason> for ResolverFailure {
    fn from(reason: StopReason) -> Self {
        Self::Stopped(reason)
    }
}
pub(super) trait Resolver {
    fn resolve(
        &self,
        host: &str,
        window: &mut ExecutionWindow,
        cancellation: &CancellationToken,
    ) -> Result<ResolvedCandidates, ResolverFailure>;
}
pub(super) struct NativeResolver;

/// GetAddrInfoExOverlappedResult documents WSAEINPROGRESS (10036). The two
/// overlapped pending codes are also refused conservatively, never freed early.
#[cfg(any(target_os = "windows", test))]
pub(super) fn windows_result_pending(code: i32) -> bool {
    matches!(code, 10036 | 996 | 997)
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn windows_in_progress_does_not_authorize_freeing_overlapped_storage() {
        for code in [10036, 996, 997] {
            assert!(windows_result_pending(code));
        }
        for code in [0, 10111, 10022] {
            assert!(!windows_result_pending(code));
        }
    }
}
impl Resolver for NativeResolver {
    fn resolve(
        &self,
        host: &str,
        window: &mut ExecutionWindow,
        cancellation: &CancellationToken,
    ) -> Result<ResolvedCandidates, ResolverFailure> {
        window.check(cancellation)?;
        if let Ok(ip) = host.trim_matches(['[', ']']).parse::<IpAddr>() {
            return Ok(ResolvedCandidates {
                addresses: vec![SocketAddr::new(ip, 443)],
                method: "literal_ip",
                authoritative_complete_set: false,
            });
        }
        #[cfg(target_os = "windows")]
        {
            native(host, window, cancellation)
        }
        #[cfg(not(target_os = "windows"))]
        {
            native(host, window, cancellation).map_err(Into::into)
        }
    }
}

#[cfg(target_os = "macos")]
#[path = "resolver_macos.rs"]
mod platform;
#[cfg(target_os = "windows")]
#[path = "resolver_windows.rs"]
mod platform;
#[cfg(any(target_os = "macos", target_os = "windows"))]
use platform::native;
#[cfg(not(any(target_os = "macos", target_os = "windows")))]
fn native(
    _: &str,
    _: &mut ExecutionWindow,
    _: &CancellationToken,
) -> Result<ResolvedCandidates, StopReason> {
    Err(StopReason::ResolverUnavailable)
}
