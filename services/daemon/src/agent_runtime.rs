use crate::models::AgentDefinition;
use std::path::PathBuf;
use tokio::{process::Command, sync::Mutex};

/// Resolves an agent definition to an executable. Managed adapters are installed
/// privately beneath the daemon data directory; custom agents stay untouched.
pub struct AgentRuntime {
    tools_dir: PathBuf,
    install_lock: Mutex<()>,
}

struct ManagedAgentSpec {
    agent_id: &'static str,
    executable: &'static str,
    installer: Installer,
}

enum Installer {
    NpmPackage { package: &'static str },
}

const MANAGED_AGENTS: &[ManagedAgentSpec] = &[
    ManagedAgentSpec {
        agent_id: "codex-acp",
        executable: "codex-acp",
        installer: Installer::NpmPackage {
            package: "@agentclientprotocol/codex-acp",
        },
    },
    ManagedAgentSpec {
        agent_id: "claude-acp",
        executable: "claude-agent-acp",
        installer: Installer::NpmPackage {
            package: "@agentclientprotocol/claude-agent-acp",
        },
    },
];

impl AgentRuntime {
    pub fn new(tools_dir: PathBuf) -> Self {
        Self {
            tools_dir,
            install_lock: Mutex::new(()),
        }
    }

    pub async fn command_for(&self, agent: &AgentDefinition) -> Result<PathBuf, String> {
        let Some(spec) = MANAGED_AGENTS.iter().find(|spec| spec.agent_id == agent.id) else {
            return Ok(PathBuf::from(&agent.command));
        };
        let executable = self.managed_executable(spec.executable);
        if executable.is_file() {
            return Ok(executable);
        }

        // Concurrent session requests share the same installed adapter.
        let _install = self.install_lock.lock().await;
        if executable.is_file() {
            return Ok(executable);
        }
        std::fs::create_dir_all(&self.tools_dir).map_err(|error| error.to_string())?;
        self.install(spec).await?;
        executable.is_file().then_some(executable).ok_or_else(|| {
            format!(
                "{} installation completed without its executable",
                agent.name
            )
        })
    }

    fn managed_executable(&self, name: &str) -> PathBuf {
        self.tools_dir.join("node_modules/.bin").join(name)
    }

    async fn install(&self, spec: &ManagedAgentSpec) -> Result<(), String> {
        match spec.installer {
            Installer::NpmPackage { package } => self.install_npm_package(package).await,
        }
    }

    async fn install_npm_package(&self, package: &str) -> Result<(), String> {
        let output = Command::new("npm")
            .args(["install", "--no-audit", "--no-fund", "--prefix"])
            .arg(&self.tools_dir)
            .arg(package)
            .output()
            .await
            .map_err(|error| {
                format!("{package} requires npm to install its managed adapter: {error}")
            })?;
        if output.status.success() {
            return Ok(());
        }
        Err(format!(
            "Could not install {package}: {}",
            String::from_utf8_lossy(&output.stderr).trim()
        ))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::path::Path;
    #[test]
    fn managed_agents_use_the_daemon_tools_directory() {
        let runtime = AgentRuntime::new(PathBuf::from("/tmp/acp-tools"));
        assert_eq!(
            runtime.managed_executable("codex-acp"),
            Path::new("/tmp/acp-tools/node_modules/.bin/codex-acp")
        );
    }

    #[test]
    fn managed_agent_registry_includes_codex_and_claude() {
        assert_eq!(
            MANAGED_AGENTS
                .iter()
                .map(|spec| spec.agent_id)
                .collect::<Vec<_>>(),
            ["codex-acp", "claude-acp"]
        );
    }
}
