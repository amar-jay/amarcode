//! Agent catalog and executable resolution.
//!
//! Responsibilities:
//! - list/create agents via `store`
//! - resolve an agent row to a concrete command (managed install under
//!   app data dir vs custom PATH command)
//!
//! Does not own live ACP sessions — that is `session`.

use std::{
    ffi::OsString,
    path::{Path, PathBuf},
    sync::{Arc, Mutex},
};

use base64::{engine::general_purpose::STANDARD as BASE64, Engine as _};

use crate::{
    protocol::AgentInfo,
    store::{AgentDefinition, Store},
    Error, Result,
};

/// Resolved launch plan for spawning an ACP agent process.
#[derive(Debug, Clone)]
pub struct ResolvedAgent {
    pub agent_id: String,
    pub name: String,
    /// Absolute path or bare command name for `Command::new`.
    pub command: PathBuf,
    pub arguments: Vec<String>,
    pub environment: Vec<(String, String)>,
}

#[derive(Clone)]
pub struct AgentManager {
    store: Arc<Store>,
    app_dir: PathBuf,
    tools_dir: PathBuf,
    registry_dir: PathBuf,
    amarcode_acp_install_lock: Arc<Mutex<()>>,
}

impl AgentManager {
    pub fn new(store: Arc<Store>, app_dir: impl Into<PathBuf>) -> Self {
        let app_dir = app_dir.into();
        let tools_dir = app_dir.join("tools");
        let registry_dir = app_dir.join(crate::registry::CHECKOUT_DIRECTORY);
        Self {
            store,
            app_dir,
            tools_dir,
            registry_dir,
            amarcode_acp_install_lock: Arc::new(Mutex::new(())),
        }
    }

    pub fn tools_dir(&self) -> &Path {
        &self.tools_dir
    }

    pub(crate) fn managed_agents_dir(&self) -> PathBuf {
        self.app_dir.join("agents")
    }

    pub(crate) fn credentials_dir(&self) -> PathBuf {
        self.app_dir.join("credentials")
    }

    pub fn registry_dir(&self) -> &Path {
        &self.registry_dir
    }

    /// Whether this agent can be launched without a first-run package download.
    pub fn is_available(&self, agent: &AgentDefinition) -> bool {
        agent_is_available(&self.tools_dir, agent)
    }

    /// Build the UI-facing info row for one stored agent (after availability refresh).
    pub fn agent_info(&self, agent: &AgentDefinition) -> AgentInfo {
        self.info(agent)
    }

    pub fn list(&self) -> Result<Vec<AgentInfo>> {
        self.refresh_availability()?;
        Ok(self
            .store
            .agents()?
            .into_iter()
            .map(|agent| self.info(&agent))
            .collect())
    }

    /// Resolve each agent command on this host and persist `available`.
    ///
    /// For registry `bunx` / `uvx` agents, the runner existing on PATH is not
    /// enough — the package argument must already be present in the local
    /// package cache so the first prompt does not block on a download.
    pub fn refresh_availability(&self) -> Result<()> {
        for agent in self.store.agents()? {
            let available = agent_is_available(&self.tools_dir, &agent);
            if agent.available != available {
                self.store.set_agent_available(&agent.id, available)?;
            }
        }
        Ok(())
    }

    pub fn get(&self, id: &str) -> Result<Option<AgentDefinition>> {
        self.store.get_agent(id)
    }

    /// Upsert an agent definition.
    pub fn save(&self, agent: &AgentDefinition) -> Result<()> {
        self.store.save_agent(agent)
    }

    pub fn sync_builtin_preset(&self) -> Result<()> {
        self.migrate_amarcode_acp_layout()?;
        let existing = self.get(super::amarcode_acp::AGENT_ID)?;
        let now = timestamp();
        let executable = existing
            .as_ref()
            .map(|agent| PathBuf::from(&agent.command))
            .filter(|command| command.is_file())
            .or_else(|| self.installed_amarcode_acp_executable())
            .map(|command| command.to_string_lossy().into_owned())
            .unwrap_or_default();
        let definition = AgentDefinition {
            id: super::amarcode_acp::AGENT_ID.into(),
            name: "Amarcode ACP".into(),
            command: executable,
            arguments: vec![
                "--config".into(),
                self.amarcode_acp_config_path()
                    .to_string_lossy()
                    .into_owned(),
            ],
            environment: vec![],
            available: false,
            created_at: existing
                .map(|agent| agent.created_at)
                .unwrap_or_else(|| now.clone()),
            updated_at: now,
        };
        self.store.save_builtin_agent(&definition)
    }

    pub(crate) fn save_builtin_definition(&self, executable: &Path) -> Result<()> {
        let existing = self.get(super::amarcode_acp::AGENT_ID)?;
        let now = timestamp();
        self.store.save_builtin_agent(&AgentDefinition {
            id: super::amarcode_acp::AGENT_ID.into(),
            name: "Amarcode ACP".into(),
            command: executable.to_string_lossy().into_owned(),
            arguments: vec![
                "--config".into(),
                self.amarcode_acp_config_path()
                    .to_string_lossy()
                    .into_owned(),
            ],
            environment: vec![],
            available: false,
            created_at: existing
                .map(|agent| agent.created_at)
                .unwrap_or_else(|| now.clone()),
            updated_at: now,
        })
    }

    pub(crate) fn refresh_builtin_definition(&self) -> Result<()> {
        let executable = self
            .get(super::amarcode_acp::AGENT_ID)?
            .map(|agent| PathBuf::from(agent.command))
            .filter(|command| command.is_file())
            .or_else(|| self.installed_amarcode_acp_executable());
        if let Some(executable) = executable {
            self.save_builtin_definition(&executable)?;
        }
        self.refresh_availability()
    }

    pub(crate) fn amarcode_acp_install_guard(&self) -> Result<std::sync::MutexGuard<'_, ()>> {
        self.amarcode_acp_install_lock
            .lock()
            .map_err(|_| Error::msg("amarcode-acp installation lock is poisoned"))
    }

    fn installed_amarcode_acp_executable(&self) -> Option<PathBuf> {
        let install_root = self
            .managed_agents_dir()
            .join(super::amarcode_acp::AGENT_ID);
        let executable_name = if cfg!(windows) {
            "amarcode-acp.exe"
        } else {
            "amarcode-acp"
        };
        std::fs::read_dir(install_root)
            .ok()?
            .filter_map(|entry| entry.ok())
            .filter_map(|entry| {
                let executable = entry.path().join(executable_name);
                let modified = executable.metadata().ok()?.modified().ok()?;
                executable.is_file().then_some((modified, executable))
            })
            .max_by_key(|(modified, _)| *modified)
            .map(|(_, executable)| executable)
    }

    fn migrate_amarcode_acp_layout(&self) -> Result<()> {
        let old_root = self
            .tools_dir
            .join("agents")
            .join(super::amarcode_acp::AGENT_ID);
        if !old_root.is_dir() {
            return Ok(());
        }

        move_if_missing(
            &old_root.join("config.json"),
            &self.amarcode_acp_config_path(),
        )?;
        move_if_missing(
            &old_root.join("config.sessions.json"),
            &self
                .amarcode_acp_config_path()
                .with_extension("sessions.json"),
        )?;

        let install_root = self
            .managed_agents_dir()
            .join(super::amarcode_acp::AGENT_ID);
        for entry in std::fs::read_dir(&old_root).map_err(|error| Error::msg(error.to_string()))? {
            let entry = entry.map_err(|error| Error::msg(error.to_string()))?;
            if entry.path().is_dir() {
                move_if_missing(&entry.path(), &install_root.join(entry.file_name()))?;
            }
        }
        remove_if_empty(&old_root)?;
        remove_if_empty(&self.tools_dir.join("agents"))?;
        remove_if_empty(&self.tools_dir)?;
        Ok(())
    }

    /// Create a custom agent with a new id.
    pub fn create(
        &self,
        name: impl Into<String>,
        command: impl Into<String>,
        arguments: Vec<String>,
        environment: Vec<(String, String)>,
    ) -> Result<AgentDefinition> {
        let now = timestamp();
        let mut agent = AgentDefinition {
            id: uuid::Uuid::new_v4().to_string(),
            name: name.into(),
            command: command.into(),
            arguments,
            environment,
            available: false,
            created_at: now.clone(),
            updated_at: now,
        };
        agent.available = agent_is_available(&self.tools_dir, &agent);
        self.store.save_agent(&agent)?;
        Ok(agent)
    }

    /// Resolve command path + args/env for process spawn.
    ///
    /// Lookup order:
    /// 1. absolute `agent.command` if it exists on disk
    /// 2. `{tools_dir}/{command}` if present (managed install)
    /// 3. bare command name (PATH search by the OS)
    ///
    /// Registry `bunx` / `uvx` agents also require their package to already be
    /// cached locally; otherwise resolve fails instead of downloading inline.
    pub fn resolve(&self, agent_id: &str) -> Result<ResolvedAgent> {
        let agent = self
            .store
            .get_agent(agent_id)?
            .ok_or_else(|| Error::msg(format!("agent not found: {agent_id}")))?;
        self.resolve_definition(&agent)
    }

    pub fn resolve_definition(&self, agent: &AgentDefinition) -> Result<ResolvedAgent> {
        if !agent_is_available(&self.tools_dir, agent) {
            return Err(Error::msg(unavailable_reason(&self.tools_dir, agent)));
        }
        let command = find_command(&self.tools_dir, agent)
            .ok_or_else(|| Error::msg(unavailable_reason(&self.tools_dir, agent)))?;
        let arguments = resolved_arguments(agent, &command);
        Ok(ResolvedAgent {
            agent_id: agent.id.clone(),
            name: agent.name.clone(),
            command,
            arguments,
            environment: agent.environment.clone(),
        })
    }

    fn info(&self, agent: &AgentDefinition) -> AgentInfo {
        let resolved = find_command(&self.tools_dir, agent);
        let available = agent.available;
        AgentInfo {
            id: agent.id.clone(),
            name: agent.name.clone(),
            icon: self.registry_icon(agent),
            command: agent.command.clone(),
            arguments: agent.arguments.clone(),
            environment: agent.environment.clone(),
            created_at: agent.created_at.clone(),
            updated_at: agent.updated_at.clone(),
            available,
            resolved_command: resolved
                .as_ref()
                .filter(|_| available)
                .map(|path| path.to_string_lossy().into_owned()),
            unavailable_reason: (!available).then(|| unavailable_reason(&self.tools_dir, agent)),
        }
    }

    fn registry_icon(&self, agent: &AgentDefinition) -> Option<String> {
        if !valid_registry_id(&agent.id) {
            return None;
        }
        let bytes = std::fs::read(self.registry_dir.join(&agent.id).join("icon.svg")).ok()?;
        // Registry icons are tiny; avoid putting an unexpectedly large file on
        // the JSON-line protocol if a checkout has been modified locally.
        if bytes.len() > 256 * 1024 {
            return None;
        }
        Some(format!(
            "data:image/svg+xml;base64,{}",
            BASE64.encode(bytes)
        ))
    }
}

fn valid_registry_id(id: &str) -> bool {
    !id.is_empty()
        && id.bytes().enumerate().all(|(index, byte)| {
            byte.is_ascii_lowercase()
                || byte.is_ascii_digit() && index > 0
                || byte == b'-' && index > 0
        })
}

fn move_if_missing(source: &Path, destination: &Path) -> Result<()> {
    if !source.exists() || destination.exists() {
        return Ok(());
    }
    let parent = destination
        .parent()
        .ok_or_else(|| Error::msg("invalid migration destination"))?;
    std::fs::create_dir_all(parent).map_err(|error| Error::msg(error.to_string()))?;
    std::fs::rename(source, destination).map_err(|error| {
        Error::msg(format!(
            "failed moving {} to {}: {error}",
            source.display(),
            destination.display()
        ))
    })
}

fn remove_if_empty(path: &Path) -> Result<()> {
    if path.is_dir()
        && std::fs::read_dir(path)
            .map_err(|error| Error::msg(error.to_string()))?
            .next()
            .is_none()
    {
        std::fs::remove_dir(path).map_err(|error| Error::msg(error.to_string()))?;
    }
    Ok(())
}

fn agent_is_available(tools_dir: &Path, agent: &AgentDefinition) -> bool {
    if agent.id == super::amarcode_acp::AGENT_ID {
        return find_command(tools_dir, agent).is_some()
            && agent
                .arguments
                .windows(2)
                .find(|pair| pair[0] == "--config")
                .is_some_and(|pair| {
                    super::amarcode_acp::config_file_is_ready(Path::new(&pair[1]))
                });
    }
    agent_is_available_with_caches(
        tools_dir,
        agent,
        bun_install_cache_dir().as_deref(),
        uv_wheels_dir().as_deref(),
        uv_tools_dir().as_deref(),
    )
}

fn agent_is_available_with_caches(
    tools_dir: &Path,
    agent: &AgentDefinition,
    bun_cache: Option<&Path>,
    uv_wheels: Option<&Path>,
    uv_tools: Option<&Path>,
) -> bool {
    if find_command(tools_dir, agent).is_none() {
        return false;
    }
    match package_runner(agent.command.trim()) {
        Some(PackageRunner::Bunx) => agent
            .arguments
            .first()
            .is_some_and(|spec| bun_package_cached_in(spec, bun_cache)),
        Some(PackageRunner::Uvx) => agent
            .arguments
            .first()
            .is_some_and(|spec| uv_package_cached_in(spec, uv_wheels, uv_tools)),
        None => true,
    }
}

fn find_command(tools_dir: &Path, agent: &AgentDefinition) -> Option<PathBuf> {
    let command = agent.command.trim();
    if command.is_empty() {
        return None;
    }

    let as_path = PathBuf::from(command);
    if as_path.is_absolute() {
        return executable_path(&as_path, &agent.environment);
    }

    if let Some(path) = executable_path(&tools_dir.join(&as_path), &agent.environment) {
        return Some(path);
    }

    // Commands containing a directory component are paths rather than names
    // suitable for PATH lookup. Relative paths cannot be assessed without the
    // session workspace, so only managed-tool resolution is supported here.
    if as_path.components().count() > 1 {
        return None;
    }

    let search_path = environment_value(&agent.environment, "PATH")
        .map(OsString::from)
        .or_else(|| std::env::var_os("PATH"))?;
    let directories = std::env::split_paths(&search_path).collect::<Vec<_>>();
    let found = directories
        .iter()
        .find_map(|directory| executable_path(&directory.join(&as_path), &agent.environment));
    if found.is_some() || command != "bunx" {
        return found;
    }
    directories
        .iter()
        .find_map(|directory| executable_path(&directory.join("bun"), &agent.environment))
}

fn command_is_bun(path: &Path) -> bool {
    path.file_stem()
        .and_then(|name| name.to_str())
        .is_some_and(|name| name.eq_ignore_ascii_case("bun"))
}

fn resolved_arguments(agent: &AgentDefinition, command: &Path) -> Vec<String> {
    let mut arguments = agent.arguments.clone();
    if agent.command.trim() == "bunx" && command_is_bun(command) {
        arguments.insert(0, "x".into());
    }
    arguments
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum PackageRunner {
    Bunx,
    Uvx,
}

fn package_runner(command: &str) -> Option<PackageRunner> {
    match command {
        "bunx" => Some(PackageRunner::Bunx),
        "uvx" => Some(PackageRunner::Uvx),
        _ => None,
    }
}

/// Whether a `bunx` package spec is already materialized in Bun's install cache.
///
/// Specs look like `@scope/name@1.2.3` or `name@1.2.3`. Bun stores matching
/// directories under `~/.bun/install/cache` as `name@1.2.3` or `name@1.2.3@@@N`.
fn bun_package_cached_in(spec: &str, cache: Option<&Path>) -> bool {
    let spec = spec.trim();
    if spec.is_empty() {
        return false;
    }
    let Some(cache) = cache else {
        return false;
    };
    let (parent, prefix) = match scoped_npm_package_parts(spec) {
        Some((scope, name_and_version)) => (cache.join(scope), name_and_version),
        None => (cache.to_path_buf(), spec.to_owned()),
    };
    directory_has_package_entry(&parent, &prefix)
}

/// Whether a `uvx` package spec is already present in uv's local caches.
///
/// Accepts `name@1.2.3`, `name==1.2.3`, or bare `name`. Ready means a matching
/// wheel or installed tool env exists so `uvx --offline` would not need network.
fn uv_package_cached_in(spec: &str, wheels: Option<&Path>, tools: Option<&Path>) -> bool {
    let Some((name, version)) = parse_python_package_spec(spec) else {
        return false;
    };
    if tools.is_some_and(|dir| dir.join(&name).is_dir()) {
        return true;
    }
    uv_wheel_cached_in(wheels, &name, version.as_deref())
}

fn bun_install_cache_dir() -> Option<PathBuf> {
    if let Some(path) = std::env::var_os("BUN_INSTALL_CACHE_DIR") {
        let path = PathBuf::from(path);
        if path.is_dir() {
            return Some(path);
        }
    }
    if let Some(install) = std::env::var_os("BUN_INSTALL") {
        let path = PathBuf::from(install).join("install").join("cache");
        if path.is_dir() {
            return Some(path);
        }
    }
    let home = std::env::var_os("HOME").or_else(|| std::env::var_os("USERPROFILE"))?;
    let path = PathBuf::from(home)
        .join(".bun")
        .join("install")
        .join("cache");
    path.is_dir().then_some(path)
}

fn scoped_npm_package_parts(spec: &str) -> Option<(String, String)> {
    let stripped = spec.strip_prefix('@')?;
    let (scope, name_and_version) = stripped.split_once('/')?;
    if scope.is_empty() || name_and_version.is_empty() {
        return None;
    }
    Some((format!("@{scope}"), name_and_version.to_owned()))
}

fn directory_has_package_entry(parent: &Path, prefix: &str) -> bool {
    let exact = parent.join(prefix);
    if exact.is_dir() {
        return true;
    }
    let Ok(entries) = std::fs::read_dir(parent) else {
        return false;
    };
    let versioned_prefix = format!("{prefix}@@@");
    entries.filter_map(|entry| entry.ok()).any(|entry| {
        let name = entry.file_name();
        let name = name.to_string_lossy();
        (name == prefix || name.starts_with(&versioned_prefix)) && entry.path().is_dir()
    })
}

fn parse_python_package_spec(spec: &str) -> Option<(String, Option<String>)> {
    let spec = spec.trim();
    if spec.is_empty() {
        return None;
    }
    if let Some((name, version)) = spec.split_once("==") {
        return Some((name.trim().to_owned(), Some(version.trim().to_owned())));
    }
    if let Some((name, version)) = spec.split_once('=') {
        if !name.is_empty() && !version.is_empty() && !name.contains('/') {
            return Some((name.trim().to_owned(), Some(version.trim().to_owned())));
        }
    }
    if let Some((name, version)) = spec.rsplit_once('@') {
        if !name.is_empty() && !version.is_empty() && !name.starts_with('@') {
            return Some((name.to_owned(), Some(version.to_owned())));
        }
    }
    Some((spec.to_owned(), None))
}

fn uv_wheel_cached_in(wheels: Option<&Path>, name: &str, version: Option<&str>) -> bool {
    let Some(wheels) = wheels else {
        return false;
    };
    let package_dir = wheels.join(name);
    if !package_dir.is_dir() {
        return false;
    }
    let Some(version) = version else {
        return true;
    };
    let Ok(entries) = std::fs::read_dir(&package_dir) else {
        return false;
    };
    let prefix = format!("{version}-");
    entries.filter_map(|entry| entry.ok()).any(|entry| {
        let name = entry.file_name();
        let name = name.to_string_lossy();
        name == version || name.starts_with(&prefix)
    })
}

fn uv_tools_dir() -> Option<PathBuf> {
    if let Some(path) = std::env::var_os("UV_TOOL_DIR") {
        let path = PathBuf::from(path);
        return path.is_dir().then_some(path);
    }
    #[cfg(windows)]
    {
        let local = std::env::var_os("LOCALAPPDATA")?;
        let path = PathBuf::from(local).join("uv").join("tools");
        return path.is_dir().then_some(path);
    }
    #[cfg(not(windows))]
    {
        if let Some(xdg) = std::env::var_os("XDG_DATA_HOME") {
            let path = PathBuf::from(xdg).join("uv").join("tools");
            if path.is_dir() {
                return Some(path);
            }
        }
        let home = std::env::var_os("HOME")?;
        let path = PathBuf::from(home)
            .join(".local")
            .join("share")
            .join("uv")
            .join("tools");
        path.is_dir().then_some(path)
    }
}

fn uv_wheels_dir() -> Option<PathBuf> {
    let cache_root = if let Some(path) = std::env::var_os("UV_CACHE_DIR") {
        PathBuf::from(path)
    } else if let Some(xdg) = std::env::var_os("XDG_CACHE_HOME") {
        PathBuf::from(xdg).join("uv")
    } else {
        let home = std::env::var_os("HOME").or_else(|| std::env::var_os("USERPROFILE"))?;
        PathBuf::from(home).join(".cache").join("uv")
    };
    let path = cache_root.join("wheels-v6").join("pypi");
    path.is_dir().then_some(path)
}

fn unavailable_reason(tools_dir: &Path, agent: &AgentDefinition) -> String {
    if agent.id == super::amarcode_acp::AGENT_ID
        && find_command(tools_dir, agent).is_some()
        && !agent
            .arguments
            .windows(2)
            .find(|pair| pair[0] == "--config")
            .is_some_and(|pair| super::amarcode_acp::config_file_is_ready(Path::new(&pair[1])))
    {
        return "Provider configuration is required in Settings".into();
    }
    if agent.command.trim().is_empty() {
        return "Agent command is empty".into();
    }
    if find_command(tools_dir, agent).is_none() {
        return format!(
            "Executable '{}' was not found in managed tools or PATH",
            agent.command
        );
    }
    match package_runner(agent.command.trim()) {
        Some(PackageRunner::Bunx) | Some(PackageRunner::Uvx) => {
            format!("{} is not installed", agent.name)
        }
        None => format!(
            "Executable '{}' was not found in managed tools or PATH",
            agent.command
        ),
    }
}

fn environment_value<'a>(environment: &'a [(String, String)], key: &str) -> Option<&'a str> {
    environment
        .iter()
        .rev()
        .find(|(candidate, _)| environment_key_eq(candidate, key))
        .map(|(_, value)| value.as_str())
}

#[cfg(windows)]
fn environment_key_eq(left: &str, right: &str) -> bool {
    left.eq_ignore_ascii_case(right)
}

#[cfg(not(windows))]
fn environment_key_eq(left: &str, right: &str) -> bool {
    left == right
}

fn executable_path(path: &Path, environment: &[(String, String)]) -> Option<PathBuf> {
    if is_executable(path) {
        return Some(path.to_owned());
    }

    #[cfg(windows)]
    {
        if path.extension().is_none() {
            let extensions = environment_value(environment, "PATHEXT")
                .map(str::to_owned)
                .or_else(|| std::env::var("PATHEXT").ok())
                .unwrap_or_else(|| ".COM;.EXE;.BAT;.CMD".into());
            for extension in extensions.split(';').filter(|value| !value.is_empty()) {
                let candidate = path.with_extension(extension.trim_start_matches('.'));
                if is_executable(&candidate) {
                    return Some(candidate);
                }
            }
        }
    }

    let _ = environment;
    None
}

#[cfg(unix)]
fn is_executable(path: &Path) -> bool {
    use std::os::unix::fs::PermissionsExt;

    path.metadata()
        .map(|metadata| metadata.is_file() && metadata.permissions().mode() & 0o111 != 0)
        .unwrap_or(false)
}

#[cfg(windows)]
fn is_executable(path: &Path) -> bool {
    path.is_file()
}

#[cfg(not(any(unix, windows)))]
fn is_executable(path: &Path) -> bool {
    path.is_file()
}

fn timestamp() -> String {
    chrono::Utc::now().to_rfc3339()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::store::Store;

    fn agent(command: &str, environment: Vec<(String, String)>) -> AgentDefinition {
        AgentDefinition {
            id: "test-agent".into(),
            name: "Test agent".into(),
            command: command.into(),
            arguments: vec![],
            environment,
            available: false,
            created_at: String::new(),
            updated_at: String::new(),
        }
    }

    fn test_directory() -> PathBuf {
        let directory = std::env::temp_dir().join(format!(
            "amarcode-agent-resolution-{}",
            uuid::Uuid::new_v4()
        ));
        std::fs::create_dir_all(&directory).expect("create test directory");
        directory
    }

    #[cfg(unix)]
    fn create_test_command(directory: &Path, name: &str) -> PathBuf {
        use std::os::unix::fs::PermissionsExt;

        let path = directory.join(name);
        std::fs::write(&path, "#!/bin/sh\n").expect("write test command");
        let mut permissions = path.metadata().expect("command metadata").permissions();
        permissions.set_mode(0o755);
        std::fs::set_permissions(&path, permissions).expect("make test command executable");
        path
    }

    #[cfg(windows)]
    fn create_test_command(directory: &Path, name: &str) -> PathBuf {
        let path = directory.join(format!("{name}.CMD"));
        std::fs::write(&path, "@echo off\r\n").expect("write test command");
        path
    }

    #[test]
    fn finds_managed_command_before_path() {
        let tools = test_directory();
        let expected = create_test_command(&tools, "managed-agent");
        let definition = agent("managed-agent", vec![]);

        assert_eq!(find_command(&tools, &definition), Some(expected));
        std::fs::remove_dir_all(tools).expect("remove test directory");
    }

    #[test]
    fn finds_command_in_agent_path_override() {
        let tools = test_directory();
        let bin = test_directory();
        let expected = create_test_command(&bin, "path-agent");
        let environment = vec![("PATH".into(), bin.to_string_lossy().into_owned())];
        #[cfg(windows)]
        let environment = {
            let mut environment = environment;
            environment.push(("PATHEXT".into(), ".COM;.EXE;.BAT;.CMD".into()));
            environment
        };
        let definition = agent("path-agent", environment);

        assert_eq!(find_command(&tools, &definition), Some(expected));
        std::fs::remove_dir_all(tools).expect("remove tools directory");
        std::fs::remove_dir_all(bin).expect("remove bin directory");
    }

    #[cfg(windows)]
    #[test]
    fn bunx_falls_back_to_winget_style_bun_executable() {
        let tools = test_directory();
        let bin = test_directory();
        let expected = create_test_command(&bin, "bun");
        let mut definition = agent(
            "bunx",
            vec![
                ("PATH".into(), bin.to_string_lossy().into_owned()),
                ("PATHEXT".into(), ".COM;.EXE;.BAT;.CMD".into()),
            ],
        );
        definition.arguments = vec!["codex-acp@latest".into()];

        let resolved = find_command(&tools, &definition).expect("resolve bun fallback");
        assert_eq!(resolved, expected);
        assert_eq!(
            resolved_arguments(&definition, &resolved),
            ["x", "codex-acp@latest"]
        );
        std::fs::remove_dir_all(tools).expect("remove tools directory");
        std::fs::remove_dir_all(bin).expect("remove bin directory");
    }

    #[test]
    fn missing_command_is_unavailable() {
        let tools = test_directory();
        let definition = agent(
            "definitely-not-an-amarcode-agent",
            vec![("PATH".into(), String::new())],
        );

        assert_eq!(find_command(&tools, &definition), None);
        assert!(!agent_is_available(&tools, &definition));
        assert!(unavailable_reason(&tools, &definition).contains(&definition.command));
        std::fs::remove_dir_all(tools).expect("remove test directory");
    }

    #[test]
    fn builtin_preset_recovers_an_installed_executable() {
        let root = test_directory();
        let legacy_root = root
            .join("tools")
            .join("agents")
            .join(super::super::amarcode_acp::AGENT_ID);
        let executable_directory = legacy_root.join("0.1.7");
        std::fs::create_dir_all(&executable_directory).expect("create install directory");
        let executable_name = if cfg!(windows) {
            "amarcode-acp.exe"
        } else {
            "amarcode-acp"
        };
        let legacy_executable = executable_directory.join(executable_name);
        std::fs::write(&legacy_executable, b"test executable").expect("write executable");
        std::fs::write(&legacy_root.join("config.json"), b"config").expect("write config");
        std::fs::write(&legacy_root.join("config.sessions.json"), b"sessions")
            .expect("write sessions");
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            std::fs::set_permissions(&legacy_executable, std::fs::Permissions::from_mode(0o755))
                .expect("make executable");
        }
        let store = Arc::new(Store::open(&root.join("store.sqlite3")).expect("open store"));
        let manager = AgentManager::new(store, &root);

        manager.sync_builtin_preset().expect("sync preset");

        let executable = root
            .join("agents")
            .join(super::super::amarcode_acp::AGENT_ID)
            .join("0.1.7")
            .join(executable_name);
        let preset = manager
            .get(super::super::amarcode_acp::AGENT_ID)
            .expect("read preset")
            .expect("builtin preset");
        assert_eq!(PathBuf::from(preset.command), executable);
        assert_eq!(
            std::fs::read(manager.amarcode_acp_config_path()).expect("migrated config"),
            b"config"
        );
        assert_eq!(
            std::fs::read(
                manager
                    .amarcode_acp_config_path()
                    .with_extension("sessions.json")
            )
            .expect("migrated sessions"),
            b"sessions"
        );
        assert!(!root.join("tools").exists());
        std::fs::remove_dir_all(root).expect("remove test directory");
    }

    #[test]
    fn bunx_runner_alone_is_not_available_without_cached_package() {
        let tools = test_directory();
        let bin = test_directory();
        let cache = test_directory();
        create_test_command(&bin, "bunx");
        let mut definition = agent(
            "bunx",
            vec![("PATH".into(), bin.to_string_lossy().into_owned())],
        );
        definition.name = "Grok Build".into();
        definition.arguments = vec!["@xai-official/grok@1.0.21".into(), "agent".into()];

        assert!(find_command(&tools, &definition).is_some());
        assert!(!agent_is_available_with_caches(
            &tools,
            &definition,
            Some(&cache),
            None,
            None
        ));
        assert_eq!(
            unavailable_reason(&tools, &definition),
            "Grok Build is not installed"
        );
        std::fs::remove_dir_all(tools).expect("remove tools directory");
        std::fs::remove_dir_all(bin).expect("remove bin directory");
        std::fs::remove_dir_all(cache).expect("remove cache directory");
    }

    #[test]
    fn bunx_agent_is_available_when_package_is_cached() {
        let tools = test_directory();
        let bin = test_directory();
        let cache = test_directory();
        create_test_command(&bin, "bunx");
        std::fs::create_dir_all(
            cache
                .join("@agentclientprotocol")
                .join("codex-acp@1.10.0@@@1"),
        )
        .expect("create cached package");
        let mut definition = agent(
            "bunx",
            vec![("PATH".into(), bin.to_string_lossy().into_owned())],
        );
        definition.arguments = vec!["@agentclientprotocol/codex-acp@1.10.0".into()];

        assert!(agent_is_available_with_caches(
            &tools,
            &definition,
            Some(&cache),
            None,
            None
        ));
        std::fs::remove_dir_all(tools).expect("remove tools directory");
        std::fs::remove_dir_all(bin).expect("remove bin directory");
        std::fs::remove_dir_all(cache).expect("remove cache directory");
    }

    #[test]
    fn uvx_agent_is_available_when_wheel_is_cached() {
        let tools = test_directory();
        let bin = test_directory();
        let cache = test_directory();
        let wheels = cache.join("wheels-v6").join("pypi");
        create_test_command(&bin, "uvx");
        std::fs::create_dir_all(wheels.join("fast-agent-acp").join("0.10.1-py3-none-any"))
            .expect("create cached wheel");
        let mut definition = agent(
            "uvx",
            vec![("PATH".into(), bin.to_string_lossy().into_owned())],
        );
        definition.arguments = vec!["fast-agent-acp==0.10.1".into(), "-x".into()];

        assert!(agent_is_available_with_caches(
            &tools,
            &definition,
            None,
            Some(&wheels),
            None
        ));
        std::fs::remove_dir_all(tools).expect("remove tools directory");
        std::fs::remove_dir_all(bin).expect("remove bin directory");
        std::fs::remove_dir_all(cache).expect("remove cache directory");
    }
}
