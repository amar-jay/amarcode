//! Tauri IPC adapters for application operations.
use super::report_failure;
use crate::daemon;
use crate::{app_update, clipboard_files};
use tauri::{ipc::Channel, AppHandle, State};

#[tauri::command]
pub async fn app_check_update(app: AppHandle) -> Result<app_update::AppUpdateCheck, String> {
    app_update::check(&app).await
}

#[tauri::command]
pub async fn app_install_update(
    app: AppHandle,
    on_status: Channel<app_update::AppUpdateStatus>,
) -> Result<(), String> {
    report_failure(
        app_update::install(app, on_status.clone()).await,
        &on_status,
        |error| app_update::AppUpdateStatus::Failed { error },
    )
}

#[tauri::command]
pub async fn prepare_application_uninstall(
    app: AppHandle,
    manager: State<'_, daemon::DaemonManager>,
    confirmed: bool,
    on_status: Channel<daemon::ApplicationCleanupStatus>,
) -> Result<daemon::ApplicationCleanupResult, String> {
    report_failure(
        manager.prepare_uninstall(&app, confirmed, &on_status).await,
        &on_status,
        |error| daemon::ApplicationCleanupStatus::Failed { error },
    )
}

#[tauri::command]
pub fn exit_application(app: AppHandle) {
    app.exit(0);
}

#[tauri::command]
pub async fn read_pasted_files(paths: Vec<String>) -> Result<clipboard_files::PastedFiles, String> {
    // Native clipboard reads, file I/O, and screenshot PNG encoding all stay
    // off the UI and async runtime workers, using a single clipboard handle.
    tauri::async_runtime::spawn_blocking(move || clipboard_files::read_pasted_files(paths))
        .await
        .map_err(|error| error.to_string())?
}
