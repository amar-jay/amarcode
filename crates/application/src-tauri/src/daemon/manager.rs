//! Desktop adapter for the bundled daemon CLI and running daemon RPC.
#[cfg(unix)]
use std::ffi::OsString;
use std::{
    fs,
    io::Read,
    path::{Path, PathBuf},
    process::{Command, Stdio},
    time::{Duration, Instant},
};

use crate::{config::Config, daemon::DaemonClient};
use amarcode_protocol::rpc::{
    DaemonBootstrapEvent, DaemonUpdateJob, GetDaemonUpdateParams, HealthResult,
    StartDaemonUpdateParams,
};
pub use amarcode_protocol::rpc::{DaemonBootstrapStatus, DaemonUpdateCheck, DaemonUpdateStatus};
use serde::{Deserialize, Serialize};
use tauri::{ipc::Channel, AppHandle, Manager};
use tokio::{
    io::{AsyncBufReadExt, AsyncReadExt, BufReader},
    sync::Mutex as AsyncMutex,
};

#[derive(Clone, Debug, Serialize)]
#[serde(tag = "status", rename_all = "camelCase")]
pub enum ApplicationCleanupStatus {
    Preparing,
    RemovingServiceAndData,
    RemovingReleaseCache,
    Ready,
    Failed { error: String },
}

#[derive(Clone, Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ApplicationCleanupResult {
    pub service_and_daemon_data_removed: bool,
    pub daemon_release_cache_removed: bool,
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
        on_status: &Channel<DaemonBootstrapStatus>,
    ) -> Result<Option<HealthResult>, String> {
        let _guard = self.bootstrap_lock.lock().await;
        if super::remote::is_remote()? {
            let health = DaemonClient::new().health().await?;
            ready(on_status, &health);
            return Ok(Some(health));
        }
        if let Some(executable) = &Config::get().daemon_service_executable {
            let status = service_status(executable).await?;
            if !status.installed {
                send(
                    on_status,
                    DaemonBootstrapStatus::InstallRequired {
                        reason: "The configured development service has not been installed.".into(),
                    },
                );
                return Ok(None);
            }
            if status.state != ServiceState::Running {
                send(on_status, DaemonBootstrapStatus::Starting);
                command_output(executable, &["start"], None, Duration::from_secs(15)).await?;
            }
            let health = wait_for_health().await?;
            ready(on_status, &health);
            return Ok(Some(health));
        }
        bootstrap_command(&bundled_executable()?, "ensure", on_status).await
    }

    pub async fn install(
        &self,
        on_status: &Channel<DaemonBootstrapStatus>,
    ) -> Result<HealthResult, String> {
        let _guard = self.bootstrap_lock.lock().await;
        if super::remote::is_remote()? {
            return Err("Install or repair the remote daemon on its host".into());
        }
        if let Some(executable) = &Config::get().daemon_service_executable {
            send(on_status, DaemonBootstrapStatus::Installing);
            let path = tokio::task::spawn_blocking(agent_path)
                .await
                .map_err(|e| e.to_string())?;
            command_output(
                executable,
                &["install"],
                path.as_deref(),
                Duration::from_secs(15),
            )
            .await?;
            command_output(executable, &["restart"], None, Duration::from_secs(15)).await?;
            let health = wait_for_health().await?;
            ready(on_status, &health);
            return Ok(health);
        }
        bootstrap_command(&bundled_executable()?, "install", on_status)
            .await?
            .ok_or("bundled daemon returned no installation result".into())
    }

    pub async fn check_update(&self) -> Result<DaemonUpdateCheck, String> {
        if super::remote::is_remote()? {
            return Ok(DaemonUpdateCheck::Unavailable {
                reason: "Update the remote daemon using its host installer".into(),
            });
        }
        if Config::get().daemon_service_executable.is_some() {
            return Ok(DaemonUpdateCheck::Unavailable {
                reason: "updates are disabled for a development daemon".into(),
            });
        }
        match DaemonClient::new().check_daemon_update().await {
            Ok(result) => Ok(result),
            Err(error) if error.contains("unknown method: check_daemon_update") => {
                cli_json(&bundled_executable()?, &["bootstrap", "check-update"]).await
            }
            Err(error) => Err(error),
        }
    }

    /// The app requests and observes jobs. The daemon owns their execution.
    pub async fn update(
        &self,
        version: String,
        on_status: &Channel<DaemonUpdateStatus>,
    ) -> Result<HealthResult, String> {
        if super::remote::is_remote()? {
            return Err("Update the remote daemon on its host".into());
        }
        if Config::get().daemon_service_executable.is_some() {
            return Err("updates are disabled for a development daemon".into());
        }
        let executable = bundled_executable()?;
        let result = DaemonClient::new()
            .start_daemon_update(StartDaemonUpdateParams {
                version: version.clone(),
            })
            .await;
        let job = match result {
            Ok(job) => job,
            Err(error) if error.contains("unknown method: start_daemon_update") => {
                // The bundled CLI hands off to its independent supervisor even
                // when the running daemon predates update RPC.
                cli_json(
                    &executable,
                    &["bootstrap", "start-update", "--version", &version],
                )
                .await?
            }
            Err(error) => match cli_update_job(&executable, None).await? {
                Some(job)
                    if job.version == version
                        && (!job.finished
                            || matches!(&job.status, DaemonUpdateStatus::Ready { .. })) =>
                {
                    job
                }
                _ => return Err(error),
            },
        };
        wait_for_update(&executable, &job.id, |status| {
            send_update(on_status, status)
        })
        .await?;
        let health = DaemonClient::new().health().await?;
        ensure_expected_version(Some(&job.version), &health)?;
        Ok(health)
    }

    /// Permanently remove the native service, daemon-owned data and cached
    /// daemon releases after explicit user confirmation. A durable phase
    /// marker makes the operation safe to retry after process interruption.
    pub async fn prepare_uninstall(
        &self,
        app: &AppHandle,
        confirmed: bool,
        on_status: &Channel<ApplicationCleanupStatus>,
    ) -> Result<ApplicationCleanupResult, String> {
        if !confirmed {
            return Err("application cleanup requires explicit data-loss confirmation".into());
        }

        let _guard = self.bootstrap_lock.lock().await;
        send_cleanup(on_status, ApplicationCleanupStatus::Preparing);

        let app_data_root = app
            .path()
            .app_local_data_dir()
            .map_err(|error| error.to_string())?;
        let install_root = app_data_root.join("daemon");
        let marker = cleanup_marker(&app_data_root);
        let phase = read_cleanup_phase(&marker)?;

        if phase != Some(CleanupPhase::DaemonPurged) {
            let executable = Config::get()
                .daemon_service_executable
                .clone()
                .map(Ok)
                .unwrap_or_else(bundled_executable)?;
            write_cleanup_phase(&marker, CleanupPhase::Started)?;
            send_cleanup(on_status, ApplicationCleanupStatus::RemovingServiceAndData);
            command_output(
                &executable,
                &["purge", "--confirm-data-loss"],
                None,
                Duration::from_secs(30),
            )
            .await?;
            write_cleanup_phase(&marker, CleanupPhase::DaemonPurged)?;
        }

        send_cleanup(on_status, ApplicationCleanupStatus::RemovingReleaseCache);
        remove_install_root(&install_root, &app_data_root)?;
        clear_cleanup_marker(&marker)?;
        send_cleanup(on_status, ApplicationCleanupStatus::Ready);

        Ok(ApplicationCleanupResult {
            service_and_daemon_data_removed: true,
            daemon_release_cache_removed: true,
        })
    }
}

/// Tauri copies externalBin beside the desktop executable in development and
/// packages it in the same executable directory on Linux, Windows and macOS.
fn bundled_executable() -> Result<PathBuf, String> {
    let desktop = tauri::utils::platform::current_exe().map_err(|e| e.to_string())?;
    let path = desktop
        .parent()
        .ok_or("desktop executable has no parent directory")?
        .join(format!(
            "amarcode-bootstrap{}",
            std::env::consts::EXE_SUFFIX
        ));
    if !path.is_file() {
        return Err("bundled daemon is missing; reinstall Amarcode (developers: run bun scripts/prepare-desktop-daemon.ts --debug)".into());
    }
    Ok(path)
}

fn process(
    executable: &Path,
    arguments: &[&str],
    path: Option<&std::ffi::OsStr>,
) -> tokio::process::Command {
    let mut command = tokio::process::Command::new(executable);
    command
        .args(arguments)
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .kill_on_drop(true);
    if let Some(path) = path {
        command.env("PATH", path);
    }
    #[cfg(windows)]
    command.creation_flags(0x0800_0000);
    command
}

async fn command_output(
    executable: &Path,
    arguments: &[&str],
    path: Option<&std::ffi::OsStr>,
    timeout: Duration,
) -> Result<String, String> {
    let output = tokio::time::timeout(timeout, process(executable, arguments, path).output())
        .await
        .map_err(|_| "daemon CLI command timed out")?
        .map_err(|e| format!("failed to run {}: {e}", executable.display()))?;
    if !output.status.success() {
        return Err(String::from_utf8_lossy(&output.stderr).trim().to_owned());
    }
    String::from_utf8(output.stdout).map_err(|e| e.to_string())
}
async fn cli_json<T: serde::de::DeserializeOwned>(
    executable: &Path,
    arguments: &[&str],
) -> Result<T, String> {
    serde_json::from_str(
        &command_output(executable, arguments, None, Duration::from_secs(120)).await?,
    )
    .map_err(|e| format!("invalid daemon CLI response: {e}"))
}
async fn bootstrap_command(
    executable: &Path,
    mode: &str,
    channel: &Channel<DaemonBootstrapStatus>,
) -> Result<Option<HealthResult>, String> {
    let path = tokio::task::spawn_blocking(agent_path)
        .await
        .map_err(|e| e.to_string())?;
    tokio::time::timeout(Duration::from_secs(360), async {
        let mut child = process(executable, &["bootstrap", mode], path.as_deref())
            .spawn()
            .map_err(|e| e.to_string())?;
        let stdout = child.stdout.take().ok_or("missing bootstrap stdout")?;
        let mut stderr = child.stderr.take().ok_or("missing bootstrap stderr")?;
        // Drain stderr concurrently so a full pipe cannot block the helper.
        let errors = tokio::spawn(async move {
            let mut text = Vec::new();
            let mut buffer = [0; 4096];
            loop {
                let count = stderr.read(&mut buffer).await?;
                if count == 0 {
                    break;
                }
                let retain = count.min((64 * 1024usize).saturating_sub(text.len()));
                text.extend_from_slice(&buffer[..retain]);
            }
            Ok::<_, std::io::Error>(String::from_utf8_lossy(&text).into_owned())
        });
        let mut lines = BufReader::new(stdout).lines();
        let mut result = None;
        while let Some(line) = lines.next_line().await.map_err(|e| e.to_string())? {
            let event: DaemonBootstrapEvent =
                serde_json::from_str(&line).map_err(|e| format!("invalid bootstrap event: {e}"))?;
            match event {
                DaemonBootstrapEvent::Progress { status } => send(channel, status),
                DaemonBootstrapEvent::Complete { health } => result = Some(health),
            }
        }
        let status = child.wait().await.map_err(|e| e.to_string())?;
        let error = errors
            .await
            .map_err(|e| e.to_string())?
            .map_err(|e| e.to_string())?;
        if !status.success() {
            return Err(error.trim().to_owned());
        }
        result.ok_or("bundled daemon returned no bootstrap result".into())
    })
    .await
    .map_err(|_| {
        "daemon bootstrap timed out; retry to recover the interrupted operation".to_string()
    })?
}
async fn cli_update_job(
    executable: &Path,
    id: Option<&str>,
) -> Result<Option<DaemonUpdateJob>, String> {
    match id {
        Some(id) => cli_json(executable, &["bootstrap", "update-status", "--job-id", id]).await,
        None => cli_json(executable, &["bootstrap", "update-status"]).await,
    }
}
async fn wait_for_update(
    executable: &Path,
    id: &str,
    progress: impl Fn(DaemonUpdateStatus),
) -> Result<(), String> {
    let deadline = tokio::time::Instant::now() + Duration::from_secs(300);
    loop {
        let rpc = tokio::time::timeout(
            Duration::from_secs(2),
            DaemonClient::new().get_daemon_update(GetDaemonUpdateParams {
                job_id: Some(id.into()),
            }),
        )
        .await;
        let job = match rpc {
            Ok(Ok(Some(job))) => Some(job),
            _ => cli_update_job(executable, Some(id)).await?,
        }
        .ok_or("daemon update job disappeared")?;
        progress(job.status.clone());
        if job.finished {
            return match job.status {
                DaemonUpdateStatus::Ready { .. } => Ok(()),
                DaemonUpdateStatus::Failed { error } => Err(error),
                _ => Err("invalid terminal daemon update state".into()),
            };
        }
        if tokio::time::Instant::now() >= deadline {
            return Err(
                "daemon update is still running; its supervisor will continue independently".into(),
            );
        }
        tokio::time::sleep(Duration::from_millis(300)).await;
    }
}
async fn service_status(executable: &Path) -> Result<ServiceStatus, String> {
    cli_json(executable, &["status", "--json"]).await
}
const CLEANUP_MARKER: &str = "daemon-cleanup-pending-v1";

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
enum CleanupPhase {
    Started,
    DaemonPurged,
}

impl CleanupPhase {
    fn as_str(self) -> &'static str {
        match self {
            Self::Started => "started",
            Self::DaemonPurged => "daemon-purged",
        }
    }

    fn parse(value: &str) -> Result<Self, String> {
        match value.trim() {
            "started" => Ok(Self::Started),
            "daemon-purged" => Ok(Self::DaemonPurged),
            value => Err(format!("invalid daemon cleanup phase: {value}")),
        }
    }
}

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

fn cleanup_marker(app_data_root: &Path) -> PathBuf {
    app_data_root.join(CLEANUP_MARKER)
}

fn read_cleanup_phase(marker: &Path) -> Result<Option<CleanupPhase>, String> {
    match fs::read_to_string(marker) {
        Ok(value) => CleanupPhase::parse(&value).map(Some),
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => Ok(None),
        Err(error) => Err(format!(
            "failed to read daemon cleanup state {}: {error}",
            marker.display()
        )),
    }
}

fn write_cleanup_phase(marker: &Path, phase: CleanupPhase) -> Result<(), String> {
    let parent = marker
        .parent()
        .ok_or_else(|| "daemon cleanup marker has no parent".to_string())?;
    fs::create_dir_all(parent)
        .map_err(|error| format!("failed to create application data directory: {error}"))?;
    let temporary = marker.with_extension(format!("{}.part", std::process::id()));
    let mut file = fs::OpenOptions::new()
        .create(true)
        .truncate(true)
        .write(true)
        .open(&temporary)
        .map_err(|error| format!("failed to write daemon cleanup state: {error}"))?;
    use std::io::Write as _;
    file.write_all(format!("{}\n", phase.as_str()).as_bytes())
        .and_then(|_| file.sync_all())
        .map_err(|error| format!("failed to persist daemon cleanup state: {error}"))?;
    match fs::remove_file(marker) {
        Ok(()) => {}
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => {}
        Err(error) => return Err(format!("failed to replace daemon cleanup state: {error}")),
    }
    fs::rename(&temporary, marker)
        .map_err(|error| format!("failed to commit daemon cleanup state: {error}"))
}

fn clear_cleanup_marker(marker: &Path) -> Result<(), String> {
    match fs::remove_file(marker) {
        Ok(()) => Ok(()),
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => Ok(()),
        Err(error) => Err(format!("failed to clear daemon cleanup state: {error}")),
    }
}

fn remove_install_root(install_root: &Path, app_data_root: &Path) -> Result<(), String> {
    if install_root != app_data_root.join("daemon") {
        return Err("refusing to remove an unexpected daemon release cache path".into());
    }
    if let Ok(metadata) = fs::symlink_metadata(install_root) {
        if metadata.file_type().is_symlink() {
            return Err("refusing to remove a symlinked daemon release cache".into());
        }
    }
    match fs::remove_dir_all(install_root) {
        Ok(()) => Ok(()),
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => Ok(()),
        Err(error) => Err(format!("failed to remove cached daemon releases: {error}")),
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

fn send_update(channel: &Channel<DaemonUpdateStatus>, status: DaemonUpdateStatus) {
    let _ = channel.send(status);
}

fn send_cleanup(channel: &Channel<ApplicationCleanupStatus>, status: ApplicationCleanupStatus) {
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

async fn wait_for_health() -> Result<HealthResult, String> {
    let deadline = tokio::time::Instant::now() + Duration::from_secs(12);
    let mut last_error = "daemon did not start".to_string();
    while tokio::time::Instant::now() < deadline {
        match DaemonClient::new().health().await {
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
    use super::{
        clear_cleanup_marker, ensure_expected_version, read_cleanup_phase, remove_install_root,
        write_cleanup_phase, ApplicationCleanupResult, ApplicationCleanupStatus, CleanupPhase,
        DaemonUpdateCheck, ServiceState, ServiceStatus,
    };
    use amarcode_protocol::rpc::HealthResult;

    fn health(version: &str) -> HealthResult {
        HealthResult {
            remote_auth_required: false,
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
    fn cleanup_phase_is_durable_and_idempotently_cleared() {
        let directory =
            std::env::temp_dir().join(format!("amarcode-cleanup-marker-{}", std::process::id()));
        let marker = directory.join("cleanup-marker");

        assert_eq!(read_cleanup_phase(&marker).unwrap(), None);
        write_cleanup_phase(&marker, CleanupPhase::Started).unwrap();
        assert_eq!(
            read_cleanup_phase(&marker).unwrap(),
            Some(CleanupPhase::Started)
        );
        write_cleanup_phase(&marker, CleanupPhase::DaemonPurged).unwrap();
        assert_eq!(
            read_cleanup_phase(&marker).unwrap(),
            Some(CleanupPhase::DaemonPurged)
        );
        clear_cleanup_marker(&marker).unwrap();
        clear_cleanup_marker(&marker).unwrap();

        std::fs::remove_dir_all(directory).unwrap();
    }

    #[test]
    fn release_cache_removal_is_scoped_and_idempotent() {
        let app_data =
            std::env::temp_dir().join(format!("amarcode-cleanup-cache-{}", std::process::id()));
        let install_root = app_data.join("daemon");
        std::fs::create_dir_all(&install_root).unwrap();
        std::fs::write(install_root.join("release"), b"cached").unwrap();

        remove_install_root(&install_root, &app_data).unwrap();
        remove_install_root(&install_root, &app_data).unwrap();
        assert!(!install_root.exists());
        assert!(remove_install_root(&app_data.join("other"), &app_data).is_err());

        std::fs::remove_dir_all(app_data).unwrap();
    }

    #[cfg(unix)]
    #[test]
    fn release_cache_removal_refuses_symlinks() {
        use std::os::unix::fs::symlink;

        let app_data = std::env::temp_dir().join(format!(
            "amarcode-cleanup-cache-link-{}",
            std::process::id()
        ));
        let real = app_data.join("real");
        let install_root = app_data.join("daemon");
        std::fs::create_dir_all(&real).unwrap();
        symlink(&real, &install_root).unwrap();

        assert!(remove_install_root(&install_root, &app_data).is_err());
        assert!(real.is_dir());

        std::fs::remove_file(install_root).unwrap();
        std::fs::remove_dir_all(app_data).unwrap();
    }

    #[test]
    fn update_ipc_contract_uses_camel_case_fields() {
        let value = serde_json::to_value(DaemonUpdateCheck::Available {
            current_version: "0.3.1".into(),
            version: "0.3.2".into(),
        })
        .expect("serialize update check");
        assert_eq!(value["status"], "available");
        assert_eq!(value["currentVersion"], "0.3.1");
        assert_eq!(value["version"], "0.3.2");
        assert!(value.get("current_version").is_none());
    }

    #[test]
    fn cleanup_ipc_contract_uses_camel_case_fields() {
        let status = serde_json::to_value(ApplicationCleanupStatus::RemovingServiceAndData)
            .expect("serialize cleanup status");
        assert_eq!(status["status"], "removingServiceAndData");

        let result = serde_json::to_value(ApplicationCleanupResult {
            service_and_daemon_data_removed: true,
            daemon_release_cache_removed: true,
        })
        .expect("serialize cleanup result");
        assert_eq!(result["serviceAndDaemonDataRemoved"], true);
        assert_eq!(result["daemonReleaseCacheRemoved"], true);
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
