//! Runtime checkout management for the ACP agent registry.
//!
//! The source checkout used by developers lives at `vendor/acp-registry` as a
//! Git submodule. Installed daemons keep an independent checkout below the app
//! data directory so packaged applications do not depend on the source tree.

use std::path::{Path, PathBuf};

use tokio::process::Command;
use tokio::time::{timeout, Duration};

use crate::{Error, Result};

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
        .map_err(|_| Error::msg(format!("git is not available")))?;

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
    .map_err(|_| Error::msg(format!(
        "timed out while synchronizing ACP registry after {GIT_TIMEOUT:?}"
    )))?
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
}
