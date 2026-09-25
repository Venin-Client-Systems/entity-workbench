//! Interpret one isolated synthetic handle attempt, never general worker failures.
use serde::{Deserialize, Serialize};
use workbench_windows_worker::{Error, Output, ProbeCheckpoint, ProbeDiagnostics, Result};

#[derive(Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub(super) struct HandleObservation {
    pub app_container: bool,
    pub sentinel_read: bool,
}

#[derive(Debug, Serialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub(super) enum HandleOutcome {
    ReadDenied,
    TerminatedOnInvalidReference,
}

/// The caller must first establish the matching inherited-handle positive
/// control and all other permission observations. The launcher independently
/// verifies exact package identity, zero capabilities and job assignment before
/// resume. Only a direct Exit can qualify; launch/termination/cleanup failures do
/// not become a successful denial merely because an untrusted hint exists.
pub(super) fn accept_handle_outcome(
    positive_control_read: bool,
    other_permissions_completed: bool,
    result: Result<Output>,
    diagnostics: &ProbeDiagnostics,
) -> Result<HandleOutcome> {
    if !positive_control_read || !other_permissions_completed {
        return Err(Error::Blocked("handle probe controls are incomplete"));
    }
    match result {
        Ok(output) => {
            let observation: HandleObservation = serde_json::from_slice(&output.bytes)
                .map_err(|_| Error::Blocked("invalid handle probe result"))?;
            if !observation.app_container
                || observation.sentinel_read
                || diagnostics.last_worker_checkpoint != Some(ProbeCheckpoint::Completed)
            {
                return Err(Error::Blocked(
                    "inherited-handle boundary was not established",
                ));
            }
            Ok(HandleOutcome::ReadDenied)
        }
        Err(Error::Exit(0xc0000008))
            if diagnostics.last_worker_checkpoint == Some(ProbeCheckpoint::ConfinedHandleRead) =>
        {
            Ok(HandleOutcome::TerminatedOnInvalidReference)
        }
        Err(error) => Err(error),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    fn diagnostic(checkpoint: ProbeCheckpoint) -> ProbeDiagnostics {
        ProbeDiagnostics {
            last_worker_checkpoint: Some(checkpoint),
        }
    }
    fn denied() -> Result<Output> {
        Ok(Output {
            bytes: br#"{"app_container":true,"sentinel_read":false}"#.to_vec(),
        })
    }

    #[test]
    fn only_proven_read_denial_or_exact_invalid_reference_termination_qualifies() {
        assert_eq!(
            accept_handle_outcome(
                true,
                true,
                denied(),
                &diagnostic(ProbeCheckpoint::Completed)
            )
            .unwrap(),
            HandleOutcome::ReadDenied
        );
        assert_eq!(
            accept_handle_outcome(
                true,
                true,
                Err(Error::Exit(0xc0000008)),
                &diagnostic(ProbeCheckpoint::ConfinedHandleRead)
            )
            .unwrap(),
            HandleOutcome::TerminatedOnInvalidReference
        );
        for checkpoint in [
            ProbeCheckpoint::ChildEntered,
            ProbeCheckpoint::InheritedHandleSeek,
            ProbeCheckpoint::InheritedHandleRead,
            ProbeCheckpoint::HandleReadReturned,
            ProbeCheckpoint::Completed,
        ] {
            assert!(accept_handle_outcome(
                true,
                true,
                Err(Error::Exit(0xc0000008)),
                &diagnostic(checkpoint)
            )
            .is_err());
        }
        assert!(accept_handle_outcome(
            true,
            true,
            Err(Error::Exit(0xc0000142)),
            &diagnostic(ProbeCheckpoint::ConfinedHandleRead)
        )
        .is_err());
        assert!(accept_handle_outcome(
            true,
            true,
            Err(Error::Cleanup {
                prior: Some(Box::new(Error::Exit(0xc0000008)))
            }),
            &diagnostic(ProbeCheckpoint::ConfinedHandleRead)
        )
        .is_err());
    }

    #[test]
    fn missing_controls_or_successful_sentinel_read_fail_the_probe() {
        for (control, permissions) in [(false, true), (true, false), (false, false)] {
            assert!(accept_handle_outcome(
                control,
                permissions,
                denied(),
                &diagnostic(ProbeCheckpoint::Completed)
            )
            .is_err());
        }
        for bytes in [
            br#"{"app_container":false,"sentinel_read":false}"#.as_slice(),
            br#"{"app_container":true,"sentinel_read":true}"#,
            br#"{"app_container":true,"sentinel_read":false,"extra":true}"#,
        ] {
            assert!(accept_handle_outcome(
                true,
                true,
                Ok(Output {
                    bytes: bytes.to_vec()
                }),
                &diagnostic(ProbeCheckpoint::Completed)
            )
            .is_err());
        }
        assert!(accept_handle_outcome(true, true, denied(), &ProbeDiagnostics::default()).is_err());
    }
}
