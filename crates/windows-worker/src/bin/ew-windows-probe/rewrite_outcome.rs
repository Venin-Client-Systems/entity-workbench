//! Closed interpretation of a synthetic directory DACL rewrite attempt.
use serde::{Deserialize, Serialize};
use workbench_windows_worker::{Error, Output, Result};
const ACCESS_DENIED: u32 = 5;

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "state", rename_all = "snake_case", deny_unknown_fields)]
pub enum RewriteOutcome {
    WriteDacOpenDenied {
        code: u32,
    },
    AppliedAndRestored {
        empty_dacl_verified: bool,
        inspection_denied: bool,
    },
}
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct RewriteObservation {
    pub outcome: RewriteOutcome,
    pub low_label: bool,
    pub dacl_unchanged: bool,
    pub directory_readable: bool,
    pub roundtrip: bool,
}
impl RewriteObservation {
    pub fn positive_control(&self) -> bool {
        matches!(
            self.outcome,
            RewriteOutcome::AppliedAndRestored {
                empty_dacl_verified: true,
                inspection_denied: true
            }
        ) && self.dacl_unchanged
            && self.directory_readable
            && self.roundtrip
    }
}
pub fn accept_denied_rewrite(
    control: &RewriteObservation,
    result: Result<Output>,
) -> Result<RewriteObservation> {
    if !control.positive_control() {
        return Err(Error::Blocked("directory rewrite positive control failed"));
    }
    // No process exit, launch, inspection or cleanup error is interpreted as a
    // successful denial. A complete successful worker result is mandatory.
    let output = result?;
    let observation: RewriteObservation = serde_json::from_slice(&output.bytes)
        .map_err(|_| Error::Blocked("invalid directory rewrite observation"))?;
    if !matches!(
        observation.outcome,
        RewriteOutcome::WriteDacOpenDenied {
            code: ACCESS_DENIED
        }
    ) || !observation.low_label
        || !observation.dacl_unchanged
        || !observation.directory_readable
        || !observation.roundtrip
    {
        return Err(Error::Blocked(
            "directory rewrite prevention was not established",
        ));
    }
    Ok(observation)
}

#[cfg(test)]
mod tests {
    use super::*;
    fn control() -> RewriteObservation {
        RewriteObservation {
            outcome: RewriteOutcome::AppliedAndRestored {
                empty_dacl_verified: true,
                inspection_denied: true,
            },
            low_label: false,
            dacl_unchanged: true,
            directory_readable: true,
            roundtrip: true,
        }
    }
    fn denied() -> RewriteObservation {
        RewriteObservation {
            outcome: RewriteOutcome::WriteDacOpenDenied {
                code: ACCESS_DENIED,
            },
            low_label: true,
            dacl_unchanged: true,
            directory_readable: true,
            roundtrip: true,
        }
    }
    fn output(value: &RewriteObservation) -> Result<Output> {
        Ok(Output {
            bytes: serde_json::to_vec(value).unwrap(),
        })
    }
    #[test]
    fn only_exact_denial_with_control_and_intact_inspectability_qualifies() {
        assert!(accept_denied_rewrite(&control(), output(&denied())).is_ok());
        for changed in 0..6 {
            let mut value = denied();
            match changed {
                0 => value.outcome = RewriteOutcome::WriteDacOpenDenied { code: 6 },
                1 => value.low_label = false,
                2 => value.dacl_unchanged = false,
                3 => value.directory_readable = false,
                4 => value.roundtrip = false,
                _ => value.outcome = control().outcome,
            }
            assert!(accept_denied_rewrite(&control(), output(&value)).is_err());
        }
        let mut failed = control();
        failed.outcome = RewriteOutcome::AppliedAndRestored {
            empty_dacl_verified: true,
            inspection_denied: false,
        };
        assert!(accept_denied_rewrite(&failed, output(&denied())).is_err());
    }
    #[test]
    fn process_and_cleanup_errors_never_count_as_rewrite_denial() {
        for failure in [
            Error::Exit(1),
            Error::Exit(5),
            Error::Io(std::io::ErrorKind::PermissionDenied),
            Error::Api {
                operation: "unrelated",
                code: 5,
            },
            Error::Cleanup {
                prior: Some(Box::new(Error::Exit(1))),
            },
        ] {
            assert!(accept_denied_rewrite(&control(), Err(failure)).is_err());
        }
    }
    #[test]
    fn malformed_and_ambiguous_worker_results_are_rejected() {
        for bytes in [
            b"{}".as_slice(),
            b"{} {}",
            br#"{"state":"write_dac_open_denied","code":5}"#,
        ] {
            assert!(accept_denied_rewrite(
                &control(),
                Ok(Output {
                    bytes: bytes.to_vec()
                })
            )
            .is_err());
        }
        let mut value = serde_json::to_value(denied()).unwrap();
        value["unexpected"] = serde_json::json!(true);
        assert!(accept_denied_rewrite(
            &control(),
            Ok(Output {
                bytes: serde_json::to_vec(&value).unwrap()
            })
        )
        .is_err());
    }
}
