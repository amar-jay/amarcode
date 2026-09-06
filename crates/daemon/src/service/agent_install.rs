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
    time::Duration,
};

use crate::{
    protocol::AgentInfo,
    registry::{self, BinaryDistribution, PackageDistribution, RegistryAgent},
    Error, Result,
};

use super::agent_manager::AgentManager;

const INSTALL_TIMEOUT: Duration = Duration::from_secs(10 * 60);

impl AgentManager {
    /// Materialize a registry agent so it becomes `available`.
    pub fn install(&self, agent_id: &str) -> Result<AgentInfo> {
        let agent = self
            .get(agent_id)?
            .ok_or_else(|| Error::msg(format!("agent not found: {agent_id}")))?;

        if self.is_available(&agent) {
            self.refresh_availability()?;
            let agent = self.get(agent_id)?.ok_or_else(|| {
                Error::msg(format!("agent missing after refresh: {agent_id}"))
            })?;
            return Ok(self.agent_info(&agent));
        }

        let manifest = registry::load_agent_manifest(self.registry_dir(), agent_id)?;
        match install_kind(&manifest)? {
            InstallKind::Npx(package) => {
                ensure_runner("bun", &["bun", "bunx"])?;
                prefetch_bun_package(&package.package)?;
            }
            InstallKind::Uvx(package) => {
                ensure_runner("uv", &["uv", "uvx"])?;
                prefetch_uv_package(&package.package)?;
            }
            InstallKind::Binary(binary) => {
                let executable = install_binary_distribution(
                    self.tools_dir(),
                    agent_id,
                    &manifest.version,
                    &binary,
                )?;
                let mut updated = agent.clone();
                updated.command = executable.to_string_lossy().into_owned();
                updated.arguments = binary.args;
                updated.environment = binary.env.into_iter().collect();
                updated.available = false;
                self.save(&updated)?;
            }
        }

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
        Ok(self.agent_info(&updated))
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
    let target = registry::current_binary_target().ok_or_else(|| {
        Error::msg("no binary distribution is defined for this platform")
    })?;
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

fn prefetch_bun_package(package: &str) -> Result<()> {
    let temp = temp_dir("bun-prefetch")?;
    write_minimal_package_json(&temp)?;
    let status = run_timed(
        Command::new("bun")
            .arg("add")
            .arg(package)
            .current_dir(&temp),
        INSTALL_TIMEOUT,
        &format!("bun add {package}"),
    )?;
    let _ = fs::remove_dir_all(&temp);
    if status.success() {
        Ok(())
    } else {
        Err(Error::msg(format!(
            "failed to install npm package {package} with bun"
        )))
    }
}

fn prefetch_uv_package(package: &str) -> Result<()> {
    let temp = temp_dir("uv-prefetch")?;
    let requirement = uv_requirement(package);
    let status = run_timed(
        Command::new("uv")
            .arg("pip")
            .arg("install")
            .arg("--target")
            .arg(&temp)
            .arg(&requirement),
        INSTALL_TIMEOUT,
        &format!("uv pip install {requirement}"),
    )?;
    let _ = fs::remove_dir_all(&temp);
    if status.success() {
        Ok(())
    } else {
        Err(Error::msg(format!(
            "failed to install Python package {package} with uv"
        )))
    }
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
) -> Result<PathBuf> {
    let dest = tools_dir.join("agents").join(agent_id).join(version);
    if dest.exists() {
        fs::remove_dir_all(&dest).map_err(|error| {
            Error::msg(format!(
                "failed to clear previous install {}: {error}",
                dest.display()
            ))
        })?;
    }
    fs::create_dir_all(&dest).map_err(|error| {
        Error::msg(format!(
            "failed to create install directory {}: {error}",
            dest.display()
        ))
    })?;

    let archive_name = archive_file_name(&binary.archive);
    let archive_path = dest.join(&archive_name);
    download_file(&binary.archive, &archive_path)?;
    if let Some(expected) = binary.sha256.as_deref() {
        verify_sha256(&archive_path, expected)?;
    }
    extract_archive(&archive_path, &dest)?;
    let _ = fs::remove_file(&archive_path);

    let command = PathBuf::from(binary.cmd.trim());
    let relative = command.strip_prefix("./").unwrap_or(&command);
    let executable = if command.is_absolute() {
        command
    } else {
        dest.join(relative)
    };
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
    Ok(executable)
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

fn download_file(url: &str, destination: &Path) -> Result<()> {
    let status = run_timed(
        Command::new("curl")
            .args(["-fsSL", "--connect-timeout", "30", "-o"])
            .arg(destination)
            .arg(url),
        INSTALL_TIMEOUT,
        &format!("download {url}"),
    )?;
    if status.success() && destination.is_file() {
        Ok(())
    } else {
        Err(Error::msg(format!("failed to download {url}")))
    }
}

fn verify_sha256(path: &Path, expected: &str) -> Result<()> {
    let output = Command::new("sha256sum")
        .arg(path)
        .output()
        .or_else(|_| {
            Command::new("shasum")
                .args(["-a", "256"])
                .arg(path)
                .output()
        })
        .map_err(|error| Error::msg(format!("failed to hash {}: {error}", path.display())))?;
    if !output.status.success() {
        return Err(Error::msg(format!("failed to hash {}", path.display())));
    }
    let stdout = String::from_utf8_lossy(&output.stdout);
    let actual = stdout
        .split_whitespace()
        .next()
        .unwrap_or("")
        .to_ascii_lowercase();
    if actual != expected.to_ascii_lowercase() {
        return Err(Error::msg(format!(
            "checksum mismatch for {}: expected {expected}, got {actual}",
            path.display()
        )));
    }
    Ok(())
}

fn extract_archive(archive: &Path, destination: &Path) -> Result<()> {
    let name = archive
        .file_name()
        .and_then(|value| value.to_str())
        .unwrap_or_default()
        .to_ascii_lowercase();
    if name.ends_with(".zip") {
        let status = run_timed(
            Command::new("unzip")
                .args(["-qo"])
                .arg(archive)
                .arg("-d")
                .arg(destination),
            INSTALL_TIMEOUT,
            "unzip archive",
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
    let path = std::env::temp_dir().join(format!(
        "amarcode-{prefix}-{}",
        uuid::Uuid::new_v4()
    ));
    fs::create_dir_all(&path).map_err(|error| {
        Error::msg(format!(
            "failed to create temp directory {}: {error}",
            path.display()
        ))
    })?;
    Ok(path)
}

fn run_timed(
    command: &mut Command,
    timeout: Duration,
    label: &str,
) -> Result<std::process::ExitStatus> {
    let mut child = command
        .stdout(std::process::Stdio::piped())
        .stderr(std::process::Stdio::piped())
        .spawn()
        .map_err(|error| Error::msg(format!("failed to start {label}: {error}")))?;
    let started = std::time::Instant::now();
    loop {
        match child.try_wait() {
            Ok(Some(status)) => return Ok(status),
            Ok(None) if started.elapsed() > timeout => {
                let _ = child.kill();
                let _ = child.wait();
                return Err(Error::msg(format!(
                    "{label} timed out after {}s",
                    timeout.as_secs()
                )));
            }
            Ok(None) => std::thread::sleep(Duration::from_millis(100)),
            Err(error) => {
                return Err(Error::msg(format!("failed waiting for {label}: {error}")));
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

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
        prefetch_bun_package(package).expect("bun add should prefetch package");
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