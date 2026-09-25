#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]
mod exports;
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
            .dispatch_summary(command)
            .map_err(|e| e.to_string())
    })
    .await
    .map_err(|_| "Workspace task interrupted".to_string())?
}
#[tauri::command]
async fn image_region_raster(
    extraction_id: String,
    state: tauri::State<'_, Arc<JobCoordinator>>,
) -> Result<tauri::ipc::Response, String> {
    if extraction_id.len() != 64
        || !extraction_id
            .bytes()
            .all(|b| b.is_ascii_digit() || (b'a'..=b'f').contains(&b))
    {
        return Err("Invalid extraction identity".into());
    }
    let coordinator = state.inner().clone();
    tauri::async_runtime::spawn_blocking(move || {
        coordinator
            .read_image_region_raster(&extraction_id)
            .map(tauri::ipc::Response::new)
            .map_err(|_| "Retained raster could not be verified or read".to_owned())
    })
    .await
    .map_err(|_| "Raster read interrupted".to_owned())?
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
            let resources = app.path().resource_dir()?;
            workspace.attach_runtime(workbench_core::engines::Runtime {
                root: resources.join("engines"),
            });
            #[cfg(feature = "development-graph-runtime")]
            let coordinator = JobCoordinator::start_with_development_app_resources(
                workspace,
                2,
                &resources,
                &workbench_core::engines::CancellationToken::default(),
            )?;
            #[cfg(not(feature = "development-graph-runtime"))]
            let coordinator = JobCoordinator::start(workspace, 2)?;
            app.manage(Arc::new(coordinator));
            tauri::WebviewWindowBuilder::from_config(app, &app.config().app.windows[0])?
                .on_download(|_, _| false)
                .build()?;
            Ok(())
        })
        .invoke_handler(tauri::generate_handler![
            workbench,
            image_region_raster,
            exports::prepare_native_export,
            exports::commit_native_export,
            exports::discard_native_export
        ])
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
