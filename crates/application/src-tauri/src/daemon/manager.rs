use std::{
    path::Path,
    process::{Child, Command, Stdio},
    sync::Mutex,
    time::Duration,
};

#[cfg(unix)]
use std::{ffi::OsString, io::Read, time::Instant};

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

        // Desktop launchers inherit the graphical session's PATH, which often
        // omits tools installed by shell-managed runtimes such as nvm, Bun,
        // Cargo, mise, or asdf.  The daemon launches ACP adapters by command
        // name, so give it the same PATH the user gets in a login shell.
        #[cfg(unix)]
        if let Some(path) = agent_path() {
            command.env("PATH", path);
        }

        // GUI processes on Windows can likewise have a stale or incomplete
        // user PATH. npm in particular installs ACP command shims under
        // `%APPDATA%\npm`, which may be missing when Amarcode is opened from
        // the Start menu instead of a terminal.
        #[cfg(windows)]
        if let Some(path) = agent_path() {
            command.env("PATH", path);
        }

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

#[cfg(unix)]
const PATH_MARKER: &str = "__AMARCODE_LOGIN_PATH__";

/// Resolve the user's interactive login-shell PATH and merge it ahead of the
/// graphical session PATH. Shell startup output is ignored by extracting only
/// the marker-prefixed line emitted by our command.
#[cfg(unix)]
fn agent_path() -> Option<OsString> {
    let shell = std::env::var_os("SHELL").filter(|value| !value.is_empty())?;
    let mut child = Command::new(shell)
        .args(["-ilc", "printf '\n__AMARCODE_LOGIN_PATH__%s\n' \"$PATH\""])
        .stdin(Stdio::null())
        .stderr(Stdio::null())
        .stdout(Stdio::piped())
        .spawn()
        .ok()?;

    let deadline = Instant::now() + Duration::from_secs(5);
    let status = loop {
        if let Some(status) = child.try_wait().ok()? {
            break status;
        }
        if Instant::now() >= deadline {
            let _ = child.kill();
            let _ = child.wait();
            return None;
        }
        std::thread::sleep(Duration::from_millis(10));
    };
    if !status.success() {
        return None;
    }

    let mut output = Vec::new();
    child.stdout.take()?.read_to_end(&mut output).ok()?;
    let login_path = marked_path(&output)?;
    let inherited = std::env::var_os("PATH");
    let mut paths = Vec::new();
    for path in std::env::split_paths(&login_path).chain(
        inherited
            .as_deref()
            .into_iter()
            .flat_map(std::env::split_paths),
    ) {
        if !paths.contains(&path) {
            paths.push(path);
        }
    }
    std::env::join_paths(paths).ok().or(Some(login_path))
}

#[cfg(unix)]
fn marked_path(output: &[u8]) -> Option<OsString> {
    use std::os::unix::ffi::OsStringExt;

    output
        .split(|byte| *byte == b'\n')
        .rev()
        .find_map(|line| line.strip_prefix(PATH_MARKER.as_bytes()))
        .filter(|path| !path.is_empty())
        .map(|path| OsString::from_vec(path.strip_suffix(b"\r").unwrap_or(path).to_vec()))
}

#[cfg(windows)]
fn agent_path() -> Option<std::ffi::OsString> {
    use std::path::PathBuf;

    let inherited = std::env::var_os("PATH");
    let user_profile = std::env::var_os("USERPROFILE").map(PathBuf::from);
    let app_data = std::env::var_os("APPDATA").map(PathBuf::from);
    let local_app_data = std::env::var_os("LOCALAPPDATA").map(PathBuf::from);

    let mut paths = Vec::new();
    for path in [
        std::env::var_os("PNPM_HOME").map(PathBuf::from),
        std::env::var_os("NVM_SYMLINK").map(PathBuf::from),
        app_data.as_ref().map(|root| root.join("npm")),
        local_app_data.as_ref().map(|root| root.join("pnpm")),
        user_profile.as_ref().map(|root| root.join(".bun/bin")),
        user_profile.as_ref().map(|root| root.join(".cargo/bin")),
        user_profile.as_ref().map(|root| root.join("scoop/shims")),
    ]
    .into_iter()
    .flatten()
    .chain(
        inherited
            .as_deref()
            .into_iter()
            .flat_map(std::env::split_paths),
    ) {
        if !paths.contains(&path) {
            paths.push(path);
        }
    }

    std::env::join_paths(paths).ok()
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

#[cfg(all(test, unix))]
mod tests {
    use super::marked_path;

    #[test]
    fn extracts_path_without_shell_startup_output() {
        let output = b"shell banner\nwarning\n__AMARCODE_LOGIN_PATH__/nvm/bin:/usr/bin\r\n";
        assert_eq!(
            marked_path(output).unwrap(),
            std::ffi::OsString::from("/nvm/bin:/usr/bin")
        );
    }

    #[test]
    fn rejects_output_without_a_nonempty_marked_path() {
        assert!(marked_path(b"shell banner\n").is_none());
        assert!(marked_path(b"__AMARCODE_LOGIN_PATH__\n").is_none());
    }
}
