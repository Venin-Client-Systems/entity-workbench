//! Local development harness, excluded from desktop distribution.
use std::{
    io::{self, Read},
    path::PathBuf,
};
use workbench_core::{
    collection_receipt::CollectionReceipt,
    domain::*,
    policy::WorkerRequest,
    statements::{StatementMapping, StatementPreview, StatementSample},
    store::Workspace,
};
fn main() {
    let result = run();
    match result {
        Ok(value) => println!("{}", value),
        Err(error) => {
            println!("{}", serde_json::json!({"error":error.to_string()}));
            std::process::exit(1);
        }
    }
}
fn run() -> workbench_core::Result<serde_json::Value> {
    let arg = std::env::args().nth(1).unwrap_or_default();
    #[cfg(debug_assertions)]
    if arg == "seed-collection-review" {
        let path = std::env::args().nth(2).ok_or_else(|| {
            workbench_core::Error::Validation("Provide a fresh synthetic workspace path".into())
        })?;
        let mut workspace = Workspace::open(path)?;
        workspace.seed_collection_review()?;
        return Ok(serde_json::to_value(workspace.view()?)?);
    }
    #[cfg(debug_assertions)]
    if arg == "seed-pdf-processing-review" {
        let path = std::env::args().nth(2).ok_or_else(|| {
            workbench_core::Error::Validation("Provide a fresh synthetic workspace path".into())
        })?;
        let mut workspace = Workspace::open(path)?;
        workspace.seed_pdf_processing_review()?;
        return Ok(serde_json::to_value(workspace.view()?)?);
    }
    #[cfg(debug_assertions)]
    if arg == "seed-image-processing-review" {
        let path = std::env::args().nth(2).ok_or_else(|| {
            workbench_core::Error::Validation("Provide a fresh synthetic workspace path".into())
        })?;
        let mut workspace = Workspace::open(path)?;
        workspace.seed_image_processing_review()?;
        return Ok(serde_json::to_value(workspace.view()?)?);
    }
    #[cfg(debug_assertions)]
    if arg == "seed-processing-review" || arg == "seed-processing-recovery-review" {
        let path = std::env::args().nth(2).ok_or_else(|| {
            workbench_core::Error::Validation("Provide a fresh synthetic workspace path".into())
        })?;
        let mut workspace = Workspace::open(path)?;
        if arg == "seed-processing-recovery-review" {
            workspace.seed_processing_recovery_review()?;
        } else {
            workspace.seed_processing_review()?;
        }
        return Ok(serde_json::to_value(workspace.view()?)?);
    }
    if arg == "schemas" {
        let root = PathBuf::from("schemas");
        std::fs::create_dir_all(&root)?;
        // Prior command/job schema files are immutable history. Emit only current versions;
        // extraction v1 retains its parse-only shape and is checked against its saved snapshot.
        let schemas = [
            (
                "transaction-page-request",
                1,
                serde_json::to_value(schemars::schema_for!(
                    workbench_core::transaction_page::TransactionPageRequest
                ))?,
            ),
            (
                "transaction-page",
                1,
                serde_json::to_value(schemars::schema_for!(
                    workbench_core::transaction_page::TransactionPage
                ))?,
            ),
            (
                "workspace-presentation",
                1,
                serde_json::to_value(schemars::schema_for!(WorkspaceView<ReportMetadata>))?,
            ),
            (
                "workspace",
                3,
                serde_json::to_value(schemars::schema_for!(WorkspaceView))?,
            ),
            (
                "command",
                12,
                serde_json::to_value(schemars::schema_for!(Command))?,
            ),
            (
                "transaction-comparison-request",
                1,
                serde_json::to_value(schemars::schema_for!(
                    workbench_core::transaction_comparison::TransactionComparisonRequest
                ))?,
            ),
            (
                "transaction-comparison",
                1,
                serde_json::to_value(schemars::schema_for!(
                    workbench_core::transaction_comparison::TransactionComparison
                ))?,
            ),
            (
                "transaction-analysis-request",
                1,
                serde_json::to_value(schemars::schema_for!(
                    workbench_core::transaction_analysis::TransactionAnalysisRequest
                ))?,
            ),
            (
                "transaction-analysis",
                1,
                serde_json::to_value(schemars::schema_for!(
                    workbench_core::transaction_analysis::TransactionAnalysis
                ))?,
            ),
            (
                "processing-job",
                4,
                serde_json::to_value(schemars::schema_for!(
                    workbench_core::processing::ProcessingJob
                ))?,
            ),
            (
                "extraction",
                1,
                serde_json::to_value(schemars::schema_for!(
                    workbench_core::processing::ExtractionRecord
                ))?,
            ),
            (
                "pdf-extraction",
                1,
                serde_json::to_value(schemars::schema_for!(
                    workbench_core::processing::PdfExtractionRecord
                ))?,
            ),
            (
                "image-region-extraction",
                1,
                serde_json::to_value(schemars::schema_for!(
                    workbench_core::processing::ImageRegionExtractionRecord
                ))?,
            ),
            (
                "image-region-result",
                1,
                serde_json::to_value(schemars::schema_for!(
                    workbench_core::processing::ImageRegionResult
                ))?,
            ),
            (
                "image-region-inspection",
                1,
                serde_json::to_value(schemars::schema_for!(
                    workbench_core::processing::ImageRegionInspection
                ))?,
            ),
            (
                "image-extraction",
                1,
                serde_json::to_value(schemars::schema_for!(
                    workbench_core::processing::ImageExtractionRecord
                ))?,
            ),
            (
                "collection-receipt",
                1,
                serde_json::to_value(schemars::schema_for!(CollectionReceipt))?,
            ),
            (
                "worker-request",
                1,
                serde_json::to_value(schemars::schema_for!(WorkerRequest))?,
            ),
            (
                "analysis-manifest",
                1,
                serde_json::to_value(schemars::schema_for!(AnalysisManifest))?,
            ),
            (
                "identity-comparison",
                1,
                serde_json::to_value(schemars::schema_for!(IdentityComparison))?,
            ),
            (
                "source-excerpt",
                1,
                serde_json::to_value(schemars::schema_for!(SourceExcerpt))?,
            ),
            (
                "statement-mapping",
                1,
                serde_json::to_value(schemars::schema_for!(StatementMapping))?,
            ),
            (
                "statement-sample",
                1,
                serde_json::to_value(schemars::schema_for!(StatementSample))?,
            ),
            (
                "statement-preview",
                1,
                serde_json::to_value(schemars::schema_for!(StatementPreview))?,
            ),
        ];
        for (name, version, value) in &schemas {
            std::fs::write(
                root.join(format!("{name}.v{version}.schema.json")),
                serde_json::to_vec_pretty(&value)?,
            )?;
        }
        return Ok(serde_json::json!({"schemas":schemas.len()}));
    }
    workbench_core::require(!arg.is_empty(), "Provide a development workspace path")?;
    let mut input = String::new();
    io::stdin()
        .take(40 * 1024 * 1024)
        .read_to_string(&mut input)?;
    let mut workspace = Workspace::open(arg)?;
    let mut extra = std::env::args().skip(2);
    let next = extra.next();
    let presentation = next.as_deref() == Some("--presentation");
    let runtime = if presentation { extra.next() } else { next };
    workbench_core::require(extra.next().is_none(), "Unexpected development argument")?;
    if let Some(runtime) = runtime {
        workspace.attach_runtime(workbench_core::engines::Runtime {
            root: runtime.into(),
        });
    }
    let command = serde_json::from_str(&input)?;
    if presentation {
        workspace.dispatch_presentation(command)
    } else {
        workspace.dispatch(command)
    }
}
