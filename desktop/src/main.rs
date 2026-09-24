#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]
use std::sync::Arc;
use tauri::Manager;
use workbench_core::{coordinator::JobCoordinator, domain::Command, store::Workspace};

#[tauri::command]
async fn workbench(
    command: Command,
    state: tauri::State<'_, Arc<JobCoordinator>>,
) -> Result<serde_json::Value, String> {
    let workspace = state.inner().clone();
    tauri::async_runtime::spawn_blocking(move || {
        workspace
            .dispatch_presentation(command)
            .map_err(|e| e.to_string())
    })
    .await
    .map_err(|_| "Workspace task interrupted".to_string())?
}
fn main() {
    tauri::Builder::default()
        .setup(|app| {
            let root = app
                .path()
                .app_local_data_dir()?
                .join("workspaces")
                .join("default");
            let mut workspace = Workspace::open(root)?;
            workspace.attach_runtime(workbench_core::engines::Runtime {
                root: app.path().resource_dir()?.join("engines"),
            });
            app.manage(Arc::new(JobCoordinator::start(workspace, 2)?));
            Ok(())
        })
        .invoke_handler(tauri::generate_handler![workbench])
        .build(tauri::generate_context!())
        .expect("Unable to launch Entity Workbench")
        .run(|app, event| {
            if matches!(
                event,
                tauri::RunEvent::ExitRequested { .. } | tauri::RunEvent::Exit
            ) {
                if let Some(coordinator) = app.try_state::<Arc<JobCoordinator>>() {
                    if coordinator.shutdown().is_err() {
                        // No case data or worker output enters the diagnostic. Durable running
                        // records remain recoverable as interrupted at the next launch.
                        eprintln!("Document worker shutdown did not complete normally");
                    }
                }
            }
        });
}
