//! Process-wide ownership lock for a daemon database.
//!
//! The lock is an operating-system advisory lock, so it is released
//! automatically if the daemon exits or crashes. The lock file deliberately
//! remains on disk: deleting it on shutdown could let a racing process lock a
//! different inode while the original process still owns the old one.

use std::{
    fs::{self, File, OpenOptions, TryLockError},
    io::Write,
    path::{Path, PathBuf},
};

use crate::{Error, Result};

const LOCK_FILE_SUFFIX: &str = ".amarcode.lock";

/// Keeps exclusive ownership of one daemon database for its entire lifetime.
#[derive(Debug)]
pub(crate) struct InstanceLock {
    _file: File,
    _path: PathBuf,
}

impl InstanceLock {
    /// Acquire the lock associated with `db_path` without waiting.
    pub(crate) fn acquire(db_path: &Path) -> Result<Self> {
        let path = lock_path(db_path)?;
        let mut file = OpenOptions::new()
            .create(true)
            .read(true)
            .write(true)
            .open(&path)
            .map_err(|error| {
                Error::msg(format!(
                    "failed to open daemon instance lock {}: {error}",
                    path.display()
                ))
            })?;

        file.try_lock().map_err(|error| match error {
            TryLockError::WouldBlock => Error::msg(format!(
                "another Amarcode daemon already owns database {} (lock: {})",
                db_path.display(),
                path.display()
            )),
            TryLockError::Error(error) => Error::msg(format!(
                "failed to acquire daemon instance lock {}: {error}",
                path.display()
            )),
        })?;

        // This metadata is diagnostic only. Lock ownership comes from the OS,
        // never from the file contents, so stale text after a crash is harmless.
        file.set_len(0).map_err(|error| {
            Error::msg(format!(
                "failed to update daemon instance lock {}: {error}",
                path.display()
            ))
        })?;
        write!(
            file,
            "pid={}\nversion={}\ndatabase={}\n",
            std::process::id(),
            env!("CARGO_PKG_VERSION"),
            db_path.display()
        )
        .map_err(|error| {
            Error::msg(format!(
                "failed to update daemon instance lock {}: {error}",
                path.display()
            ))
        })?;

        Ok(Self {
            _file: file,
            _path: path,
        })
    }

    #[cfg(test)]
    fn path(&self) -> &Path {
        &self._path
    }
}

fn lock_path(db_path: &Path) -> Result<PathBuf> {
    let parent = db_path.parent().filter(|path| !path.as_os_str().is_empty());
    if let Some(parent) = parent {
        fs::create_dir_all(parent).map_err(|error| {
            Error::msg(format!(
                "failed to create database directory {}: {error}",
                parent.display()
            ))
        })?;
    }

    // Resolve aliases where possible so relative paths and symlinks converge
    // on the same lock file before SQLite is opened.
    let canonical_db = if db_path.exists() {
        fs::canonicalize(db_path)
    } else {
        let parent = parent
            .map(Path::to_path_buf)
            .unwrap_or(std::env::current_dir()?);
        fs::canonicalize(parent).map(|parent| {
            parent.join(
                db_path
                    .file_name()
                    .unwrap_or_else(|| std::ffi::OsStr::new("workspace.sqlite3")),
            )
        })
    }
    .map_err(|error| {
        Error::msg(format!(
            "failed to resolve database path {}: {error}",
            db_path.display()
        ))
    })?;

    let mut lock_name = canonical_db
        .file_name()
        .unwrap_or_else(|| std::ffi::OsStr::new("workspace.sqlite3"))
        .to_os_string();
    lock_name.push(LOCK_FILE_SUFFIX);
    Ok(canonical_db.with_file_name(lock_name))
}

#[cfg(test)]
mod tests {
    use std::fs;

    use super::InstanceLock;

    fn test_directory(name: &str) -> std::path::PathBuf {
        let path = std::env::temp_dir().join(format!(
            "amarcode-instance-lock-{name}-{}",
            uuid::Uuid::new_v4()
        ));
        fs::create_dir_all(&path).expect("create test directory");
        path
    }

    #[test]
    fn rejects_a_second_owner_for_the_same_database() {
        let directory = test_directory("contention");
        let database = directory.join("workspace.sqlite3");
        let first = InstanceLock::acquire(&database).expect("acquire first lock");

        let error = InstanceLock::acquire(&database).expect_err("reject second lock");

        assert!(error.to_string().contains("already owns database"));
        assert!(first.path().exists());
    }

    #[test]
    fn releases_ownership_when_the_guard_is_dropped() {
        let directory = test_directory("release");
        let database = directory.join("workspace.sqlite3");
        let first = InstanceLock::acquire(&database).expect("acquire first lock");
        let lock_path = first.path().to_path_buf();
        drop(first);

        InstanceLock::acquire(&database).expect("reacquire released lock");
        assert!(
            lock_path.exists(),
            "lock file intentionally remains on disk"
        );
    }

    #[test]
    fn equivalent_normalized_and_non_normalized_paths_contend() {
        let directory = test_directory("canonical");
        let database = directory.join("workspace.sqlite3");
        fs::create_dir(directory.join("alias")).expect("create alias directory");
        let non_normalized = directory.join("alias").join("..").join("workspace.sqlite3");
        let _first = InstanceLock::acquire(&non_normalized).expect("acquire non-normalized lock");

        let error = InstanceLock::acquire(&database).expect_err("reject absolute alias");
        assert!(error.to_string().contains("already owns database"));
    }
}
