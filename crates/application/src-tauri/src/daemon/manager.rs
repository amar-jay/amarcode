use std::{
    fs,
    io::Read,
    path::{Path, PathBuf},
    process::{Command, Stdio},
    time::{Duration, Instant},
};

#[cfg(unix)]
use std::ffi::OsString;

use serde::{Deserialize, Serialize};
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
    InstallRequired { reason: String },
    Downloading { received: u64, total: u64 },
    Verifying,
    Installing,
    Starting,
    Ready { version: String },
    Failed { error: String },
}

pub struct DaemonManager {
    bootstrap_lock: AsyncMutex<()>,
}

impl DaemonManager {
    pub fn new() -> Self {
        Self {
            bootstrap_lock: AsyncMutex::new(()),
        }
    }

    pub async fn bootstrap(
        &self,
        app: &AppHandle,
        on_status: &Channel<DaemonBootstrapStatus>,
    ) -> Result<Option<HealthResult>, String> {
        let _guard = self.bootstrap_lock.lock().await;
        send(on_status, DaemonBootstrapStatus::Checking);

        let target = release::platform_target()?;
        let install_root = install_root(app)?;
        let (executable, expected_version) = if let Some(executable) =
            Config::get().daemon_service_executable.clone()
        {
            if !executable.is_file() {
                install_required(
                    on_status,
                    "The configured daemon service executable does not exist.",
                );
                return Ok(None);
            }
            (executable, None)
        } else {
            if !service_installation_marker(&install_root).is_file() {
                install_required(
                    on_status,
                    "The per-user background service has not been installed yet.",
                );
                return Ok(None);
            }

            let Some((executable, expected_version)) = cached_executable(&install_root, target)?
            else {
                install_required(
                    on_status,
                    "The installed service needs to be repaired with a verified daemon executable.",
                );
                return Ok(None);
            };
            (executable, Some(expected_version))
        };

        let status = match service_status(&executable) {
            Ok(status) => status,
            Err(error) => {
                install_required(
                    on_status,
                    &format!("The daemon service status could not be verified: {error}"),
                );
                return Ok(None);
            }
        };
        if !status.installed {
            install_required(
                on_status,
                "The per-user background service is no longer registered.",
            );
            return Ok(None);
        }

        // Health is trusted only after native service registration has been
        // verified. A random foreground daemon must not satisfy bootstrap.
        if status.state == ServiceState::Running {
            if let Ok(health) = compatible_health().await {
                ensure_expected_version(expected_version.as_deref(), &health)?;
                ready(on_status, &health);
                return Ok(Some(health));
            }
        }

        send(on_status, DaemonBootstrapStatus::Starting);
        run_lifecycle_command(&executable, &["start"], None)?;
        let health = wait_for_health().await?;
        ensure_service_running(&executable)?;
        ensure_expected_version(expected_version.as_deref(), &health)?;
        ready(on_status, &health);
        Ok(Some(health))
    }

    /// Download, verify, register, and start the service after explicit user
    /// consent. Ordinary bootstrap never calls this method.
    pub async fn install(
        &self,
        app: &AppHandle,
        on_status: &Channel<DaemonBootstrapStatus>,
    ) -> Result<HealthResult, String> {
        let _guard = self.bootstrap_lock.lock().await;
        let target = release::platform_target()?;
        let install_root = install_root(app)?;
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
            if let Err(error) = verify_lifecycle_cli(&executable) {
                failures.push(error);
                continue;
            }
            send(on_status, DaemonBootstrapStatus::Installing);
            send(on_status, DaemonBootstrapStatus::Starting);

            let service_path = agent_path();
            if let Err(error) =
                run_lifecycle_command(&executable, &["install"], service_path.as_deref())
            {
                failures.push(error);
                continue;
            }
            if let Err(error) = run_lifecycle_command(&executable, &["restart"], None) {
                failures.push(error);
                continue;
            }
            match wait_for_health().await {
                Ok(health) => {
                    if let Err(error) = ensure_service_running(&executable) {
                        failures.push(error);
                        continue;
                    }
                    if health.version != manifest.manifest.version {
                        failures.push(format!(
                            "daemon version mismatch: installed {}, launched {}",
                            manifest.manifest.version, health.version
                        ));
                        continue;
                    }
                    manifest.save_as_current(&install_root)?;
                    fs::write(service_installation_marker(&install_root), b"1\n").map_err(
                        |error| format!("failed to save service installation state: {error}"),
                    )?;
                    ready(on_status, &health);
                    return Ok(health);
                }
                Err(error) => {
                    failures.push(error);
                }
            }
        }

        Err(if failures.is_empty() {
            "no compatible daemon release is installed or available".into()
        } else {
            failures.join("; ")
        })
    }
}

const SERVICE_INSTALLATION_MARKER: &str = "service-installed-v1";

#[derive(Clone, Copy, Debug, Deserialize, Eq, PartialEq)]
#[serde(rename_all = "camelCase")]
enum ServiceState {
    NotInstalled,
    Stopped,
    Running,
    Unknown,
}

#[derive(Debug, Deserialize)]
struct ServiceStatus {
    installed: bool,
    state: ServiceState,
}

fn install_root(app: &AppHandle) -> Result<PathBuf, String> {
    Ok(app
        .path()
        .app_local_data_dir()
        .map_err(|error| error.to_string())?
        .join("daemon"))
}

fn service_installation_marker(install_root: &Path) -> PathBuf {
    install_root.join(SERVICE_INSTALLATION_MARKER)
}

fn cached_executable(
    install_root: &Path,
    target: &str,
) -> Result<Option<(PathBuf, String)>, String> {
    let manifest = match release::load_cached(install_root) {
        Ok(manifest) => manifest,
        Err(_) => return Ok(None),
    };
    if ensure_manifest_compatible(&manifest).is_err() {
        return Ok(None);
    }
    let executable = manifest.installed_executable(install_root, target)?;
    Ok(executable.map(|path| (path, manifest.manifest.version)))
}

fn service_status(executable: &Path) -> Result<ServiceStatus, String> {
    let output = run_lifecycle_command(executable, &["status", "--json"], None)?;
    serde_json::from_str(&output)
        .map_err(|error| format!("daemon returned invalid service status: {error}"))
}

fn ensure_service_running(executable: &Path) -> Result<(), String> {
    let status = service_status(executable)?;
    if status.installed && status.state == ServiceState::Running {
        Ok(())
    } else {
        Err("daemon answered health checks, but the native service is not running".into())
    }
}

fn verify_lifecycle_cli(executable: &Path) -> Result<(), String> {
    let help =
        run_lifecycle_command_with_timeout(executable, &["--help"], None, Duration::from_secs(3))
            .map_err(|error| {
            format!("downloaded daemon does not support service lifecycle commands: {error}")
        })?;
    validate_lifecycle_help(&help)
}

fn validate_lifecycle_help(help: &str) -> Result<(), String> {
    for command in ["install", "start", "restart", "status"] {
        if !help
            .lines()
            .any(|line| line.trim_start().starts_with(command))
        {
            return Err(format!(
                "downloaded daemon is missing the required `{command}` lifecycle command"
            ));
        }
    }
    Ok(())
}

fn run_lifecycle_command(
    executable: &Path,
    arguments: &[&str],
    path: Option<&std::ffi::OsStr>,
) -> Result<String, String> {
    run_lifecycle_command_with_timeout(executable, arguments, path, Duration::from_secs(15))
}

fn run_lifecycle_command_with_timeout(
    executable: &Path,
    arguments: &[&str],
    path: Option<&std::ffi::OsStr>,
    timeout: Duration,
) -> Result<String, String> {
    let mut command = Command::new(executable);
    command
        .args(arguments)
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped());
    if let Some(path) = path {
        command.env("PATH", path);
    }
    #[cfg(windows)]
    {
        use std::os::windows::process::CommandExt;
        command.creation_flags(0x0800_0000);
    }

    let mut child = command.spawn().map_err(|error| {
        format!(
            "failed to run {} {}: {error}",
            executable.display(),
            arguments.join(" ")
        )
    })?;
    let deadline = Instant::now() + timeout;
    let status = loop {
        if let Some(status) = child.try_wait().map_err(|error| error.to_string())? {
            break status;
        }
        if Instant::now() >= deadline {
            let _ = child.kill();
            let _ = child.wait();
            return Err(format!(
                "daemon lifecycle command timed out: {}",
                arguments.join(" ")
            ));
        }
        std::thread::sleep(Duration::from_millis(20));
    };

    let mut stdout = String::new();
    let mut stderr = String::new();
    if let Some(mut reader) = child.stdout.take() {
        reader
            .read_to_string(&mut stdout)
            .map_err(|error| error.to_string())?;
    }
    if let Some(mut reader) = child.stderr.take() {
        reader
            .read_to_string(&mut stderr)
            .map_err(|error| error.to_string())?;
    }
    if status.success() {
        Ok(stdout.trim().to_owned())
    } else {
        let detail = if stderr.trim().is_empty() {
            stdout.trim()
        } else {
            stderr.trim()
        };
        Err(if detail.is_empty() {
            format!(
                "daemon lifecycle command failed ({}): {status}",
                arguments.join(" ")
            )
        } else {
            detail.to_owned()
        })
    }
}

fn install_required(channel: &Channel<DaemonBootstrapStatus>, reason: &str) {
    send(
        channel,
        DaemonBootstrapStatus::InstallRequired {
            reason: reason.to_owned(),
        },
    );
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

fn ensure_expected_version(expected: Option<&str>, health: &HealthResult) -> Result<(), String> {
    if expected.is_none_or(|expected| expected == health.version) {
        return Ok(());
    }
    Err(format!(
        "daemon version mismatch: installed {}, launched {}",
        expected.unwrap_or_default(),
        health.version
    ))
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

#[cfg(test)]
mod tests {
    use super::{ensure_expected_version, validate_lifecycle_help, ServiceState, ServiceStatus};
    use crate::protocol::rpc::HealthResult;

    fn health(version: &str) -> HealthResult {
        HealthResult {
            status: "ok".into(),
            version: version.into(),
            protocol_version: amarcode_protocol::PROTOCOL_VERSION,
            addr: "127.0.0.1:43821".into(),
        }
    }

    #[test]
    fn parses_native_service_status() {
        let status: ServiceStatus = serde_json::from_str(
            r#"{"installed":true,"state":"running","definition_path":"ignored"}"#,
        )
        .expect("service status");
        assert!(status.installed);
        assert_eq!(status.state, ServiceState::Running);
    }

    #[test]
    fn verified_release_requires_its_expected_daemon_version() {
        assert!(ensure_expected_version(Some("0.3.0"), &health("0.3.0")).is_ok());
        assert!(ensure_expected_version(Some("0.3.0"), &health("0.2.0")).is_err());
        assert!(ensure_expected_version(None, &health("dev")).is_ok());
    }

    #[test]
    fn release_binary_must_advertise_every_service_lifecycle_command() {
        let complete =
            "  install  register\n  start  start\n  restart  restart\n  status  status\n";
        assert!(validate_lifecycle_help(complete).is_ok());
        assert!(validate_lifecycle_help("  run  foreground\n").is_err());
    }

    #[cfg(unix)]
    #[test]
    fn extracts_path_without_shell_startup_output() {
        use super::marked_path;

        let output = b"shell banner\nwarning\n__AMARCODE_LOGIN_PATH__/nvm/bin:/usr/bin\r\n";
        assert_eq!(
            marked_path(output).unwrap(),
            std::ffi::OsString::from("/nvm/bin:/usr/bin")
        );
    }

    #[cfg(unix)]
    #[test]
    fn rejects_output_without_a_nonempty_marked_path() {
        use super::marked_path;

        assert!(marked_path(b"shell banner\n").is_none());
        assert!(marked_path(b"__AMARCODE_LOGIN_PATH__\n").is_none());
    }
}
