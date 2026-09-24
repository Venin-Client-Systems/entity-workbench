//! Fixed AppContainer parser recipe. Canonical publication stays in the job coordinator.
use super::{validate_result, ParseResult, MAX_RESULT_BYTES};
use crate::{engines::CancellationToken, Error, Result};
use sha2::{Digest, Sha256};
use std::path::Path;
use workbench_windows_worker::{java, Error as WorkerError};

pub(super) fn parse(
    runtime: &Path,
    scratch: &Path,
    original: &[u8],
    cancellation: &CancellationToken,
) -> Result<ParseResult> {
    execute_with(
        runtime,
        scratch,
        original,
        cancellation,
        |root, area, job, token| java::execute(root, area, job, || token.is_cancelled()),
    )
}

fn execute_with(
    runtime: &Path,
    scratch: &Path,
    original: &[u8],
    cancellation: &CancellationToken,
    execute: impl FnOnce(
        &Path,
        &Path,
        &java::Job,
        &CancellationToken,
    ) -> workbench_windows_worker::Result<java::JavaOutput>,
) -> Result<ParseResult> {
    if cancellation.is_cancelled() {
        return Err(Error::Interrupted(
            "Document parsing cancelled before preparation".into(),
        ));
    }
    crate::require(
        original.len() <= crate::policy::MAX_IMPORT_BYTES,
        "Original exceeds parser input limit",
    )?;
    let job = java::Job::parse(original.to_vec()).map_err(classify)?;
    let output = execute(&runtime.join("parser"), scratch, &job, cancellation).map_err(classify)?;
    // execute returns only after confirmed exit and successful cleanup. Its error
    // contract preserves unverified termination even when cancellation is set.
    let result = accept(&job, original, output)?;
    if cancellation.is_cancelled() {
        return Err(Error::Interrupted(
            "Document parsing cancelled after worker exit".into(),
        ));
    }
    Ok(result)
}

fn accept(job: &java::Job, original: &[u8], output: java::JavaOutput) -> Result<ParseResult> {
    let invalid = || {
        Error::InvalidWorkerResult(
            "Windows parser output is not bound to its assigned request and source".into(),
        )
    };
    if output.job_id != job.id()
        || output.index.is_some()
        || output.bytes.len() as u64 > MAX_RESULT_BYTES
        || output.output_sha256 != format!("{:x}", Sha256::digest(&output.bytes))
    {
        return Err(invalid());
    }
    let result: ParseResult = serde_json::from_slice(&output.bytes).map_err(|_| invalid())?;
    if result.job_id != job.id().to_string() {
        return Err(invalid());
    }
    validate_result(
        &result,
        &format!("{:x}", Sha256::digest(original)),
        original.len() as u64,
    )
    .map_err(|_| invalid())?;
    Ok(result)
}

fn classify(error: WorkerError) -> Error {
    match error {
        WorkerError::TerminationUnverified { .. } => Error::TerminationUnverified(
            "Windows worker exit could not be confirmed; its assignment is retained".into(),
        ),
        WorkerError::Cleanup { .. } => {
            Error::Cleanup("Windows worker assignment cleanup failed after verified exit".into())
        }
        WorkerError::Cancelled => {
            Error::Interrupted("Windows worker cancelled after verified exit".into())
        }
        WorkerError::ResourceLimit(_) => {
            Error::QuotaExhausted("Windows worker reached an observed resource limit".into())
        }
        WorkerError::InvalidResult(_) => Error::InvalidWorkerResult(
            "Windows worker returned an invalid or unbound result".into(),
        ),
        WorkerError::Blocked(_) => Error::Blocked(
            "The verified Windows parser runtime or confinement configuration is unavailable"
                .into(),
        ),
        WorkerError::Api { .. } | WorkerError::Io(_) | WorkerError::Exit(_) => {
            Error::Validation("Windows document worker failed".into())
        }
    }
}

#[cfg(test)]
mod tests;
