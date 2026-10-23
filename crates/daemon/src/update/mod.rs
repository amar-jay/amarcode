//! Daemon-owned update jobs. Activation runs in a separate OS-managed supervisor.
pub mod journal;
pub mod releases;
pub mod supervisor;
use std::path::Path;

use crate::{
    config::{DaemonTransport, DEFAULT_DAEMON_ADDR},
    Config,
};
use amarcode_protocol::rpc::{DaemonUpdateCheck, DaemonUpdateJob, DaemonUpdateStatus};
use journal::{Phase, Record};
use std::{path::PathBuf, time::Duration};

pub fn release_root() -> Result<PathBuf, String> {
    // Deliberately independent of workspace/database overrides.
    crate::config::resolve_default()
        .map_err(|e| e.to_string())?
        .parent()
        .map(|p| p.join("daemon"))
        .ok_or("daemon release directory unavailable".into())
}
fn client() -> Result<reqwest::Client, String> {
    reqwest::Client::builder()
        .timeout(Duration::from_secs(90))
        .build()
        .map_err(|e| e.to_string())
}
pub fn compare(current: &str, latest: &str) -> Result<DaemonUpdateCheck, String> {
    let current_version = semver::Version::parse(current).map_err(|e| e.to_string())?;
    let latest_version = semver::Version::parse(latest).map_err(|e| e.to_string())?;
    Ok(if latest_version > current_version {
        DaemonUpdateCheck::Available {
            current_version: current.into(),
            version: latest.into(),
        }
    } else {
        DaemonUpdateCheck::UpToDate {
            current_version: current.into(),
        }
    })
}

/// Launch through the OS service manager, never as a daemon-owned child.
pub fn launch(executable: &Path, root: &Path, restart: bool) -> Result<(), String> {
    crate::service_control::launch_update_supervisor(executable, root, restart)
        .map_err(|e| e.to_string())
}

pub(crate) fn compatible(manifest: &releases::VerifiedManifest) -> Result<(), String> {
    if manifest.manifest.protocol_version != amarcode_protocol::PROTOCOL_VERSION {
        return Err("daemon release has an incompatible protocol version".into());
    }
    manifest.artifact(releases::platform_target()?)?;
    Ok(())
}
fn installed(config: &Config) -> Result<(PathBuf, releases::VerifiedManifest, PathBuf), String> {
    // Native service registration currently supports the desktop's standard TCP
    // environment. Do not re-register a custom/remote service with lost settings.
    if config.transport != DaemonTransport::Tcp
        || config.daemon_addr != DEFAULT_DAEMON_ADDR
        || config.app_dir != crate::config::resolve_default().map_err(|e| e.to_string())?
        || config.db_path != config.app_dir.join(crate::config::DEFAULT_DB_NAME)
        || config.acp_registry_source.as_deref() != Some(crate::config::DEFAULT_ACP_REGISTRY_SOURCE)
    {
        return Err("updates require the standard managed desktop daemon service".into());
    }
    let service = crate::service_control::ServiceController::status(
        &crate::service_control::NativeServiceController,
    )
    .map_err(|e| e.to_string())?;
    if !service.installed || service.state != crate::service_control::ServiceState::Running {
        return Err("the native daemon service is not running".into());
    }
    let root = release_root()?;
    let current = releases::load_cached(&root)?;
    compatible(&current)?;
    let executable = current
        .installed_executable(&root, releases::platform_target()?)?
        .ok_or("verified installed daemon executable is missing")?;
    if current.manifest.version != env!("CARGO_PKG_VERSION")
        || executable.canonicalize().map_err(|e| e.to_string())?
            != std::env::current_exe()
                .and_then(|path| path.canonicalize())
                .map_err(|e| e.to_string())?
    {
        return Err(
            "updates are disabled for a development or unregistered daemon executable".into(),
        );
    }
    Ok((root, current, executable))
}
pub async fn check(config: &Config) -> Result<DaemonUpdateCheck, String> {
    let owned = config.clone();
    let (_, current, _) = match tokio::task::spawn_blocking(move || installed(&owned))
        .await
        .map_err(|e| e.to_string())?
    {
        Ok(installed) => installed,
        Err(reason) => return Ok(DaemonUpdateCheck::Unavailable { reason }),
    };
    let latest = releases::fetch_latest(&client()?).await?;
    compatible(&latest)?;
    compare(&current.manifest.version, &latest.manifest.version)
}
pub async fn start(config: &Config, version: String) -> Result<DaemonUpdateJob, String> {
    let owned = config.clone();
    let (root, previous, executable) = tokio::task::spawn_blocking(move || installed(&owned))
        .await
        .map_err(|e| e.to_string())??;
    let _request = journal::lock(&root, "request.lock")?;
    if let Some(record) = journal::load(&root, None)? {
        if record.job.finished
            && record.job.version == version
            && previous.manifest.version == version
            && matches!(&record.job.status, DaemonUpdateStatus::Ready { .. })
        {
            return Ok(record.job);
        }
        if !record.job.finished {
            if record.job.version != version {
                return Err("another update version is already in progress".into());
            }
            let launch_root = root.clone();
            tokio::task::spawn_blocking(move || launch(&executable, &launch_root, false))
                .await
                .map_err(|e| e.to_string())??;
            return Ok(record.job);
        }
    }
    let latest = releases::fetch_latest(&client()?).await?;
    compatible(&latest)?;
    if latest.manifest.version != version {
        return Err("the approved release has changed; check for updates again".into());
    }
    if !matches!(
        compare(&previous.manifest.version, &version)?,
        DaemonUpdateCheck::Available { .. }
    ) {
        return Err("the selected release is not newer than the installed daemon".into());
    }
    let mut record = Record {
        job: DaemonUpdateJob {
            id: uuid::Uuid::new_v4().to_string(),
            version,
            previous_version: previous.manifest.version.clone(),
            status: DaemonUpdateStatus::Downloading {
                received: 0,
                total: latest.artifact(releases::platform_target()?)?.size,
            },
            finished: false,
        },
        phase: Phase::Queued,
        address: config.daemon_addr.clone(),
        recovery_failures: 0,
        last_error: None,
    };
    let directory = record.directory(&root);
    previous.save_as_current(&directory.join("previous"))?;
    latest.save_as_current(&directory.join("target"))?;
    journal::publish(&root, &record)?;
    let launch_root = root.clone();
    let launch = tokio::task::spawn_blocking(move || launch(&executable, &launch_root, true))
        .await
        .map_err(|e| e.to_string())?;
    if let Err(error) = launch {
        // A scheduler may accept the job before its client command reports an
        // error. Never overwrite a running/completed supervisor's journal.
        if let Ok(_supervisor) = journal::lock(&root, "supervisor.lock") {
            if let Some(current) = journal::load(&root, Some(&record.job.id))? {
                if current.job.finished {
                    return Ok(current.job);
                }
            }
            record.finish(
                &root,
                DaemonUpdateStatus::Failed {
                    error: error.clone(),
                },
            )?;
            return Err(error);
        }
    }
    Ok(record.job)
}
pub fn get(id: Option<&str>) -> Result<Option<DaemonUpdateJob>, String> {
    Ok(journal::load(&release_root()?, id)?.map(|record| record.job))
}

/// Resume a journaled job if the original RPC daemon exited before handing it
/// to the native supervisor. This never runs inside the workspace DB lock.
pub fn resume(config: &Config) -> Result<(), String> {
    let root = release_root()?;
    let Some(record) = journal::load(&root, None)? else {
        return Ok(());
    };
    if record.job.finished {
        return Ok(());
    }
    let _request = journal::lock(&root, "request.lock")?;
    if let Ok((_, _, executable)) = installed(config) {
        launch(&executable, &root, false)?;
    }
    Ok(())
}
