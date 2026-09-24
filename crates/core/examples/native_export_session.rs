//! Developer-only line transport for real-core UI lifecycle tests; not shipped as a desktop command.
use serde::Deserialize;
use serde_json::{json, Value};
use std::io::{self, BufRead, Read, Write};
use workbench_core::{
    coordinator::JobCoordinator, domain::Command, local_export::NativeExportRequest,
    store::Workspace, Result,
};
#[derive(Deserialize)]
#[serde(tag = "method", rename_all = "snake_case", deny_unknown_fields)]
enum Request {
    Workbench {
        command: Box<Command>,
    },
    PrepareNativeExport {
        request: NativeExportRequest,
    },
    CommitNativeExport {
        ticket: String,
        expected_sha256: String,
        expected_bytes: u64,
    },
    DiscardNativeExport {
        ticket: String,
    },
    Shutdown,
}
fn dispatch(coordinator: &JobCoordinator, request: Request) -> Result<Value> {
    match request {
        Request::Workbench { command } => coordinator.dispatch_summary(*command),
        Request::PrepareNativeExport { request } => Ok(serde_json::to_value(
            coordinator.prepare_native_export(request)?,
        )?),
        Request::CommitNativeExport {
            ticket,
            expected_sha256,
            expected_bytes,
        } => Ok(serde_json::to_value(coordinator.commit_native_export(
            &ticket,
            &expected_sha256,
            expected_bytes,
        )?)?),
        Request::DiscardNativeExport { ticket } => Ok(serde_json::to_value(
            coordinator.discard_native_export(&ticket)?,
        )?),
        Request::Shutdown => {
            coordinator.shutdown()?;
            Ok(json!({"stopped":true}))
        }
    }
}
fn main() -> Result<()> {
    let path = std::env::args().nth(1).expect("synthetic workspace path");
    let coordinator = JobCoordinator::start(Workspace::open(path)?, 1)?;
    let mut input = io::stdin().lock();
    loop {
        let mut line = Vec::new();
        if (&mut input)
            .take(1024 * 1024 + 1)
            .read_until(b'\n', &mut line)?
            == 0
        {
            break;
        }
        workbench_core::require(line.len() <= 1024 * 1024, "Test request exceeds 1 MiB")?;
        let response = match serde_json::from_slice::<Request>(&line)
            .map_err(Into::into)
            .and_then(|r| dispatch(&coordinator, r))
        {
            Ok(value) => json!({"ok":value}),
            Err(error) => json!({"error":error.to_string()}),
        };
        println!("{response}");
        io::stdout().flush()?;
    }
    coordinator.shutdown()
}
