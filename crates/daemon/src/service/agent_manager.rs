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
    sync::Arc,
};

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
    tools_dir: PathBuf,
}

impl AgentManager {
    pub fn new(store: Arc<Store>, tools_dir: impl Into<PathBuf>) -> Self {
        Self {
            store,
            tools_dir: tools_dir.into(),
        }
    }

    pub fn tools_dir(&self) -> &Path {
        &self.tools_dir
    }

    pub fn list(&self) -> Result<Vec<AgentInfo>> {
        Ok(self
            .store
            .agents()?
            .into_iter()
            .map(|agent| self.info(&agent))
            .collect())
    }

    pub fn get(&self, id: &str) -> Result<Option<AgentDefinition>> {
        self.store.get_agent(id)
    }

    /// Upsert a user-defined (or preset) agent definition.
    pub fn save(&self, agent: &AgentDefinition) -> Result<()> {
        self.store.save_agent(agent)
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
        let agent = AgentDefinition {
            id: uuid::Uuid::new_v4().to_string(),
            name: name.into(),
            command: command.into(),
            arguments,
            environment,
            is_preset: false,
            created_at: now.clone(),
            updated_at: now,
        };
        self.store.save_agent(&agent)?;
        Ok(agent)
    }

    /// Resolve command path + args/env for process spawn.
    ///
    /// Lookup order:
    /// 1. absolute `agent.command` if it exists on disk
    /// 2. `{tools_dir}/{command}` if present (managed install)
    /// 3. bare command name (PATH search by the OS)
    pub fn resolve(&self, agent_id: &str) -> Result<ResolvedAgent> {
        let agent = self
            .store
            .get_agent(agent_id)?
            .ok_or_else(|| Error::msg(format!("agent not found: {agent_id}")))?;
        self.resolve_definition(&agent)
    }

    pub fn resolve_definition(&self, agent: &AgentDefinition) -> Result<ResolvedAgent> {
        let command = find_command(&self.tools_dir, agent)
            .ok_or_else(|| Error::msg(unavailable_reason(&agent.command)))?;
        Ok(ResolvedAgent {
            agent_id: agent.id.clone(),
            name: agent.name.clone(),
            command,
            arguments: agent.arguments.clone(),
            environment: agent.environment.clone(),
        })
    }

    fn info(&self, agent: &AgentDefinition) -> AgentInfo {
        let resolved = find_command(&self.tools_dir, agent);
        AgentInfo {
            id: agent.id.clone(),
            name: agent.name.clone(),
            command: agent.command.clone(),
            arguments: agent.arguments.clone(),
            environment: agent.environment.clone(),
            is_preset: agent.is_preset,
            created_at: agent.created_at.clone(),
            updated_at: agent.updated_at.clone(),
            available: resolved.is_some(),
            resolved_command: resolved
                .as_ref()
                .map(|path| path.to_string_lossy().into_owned()),
            unavailable_reason: resolved
                .is_none()
                .then(|| unavailable_reason(&agent.command)),
        }
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
    std::env::split_paths(&search_path)
        .find_map(|directory| executable_path(&directory.join(&as_path), &agent.environment))
}

fn unavailable_reason(command: &str) -> String {
    if command.trim().is_empty() {
        "Agent command is empty".into()
    } else {
        format!("Executable '{command}' was not found in managed tools or PATH")
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

    fn agent(command: &str, environment: Vec<(String, String)>) -> AgentDefinition {
        AgentDefinition {
            id: "test-agent".into(),
            name: "Test agent".into(),
            command: command.into(),
            arguments: vec![],
            environment,
            is_preset: false,
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

    #[test]
    fn missing_command_is_unavailable() {
        let tools = test_directory();
        let definition = agent(
            "definitely-not-an-amarcode-agent",
            vec![("PATH".into(), String::new())],
        );

        assert_eq!(find_command(&tools, &definition), None);
        assert!(unavailable_reason(&definition.command).contains(&definition.command));
        std::fs::remove_dir_all(tools).expect("remove test directory");
    }
}
