//! Typed native saves bypass WebKit's download destination / sandbox-extension path.
use std::sync::Arc;
use workbench_core::{coordinator::JobCoordinator, local_export::*};

#[tauri::command]
pub async fn prepare_native_export(
    request: NativeExportRequest,
    state: tauri::State<'_, Arc<JobCoordinator>>,
) -> Result<PreparedExport, String> {
    let coordinator = state.inner().clone();
    tauri::async_runtime::spawn_blocking(move || {
        coordinator
            .prepare_native_export(request)
            .map_err(|e| e.to_string())
    })
    .await
    .map_err(|_| "Export preparation interrupted".to_owned())?
}
#[tauri::command]
pub async fn commit_native_export(
    ticket: String,
    expected_sha256: String,
    expected_bytes: u64,
    state: tauri::State<'_, Arc<JobCoordinator>>,
) -> Result<SavedExportReceipt, String> {
    let coordinator = state.inner().clone();
    tauri::async_runtime::spawn_blocking(move || {
        coordinator
            .commit_native_export(&ticket, &expected_sha256, expected_bytes)
            .map_err(|e| e.to_string())
    })
    .await
    .map_err(|_| {
        "Export commit interrupted; retry the same ticket to verify completion".to_owned()
    })?
}
#[tauri::command]
pub async fn discard_native_export(
    ticket: String,
    state: tauri::State<'_, Arc<JobCoordinator>>,
) -> Result<DiscardedExport, String> {
    let coordinator = state.inner().clone();
    tauri::async_runtime::spawn_blocking(move || {
        coordinator
            .discard_native_export(&ticket)
            .map_err(|e| e.to_string())
    })
    .await
    .map_err(|_| "Export staging cleanup interrupted".to_owned())?
}
