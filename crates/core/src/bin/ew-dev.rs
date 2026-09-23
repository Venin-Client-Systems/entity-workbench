//! Local development harness, excluded from desktop distribution.
use std::{
    io::{self, Read},
    path::PathBuf,
};
use workbench_core::{
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
                3,
                serde_json::to_value(schemars::schema_for!(Command))?,
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
        return Ok(serde_json::json!({"schemas":9}));
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
