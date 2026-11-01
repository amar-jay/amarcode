//! Desktop launch requests stay pending until the renderer acknowledges them.
use serde::Serialize;
use std::{
    ffi::OsString,
    path::{Path, PathBuf},
    sync::Mutex,
};
use tauri::{Emitter, Manager, State};

pub enum Command {
    Open(Option<String>),
    Help,
    Version,
}
pub fn parse(args: impl IntoIterator<Item = OsString>, cwd: &Path) -> Result<Command, String> {
    let mut args = args.into_iter();
    let Some(first) = args.next() else {
        return Ok(Command::Open(None));
    };
    let path = if first == "--" {
        args.next().ok_or("Expected a workspace path after --")?
    } else {
        if first == "--help" || first == "-h" {
            return Ok(Command::Help);
        }
        if first == "--version" || first == "-V" {
            return Ok(Command::Version);
        }
        if first.to_string_lossy().starts_with('-') {
            return Err(format!(
                "Unknown option: {}. Use amarcode --help.",
                first.to_string_lossy()
            ));
        }
        first
    };
    if args.next().is_some() {
        return Err("Open one workspace at a time. Quote paths containing spaces.".into());
    }
    let path = PathBuf::from(path);
    let path = if path.is_absolute() {
        path
    } else {
        cwd.join(path)
    };
    let path = path
        .canonicalize()
        .map_err(|e| format!("Cannot open workspace {}: {e}", path.display()))?;
    if !path.is_dir() {
        return Err(format!("Workspace is not a directory: {}", path.display()));
    }
    // Windows canonicalization adds a verbatim prefix which is unsuitable for
    // display and can confuse tools expecting ordinary drive/UNC paths.
    #[cfg(windows)]
    let path = PathBuf::from(
        path.to_str()
            .ok_or("Workspace path must be valid Unicode")?
            .strip_prefix(r"\\?\UNC\")
            .map(|p| format!(r"\\{p}"))
            .unwrap_or_else(|| {
                path.to_string_lossy()
                    .trim_start_matches(r"\\?\")
                    .to_owned()
            }),
    );
    Ok(Command::Open(Some(
        path.to_str()
            .ok_or("Workspace path must be valid Unicode")?
            .to_owned(),
    )))
}
#[derive(Clone, Serialize)]
pub struct WorkspaceLaunch {
    pub id: u64,
    pub path: String,
}
#[derive(Default)]
struct Pending {
    revision: u64,
    request: Option<WorkspaceLaunch>,
}
#[derive(Default)]
pub struct Launcher(Mutex<Pending>);
impl Launcher {
    pub fn new(path: Option<String>) -> Self {
        let state = Self::default();
        if let Some(path) = path {
            state.open(path);
        }
        state
    }
    fn open(&self, path: String) {
        if let Ok(mut state) = self.0.lock() {
            state.revision += 1;
            state.request = Some(WorkspaceLaunch {
                id: state.revision,
                path,
            });
        }
    }
    fn acknowledge(&self, id: u64) -> Result<(), String> {
        let mut state = self
            .0
            .lock()
            .map_err(|_| "Workspace launch lock poisoned")?;
        if state
            .request
            .as_ref()
            .is_some_and(|request| request.id == id)
        {
            state.request = None;
        }
        Ok(())
    }
}
#[tauri::command]
pub fn get_workspace_launch(
    launcher: State<'_, Launcher>,
) -> Result<Option<WorkspaceLaunch>, String> {
    Ok(launcher
        .0
        .lock()
        .map_err(|_| "Workspace launch lock poisoned")?
        .request
        .clone())
}
#[tauri::command]
pub fn acknowledge_workspace_launch(launcher: State<'_, Launcher>, id: u64) -> Result<(), String> {
    launcher.acknowledge(id)
}
pub fn forward(app: &tauri::AppHandle, args: Vec<String>, cwd: String) {
    match parse(
        args.into_iter().skip(1).map(OsString::from),
        Path::new(&cwd),
    ) {
        Ok(Command::Open(Some(path))) => {
            app.state::<Launcher>().open(path);
            let _ = app.emit("workspace-launch-requested", ());
        }
        Ok(_) => {}
        Err(error) => {
            eprintln!("amarcode: {error}");
        }
    }
    if let Some(window) = app.get_webview_window("main") {
        let _ = window.show();
        let _ = window.unminimize();
        let _ = window.set_focus();
    }
}
#[cfg(test)]
mod tests {
    use super::*;
    fn parse_args(args: &[&str], cwd: &Path) -> Result<Option<String>, String> {
        match parse(args.iter().map(OsString::from), cwd)? {
            Command::Open(path) => Ok(path),
            _ => Err("Expected open".into()),
        }
    }
    #[test]
    fn resolves_caller_directory_and_paths_with_spaces() {
        let root = tempfile::tempdir().unwrap();
        let folder = root.path().join("my project");
        std::fs::create_dir(&folder).unwrap();
        let current = parse_args(&["."], root.path()).unwrap().unwrap();
        assert!(Path::new(&current).is_absolute());
        let relative = parse_args(&["my project"], root.path()).unwrap().unwrap();
        assert!(Path::new(&relative).ends_with("my project"));
        assert_eq!(
            Some(relative.clone()),
            parse_args(&[&relative], root.path()).unwrap()
        );
        assert_eq!(None, parse_args(&[], root.path()).unwrap());
    }
    #[test]
    fn rejects_files_missing_paths_and_extra_arguments() {
        let root = tempfile::tempdir().unwrap();
        std::fs::write(root.path().join("file"), "text").unwrap();
        for args in [
            vec!["file"],
            vec!["missing"],
            vec![".", "other"],
            vec!["--bad"],
            vec!["--"],
        ] {
            assert!(parse_args(&args, root.path()).is_err());
        }
        std::fs::create_dir(root.path().join("-folder")).unwrap();
        assert!(parse_args(&["--", "-folder"], root.path()).is_ok());
    }
    #[test]
    fn launch_requests_keep_the_latest_workspace() {
        let launcher = Launcher::new(Some("/one".into()));
        launcher.open("/two".into());
        let state = launcher.0.lock().unwrap();
        let request = state.request.as_ref().unwrap();
        assert_eq!(request.id, 2);
        assert_eq!(request.path, "/two");
    }
    #[test]
    fn stale_acknowledgments_cannot_erase_a_new_launch() {
        let launcher = Launcher::new(Some("/one".into()));
        launcher.open("/two".into());
        launcher.acknowledge(1).unwrap();
        assert_eq!(
            launcher.0.lock().unwrap().request.as_ref().unwrap().path,
            "/two"
        );
        launcher.acknowledge(2).unwrap();
        assert!(launcher.0.lock().unwrap().request.is_none());
        launcher.acknowledge(2).unwrap();
    }
}
