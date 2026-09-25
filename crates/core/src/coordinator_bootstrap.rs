//! Explicit host resource configuration. No worker is executed during verification.
use super::*;
use crate::engines::python_graph::VerifiedGraphRuntime;

impl JobCoordinator {
    /// Development-only host entry point for the native application's resource root.
    ///
    /// The desktop supplies Tauri's resource directory, never a command or user path.
    /// Only its fixed `engines/python` child can configure the graph executor. Missing,
    /// unsupported or invalid resources leave graph execution unavailable while the
    /// usual document/review coordinator still starts. Cancellation aborts startup.
    /// The default `start` entry point continues to leave graph execution unavailable.
    pub fn start_with_development_app_resources(
        workspace: Workspace,
        concurrency: usize,
        resources: &Path,
        cancel: &CancellationToken,
    ) -> Result<Self> {
        crate::require(
            (1..=2).contains(&concurrency),
            "Processing concurrency must be one or two",
        )?;
        let graph = resolve_graph(resources, cancel, VerifiedGraphRuntime::from_app_engines)?;
        // Install the verified capability before spawning any coordinator thread.
        // Existing ownership recovery/quarantine remains authoritative at bootstrap.
        Self::with_graph_execution(workspace, concurrency, document_executor(), None, graph)
    }
}

fn check_cancel(cancel: &CancellationToken) -> Result<()> {
    if cancel.is_cancelled() {
        return Err(Error::Interrupted(
            "Development graph configuration cancelled".into(),
        ));
    }
    Ok(())
}

// This private seam permits source tests to observe preflight without fabricating
// an opaque runtime or invoking a candidate. Production always supplies the fixed
// verifier above; there is no public verifier/plugin injection API.
fn resolve_graph(
    resources: &Path,
    cancel: &CancellationToken,
    verify: impl FnOnce(&Path, &CancellationToken) -> Result<VerifiedGraphRuntime>,
) -> Result<graph::GraphExecution> {
    check_cancel(cancel)?;
    // Never reinterpret an unavailable application path relative to the process CWD.
    if !resources.is_absolute() {
        return Ok(graph::GraphExecution::Unavailable);
    }
    let verified = verify(&resources.join("engines"), cancel);
    check_cancel(cancel)?;
    match verified {
        Ok(runtime) => Ok(graph::GraphExecution::Configured(runtime)),
        Err(Error::Blocked(_)) => Ok(graph::GraphExecution::Unavailable),
        Err(error) => Err(error),
    }
}

pub(super) fn document_executor() -> Arc<Executor> {
    Arc::new(|runtime, scratch, input, bytes, token| {
        let runtime = runtime
            .ok_or_else(|| Error::Blocked("Packaged processing runtime is unavailable".into()))?;
        match input {
            ProcessingInput::ShortestConnectionPath { .. } => Err(Error::Blocked(
                "Application-local Python graph execution is not activated".into(),
            )),
            ProcessingInput::ParseDocument { .. } => runtime
                .parse_with_cancel(scratch, bytes, token)
                .map(ProcessingOutput::Document),
            ProcessingInput::PdfPageOcr {
                page_number, dpi, ..
            } => runtime
                .ocr_pdf_page_with_cancel(scratch, bytes, *page_number, *dpi, token)
                .map(|output| ProcessingOutput::Pdf(Box::new(output))),
            ProcessingInput::ImageOcrRegions { .. } => runtime
                .ocr_image_regions_with_cancel(scratch, bytes, token)
                .map(|output| ProcessingOutput::ImageRegions(Box::new(output))),
            ProcessingInput::ImageOcr { .. } => runtime
                .ocr_image_with_cancel(scratch, bytes, token)
                .map(|output| ProcessingOutput::Image(Box::new(output))),
        }
    })
}

#[cfg(test)]
#[path = "coordinator_bootstrap_tests.rs"]
mod tests;
