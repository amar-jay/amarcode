//! Agent catalog and executable resolution.
//!
//! Responsibilities:
//! - list/create agents via `store`
//! - resolve an agent row to a concrete command (managed install under
//!   app data dir vs custom PATH command)
//!
//! Does not own live ACP sessions — that is `session_manager`.

use std::{
    path::{Path, PathBuf},
    sync::Arc,
};

use crate::{
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

    pub fn list(&self) -> Result<Vec<AgentDefinition>> {
        self.store.agents()
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
        let command = resolve_command_path(&self.tools_dir, &agent.command);
        Ok(ResolvedAgent {
            agent_id: agent.id.clone(),
            name: agent.name.clone(),
            command,
            arguments: agent.arguments.clone(),
            environment: agent.environment.clone(),
        })
    }
}

fn resolve_command_path(tools_dir: &Path, command: &str) -> PathBuf {
    let as_path = PathBuf::from(command);
    if as_path.is_absolute() && as_path.exists() {
        return as_path;
    }
    let managed = tools_dir.join(command);
    if managed.exists() {
        return managed;
    }
    as_path
}

fn timestamp() -> String {
    chrono::Utc::now().to_rfc3339()
}
