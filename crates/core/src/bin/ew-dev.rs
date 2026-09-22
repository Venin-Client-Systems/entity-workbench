//! Local development harness, excluded from desktop distribution.
use std::{
    io::{self, Read},
    path::PathBuf,
};
use workbench_core::{domain::*, policy::WorkerRequest, store::Workspace};
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
        for (name, value) in [
            (
                "workspace",
                serde_json::to_value(schemars::schema_for!(WorkspaceView))?,
            ),
            (
                "command",
                serde_json::to_value(schemars::schema_for!(Command))?,
            ),
            (
                "worker-request",
                serde_json::to_value(schemars::schema_for!(WorkerRequest))?,
            ),
            (
                "analysis-manifest",
                serde_json::to_value(schemars::schema_for!(AnalysisManifest))?,
            ),
            (
                "identity-comparison",
                serde_json::to_value(schemars::schema_for!(IdentityComparison))?,
            ),
            (
                "source-excerpt",
                serde_json::to_value(schemars::schema_for!(SourceExcerpt))?,
            ),
        ] {
            std::fs::write(
                root.join(format!("{name}.v1.schema.json")),
                serde_json::to_vec_pretty(&value)?,
            )?;
        }
        return Ok(serde_json::json!({"schemas":6}));
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
