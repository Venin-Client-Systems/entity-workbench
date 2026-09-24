//! One precedence rule for both native assignment paths. No message parsing.
#![cfg_attr(not(any(windows, test)), allow(dead_code))]
use crate::{Error, ResourceLimit, Result};

pub(crate) fn check_cancelled(cancelled: &impl Fn() -> bool) -> Result<()> {
    if cancelled() {
        Err(Error::Cancelled)
    } else {
        Ok(())
    }
}
pub(crate) fn limit(ok: bool, kind: ResourceLimit) -> Result<()> {
    if ok {
        Ok(())
    } else {
        Err(Error::ResourceLimit(kind))
    }
}
pub(crate) fn valid_result(ok: bool, reason: &'static str) -> Result<()> {
    if ok {
        Ok(())
    } else {
        Err(Error::InvalidResult(reason))
    }
}

/// Shared filesystem validators also inspect trusted runtime configuration.
/// Convert their boundary rejections only when examining worker-owned outputs.
pub(crate) fn result_context(error: Error) -> Error {
    match error {
        Error::Blocked(reason) => Error::InvalidResult(reason),
        other => other,
    }
}

pub(crate) fn after_termination<T>(operation: Result<T>, termination: Result<()>) -> Result<T> {
    match termination {
        Ok(()) => operation,
        Err(cause) => Err(Error::TerminationUnverified {
            cause: Box::new(cause),
            prior: operation.err().map(Box::new),
        }),
    }
}

/// The caller disables profile Drop cleanup before calling with `quiescent=false`.
/// Even cancellation and shutdown cannot authorize traversing a possibly live tree.
pub(crate) fn finish_assignment<T>(
    quiescent: bool,
    result: Result<T>,
    cleanup: impl FnOnce() -> Result<()>,
) -> Result<T> {
    if !quiescent {
        return match result {
            Err(error @ Error::TerminationUnverified { .. }) => Err(error),
            other => after_termination(other, Err(Error::Blocked("worker exit not acknowledged"))),
        };
    }
    // Defensive: an inconsistent quiescence flag must never demote this outcome.
    if matches!(result, Err(Error::TerminationUnverified { .. })) {
        return result;
    }
    if cleanup().is_err() {
        return Err(Error::Cleanup {
            prior: result.err().map(Box::new),
        });
    }
    result
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::cell::Cell;

    #[test]
    fn uncertain_exit_preserves_failure_and_never_cleans() {
        for prior in [
            Error::Cancelled,
            Error::ResourceLimit(ResourceLimit::WallTime),
            Error::Exit(1),
        ] {
            let previous = prior.to_string();
            let result: Result<()> = after_termination(
                Err(prior),
                Err(Error::Api {
                    operation: "TerminateWorkerJob",
                    code: 5,
                }),
            );
            let cleaned = Cell::new(false);
            let result = finish_assignment(false, result, || {
                cleaned.set(true);
                Ok(())
            });
            assert!(!cleaned.get());
            let Err(Error::TerminationUnverified { cause, prior }) = result else {
                panic!("termination must govern")
            };
            assert!(matches!(*cause, Error::Api { code: 5, .. }));
            assert_eq!(prior.unwrap().to_string(), previous);
        }
    }
    #[test]
    fn unassigned_uncertain_process_is_retained_and_success_cannot_escape() {
        for result in [
            Ok(()),
            Err(Error::Api {
                operation: "AssignProcessToJob",
                code: 5,
            }),
        ] {
            assert!(matches!(
                finish_assignment(false, result, || panic!("must retain")),
                Err(Error::TerminationUnverified { .. })
            ));
        }
    }
    #[test]
    fn confirmed_exit_cleanup_failure_rejects_success_and_retains_prior() {
        assert!(matches!(
            finish_assignment(true, Ok(()), || Err(Error::Io(
                std::io::ErrorKind::PermissionDenied
            ))),
            Err(Error::Cleanup { prior: None })
        ));
        let result: Result<()> = finish_assignment(true, Err(Error::Cancelled), || {
            Err(Error::Blocked("cleanup test"))
        });
        assert!(
            matches!(result, Err(Error::Cleanup { prior: Some(prior) }) if matches!(*prior, Error::Cancelled))
        );
        assert!(matches!(
            finish_assignment(true, Err::<(), _>(Error::Cancelled), || Ok(())),
            Err(Error::Cancelled)
        ));
    }
    #[test]
    fn unexplained_exit_is_not_resource_exhaustion() {
        assert!(matches!(
            after_termination::<()>(Err(Error::Exit(0xC0000017)), Ok(())),
            Err(Error::Exit(0xC0000017))
        ));
    }

    #[test]
    fn output_context_preserves_resource_and_process_failure_classes() {
        assert!(matches!(
            result_context(Error::Blocked("named data stream rejected")),
            Error::InvalidResult("named data stream rejected")
        ));
        assert!(matches!(
            result_context(Error::ResourceLimit(ResourceLimit::TreeBytes)),
            Error::ResourceLimit(ResourceLimit::TreeBytes)
        ));
        assert!(matches!(result_context(Error::Cancelled), Error::Cancelled));
        assert!(matches!(
            result_context(Error::Api {
                operation: "InspectOutput",
                code: 5
            }),
            Error::Api { code: 5, .. }
        ));
        assert!(matches!(
            result_context(Error::Io(std::io::ErrorKind::PermissionDenied)),
            Error::Io(std::io::ErrorKind::PermissionDenied)
        ));
    }
}
