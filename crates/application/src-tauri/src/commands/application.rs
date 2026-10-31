//! Tauri IPC adapters for application operations.
use super::report_failure;
use crate::app_update;
use crate::daemon;
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
pub async fn remove_local_daemon(
    app: AppHandle,
    manager: State<'_, daemon::DaemonManager>,
    confirmation: String,
    on_status: Channel<daemon::DaemonRemovalStatus>,
) -> Result<daemon::DaemonRemovalResult, String> {
    report_failure(
        manager
            .remove_local_daemon(&app, &confirmation, &on_status)
            .await,
        &on_status,
        |error| daemon::DaemonRemovalStatus::Failed { error },
    )
}

#[tauri::command]
pub fn exit_application(app: AppHandle) {
    app.exit(0);
}
