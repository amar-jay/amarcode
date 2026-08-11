use std::{
    path::Path,
    process::{Child, Command, Stdio},
    sync::Mutex,
    time::Duration,
};

use serde::Serialize;
use tauri::{ipc::Channel, AppHandle, Manager};
use tokio::sync::Mutex as AsyncMutex;

use crate::{
    config::Config,
    daemon::{release, DaemonBridge},
    protocol::rpc::{methods, HealthResult},
};

#[derive(Clone, Debug, Serialize)]
#[serde(tag = "status", rename_all = "camelCase")]
pub enum DaemonBootstrapStatus {
    Checking,
    Downloading { received: u64, total: u64 },
    Verifying,
    Installing,
    Starting,
    Ready { version: String },
    Failed { error: String },
}

pub struct DaemonManager {
    bootstrap_lock: AsyncMutex<()>,
    child: Mutex<Option<Child>>,
}

impl DaemonManager {
    pub fn new() -> Self {
        Self {
            bootstrap_lock: AsyncMutex::new(()),
            child: Mutex::new(None),
        }
    }

    pub async fn bootstrap(
        &self,
        app: &AppHandle,
        on_status: &Channel<DaemonBootstrapStatus>,
    ) -> Result<HealthResult, String> {
        let _guard = self.bootstrap_lock.lock().await;
        send(on_status, DaemonBootstrapStatus::Checking);

        if let Ok(health) = compatible_health().await {
            ready(on_status, &health);
            return Ok(health);
        }

        let configured_command = &Config::get().daemon_command;
        let explicit_override = std::env::var_os("AMARCODE_DAEMON_COMMAND").is_some();
        if explicit_override || Path::new(configured_command).is_file() {
            send(on_status, DaemonBootstrapStatus::Starting);
            self.spawn(configured_command)?;
            match wait_for_health().await {
                Ok(health) => {
                    ready(on_status, &health);
                    return Ok(health);
                }
                Err(error) => {
                    self.stop();
                    return Err(error);
                }
            }
        }

        let target = release::platform_target()?;
        let install_root = app
            .path()
            .app_local_data_dir()
            .map_err(|error| error.to_string())?
            .join("daemon");
        let client = reqwest::Client::builder()
            .timeout(Duration::from_secs(90))
            .build()
            .map_err(|error| error.to_string())?;

        let cached = release::load_cached(&install_root).ok();
        let remote_result = release::fetch_latest(&client).await.and_then(|manifest| {
            ensure_manifest_compatible(&manifest)?;
            manifest.artifact(target)?;
            Ok(manifest)
        });

        let mut failures = Vec::new();
        let mut candidates = Vec::new();
        match remote_result {
            Ok(remote) => candidates.push((remote, true)),
            Err(error) => failures.push(error),
        }
        if let Some(cached) = cached {
            if ensure_manifest_compatible(&cached).is_ok()
                && cached.artifact(target).is_ok()
                && !candidates
                    .iter()
                    .any(|(candidate, _)| candidate.manifest.version == cached.manifest.version)
            {
                candidates.push((cached, false));
            }
        }

        for (manifest, is_remote) in candidates {
            let artifact = manifest.artifact(target)?.clone();
            if is_remote {
                send(
                    on_status,
                    DaemonBootstrapStatus::Downloading {
                        received: 0,
                        total: artifact.size,
                    },
                );
            }
            let executable = match manifest
                .ensure_installed(&client, &install_root, target)
                .await
            {
                Ok(path) => path,
                Err(error) => {
                    failures.push(error);
                    continue;
                }
            };
            if is_remote {
                send(
                    on_status,
                    DaemonBootstrapStatus::Downloading {
                        received: artifact.size,
                        total: artifact.size,
                    },
                );
            }
            send(on_status, DaemonBootstrapStatus::Verifying);
            send(on_status, DaemonBootstrapStatus::Installing);
            send(on_status, DaemonBootstrapStatus::Starting);

            if let Err(error) = self.spawn(&executable.to_string_lossy()) {
                failures.push(error);
                continue;
            }
            match wait_for_health().await {
                Ok(health) => {
                    if health.version != manifest.manifest.version {
                        failures.push(format!(
                            "daemon version mismatch: installed {}, launched {}",
                            manifest.manifest.version, health.version
                        ));
                        self.stop();
                        continue;
                    }
                    if let Err(error) = manifest.save_as_current(&install_root) {
                        failures.push(error);
                    }
                    ready(on_status, &health);
                    return Ok(health);
                }
                Err(error) => {
                    failures.push(error);
                    self.stop();
                }
            }
        }

        Err(if failures.is_empty() {
            "no compatible daemon release is installed or available".into()
        } else {
            failures.join("; ")
        })
    }

    fn spawn(&self, executable: &str) -> Result<(), String> {
        self.stop();
        let mut command = Command::new(executable);
        command
            .stdin(Stdio::null())
            .stdout(Stdio::null())
            .stderr(Stdio::null());

        #[cfg(windows)]
        {
            use std::os::windows::process::CommandExt;
            command.creation_flags(0x0800_0000);
        }

        let child = command
            .spawn()
            .map_err(|error| format!("failed to launch daemon {executable}: {error}"))?;
        *self.child.lock().expect("daemon child lock poisoned") = Some(child);
        Ok(())
    }

    pub fn stop(&self) {
        if let Some(mut child) = self
            .child
            .lock()
            .expect("daemon child lock poisoned")
            .take()
        {
            let _ = child.kill();
            let _ = child.wait();
        }
    }
}

fn send(channel: &Channel<DaemonBootstrapStatus>, status: DaemonBootstrapStatus) {
    let _ = channel.send(status);
}

fn ready(channel: &Channel<DaemonBootstrapStatus>, health: &HealthResult) {
    send(
        channel,
        DaemonBootstrapStatus::Ready {
            version: health.version.clone(),
        },
    );
}

fn ensure_manifest_compatible(manifest: &release::VerifiedManifest) -> Result<(), String> {
    let expected = amarcode_protocol::PROTOCOL_VERSION;
    let actual = manifest.manifest.protocol_version;
    if expected == actual {
        Ok(())
    } else {
        Err(format!(
            "daemon release protocol {actual} is incompatible with application protocol {expected}"
        ))
    }
}

async fn compatible_health() -> Result<HealthResult, String> {
    let health: HealthResult = DaemonBridge::new()
        .call(methods::HEALTH, serde_json::Value::Null)
        .await?;
    let expected = amarcode_protocol::PROTOCOL_VERSION;
    if health.protocol_version != expected {
        return Err(format!(
            "incompatible daemon protocol: application requires {expected}, daemon provides {}",
            health.protocol_version
        ));
    }
    Ok(health)
}

async fn wait_for_health() -> Result<HealthResult, String> {
    let deadline = tokio::time::Instant::now() + Duration::from_secs(12);
    let mut last_error = "daemon did not start".to_string();
    while tokio::time::Instant::now() < deadline {
        match compatible_health().await {
            Ok(health) => return Ok(health),
            Err(error) => last_error = error,
        }
        tokio::time::sleep(Duration::from_millis(150)).await;
    }
    Err(format!(
        "daemon failed its startup health check: {last_error}"
    ))
}
