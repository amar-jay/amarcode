//! Application data directory resolution.
//!
//! Resolves where the daemon keeps SQLite, tool caches, and other local state.
//! Override with `AMARCODE_APPDIR`.
//!
//! Platform fallbacks:
//! - Linux: `~/.amarcode`
//! - macOS: `~/Library/Application Support/amarcode`
//! - Windows: `%LOCALAPPDATA%/amarcode`

use std::{env, path::PathBuf};

use crate::{Error, Result};

/// Resolve the Amarcode application data directory.
pub fn resolve() -> Result<PathBuf> {
    if let Some(dir) = env::var_os("AMARCODE_APPDIR") {
        if !dir.is_empty() {
            return Ok(PathBuf::from(dir));
        }
    }

    #[cfg(target_os = "linux")]
    {
        let home = env::var_os("HOME").ok_or_else(|| Error::msg("HOME is not set"))?;
        return Ok(PathBuf::from(home).join(".amarcode"));
    }

    #[cfg(target_os = "windows")]
    {
        let local =
            env::var_os("LOCALAPPDATA").ok_or_else(|| Error::msg("LOCALAPPDATA is not set"))?;
        return Ok(PathBuf::from(local).join("amarcode"));
    }

    #[cfg(target_os = "macos")]
    {
        let home = env::var_os("HOME").ok_or_else(|| Error::msg("HOME is not set"))?;
        return Ok(PathBuf::from(home)
            .join("Library")
            .join("Application Support")
            .join("amarcode"));
    }

    #[cfg(not(any(target_os = "linux", target_os = "windows", target_os = "macos")))]
    {
        let home = env::var_os("HOME").ok_or_else(|| Error::msg("HOME is not set"))?;
        Ok(PathBuf::from(home).join(".amarcode"))
    }
}
