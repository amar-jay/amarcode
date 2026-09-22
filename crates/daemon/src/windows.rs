//! Environment and console setup for the Windows scheduled service.

use std::{ffi::OsStr, path::PathBuf};

use crate::{Error, Result};

fn service_path_file() -> Result<PathBuf> {
    // Keep this beside `data`, so installing a task does not create `data`
    // prematurely and prevent Config's migration of a legacy database.
    Ok(crate::config::resolve_default()?.with_file_name("service-path.json"))
}

pub(crate) fn clear_service_path() -> Result<()> {
    match std::fs::remove_file(service_path_file()?) {
        Ok(()) => Ok(()),
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => Ok(()),
        Err(error) => Err(error.into()),
    }
}

/// Task Scheduler does not inherit the installing application's environment.
pub(crate) fn save_service_path(path: Option<&OsStr>) -> Result<()> {
    let Some(path) = path else {
        return Ok(());
    };
    let file = service_path_file()?;
    let paths: Vec<PathBuf> = std::env::split_paths(path).collect();
    std::fs::create_dir_all(file.parent().expect("service data directory"))?;
    std::fs::write(file, serde_json::to_vec(&paths)?)?;
    Ok(())
}

/// Called by main before starting the async runtime, never from worker threads.
pub fn prepare_environment() -> Result<()> {
    let saved: Vec<PathBuf> = match std::fs::read(service_path_file()?) {
        Ok(bytes) => serde_json::from_slice(&bytes)?,
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => Vec::new(),
        Err(error) => return Err(error.into()),
    };
    let inherited = std::env::var_os("PATH").unwrap_or_default();
    let mut paths: Vec<PathBuf> = std::env::split_paths(&inherited).collect();
    paths.extend(saved);
    // Cover existing tasks installed before PATH persistence was introduced,
    // including tools installed after the desktop session started.
    for (variable, suffix) in [
        ("ProgramFiles", "Git/cmd"),
        ("ProgramFiles", "nodejs"),
        ("LOCALAPPDATA", "Programs/Git/cmd"),
        ("APPDATA", "npm"),
        ("LOCALAPPDATA", "pnpm"),
        ("USERPROFILE", ".bun/bin"),
        ("USERPROFILE", ".cargo/bin"),
        ("USERPROFILE", ".local/bin"),
        ("USERPROFILE", "scoop/shims"),
    ] {
        if let Some(root) = std::env::var_os(variable) {
            let path = PathBuf::from(root).join(suffix);
            if path.is_dir() && !paths.contains(&path) {
                paths.push(path);
            }
        }
    }
    if let Some(local) = std::env::var_os("LOCALAPPDATA") {
        extend_winget_bun_paths(
            &mut paths,
            &PathBuf::from(local).join("Microsoft/WinGet/Packages"),
        );
    }
    let path = std::env::join_paths(paths)
        .map_err(|error| Error::msg(format!("invalid daemon tool PATH: {error}")))?;
    std::env::set_var("PATH", path);
    Ok(())
}

fn extend_winget_bun_paths(paths: &mut Vec<PathBuf>, packages: &std::path::Path) {
    let Ok(entries) = std::fs::read_dir(packages) else {
        return;
    };
    for package in entries.flatten().filter(|entry| {
        entry
            .file_name()
            .to_string_lossy()
            .starts_with("Oven-sh.Bun_")
    }) {
        let Ok(architectures) = std::fs::read_dir(package.path()) else {
            continue;
        };
        for architecture in architectures.flatten() {
            let path = architecture.path();
            if path.is_dir() && !paths.contains(&path) {
                paths.push(path);
            }
        }
    }
}

pub fn attach_parent_console() {
    use windows_sys::Win32::{
        Foundation::INVALID_HANDLE_VALUE,
        System::Console::{
            AttachConsole, GetStdHandle, SetStdHandle, ATTACH_PARENT_PROCESS,
            STD_ERROR_HANDLE, STD_INPUT_HANDLE, STD_OUTPUT_HANDLE,
        },
    };
    // Preserve redirected lifecycle JSON output even if attaching to a CLI
    // console changes the standard handles. Never allocate a new console.
    // SAFETY: these APIs use process-global standard handles with no borrowed
    // pointers. This runs once, before other threads or stdio accesses.
    unsafe {
        let handles = [STD_INPUT_HANDLE, STD_OUTPUT_HANDLE, STD_ERROR_HANDLE]
            .map(|kind| (kind, GetStdHandle(kind)));
        AttachConsole(ATTACH_PARENT_PROCESS);
        for (kind, handle) in handles {
            if !handle.is_null() && handle != INVALID_HANDLE_VALUE {
                SetStdHandle(kind, handle);
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::extend_winget_bun_paths;

    #[test]
    fn discovers_bun_installed_by_winget() {
        let root = std::env::temp_dir().join(format!(
            "amarcode-winget-bun-{}",
            uuid::Uuid::new_v4()
        ));
        let bun = root
            .join("Oven-sh.Bun_Microsoft.Winget.Source_test")
            .join("bun-windows-x64");
        std::fs::create_dir_all(&bun).unwrap();
        std::fs::create_dir_all(root.join("Unrelated.Package").join("bin")).unwrap();

        let mut paths = Vec::new();
        extend_winget_bun_paths(&mut paths, &root);

        assert_eq!(paths, vec![bun]);
        let _ = std::fs::remove_dir_all(root);
    }
}
