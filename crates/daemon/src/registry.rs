//! Runtime checkout management for the ACP agent registry.
//!
//! The source checkout used by developers lives at `vendor/acp-registry` as a
//! Git submodule. Installed daemons keep an independent checkout below the app
//! data directory so packaged applications do not depend on the source tree.

use std::{
    collections::BTreeMap,
    path::{Path, PathBuf},
};

use serde::Deserialize;
use tokio::process::Command;
use tokio::time::{timeout, Duration};

use crate::{protocol::AgentDefinition, Error, Result};

pub const CHECKOUT_DIRECTORY: &str = "acp-registry";
const GIT_TIMEOUT: Duration = Duration::from_secs(5 * 60);

/// Clone the registry on first startup or fast-forward its existing checkout.
///
/// The caller decides whether a synchronization failure is fatal. Amarcode's
/// startup treats it as best-effort so the last successful checkout remains
/// available while offline.
pub async fn synchronize(app_dir: &Path, source: &str) -> Result<PathBuf> {
    ensure_git_available().await?;

    let checkout = app_dir.join(CHECKOUT_DIRECTORY);
    if checkout.join(".git").is_dir() {
        run_git([
            "-C",
            path_text(&checkout)?,
            "pull",
            "--ff-only",
            "--depth",
            "1",
            "origin",
            "main",
        ])
        .await?;
    } else if checkout.exists() {
        return Err(Error::msg(format!(
            "ACP registry path exists but is not a Git checkout: {}",
            checkout.display()
        )));
    } else {
        std::fs::create_dir_all(app_dir).map_err(|error| {
            Error::msg(format!(
                "failed to create ACP registry parent directory {}: {error}",
                app_dir.display()
            ))
        })?;
        run_git([
            "clone",
            "--depth",
            "1",
            "--branch",
            "main",
            source,
            path_text(&checkout)?,
        ])
        .await?;
    }

    validate_checkout(&checkout)?;
    Ok(checkout)
}

pub fn checkout_path(app_dir: &Path) -> PathBuf {
    app_dir.join(CHECKOUT_DIRECTORY)
}

/// Read registry entries and translate their preferred distribution into the
/// daemon's existing process launch definition.
pub fn load_agents(checkout: &Path) -> Result<Vec<AgentDefinition>> {
    validate_checkout(checkout)?;
    let mut definitions = Vec::new();
    for entry in std::fs::read_dir(checkout).map_err(|error| {
        Error::msg(format!(
            "failed to read ACP registry {}: {error}",
            checkout.display()
        ))
    })? {
        let entry = entry.map_err(|error| Error::msg(error.to_string()))?;
        let manifest = entry.path().join("agent.json");
        if !manifest.is_file() {
            continue;
        }
        let contents = std::fs::read_to_string(&manifest).map_err(|error| {
            Error::msg(format!(
                "failed to read ACP registry manifest {}: {error}",
                manifest.display()
            ))
        })?;
        let agent: RegistryAgent = serde_json::from_str(&contents).map_err(|error| {
            Error::msg(format!(
                "invalid ACP registry manifest {}: {error}",
                manifest.display()
            ))
        })?;
        if let Some(definition) = agent.into_definition() {
            definitions.push(definition);
        }
    }
    definitions.sort_by(|left, right| left.name.cmp(&right.name));
    Ok(definitions)
}

#[derive(Debug, Deserialize)]
struct RegistryAgent {
    id: String,
    name: String,
    distribution: RegistryDistribution,
}

#[derive(Debug, Deserialize)]
struct RegistryDistribution {
    npx: Option<PackageDistribution>,
    uvx: Option<PackageDistribution>,
    binary: Option<BTreeMap<String, BinaryDistribution>>,
}

#[derive(Debug, Deserialize)]
struct PackageDistribution {
    package: String,
    #[serde(default)]
    args: Vec<String>,
    #[serde(default)]
    env: BTreeMap<String, String>,
}

#[derive(Debug, Deserialize)]
struct BinaryDistribution {
    cmd: String,
    #[serde(default)]
    args: Vec<String>,
    #[serde(default)]
    env: BTreeMap<String, String>,
}

impl RegistryAgent {
    fn into_definition(self) -> Option<AgentDefinition> {
        let (command, mut arguments, environment) = if let Some(package) = self.distribution.npx {
            let mut arguments = vec!["--yes".to_owned(), package.package];
            arguments.extend(package.args);
            ("npx".to_owned(), arguments, package.env)
        } else if let Some(package) = self.distribution.uvx {
            let mut arguments = vec![package.package];
            arguments.extend(package.args);
            ("uvx".to_owned(), arguments, package.env)
        } else {
            let target = current_binary_target()?;
            let binary = self.distribution.binary?.remove(target)?;
            (binary.cmd, binary.args, binary.env)
        };
        arguments.shrink_to_fit();
        let now = chrono::Utc::now().to_rfc3339();
        Some(AgentDefinition {
            id: self.id,
            name: self.name,
            command,
            arguments,
            environment: environment.into_iter().collect(),
            is_preset: true,
            created_at: now.clone(),
            updated_at: now,
        })
    }
}

fn current_binary_target() -> Option<&'static str> {
    match (std::env::consts::OS, std::env::consts::ARCH) {
        ("macos", "aarch64") => Some("darwin-aarch64"),
        ("macos", "x86_64") => Some("darwin-x86_64"),
        ("linux", "aarch64") => Some("linux-aarch64"),
        ("linux", "x86_64") => Some("linux-x86_64"),
        ("windows", "aarch64") => Some("windows-aarch64"),
        ("windows", "x86_64") => Some("windows-x86_64"),
        _ => None,
    }
}

fn path_text(path: &Path) -> Result<&str> {
    path.to_str().ok_or_else(|| {
        Error::msg(format!(
            "ACP registry path is not valid UTF-8: {}",
            path.display()
        ))
    })
}

async fn ensure_git_available() -> Result<()> {
    let output = Command::new("git")
        .arg("--version")
        .kill_on_drop(true)
        .output()
        .await
        .map_err(|_| Error::msg("git is not available"))?;

    if !output.status.success() {
        return Err(Error::msg("git invocation failed"));
    }

    Ok(())
}

async fn run_git<const N: usize>(arguments: [&str; N]) -> Result<()> {
    let output = timeout(
        GIT_TIMEOUT,
        Command::new("git")
            .args(arguments)
            .kill_on_drop(true)
            .output(),
    )
    .await
    .map_err(|_| {
        Error::msg(format!(
            "timed out while synchronizing ACP registry after {GIT_TIMEOUT:?}"
        ))
    })?
    .map_err(|error| Error::msg(format!("failed to run git for ACP registry: {error}")))?;
    if output.status.success() {
        return Ok(());
    }

    let detail = String::from_utf8_lossy(&output.stderr).trim().to_owned();
    Err(Error::msg(if detail.is_empty() {
        format!(
            "git exited with {} while synchronizing ACP registry",
            output.status
        )
    } else {
        format!("failed to synchronize ACP registry: {detail}")
    }))
}

fn validate_checkout(checkout: &Path) -> Result<()> {
    for required in ["agent.schema.json", "registry.schema.json"] {
        if !checkout.join(required).is_file() {
            return Err(Error::msg(format!(
                "ACP registry checkout is missing {required}: {}",
                checkout.display()
            )));
        }
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn rejects_checkout_without_registry_schema() {
        let checkout = std::env::temp_dir().join(format!(
            "amarcode-invalid-registry-{}",
            uuid::Uuid::new_v4()
        ));
        std::fs::create_dir_all(&checkout).expect("create checkout");
        let error = validate_checkout(&checkout).expect_err("invalid checkout");
        assert!(error.to_string().contains("agent.schema.json"));
        std::fs::remove_dir_all(checkout).expect("remove checkout");
    }

    #[test]
    fn translates_npx_distribution_to_launch_definition() {
        let agent: RegistryAgent = serde_json::from_value(serde_json::json!({
            "id": "example-agent",
            "name": "Example",
            "distribution": {
                "npx": {
                    "package": "@example/agent@1.2.3",
                    "args": ["--acp"],
                    "env": { "EXAMPLE": "yes" }
                }
            }
        }))
        .expect("registry agent");
        let definition = agent.into_definition().expect("launch definition");
        assert_eq!(definition.command, "npx");
        assert_eq!(
            definition.arguments,
            ["--yes", "@example/agent@1.2.3", "--acp"]
        );
        assert_eq!(definition.environment, [("EXAMPLE".into(), "yes".into())]);
        assert!(definition.is_preset);
    }
}
