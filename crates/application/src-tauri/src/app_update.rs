use serde::Serialize;
use tauri::{ipc::Channel, AppHandle};
use tauri_plugin_updater::UpdaterExt;

#[derive(Clone, Debug, Serialize)]
#[serde(
    tag = "status",
    rename_all = "camelCase",
    rename_all_fields = "camelCase"
)]
pub enum AppUpdateCheck {
    UpToDate {
        current_version: String,
    },
    Available {
        current_version: String,
        version: String,
        notes: Option<String>,
    },
}

#[derive(Clone, Debug, Serialize)]
#[serde(
    tag = "status",
    rename_all = "camelCase",
    rename_all_fields = "camelCase"
)]
pub enum AppUpdateStatus {
    Downloading { received: u64, total: u64 },
    Installing,
    Restarting,
    Failed { error: String },
}

pub async fn check(app: &AppHandle) -> Result<AppUpdateCheck, String> {
    let update = app
        .updater()
        .map_err(|error| format!("failed to initialize app updater: {error}"))?
        .check()
        .await
        .map_err(|error| format!("failed to check for app updates: {error}"))?;

    Ok(match update {
        Some(update) => AppUpdateCheck::Available {
            current_version: update.current_version,
            version: update.version,
            notes: update.body,
        },
        None => AppUpdateCheck::UpToDate {
            current_version: app.package_info().version.to_string(),
        },
    })
}

pub async fn install(app: AppHandle, on_status: Channel<AppUpdateStatus>) -> Result<(), String> {
    let update = app
        .updater()
        .map_err(|error| format!("failed to initialize app updater: {error}"))?
        .check()
        .await
        .map_err(|error| format!("failed to check for app updates: {error}"))?
        .ok_or_else(|| "the app is already up to date".to_string())?;

    let mut received = 0_u64;
    let progress = on_status.clone();
    let finished = on_status.clone();
    update
        .download_and_install(
            move |chunk_size, total| {
                received = received.saturating_add(chunk_size as u64);
                let _ = progress.send(AppUpdateStatus::Downloading {
                    received,
                    total: total.unwrap_or(0),
                });
            },
            move || {
                let _ = finished.send(AppUpdateStatus::Installing);
            },
        )
        .await
        .map_err(|error| format!("failed to install app update: {error}"))?;

    let _ = on_status.send(AppUpdateStatus::Restarting);
    app.restart();
}
