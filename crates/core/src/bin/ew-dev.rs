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
        for (name, version, value) in [
            (
                "workspace",
                3,
                serde_json::to_value(schemars::schema_for!(WorkspaceView))?,
            ),
            (
                "command",
                5,
                serde_json::to_value(schemars::schema_for!(Command))?,
            ),
            (
                "processing-job",
                1,
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
        ] {
            std::fs::write(
                root.join(format!("{name}.v{version}.schema.json")),
                serde_json::to_vec_pretty(&value)?,
            )?;
        }
        return Ok(serde_json::json!({"schemas":12}));
    }
    workbench_core::require(!arg.is_empty(), "Provide a development workspace path")?;
    let mut input = String::new();
    io::stdin()
        .take(40 * 1024 * 1024)
        .read_to_string(&mut input)?;
    let mut workspace = Workspace::open(arg)?;
    if let Some(runtime) = std::env::args().nth(2) {
        workspace.attach_runtime(workbench_core::engines::Runtime {
            root: runtime.into(),
        });
    }
    workspace.dispatch(serde_json::from_str(&input)?)
}
