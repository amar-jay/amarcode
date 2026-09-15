use std::fs;
use std::path::PathBuf;

use serde::Serialize;
use tauri::{ipc::Channel, AppHandle, Manager};
use tauri_plugin_updater::UpdaterExt;

const APPLIED_SIGNATURE_FILE: &str = "app-update.signature";

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

fn applied_signature_path(app: &AppHandle) -> Result<PathBuf, String> {
    Ok(app
        .path()
        .app_local_data_dir()
        .map_err(|error| format!("failed to resolve app data directory: {error}"))?
        .join(APPLIED_SIGNATURE_FILE))
}

fn read_applied_signature(app: &AppHandle) -> Option<String> {
    let path = applied_signature_path(app).ok()?;
    let signature = fs::read_to_string(path).ok()?;
    let signature = signature.trim();
    if signature.is_empty() {
        None
    } else {
        Some(signature.to_string())
    }
}

fn write_applied_signature(app: &AppHandle, signature: &str) -> Result<(), String> {
    let path = applied_signature_path(app)?;
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent)
            .map_err(|error| format!("failed to create app data directory: {error}"))?;
    }
    fs::write(&path, signature)
        .map_err(|error| format!("failed to record installed update signature: {error}"))
}

async fn fetch_update(
    app: &AppHandle,
) -> Result<Option<tauri_plugin_updater::Update>, String> {
    // Ignore package semver. A new GitHub upload is a new build when the
    // signed updater artifact (minisign payload) changed.
    let update = app
        .updater_builder()
        .version_comparator(|_, _| true)
        .build()
        .map_err(|error| format!("failed to initialize app updater: {error}"))?
        .check()
        .await
        .map_err(|error| format!("failed to check for app updates: {error}"))?;

    let Some(update) = update else {
        return Ok(None);
    };
    if read_applied_signature(app).as_deref() == Some(update.signature.as_str()) {
        return Ok(None);
    }
    Ok(Some(update))
}

pub async fn check(app: &AppHandle) -> Result<AppUpdateCheck, String> {
    let current_version = app.package_info().version.to_string();
    Ok(match fetch_update(app).await? {
        Some(update) => AppUpdateCheck::Available {
            current_version: update.current_version,
            version: update.version,
            notes: update.body,
        },
        None => AppUpdateCheck::UpToDate { current_version },
    })
}

pub async fn install(app: AppHandle, on_status: Channel<AppUpdateStatus>) -> Result<(), String> {
    let update = fetch_update(&app)
        .await?
        .ok_or_else(|| "the app is already up to date".to_string())?;
    let signature = update.signature.clone();
    let previous_signature = read_applied_signature(&app);
    // Windows exits inside download_and_install, so record the signature
    // before launching the installer and restore it if install fails.
    write_applied_signature(&app, &signature)?;

    let mut received = 0_u64;
    let progress = on_status.clone();
    let finished = on_status.clone();
    if let Err(error) = update
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
    {
        match previous_signature {
            Some(previous) => write_applied_signature(&app, &previous)?,
            None => {
                let _ = fs::remove_file(applied_signature_path(&app)?);
            }
        }
        return Err(format!("failed to install app update: {error}"));
    }

    let _ = on_status.send(AppUpdateStatus::Restarting);
    app.restart();
}
