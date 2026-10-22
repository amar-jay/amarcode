//! Daemon-owned workspace file discovery, editing, and Git operations.
//!
//! Called on blocking workers by RPC. Paths refer to the daemon host. No Tauri,
//! transport, or database dependencies belong here.

use amarcode_protocol::{WorkspaceChange, WorkspaceDiff, WorkspaceInfo};
use ignore::WalkBuilder;
use std::{
    fs,
    path::{Component, Path, PathBuf},
    process::Command,
};

mod editor;
pub use editor::{read_workspace_file, save_workspace_file};

pub fn list_workspace_files(
    workspace_path: String,
    query: Option<String>,
) -> Result<Vec<String>, String> {
    let root = PathBuf::from(workspace_path);
    if !root.is_dir() {
        return Err("The selected workspace folder is unavailable.".to_string());
    }
    let query = query
        .as_deref()
        .map(str::trim)
        .filter(|query| !query.is_empty())
        .map(str::to_lowercase);
    let mut files = Vec::new();
    let mut walker = WalkBuilder::new(&root);
    walker
        // A workspace need not be a Git checkout for its .gitignore to be useful.
        .require_git(false)
        .git_ignore(true)
        // Keep this command's scope to the workspace .gitignore rather than
        // user-global or tool-specific ignore files.
        .git_global(false)
        .git_exclude(false)
        .ignore(false)
        .parents(false)
        .hidden(false)
        .max_depth(Some(6))
        .filter_entry(|entry| {
            !matches!(
                entry.file_name().to_str(),
                Some(".git" | "node_modules" | "target" | "dist")
            )
        });

    for entry in walker.build() {
        let entry = entry.map_err(|error| error.to_string())?;
        if !entry
            .file_type()
            .is_some_and(|file_type| file_type.is_file())
        {
            continue;
        }
        if let Ok(relative) = entry.path().strip_prefix(&root) {
            let relative = relative.to_string_lossy().replace('\\', "/");
            if let Some(query) = &query {
                if !relative.to_lowercase().contains(query) {
                    continue;
                }
            }
            files.push(relative);
        }
    }
    files.sort();
    Ok(files)
}

fn git_output(root: &Path, args: &[&str]) -> Result<std::process::Output, String> {
    let mut command = Command::new("git");
    #[cfg(windows)]
    {
        use std::os::windows::process::CommandExt;
        command.creation_flags(0x0800_0000); // CREATE_NO_WINDOW
    }
    command
        .arg("-C")
        .arg(root)
        .args(args)
        .output()
        .map_err(|error| format!("Could not run Git: {error}"))
}

fn workspace_changes(root: &Path) -> Result<Vec<WorkspaceChange>, String> {
    let output = git_output(root, &["status", "--porcelain=v1", "-z"])?;
    if !output.status.success() {
        return Err(String::from_utf8_lossy(&output.stderr).trim().to_string());
    }

    let mut records = output.stdout.split(|byte| *byte == b'\0');
    let mut changes = Vec::new();
    while let Some(record) = records.next() {
        if record.len() < 4 || record[0] == b'!' {
            continue;
        }
        let index = record[0] as char;
        let worktree = record[1] as char;
        let path = String::from_utf8_lossy(&record[3..]).into_owned();
        if path.is_empty() {
            continue;
        }
        // Rename/copy records contain a second, NUL-delimited source path.
        if matches!(index, 'R' | 'C') {
            let _ = records.next();
        }
        let staged = !matches!(index, ' ' | '?');
        // An untracked entry is reported as `??`; it compares an empty
        // baseline to the file currently in the working tree.
        let unstaged = worktree != ' ';
        let status = if index == '?' {
            "A".to_string()
        } else if staged {
            index.to_string()
        } else {
            worktree.to_string()
        };
        changes.push(WorkspaceChange {
            path,
            status,
            staged,
            unstaged,
        });
    }
    changes.sort_by(|left, right| left.path.cmp(&right.path));
    Ok(changes)
}

fn workspace_relative_path(root: &Path, relative_path: &str) -> Result<PathBuf, String> {
    let relative = Path::new(relative_path);
    if relative.as_os_str().is_empty()
        || relative.is_absolute()
        || relative
            .components()
            .any(|component| !matches!(component, Component::Normal(_)))
    {
        return Err("The requested file must be inside the workspace.".to_string());
    }
    let canonical_root = root
        .canonicalize()
        .map_err(|_| "The selected workspace folder is unavailable.".to_string())?;
    if !canonical_root.is_dir() {
        return Err("The selected workspace folder is unavailable.".to_string());
    }
    let full_path = canonical_root.join(relative);
    // Resolve the nearest existing ancestor even for deleted paths. A missing
    // file below an escaping symlink must not bypass workspace containment.
    let mut ancestor = full_path.as_path();
    loop {
        match fs::symlink_metadata(ancestor) {
            Ok(_) => break,
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => {
                ancestor = ancestor.parent().ok_or_else(|| {
                    "The requested file must be inside the workspace.".to_string()
                })?;
            }
            Err(error) => return Err(error.to_string()),
        }
    }
    if !ancestor
        .canonicalize()
        .map_err(|error| error.to_string())?
        .starts_with(&canonical_root)
    {
        return Err("The requested file must be inside the workspace.".to_string());
    }
    Ok(full_path)
}

fn text_content(bytes: Vec<u8>) -> Result<String, String> {
    const MAX_DIFF_FILE_BYTES: usize = 1_000_000;
    if bytes.len() > MAX_DIFF_FILE_BYTES {
        return Err("This file is too large to display as a diff.".to_string());
    }
    if bytes.contains(&0) {
        return Err("Binary files cannot be displayed as a text diff.".to_string());
    }
    Ok(String::from_utf8_lossy(&bytes).into_owned())
}

fn git_blob(root: &Path, revision: &str, path: &str) -> Result<Option<Vec<u8>>, String> {
    let spec = format!("{revision}:{path}");
    let output = git_output(root, &["show", "--no-textconv", &spec])?;
    if output.status.success() {
        Ok(Some(output.stdout))
    } else {
        Ok(None)
    }
}

pub fn list_workspace_changes(workspace_path: String) -> Result<Vec<WorkspaceChange>, String> {
    let root = PathBuf::from(workspace_path);
    if !root.is_dir() {
        return Err("The selected workspace folder is unavailable.".to_string());
    }
    workspace_changes(&root)
}

pub fn stage_workspace_file(workspace_path: String, relative_path: String) -> Result<(), String> {
    let root = PathBuf::from(workspace_path);
    if !root.is_dir() {
        return Err("The selected workspace folder is unavailable.".to_string());
    }
    workspace_relative_path(&root, &relative_path)?;
    let output = git_output(&root, &["--literal-pathspecs", "add", "--", &relative_path])?;
    if output.status.success() {
        Ok(())
    } else {
        Err(String::from_utf8_lossy(&output.stderr).trim().to_string())
    }
}

pub fn unstage_workspace_file(workspace_path: String, relative_path: String) -> Result<(), String> {
    let root = PathBuf::from(workspace_path);
    if !root.is_dir() {
        return Err("The selected workspace folder is unavailable.".to_string());
    }
    workspace_relative_path(&root, &relative_path)?;
    let output = git_output(
        &root,
        &[
            "--literal-pathspecs",
            "restore",
            "--staged",
            "--",
            &relative_path,
        ],
    )?;
    if output.status.success() {
        Ok(())
    } else {
        Err(String::from_utf8_lossy(&output.stderr).trim().to_string())
    }
}

pub fn stage_all_workspace_files(workspace_path: String) -> Result<(), String> {
    let root = PathBuf::from(workspace_path);
    if !root.is_dir() {
        return Err("The selected workspace folder is unavailable.".to_string());
    }
    let output = git_output(&root, &["add", "--all"])?;
    if output.status.success() {
        Ok(())
    } else {
        Err(String::from_utf8_lossy(&output.stderr).trim().to_string())
    }
}

pub fn commit_workspace_changes(workspace_path: String, message: String) -> Result<(), String> {
    let root = PathBuf::from(workspace_path);
    if !root.is_dir() {
        return Err("The selected workspace folder is unavailable.".to_string());
    }
    let message = message.trim();
    if message.is_empty() {
        return Err("Enter a commit message.".to_string());
    }
    let output = git_output(&root, &["commit", "-m", message])?;
    if output.status.success() {
        Ok(())
    } else {
        Err(String::from_utf8_lossy(&output.stderr).trim().to_string())
    }
}

pub fn get_workspace_file_diff(
    workspace_path: String,
    relative_path: String,
    staged: Option<bool>,
) -> Result<WorkspaceDiff, String> {
    let root = PathBuf::from(workspace_path);
    let full_path = workspace_relative_path(&root, &relative_path)?;
    let change = workspace_changes(&root)?
        .into_iter()
        .find(|change| change.path == relative_path);

    if change.is_none() {
        let contents = text_content(fs::read(&full_path).map_err(|error| error.to_string())?)?;
        return Ok(WorkspaceDiff {
            path: relative_path,
            status: "Current".to_string(),
            before: contents.clone(),
            after: contents,
            comparison: "Current file".to_string(),
        });
    }
    let change = change.expect("checked above");

    let (before, after, comparison) = if !staged.unwrap_or(!change.unstaged) {
        let before = text_content(git_blob(&root, "", &relative_path)?.unwrap_or_default())?;
        let after = match fs::read(&full_path) {
            Ok(contents) => text_content(contents)?,
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => String::new(),
            Err(error) => return Err(error.to_string()),
        };
        (before, after, "Unstaged".to_string())
    } else {
        let before = text_content(git_blob(&root, "HEAD", &relative_path)?.unwrap_or_default())?;
        let after = text_content(git_blob(&root, "", &relative_path)?.unwrap_or_default())?;
        (before, after, "Staged".to_string())
    };

    Ok(WorkspaceDiff {
        path: relative_path,
        status: change.status,
        before,
        after,
        comparison,
    })
}

pub fn get_workspace_info(workspace_path: String) -> Result<WorkspaceInfo, String> {
    let path = PathBuf::from(&workspace_path);
    if !path.is_dir() {
        return Err("The selected workspace folder is unavailable.".to_string());
    }

    let display_name = path
        .file_name()
        .filter(|name| !name.is_empty())
        .map(|name| name.to_string_lossy().into_owned())
        .unwrap_or(workspace_path);

    let is_git_repository = git_output(&path, &["rev-parse", "--is-inside-work-tree"])?
        .status
        .success();

    let branch_name = if is_git_repository {
        let branch = git_output(&path, &["symbolic-ref", "--quiet", "--short", "HEAD"])?;
        if branch.status.success() {
            Some(String::from_utf8_lossy(&branch.stdout).trim().to_string())
        } else {
            let commit = git_output(&path, &["rev-parse", "--short", "HEAD"])?;
            commit
                .status
                .success()
                .then(|| String::from_utf8_lossy(&commit.stdout).trim().to_string())
        }
        .filter(|name| !name.is_empty())
    } else {
        None
    };

    Ok(WorkspaceInfo {
        display_name,
        is_git_repository,
        branch_name,
    })
}

#[cfg(test)]
mod workspace_diff_tests {
    use super::*;

    #[test]
    fn listing_uses_workspace_ignores_without_requiring_git() {
        let directory = tempfile::tempdir().unwrap();
        let root = directory.path();
        fs::write(root.join(".gitignore"), "ignored.txt\n").unwrap();
        fs::write(root.join("ignored.txt"), "ignored").unwrap();
        fs::write(root.join("visible.txt"), "visible").unwrap();
        let path = root.to_string_lossy().into_owned();
        let files = list_workspace_files(path.clone(), None).unwrap();
        assert_eq!(files, [".gitignore", "visible.txt"]);
        assert!(list_workspace_files(path, Some("missing".into()))
            .unwrap()
            .is_empty());
        assert!(
            list_workspace_files(root.join("missing").to_string_lossy().into_owned(), None)
                .is_err()
        );
    }

    #[cfg(unix)]
    #[test]
    fn path_validation_rejects_escaping_and_dangling_symlinks() {
        use std::os::unix::fs::symlink;
        let workspace = tempfile::tempdir().unwrap();
        let outside = tempfile::tempdir().unwrap();
        fs::write(outside.path().join("secret.txt"), "outside").unwrap();
        symlink(outside.path(), workspace.path().join("escape")).unwrap();
        symlink(
            outside.path().join("absent"),
            workspace.path().join("dangling"),
        )
        .unwrap();
        for path in [
            "",
            "../secret.txt",
            "escape/secret.txt",
            "escape/deleted.txt",
            "dangling",
        ] {
            assert!(
                workspace_relative_path(workspace.path(), path).is_err(),
                "accepted {path}"
            );
        }
        fs::create_dir(workspace.path().join("inside")).unwrap();
        symlink(
            workspace.path().join("inside"),
            workspace.path().join("link"),
        )
        .unwrap();
        assert!(workspace_relative_path(workspace.path(), "link/deleted.txt").is_ok());
    }

    #[test]
    fn staging_one_file_treats_pathspec_metacharacters_literally() {
        let directory = tempfile::tempdir().unwrap();
        let root = directory.path();
        let output = git_output(root, &["init"]).unwrap();
        assert!(output.status.success());
        fs::write(root.join("file[1].txt"), "selected").unwrap();
        fs::write(root.join("file1.txt"), "other").unwrap();
        stage_workspace_file(root.to_string_lossy().into_owned(), "file[1].txt".into()).unwrap();
        let changes = workspace_changes(root).unwrap();
        assert!(
            changes
                .iter()
                .find(|change| change.path == "file[1].txt")
                .unwrap()
                .staged
        );
        assert!(
            !changes
                .iter()
                .find(|change| change.path == "file1.txt")
                .unwrap()
                .staged
        );
    }

    #[test]
    fn partially_staged_file_has_distinct_diffs() {
        let root = std::env::temp_dir().join(format!(
            "amarcode-diff-{}-{}",
            std::process::id(),
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap()
                .as_nanos()
        ));
        fs::create_dir_all(&root).unwrap();
        let git = |args: &[&str]| {
            let output = git_output(&root, args).unwrap();
            assert!(
                output.status.success(),
                "{}",
                String::from_utf8_lossy(&output.stderr)
            );
        };
        git(&["init"]);
        fs::write(root.join("file.txt"), "committed\n").unwrap();
        git(&["add", "file.txt"]);
        git(&[
            "-c",
            "user.name=Test",
            "-c",
            "user.email=test@example.com",
            "-c",
            "commit.gpgsign=false",
            "commit",
            "-m",
            "initial",
        ]);
        fs::write(root.join("file.txt"), "staged\n").unwrap();
        git(&["add", "file.txt"]);
        fs::write(root.join("file.txt"), "working\n").unwrap();
        let path = root.to_string_lossy().into_owned();
        let changes = workspace_changes(&root).unwrap();
        assert!(changes[0].staged && changes[0].unstaged);
        let staged = get_workspace_file_diff(path.clone(), "file.txt".into(), Some(true)).unwrap();
        assert_eq!(
            (staged.before.as_str(), staged.after.as_str()),
            ("committed\n", "staged\n")
        );
        assert_eq!(staged.comparison, "Staged");
        let unstaged =
            get_workspace_file_diff(path.clone(), "file.txt".into(), Some(false)).unwrap();
        assert_eq!(
            (unstaged.before.as_str(), unstaged.after.as_str()),
            ("staged\n", "working\n")
        );
        assert_eq!(unstaged.comparison, "Unstaged");
        let automatic = get_workspace_file_diff(path, "file.txt".into(), None).unwrap();
        assert_eq!(automatic.after, unstaged.after);
        fs::remove_dir_all(root).unwrap();
    }
}
