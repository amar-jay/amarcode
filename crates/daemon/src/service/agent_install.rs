//! Install registry agents from their distribution kind.
//!
//! - `npx` → prefetch with `bun add` into Bun's install cache; launch stays `bunx`
//! - `uvx` → prefetch with `uv pip install --target`; launch stays `uvx`
//! - `binary` → download/extract into `{tools}/agents/<id>/<version>/`, rewrite
//!   the stored command to the absolute extracted executable

use std::{
    fs,
    path::{Path, PathBuf},
    process::Command,
    sync::{Mutex, MutexGuard, TryLockError},
    time::Duration,
};

use crate::{
    protocol::{rpc::InstallAgentResult, AgentRuntimeStatus},
    registry::{self, BinaryDistribution, PackageDistribution, RegistryAgent},
    store::AgentDefinition,
    Error, Result,
};

use super::agent_manager::AgentManager;

const INSTALL_TIMEOUT: Duration = Duration::from_secs(10 * 60);

impl AgentManager {
    /// Bring agents that were installed before registry synchronization up to
    /// the versions pinned by the current registry checkout.
    ///
    /// This is best-effort so an offline package registry never prevents the
    /// daemon from starting with the last usable versions.
    pub fn update_installed(&self, installed_agents: &[AgentDefinition]) {
        for previous in installed_agents {
            if self.shutdown.requested() {
                break;
            }
            let agent_id = &previous.id;
            if !self
                .registry_dir()
                .join(agent_id)
                .join("agent.json")
                .is_file()
            {
                continue;
            }
            let lock = match self.registry_install_lock(agent_id) {
                Ok(lock) => lock,
                Err(error) => {
                    tracing::warn!(%agent_id, %error, "could not lock agent update");
                    continue;
                }
            };
            let _guard = match install_guard(&lock, &self.shutdown) {
                Ok(guard) => guard,
                Err(error) => {
                    tracing::warn!(%agent_id, %error, "could not lock agent update");
                    continue;
                }
            };
            // A manual install may have succeeded since startup's snapshot was
            // taken. Never restore that stale snapshot over a newer binary.
            let previous = match self.get(agent_id) {
                Ok(Some(current)) if Path::new(&current.command).is_absolute() => current,
                _ => previous.clone(),
            };
            match self.materialize_current(agent_id) {
                Ok(true) => tracing::info!(%agent_id, "updated installed ACP agent"),
                Ok(false) => {}
                Err(error) => {
                    if let Err(restore_error) = self.save(&previous) {
                        tracing::warn!(
                            %agent_id,
                            %error,
                            %restore_error,
                            "failed updating ACP agent and restoring its previous definition"
                        );
                    } else {
                        if self.shutdown.requested() {
                            tracing::debug!(%agent_id, "cancelled agent update during shutdown; restored previous installation");
                            break;
                        }
                        tracing::warn!(
                            %agent_id,
                            %error,
                            "failed updating installed ACP agent; restored previous installation"
                        );
                    }
                }
            }
        }
        if self.shutdown.requested() {
            return;
        }
        if let Err(error) = self.refresh_availability() {
            tracing::warn!(%error, "failed refreshing ACP availability after updates");
        }
    }

    /// Materialize a registry agent so it becomes `available`, then probe ACP.
    pub fn install(&self, agent_id: &str) -> Result<InstallAgentResult> {
        tracing::info!(%agent_id, "starting agent installation");
        let result = self.install_inner(agent_id);
        match &result {
            Ok(installed) if matches!(installed.runtime_status, AgentRuntimeStatus::Ready) => {
                tracing::info!(%agent_id, "agent installation completed");
            }
            Ok(installed) => {
                tracing::warn!(%agent_id, status = ?installed.runtime_status,
                    detail = ?installed.runtime_message, "agent installed but runtime is not ready");
            }
            Err(error) => tracing::error!(%agent_id, %error, "agent installation failed"),
        }
        result
    }

    fn install_inner(&self, agent_id: &str) -> Result<InstallAgentResult> {
        if agent_id == super::amarcode_acp::AGENT_ID {
            self.install_amarcode_acp()?;
            return self.builtin_install_result();
        }
        let lock = self.registry_install_lock(agent_id)?;
        let _guard = install_guard(&lock, &self.shutdown)?;
        self.materialize_current(agent_id)?;

        self.refresh_availability()?;
        let updated = self
            .get(agent_id)?
            .ok_or_else(|| Error::msg(format!("agent missing after install: {agent_id}")))?;
        if !self.is_available(&updated) {
            return Err(Error::msg(format!(
                "installed {} but it is still unavailable",
                updated.name
            )));
        }

        Ok(InstallAgentResult {
            agent: self.agent_info(&updated)?,
            runtime_status: AgentRuntimeStatus::Ready,
            runtime_message: None,
        })
    }

    // Installing this public native release has no sign-in/provider setup step.
    // Provider readiness is checked separately when resolving a chat launch.
    fn builtin_install_result(&self) -> Result<InstallAgentResult> {
        self.refresh_availability()?;
        let agent = self
            .get(super::amarcode_acp::AGENT_ID)?
            .ok_or_else(|| Error::msg("built-in agent missing after install"))?;
        if !self.is_available(&agent) {
            return Err(Error::msg(
                "Amarcode ACP executable is missing after installation",
            ));
        }
        Ok(InstallAgentResult {
            agent: self.agent_info(&agent)?,
            runtime_status: AgentRuntimeStatus::Ready,
            runtime_message: None,
        })
    }

    /// Returns whether files or package caches were changed.
    /// Caller holds the per-agent lock through publication and any rollback.
    fn materialize_current(&self, agent_id: &str) -> Result<bool> {
        self.shutdown.check()?;
        let agent = self
            .get(agent_id)?
            .ok_or_else(|| Error::msg(format!("agent not found: {agent_id}")))?;
        let manifest = registry::load_agent_manifest(self.registry_dir(), agent_id)?;
        match install_kind(&manifest)? {
            InstallKind::Npx(package) => {
                if self.is_available(&agent) {
                    return Ok(false);
                }
                ensure_runner("bun", &["bun", "bunx"])?;
                prefetch_bun_package(&package.package, &self.shutdown)?;
            }
            InstallKind::Uvx(package) => {
                if self.is_available(&agent) {
                    return Ok(false);
                }
                ensure_runner("uv", &["uv", "uvx"])?;
                prefetch_uv_package(&package.package, &self.shutdown)?;
            }
            InstallKind::Binary(binary) => {
                let expected =
                    binary_executable_path(self.tools_dir(), agent_id, &manifest.version, &binary);
                if Path::new(&agent.command) == expected && expected.is_file() {
                    return Ok(false);
                }
                let executable = install_binary_distribution(
                    self.tools_dir(),
                    agent_id,
                    &manifest.version,
                    &binary,
                    &self.shutdown,
                )?;
                let mut updated = agent;
                updated.command = executable.to_string_lossy().into_owned();
                updated.arguments = binary.args;
                updated.environment = binary.env.into_iter().collect();
                updated.available = false;
                self.shutdown.check()?;
                self.save(&updated)?;
            }
        }
        Ok(true)
    }
}

// Waiters must remain cancellable even if another installation is stalled.
fn install_guard<'a>(
    lock: &'a Mutex<()>,
    shutdown: &crate::shutdown::Shutdown,
) -> Result<MutexGuard<'a, ()>> {
    loop {
        shutdown.check()?;
        match lock.try_lock() {
            Ok(guard) => return Ok(guard),
            Err(TryLockError::Poisoned(_)) => {
                return Err(Error::msg("Agent installation lock poisoned"))
            }
            Err(TryLockError::WouldBlock) => std::thread::sleep(Duration::from_millis(25)),
        }
    }
}

#[derive(Debug)]
enum InstallKind {
    Npx(PackageDistribution),
    Uvx(PackageDistribution),
    Binary(BinaryDistribution),
}

fn install_kind(manifest: &RegistryAgent) -> Result<InstallKind> {
    if let Some(package) = manifest.distribution.npx.clone() {
        return Ok(InstallKind::Npx(package));
    }
    if let Some(package) = manifest.distribution.uvx.clone() {
        return Ok(InstallKind::Uvx(package));
    }
    let target = registry::current_binary_target()
        .ok_or_else(|| Error::msg("no binary distribution is defined for this platform"))?;
    let binary = manifest
        .distribution
        .binary
        .as_ref()
        .and_then(|map| map.get(target).cloned())
        .ok_or_else(|| {
            Error::msg(format!(
                "agent {} has no binary distribution for {target}",
                manifest.id
            ))
        })?;
    Ok(InstallKind::Binary(binary))
}

fn ensure_runner(label: &str, names: &[&str]) -> Result<()> {
    if names.iter().any(|name| which(name).is_some()) {
        return Ok(());
    }
    Err(Error::msg(format!(
        "{label} is required to install this agent"
    )))
}

fn which(name: &str) -> Option<PathBuf> {
    let search_path = std::env::var_os("PATH")?;
    std::env::split_paths(&search_path).find_map(|directory| {
        let candidate = directory.join(name);
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            if candidate
                .metadata()
                .map(|metadata| metadata.is_file() && metadata.permissions().mode() & 0o111 != 0)
                .unwrap_or(false)
            {
                return Some(candidate);
            }
        }
        #[cfg(not(unix))]
        {
            if candidate.is_file() {
                return Some(candidate);
            }
            let with_exe = candidate.with_extension("exe");
            if with_exe.is_file() {
                return Some(with_exe);
            }
        }
        None
    })
}

fn prefetch_bun_package(package: &str, shutdown: &crate::shutdown::Shutdown) -> Result<()> {
    let temp = temp_dir("bun-prefetch")?;
    write_minimal_package_json(&temp)?;
    let result = run_timed(
        Command::new("bun")
            .arg("add")
            .arg(package)
            .current_dir(&temp),
        INSTALL_TIMEOUT,
        &format!("bun add {package}"),
        shutdown,
    );
    let _ = fs::remove_dir_all(&temp);
    result.map(|_| ())
}

fn prefetch_uv_package(package: &str, shutdown: &crate::shutdown::Shutdown) -> Result<()> {
    let temp = temp_dir("uv-prefetch")?;
    let requirement = uv_requirement(package);
    let result = run_timed(
        Command::new("uv")
            .arg("pip")
            .arg("install")
            .arg("--target")
            .arg(&temp)
            .arg(&requirement),
        INSTALL_TIMEOUT,
        &format!("uv pip install {requirement}"),
        shutdown,
    );
    let _ = fs::remove_dir_all(&temp);
    result.map(|_| ())
}

fn uv_requirement(package: &str) -> String {
    if package.contains("==") {
        return package.to_owned();
    }
    if let Some((name, version)) = package.rsplit_once('@') {
        if !name.is_empty() && !version.is_empty() && !name.starts_with('@') {
            return format!("{name}=={version}");
        }
    }
    package.to_owned()
}

fn install_binary_distribution(
    tools_dir: &Path,
    agent_id: &str,
    version: &str,
    binary: &BinaryDistribution,
    shutdown: &crate::shutdown::Shutdown,
) -> Result<PathBuf> {
    let dest = tools_dir.join("agents").join(agent_id).join(version);
    let parent = dest
        .parent()
        .ok_or_else(|| Error::msg("Missing agent install parent"))?;
    fs::create_dir_all(parent)?;
    // Extract on the same filesystem, without touching the published version.
    let staging = tempfile::Builder::new()
        .prefix(".install-")
        .tempdir_in(parent)?;
    let payload = staging.path().join("payload");
    fs::create_dir(&payload)?;
    let command = PathBuf::from(binary.cmd.trim());
    if command.is_absolute()
        || command.components().any(|part| {
            matches!(
                part,
                std::path::Component::ParentDir
                    | std::path::Component::RootDir
                    | std::path::Component::Prefix(_)
            )
        })
    {
        return Err(Error::msg(
            "Binary command must be relative to its installation",
        ));
    }
    let archive_name = archive_file_name(&binary.archive);
    let archive_path = staging.path().join(&archive_name);
    shutdown.check()?;
    download_file(&binary.archive, &archive_path, shutdown)?;
    if let Some(expected) = binary.sha256.as_deref() {
        verify_sha256(&archive_path, expected, shutdown)?;
    }
    extract_archive(&archive_path, &payload, shutdown)?;
    let _ = fs::remove_file(&archive_path);

    let executable = payload.join(&command);
    if !executable.is_file() {
        return Err(Error::msg(format!(
            "installed archive for {agent_id} is missing executable {}",
            executable.display()
        )));
    }
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        let mut permissions = executable
            .metadata()
            .map_err(|error| Error::msg(error.to_string()))?
            .permissions();
        permissions.set_mode(permissions.mode() | 0o755);
        fs::set_permissions(&executable, permissions)
            .map_err(|error| Error::msg(error.to_string()))?;
    }
    shutdown.check()?;
    publish_binary_install(&payload, &dest)?;
    Ok(binary_executable_path(tools_dir, agent_id, version, binary))
}

// Called while holding the per-agent lock. Preserve the previous directory if
// publishing fails, and never delete its files during download or extraction.
fn publish_binary_install(payload: &Path, destination: &Path) -> Result<()> {
    let backup = destination.with_file_name(format!(".previous-{}", uuid::Uuid::new_v4()));
    let replaced = destination.exists();
    if replaced {
        fs::rename(destination, &backup)?;
    }
    if let Err(error) = fs::rename(payload, destination) {
        if replaced {
            fs::rename(&backup, destination).map_err(|restore| Error::msg(format!(
                "Failed publishing agent: {error}; restore failed: {restore}; previous installation retained at {}", backup.display()
            )))?;
        }
        return Err(error.into());
    }
    if replaced {
        if let Err(error) = fs::remove_dir_all(&backup) {
            tracing::warn!(%error, path = %backup.display(), "could not remove previous agent installation");
        }
    }
    Ok(())
}

fn binary_executable_path(
    tools_dir: &Path,
    agent_id: &str,
    version: &str,
    binary: &BinaryDistribution,
) -> PathBuf {
    let command = PathBuf::from(binary.cmd.trim());
    if command.is_absolute() {
        command
    } else {
        tools_dir
            .join("agents")
            .join(agent_id)
            .join(version)
            .join(command.strip_prefix("./").unwrap_or(&command))
    }
}

fn archive_file_name(url: &str) -> String {
    url.rsplit('/')
        .next()
        .filter(|name| !name.is_empty())
        .unwrap_or("archive.bin")
        .split('?')
        .next()
        .unwrap_or("archive.bin")
        .to_owned()
}

fn download_file(
    url: &str,
    destination: &Path,
    shutdown: &crate::shutdown::Shutdown,
) -> Result<()> {
    let status = run_timed(
        Command::new("curl")
            .args(["-fsSL", "--connect-timeout", "30", "-o"])
            .arg(destination)
            .arg(url),
        INSTALL_TIMEOUT,
        &format!("download {url}"),
        shutdown,
    )?;
    if status.success() && destination.is_file() {
        Ok(())
    } else {
        Err(Error::msg(format!("failed to download {url}")))
    }
}

fn verify_sha256(path: &Path, expected: &str, shutdown: &crate::shutdown::Shutdown) -> Result<()> {
    use sha2::{Digest, Sha256};
    use std::io::Read;
    let mut file = fs::File::open(path)?;
    let mut hash = Sha256::new();
    let mut buffer = [0; 64 * 1024];
    loop {
        shutdown.check()?;
        let count = file.read(&mut buffer)?;
        if count == 0 {
            break;
        }
        hash.update(&buffer[..count]);
    }
    let actual = format!("{:x}", hash.finalize());
    if actual != expected.to_ascii_lowercase() {
        return Err(Error::msg(format!(
            "checksum mismatch for {}: expected {expected}, got {actual}",
            path.display()
        )));
    }
    Ok(())
}

fn extract_archive(
    archive: &Path,
    destination: &Path,
    shutdown: &crate::shutdown::Shutdown,
) -> Result<()> {
    shutdown.check()?;
    let name = archive
        .file_name()
        .and_then(|value| value.to_str())
        .unwrap_or_default()
        .to_ascii_lowercase();
    if name.ends_with(".zip") {
        #[cfg(windows)]
        let mut command = {
            let mut command = Command::new("tar");
            command
                .args(["-xf"])
                .arg(archive)
                .arg("-C")
                .arg(destination);
            command
        };
        #[cfg(not(windows))]
        let mut command = {
            let mut command = Command::new("unzip");
            command
                .args(["-qo"])
                .arg(archive)
                .arg("-d")
                .arg(destination);
            command
        };
        let status = run_timed(
            &mut command,
            INSTALL_TIMEOUT,
            "extract zip archive",
            shutdown,
        )?;
        return if status.success() {
            Ok(())
        } else {
            Err(Error::msg(format!(
                "failed to extract {}",
                archive.display()
            )))
        };
    }
    if name.ends_with(".tar.gz")
        || name.ends_with(".tgz")
        || name.ends_with(".tar.bz2")
        || name.ends_with(".tbz2")
        || name.ends_with(".tar")
    {
        let status = run_timed(
            Command::new("tar")
                .args(["-xf"])
                .arg(archive)
                .arg("-C")
                .arg(destination),
            INSTALL_TIMEOUT,
            "extract archive",
            shutdown,
        )?;
        return if status.success() {
            Ok(())
        } else {
            Err(Error::msg(format!(
                "failed to extract {}",
                archive.display()
            )))
        };
    }

    let target = destination.join(archive.file_stem().unwrap_or_default());
    fs::rename(archive, &target).map_err(|error| {
        Error::msg(format!(
            "failed to place binary {}: {error}",
            target.display()
        ))
    })?;
    Ok(())
}

fn write_minimal_package_json(directory: &Path) -> Result<()> {
    fs::write(
        directory.join("package.json"),
        r#"{"name":"amarcode-agent-prefetch","private":true}"#,
    )
    .map_err(|error| Error::msg(format!("failed to write package.json: {error}")))
}

fn temp_dir(prefix: &str) -> Result<PathBuf> {
    let path = std::env::temp_dir().join(format!("amarcode-{prefix}-{}", uuid::Uuid::new_v4()));
    fs::create_dir_all(&path).map_err(|error| {
        Error::msg(format!(
            "failed to create temp directory {}: {error}",
            path.display()
        ))
    })?;
    Ok(path)
}

const INSTALL_OUTPUT_TAIL_BYTES: usize = 16 * 1024;
type OutputTail = std::sync::Arc<std::sync::Mutex<std::collections::VecDeque<u8>>>;

fn drain_install_output(
    reader: impl std::io::Read + Send + 'static,
) -> (OutputTail, std::thread::JoinHandle<()>) {
    let tail = std::sync::Arc::new(std::sync::Mutex::new(std::collections::VecDeque::new()));
    let output = std::sync::Arc::clone(&tail);
    let worker = std::thread::spawn(move || {
        let mut reader = reader;
        let mut buffer = [0u8; 4096];
        while let Ok(count) = reader.read(&mut buffer) {
            if count == 0 {
                break;
            }
            if let Ok(mut tail) = output.lock() {
                tail.extend(&buffer[..count]);
                let excess = tail.len().saturating_sub(INSTALL_OUTPUT_TAIL_BYTES);
                tail.drain(..excess);
            }
        }
    });
    (tail, worker)
}

fn run_timed(
    command: &mut Command,
    timeout: Duration,
    label: &str,
    shutdown: &crate::shutdown::Shutdown,
) -> Result<std::process::ExitStatus> {
    shutdown.check()?;
    #[cfg(windows)]
    {
        use std::os::windows::process::CommandExt;
        command.creation_flags(0x0800_0000); // CREATE_NO_WINDOW
    }
    #[cfg(unix)]
    {
        use std::os::unix::process::CommandExt;
        command.process_group(0);
    }
    let mut child = command
        .stdin(std::process::Stdio::null())
        .stdout(std::process::Stdio::piped())
        .stderr(std::process::Stdio::piped())
        .spawn()
        .map_err(|error| Error::msg(format!("failed to start {label}: {error}")))?;
    // Drain both pipes concurrently: package managers can otherwise fill a pipe
    // and block forever while the parent waits for their exit status.
    let (stdout, stdout_worker) = drain_install_output(child.stdout.take().expect("piped stdout"));
    let (stderr, stderr_worker) = drain_install_output(child.stderr.take().expect("piped stderr"));
    let started = std::time::Instant::now();
    let result = loop {
        match child.try_wait() {
            Ok(Some(status)) => break Ok(status),
            Ok(None) if shutdown.requested() || started.elapsed() > timeout => {
                #[cfg(unix)]
                {
                    let _ = nix::sys::signal::killpg(
                        nix::unistd::Pid::from_raw(child.id() as i32),
                        nix::sys::signal::Signal::SIGKILL,
                    );
                }
                let _ = child.kill();
                let _ = child.wait();
                if shutdown.requested() {
                    break Err(Error::msg(format!(
                        "{label} cancelled: daemon is shutting down"
                    )));
                }
                break Err(Error::msg(format!(
                    "{label} timed out after {}s",
                    timeout.as_secs()
                )));
            }
            Ok(None) => std::thread::sleep(Duration::from_millis(25)),
            Err(error) => {
                let _ = child.kill();
                let _ = child.wait();
                break Err(Error::msg(format!("failed waiting for {label}: {error}")));
            }
        }
    };
    // Do not let an inherited pipe held by a background grandchild stall RPC.
    let drain_deadline = std::time::Instant::now() + Duration::from_millis(250);
    while !(stdout_worker.is_finished() && stderr_worker.is_finished())
        && std::time::Instant::now() < drain_deadline
    {
        std::thread::sleep(Duration::from_millis(5));
    }
    if stdout_worker.is_finished() {
        let _ = stdout_worker.join();
    }
    if stderr_worker.is_finished() {
        let _ = stderr_worker.join();
    }
    let output = |tail: OutputTail| {
        let bytes = tail
            .lock()
            .map(|tail| tail.iter().copied().collect::<Vec<_>>())
            .unwrap_or_default();
        String::from_utf8_lossy(&bytes).trim().to_owned()
    };
    let stderr = output(stderr);
    let stdout = output(stdout);
    let failure = match result {
        Ok(status) if status.success() => return Ok(status),
        Ok(status) => format!("{label} exited with {status}"),
        Err(error) => error.to_string(),
    };
    let detail = [
        (!stderr.is_empty()).then(|| format!("stderr: {stderr}")),
        (!stdout.is_empty()).then(|| format!("stdout: {stdout}")),
    ]
    .into_iter()
    .flatten()
    .collect::<Vec<_>>()
    .join("\n");
    Err(Error::msg(if detail.is_empty() {
        failure
    } else {
        format!("{failure}\n{detail}")
    }))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[cfg(unix)]
    fn binary_manager(root: &Path, url: &str) -> (AgentManager, AgentDefinition) {
        let store =
            std::sync::Arc::new(crate::store::Store::open(&root.join("store.sqlite3")).unwrap());
        let manager = AgentManager::new(store, root);
        let manifest = root.join("acp-registry/binary-agent");
        fs::create_dir_all(&manifest).unwrap();
        fs::write(
            manifest.join("agent.json"),
            serde_json::to_vec(&serde_json::json!({
                "id": "binary-agent", "name": "Binary Agent", "version": "1.0.0",
                "distribution": {"binary": {registry::current_binary_target().unwrap(): {
                    "archive": url, "cmd": "agent"
                }}}
            }))
            .unwrap(),
        )
        .unwrap();
        let agent = AgentDefinition {
            id: "binary-agent".into(),
            name: "Binary Agent".into(),
            command: "agent".into(),
            arguments: vec![],
            environment: vec![],
            available: false,
            created_at: "2026-01-01".into(),
            updated_at: "2026-01-01".into(),
        };
        manager.save(&agent).unwrap();
        (manager, agent)
    }

    #[cfg(unix)]
    #[test]
    fn concurrent_manual_installs_and_background_updates_download_once() {
        use std::{
            io::{Read, Write},
            net::TcpListener,
            sync::{
                atomic::{AtomicUsize, Ordering},
                mpsc, Arc,
            },
        };
        let root = tempfile::tempdir().unwrap();
        let listener = TcpListener::bind("127.0.0.1:0").unwrap();
        listener.set_nonblocking(true).unwrap();
        let url = format!("http://{}/agent", listener.local_addr().unwrap());
        let (manager, previous) = binary_manager(root.path(), &url);
        let count = Arc::new(AtomicUsize::new(0));
        let downloads = Arc::clone(&count);
        let (started, downloading) = mpsc::channel();
        let (release, wait) = mpsc::channel();
        let (done, finished) = mpsc::channel();
        let http = std::thread::spawn(move || {
            let deadline = std::time::Instant::now() + Duration::from_secs(5);
            loop {
                match listener.accept() {
                    Ok((mut stream, _)) => {
                        stream
                            .set_read_timeout(Some(Duration::from_secs(2)))
                            .unwrap();
                        let mut header = Vec::new();
                        while !header.ends_with(b"\r\n\r\n") {
                            let mut byte = [0];
                            stream.read_exact(&mut byte).unwrap();
                            header.push(byte[0]);
                        }
                        if downloads.fetch_add(1, Ordering::SeqCst) == 0 {
                            started.send(()).unwrap();
                            wait.recv_timeout(Duration::from_secs(5)).unwrap();
                        }
                        let body = b"#!/bin/sh\nexit 0\n";
                        write!(
                            stream,
                            "HTTP/1.1 200 OK\r\nContent-Length: {}\r\nConnection: close\r\n\r\n",
                            body.len()
                        )
                        .unwrap();
                        stream.write_all(body).unwrap();
                    }
                    Err(error) if error.kind() == std::io::ErrorKind::WouldBlock => {
                        if finished.try_recv().is_ok() {
                            break;
                        }
                        assert!(std::time::Instant::now() < deadline, "install stalled");
                        std::thread::sleep(Duration::from_millis(5));
                    }
                    Err(error) => panic!("{error}"),
                }
            }
        });
        let first = manager.clone();
        let first = std::thread::spawn(move || first.install("binary-agent"));
        downloading.recv_timeout(Duration::from_secs(3)).unwrap();
        let second = manager.clone();
        let second = std::thread::spawn(move || second.install("binary-agent"));
        let updater = manager.clone();
        let updater = std::thread::spawn(move || updater.update_installed(&[previous]));
        // Keep the first download in flight while competing callers run.
        std::thread::sleep(Duration::from_millis(100));
        release.send(()).unwrap();
        assert!(first.join().unwrap().unwrap().agent.available);
        assert!(second.join().unwrap().unwrap().agent.available);
        updater.join().unwrap();
        done.send(()).unwrap();
        http.join().unwrap();
        assert_eq!(count.load(Ordering::SeqCst), 1);
        let installed = manager.get("binary-agent").unwrap().unwrap();
        assert_eq!(
            fs::read(&installed.command).unwrap(),
            b"#!/bin/sh\nexit 0\n"
        );
        assert!(manager.is_available(&installed));
    }

    #[cfg(unix)]
    #[test]
    fn failed_staged_install_preserves_published_files_and_latest_definition() {
        let root = tempfile::tempdir().unwrap();
        let download = root.path().join("agent");
        fs::write(&download, "#!/bin/sh\nexit 0\n").unwrap();
        let (manager, stale) =
            binary_manager(root.path(), &format!("file://{}", download.display()));
        manager.install("binary-agent").unwrap();
        let installed = manager.get("binary-agent").unwrap().unwrap();
        let manifest = root.path().join("acp-registry/binary-agent/agent.json");
        let mut value: serde_json::Value =
            serde_json::from_slice(&fs::read(&manifest).unwrap()).unwrap();
        value["distribution"]["binary"][registry::current_binary_target().unwrap()]["sha256"] =
            "0".repeat(64).into();
        fs::write(manifest, serde_json::to_vec(&value).unwrap()).unwrap();
        // Exercise failure at the same version, where deleting the destination
        // up front would destroy the working executable before checksum validation.
        let binary = install_kind(
            &registry::load_agent_manifest(manager.registry_dir(), "binary-agent").unwrap(),
        )
        .unwrap();
        let InstallKind::Binary(binary) = binary else {
            unreachable!()
        };
        assert!(install_binary_distribution(
            manager.tools_dir(),
            "binary-agent",
            "1.0.0",
            &binary,
            &manager.shutdown
        )
        .is_err());
        assert_eq!(
            fs::read(&installed.command).unwrap(),
            b"#!/bin/sh\nexit 0\n"
        );
        // A stale startup snapshot must not overwrite a later successful install
        // when an update to a different version fails.
        let mut value: serde_json::Value = serde_json::from_slice(
            &fs::read(root.path().join("acp-registry/binary-agent/agent.json")).unwrap(),
        )
        .unwrap();
        value["version"] = "2.0.0".into();
        fs::write(
            root.path().join("acp-registry/binary-agent/agent.json"),
            serde_json::to_vec(&value).unwrap(),
        )
        .unwrap();
        manager.update_installed(&[stale]);
        assert_eq!(
            manager.get("binary-agent").unwrap().unwrap().command,
            installed.command
        );
        assert!(manager.is_available(&installed));
        let parent = Path::new(&installed.command)
            .parent()
            .unwrap()
            .parent()
            .unwrap();
        assert!(fs::read_dir(parent).unwrap().all(|entry| !entry
            .unwrap()
            .file_name()
            .to_string_lossy()
            .starts_with(".install-")));
    }

    #[test]
    fn failed_publish_restores_previous_directory() {
        let root = tempfile::tempdir().unwrap();
        let destination = root.path().join("1.0.0");
        fs::create_dir(&destination).unwrap();
        fs::write(destination.join("agent"), "working").unwrap();
        assert!(
            publish_binary_install(&root.path().join("missing-payload"), &destination).is_err()
        );
        assert_eq!(
            fs::read_to_string(destination.join("agent")).unwrap(),
            "working"
        );
    }

    #[test]
    fn installation_lock_wait_is_cancellable_and_other_agents_are_independent() {
        use std::sync::{mpsc, Arc};
        let root = tempfile::tempdir().unwrap();
        let manager = AgentManager::new(
            Arc::new(crate::store::Store::open(&root.path().join("store.sqlite3")).unwrap()),
            root.path(),
        );
        let lock = manager.registry_install_lock("one").unwrap();
        let _held = lock.lock().unwrap();
        let clone = manager.clone();
        let waiting = clone.registry_install_lock("one").unwrap();
        assert!(Arc::ptr_eq(&lock, &waiting));
        let other = clone.registry_install_lock("two").unwrap();
        assert!(other.try_lock().is_ok());
        let shutdown = manager.shutdown.clone();
        let (send, receive) = mpsc::channel();
        let worker = std::thread::spawn(move || {
            send.send(install_guard(&waiting, &shutdown).is_err())
                .unwrap();
        });
        manager.shutdown.cancel();
        assert!(receive.recv_timeout(Duration::from_secs(1)).unwrap());
        worker.join().unwrap();
    }

    #[test]
    fn builtin_install_needs_no_sign_in_provider_or_runtime_probe() {
        use std::sync::Arc;
        let root = tempfile::tempdir().unwrap();
        let manager = AgentManager::new(
            Arc::new(crate::store::Store::open(&root.path().join("store.sqlite3")).unwrap()),
            root.path(),
        );
        manager.sync_builtin_preset().unwrap();
        assert!(
            !manager
                .get(super::super::amarcode_acp::AGENT_ID)
                .unwrap()
                .unwrap()
                .available
        );
        let executable = root.path().join(if cfg!(windows) {
            "amarcode-acp.exe"
        } else {
            "amarcode-acp"
        });
        // A runtime probe would fail; installation must not start the adapter.
        std::fs::write(&executable, "#!/bin/sh\nexit 99\n").unwrap();
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            std::fs::set_permissions(&executable, std::fs::Permissions::from_mode(0o700)).unwrap();
        }
        manager.save_builtin_definition(&executable).unwrap();
        let result = manager.builtin_install_result().unwrap();
        assert!(matches!(result.runtime_status, AgentRuntimeStatus::Ready));
        assert!(result.runtime_message.is_none());
        assert!(result.agent.available);
        assert!(result.agent.unavailable_reason.is_none());
        assert!(manager.amarcode_acp_config_path().exists());
        // Fresh installations can chat through the keyless default immediately.
        assert!(manager
            .resolve(super::super::amarcode_acp::AGENT_ID)
            .is_ok());
        manager
            .set_amarcode_acp_config(
                "https://example.com/v1".into(),
                "model".into(),
                "key".into(),
                false,
            )
            .unwrap();
        assert!(manager
            .resolve(super::super::amarcode_acp::AGENT_ID)
            .is_ok());
        manager
            .set_amarcode_acp_config(
                "https://example.com/v1".into(),
                "model".into(),
                String::new(),
                true,
            )
            .unwrap();
        let installed = manager
            .list()
            .unwrap()
            .into_iter()
            .find(|agent| agent.id == super::super::amarcode_acp::AGENT_ID)
            .unwrap();
        assert!(
            installed.available,
            "clearing provider settings must not trigger another download"
        );
    }

    #[cfg(unix)]
    #[test]
    fn install_commands_drain_large_output_and_return_bounded_failure_details() {
        let mut command = Command::new("sh");
        command.args(["-c", "head -c 262144 /dev/zero; head -c 262144 /dev/zero >&2; printf 'registry-install-failed' >&2; exit 7"]);
        let error = run_timed(
            &mut command,
            Duration::from_secs(5),
            "test install",
            &crate::shutdown::Shutdown::default(),
        )
        .unwrap_err()
        .to_string();
        assert!(error.contains("registry-install-failed"));
        assert!(error.contains("exit status: 7"));
        assert!(error.len() < 2 * INSTALL_OUTPUT_TAIL_BYTES + 256);
    }

    #[cfg(unix)]
    #[test]
    fn install_command_timeout_kills_children_and_retains_the_error_tail() {
        let mut command = Command::new("sh");
        command.args(["-c", "printf 'download stalled' >&2; sleep 10"]);
        let started = std::time::Instant::now();
        let error = run_timed(
            &mut command,
            Duration::from_millis(100),
            "test install",
            &crate::shutdown::Shutdown::default(),
        )
        .unwrap_err()
        .to_string();
        assert!(error.contains("timed out"));
        assert!(error.contains("download stalled"));
        assert!(started.elapsed() < Duration::from_secs(2));
    }

    #[cfg(unix)]
    #[test]
    fn shutdown_cancels_running_install_commands_and_prevents_new_ones() {
        let root = tempfile::tempdir().unwrap();
        let marker = root.path().join("started");
        let shutdown = crate::shutdown::Shutdown::default();
        let token = shutdown.clone();
        let marker_path = marker.clone();
        let worker = std::thread::spawn(move || {
            let mut command = Command::new("sh");
            command
                .args([
                    "-c",
                    "touch \"$1\"; printf 'still downloading' >&2; sleep 60",
                    "sh",
                ])
                .arg(marker_path);
            run_timed(
                &mut command,
                Duration::from_secs(600),
                "test install",
                &token,
            )
        });
        let deadline = std::time::Instant::now() + Duration::from_secs(3);
        while !marker.exists() {
            assert!(std::time::Instant::now() < deadline);
            std::thread::sleep(Duration::from_millis(5));
        }
        let now = std::time::Instant::now();
        shutdown.cancel();
        let error = worker.join().unwrap().unwrap_err().to_string();
        assert!(now.elapsed() < Duration::from_secs(2));
        assert!(
            error.contains("cancelled") && error.contains("still downloading"),
            "{error}"
        );
        let next = root.path().join("next");
        let mut command = Command::new("touch");
        command.arg(&next);
        assert!(run_timed(
            &mut command,
            Duration::from_secs(600),
            "next install",
            &shutdown
        )
        .is_err());
        assert!(!next.exists());
    }

    #[test]
    fn converts_uvx_at_version_to_pip_requirement() {
        assert_eq!(uv_requirement("minion-code@0.1.44"), "minion-code==0.1.44");
        assert_eq!(
            uv_requirement("fast-agent-acp==0.10.1"),
            "fast-agent-acp==0.10.1"
        );
    }

    #[test]
    fn archive_names_strip_query_strings() {
        assert_eq!(
            archive_file_name("https://example.com/app.tar.gz?token=1"),
            "app.tar.gz"
        );
    }

    #[test]
    fn install_kind_prefers_npx_then_uvx() {
        let manifest: RegistryAgent = serde_json::from_value(serde_json::json!({
            "id": "example",
            "name": "Example",
            "version": "1.0.0",
            "distribution": {
                "npx": { "package": "@example/agent@1.0.0" },
                "uvx": { "package": "example@1.0.0" }
            }
        }))
        .unwrap();
        match install_kind(&manifest).unwrap() {
            InstallKind::Npx(package) => assert_eq!(package.package, "@example/agent@1.0.0"),
            other => panic!("unexpected {other:?}"),
        }
    }

    #[test]
    fn bun_prefetch_populates_install_cache() {
        if which("bun").is_none() {
            return;
        }
        let package = "is-number@7.0.0";
        prefetch_bun_package(package, &crate::shutdown::Shutdown::default())
            .expect("bun add should prefetch package");
        let home_cache = dirs::home_dir()
            .expect("home")
            .join(".bun")
            .join("install")
            .join("cache");
        let found = home_cache
            .read_dir()
            .unwrap()
            .filter_map(|entry| entry.ok())
            .any(|entry| {
                let name = entry.file_name();
                let name = name.to_string_lossy();
                name == package || name.starts_with(&format!("{package}@@@"))
            });
        assert!(found, "expected {package} in bun cache");
    }
}
