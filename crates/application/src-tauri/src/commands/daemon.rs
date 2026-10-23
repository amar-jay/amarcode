//! Tauri IPC adapters for daemon operations.
use super::report_failure;
use crate::daemon;
use crate::daemon::DaemonClient;
use amarcode_protocol::rpc::{
    DaemonConfigResult, HealthResult, SetDaemonConfigParams, VacuumDatabaseResult, VersionResult,
};
use tauri::{ipc::Channel, AppHandle, State};

#[tauri::command]
pub async fn daemon_bootstrap(
    app: AppHandle,
    manager: State<'_, daemon::DaemonManager>,
    on_status: Channel<daemon::DaemonBootstrapStatus>,
) -> Result<Option<HealthResult>, String> {
    report_failure(
        manager.bootstrap(&app, &on_status).await,
        &on_status,
        |error| daemon::DaemonBootstrapStatus::Failed { error },
    )
}

#[tauri::command]
pub async fn daemon_install(
    app: AppHandle,
    manager: State<'_, daemon::DaemonManager>,
    on_status: Channel<daemon::DaemonBootstrapStatus>,
) -> Result<HealthResult, String> {
    report_failure(
        manager.install(&app, &on_status).await,
        &on_status,
        |error| daemon::DaemonBootstrapStatus::Failed { error },
    )
}

#[tauri::command]
pub async fn daemon_check_update(
    app: AppHandle,
    manager: State<'_, daemon::DaemonManager>,
) -> Result<daemon::DaemonUpdateCheck, String> {
    manager.check_update(&app).await
}

#[tauri::command]
pub async fn daemon_update(
    app: AppHandle,
    version: String,
    manager: State<'_, daemon::DaemonManager>,
    on_status: Channel<daemon::DaemonUpdateStatus>,
) -> Result<HealthResult, String> {
    report_failure(
        manager.update(&app, version, &on_status).await,
        &on_status,
        |error| daemon::DaemonUpdateStatus::Failed { error },
    )
}

#[tauri::command]
pub async fn daemon_health(client: State<'_, DaemonClient>) -> Result<HealthResult, String> {
    client.health().await
}

#[tauri::command]
pub async fn daemon_version(client: State<'_, DaemonClient>) -> Result<VersionResult, String> {
    client.version().await
}

#[tauri::command]
pub async fn get_daemon_config(
    client: State<'_, DaemonClient>,
) -> Result<DaemonConfigResult, String> {
    client.daemon_config().await
}

#[tauri::command]
pub async fn set_daemon_config(
    client: State<'_, DaemonClient>,
    params: SetDaemonConfigParams,
) -> Result<DaemonConfigResult, String> {
    client.set_daemon_config(params).await
}

#[tauri::command]
pub async fn vacuum_database(
    client: State<'_, DaemonClient>,
) -> Result<VacuumDatabaseResult, String> {
    client.vacuum_database().await
}
