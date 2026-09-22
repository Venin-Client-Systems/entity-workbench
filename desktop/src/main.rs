#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]
use std::sync::{Arc, Mutex};
use tauri::Manager;
use workbench_core::{domain::Command, store::Workspace};

#[tauri::command]
async fn workbench(
    command: Command,
    state: tauri::State<'_, Arc<Mutex<Workspace>>>,
) -> Result<serde_json::Value, String> {
    let workspace = state.inner().clone();
    tauri::async_runtime::spawn_blocking(move || {
        workspace
            .lock()
            .map_err(|_| "Workspace is unavailable".to_string())?
            .dispatch(command)
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
            app.manage(Arc::new(Mutex::new(workspace)));
            Ok(())
        })
        .invoke_handler(tauri::generate_handler![workbench])
        .run(tauri::generate_context!())
        .expect("Unable to launch Entity Workbench");
}
