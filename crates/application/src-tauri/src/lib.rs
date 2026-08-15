mod config;
mod daemon;
mod protocol;
mod state;
mod window;

use std::{
    fs,
    path::{Component, Path, PathBuf},
    process::Command,
};

use ignore::WalkBuilder;
use serde::Serialize;
use tauri::{ipc::Channel, AppHandle, State};

use crate::{
    protocol::{
        events::EditorEvent,
        rpc::{
            CancelResult, DeleteChatResult, GetAttachmentResult, HealthResult, PromptAttachment,
            PromptResultDto, RespondAgentParams, RespondAgentResult, VersionResult,
        },
        AgentInfo, Chat, GetChatResult,
    },
    state::AppState,
};

#[tauri::command]
async fn daemon_bootstrap(
    app: AppHandle,
    manager: State<'_, daemon::DaemonManager>,
    on_status: Channel<daemon::DaemonBootstrapStatus>,
) -> Result<Option<HealthResult>, String> {
    match manager.bootstrap(&app, &on_status).await {
        Ok(health) => Ok(health),
        Err(error) => {
            let _ = on_status.send(daemon::DaemonBootstrapStatus::Failed {
                error: error.clone(),
            });
            Err(error)
        }
    }
}

#[tauri::command]
async fn daemon_install(
    app: AppHandle,
    manager: State<'_, daemon::DaemonManager>,
    on_status: Channel<daemon::DaemonBootstrapStatus>,
) -> Result<HealthResult, String> {
    match manager.install(&app, &on_status).await {
        Ok(health) => Ok(health),
        Err(error) => {
            let _ = on_status.send(daemon::DaemonBootstrapStatus::Failed {
                error: error.clone(),
            });
            Err(error)
        }
    }
}

#[tauri::command]
async fn daemon_check_update(
    app: AppHandle,
    manager: State<'_, daemon::DaemonManager>,
) -> Result<daemon::DaemonUpdateCheck, String> {
    manager.check_update(&app).await
}

#[tauri::command]
async fn daemon_update(
    app: AppHandle,
    manager: State<'_, daemon::DaemonManager>,
    on_status: Channel<daemon::DaemonUpdateStatus>,
) -> Result<HealthResult, String> {
    match manager.update(&app, &on_status).await {
        Ok(health) => Ok(health),
        Err(error) => {
            let _ = on_status.send(daemon::DaemonUpdateStatus::Failed {
                error: error.clone(),
            });
            Err(error)
        }
    }
}

#[tauri::command]
async fn prepare_application_uninstall(
    app: AppHandle,
    manager: State<'_, daemon::DaemonManager>,
    confirmed: bool,
    on_status: Channel<daemon::ApplicationCleanupStatus>,
) -> Result<daemon::ApplicationCleanupResult, String> {
    match manager.prepare_uninstall(&app, confirmed, &on_status).await {
        Ok(result) => Ok(result),
        Err(error) => {
            let _ = on_status.send(daemon::ApplicationCleanupStatus::Failed {
                error: error.clone(),
            });
            Err(error)
        }
    }
}

#[tauri::command]
fn exit_application(app: AppHandle) {
    app.exit(0);
}

#[tauri::command]
async fn daemon_health(state: State<'_, AppState>) -> Result<HealthResult, String> {
    state.health().await
}

#[tauri::command]
async fn daemon_version(state: State<'_, AppState>) -> Result<VersionResult, String> {
    state.version().await
}

#[tauri::command]
async fn list_agents(state: State<'_, AppState>) -> Result<Vec<AgentInfo>, String> {
    state.list_agents().await
}

#[tauri::command]
async fn create_chat(
    state: State<'_, AppState>,
    workspace_path: String,
    title: Option<String>,
) -> Result<Chat, String> {
    state.create_chat(workspace_path, title).await
}

#[tauri::command]
async fn list_chats(
    state: State<'_, AppState>,
    workspace_path: Option<String>,
) -> Result<Vec<Chat>, String> {
    state.list_chats(workspace_path).await
}

#[tauri::command]
async fn get_chat(
    state: State<'_, AppState>,
    chat_id: String,
    include_messages: bool,
) -> Result<GetChatResult, String> {
    state.get_chat(chat_id, include_messages).await
}

#[tauri::command]
async fn delete_chat(
    state: State<'_, AppState>,
    chat_id: String,
) -> Result<DeleteChatResult, String> {
    state.delete_chat(chat_id).await
}

#[tauri::command]
async fn get_attachment(
    state: State<'_, AppState>,
    chat_id: String,
    attachment_id: String,
) -> Result<GetAttachmentResult, String> {
    state.get_attachment(chat_id, attachment_id).await
}

#[tauri::command]
async fn prompt(
    state: State<'_, AppState>,
    chat_id: String,
    agent_id: String,
    text: String,
    attachments: Vec<PromptAttachment>,
    session_mode: Option<String>,
) -> Result<PromptResultDto, String> {
    state
        .prompt(chat_id, agent_id, text, attachments, session_mode)
        .await
}

#[tauri::command]
async fn set_session_mode(
    state: State<'_, AppState>,
    chat_id: String,
    mode: String,
) -> Result<(), String> {
    state.set_session_mode(chat_id, mode).await
}

#[tauri::command]
async fn cancel(state: State<'_, AppState>, chat_id: String) -> Result<CancelResult, String> {
    state.cancel(chat_id).await
}

#[tauri::command]
async fn respond_permission(
    state: State<'_, AppState>,
    params: RespondAgentParams,
) -> Result<RespondAgentResult, String> {
    state.respond_permission(params).await
}

#[tauri::command]
async fn respond_input(
    state: State<'_, AppState>,
    params: RespondAgentParams,
) -> Result<RespondAgentResult, String> {
    state.respond_input(params).await
}

#[derive(Clone, Serialize)]
#[serde(tag = "status", rename_all = "camelCase")]
enum DaemonEventStreamStatus {
    Connected,
    Disconnected { error: String },
}

/// Run one daemon event stream for the lifetime of its dedicated TCP
/// connection. Keeping this command pending is intentional: if the daemon
/// disconnects, the returned error rejects the frontend `invoke` promise so
/// React can reconnect instead of silently retaining a dead subscription.
#[tauri::command]
async fn subscribe_events(
    state: State<'_, AppState>,
    filter: crate::protocol::rpc::SubscribeEventsParams,
    on_event: Channel<EditorEvent>,
    on_status: Channel<DaemonEventStreamStatus>,
) -> Result<(), String> {
    let mut subscription = state.subscribe_events(filter).await?;
    on_status
        .send(DaemonEventStreamStatus::Connected)
        .map_err(|error| error.to_string())?;

    loop {
        match subscription.next::<EditorEvent>().await {
            Ok(event) => on_event.send(event).map_err(|error| error.to_string())?,
            Err(error) => {
                let _ = on_status.send(DaemonEventStreamStatus::Disconnected {
                    error: error.clone(),
                });
                return Err(error);
            }
        }
    }
}

#[tauri::command]
fn list_workspace_files(workspace_path: String) -> Result<Vec<String>, String> {
    let root = PathBuf::from(workspace_path);
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
            files.push(relative.to_string_lossy().to_string());
        }
    }
    files.sort();
    Ok(files)
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
struct WorkspaceInfo {
    display_name: String,
    is_git_repository: bool,
    branch_name: Option<String>,
}

#[derive(Clone, Serialize)]
#[serde(rename_all = "camelCase")]
struct WorkspaceChange {
    path: String,
    status: String,
    staged: bool,
    unstaged: bool,
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
struct WorkspaceDiff {
    path: String,
    status: String,
    before: String,
    after: String,
    comparison: String,
}

fn git_output(root: &Path, args: &[&str]) -> Result<std::process::Output, String> {
    Command::new("git")
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
    if relative.is_absolute()
        || relative
            .components()
            .any(|component| !matches!(component, Component::Normal(_)))
    {
        return Err("The requested file must be inside the workspace.".to_string());
    }
    let full_path = root.join(relative);
    if !full_path.starts_with(root) {
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

#[tauri::command]
fn list_workspace_changes(workspace_path: String) -> Result<Vec<WorkspaceChange>, String> {
    let root = PathBuf::from(workspace_path);
    if !root.is_dir() {
        return Err("The selected workspace folder is unavailable.".to_string());
    }
    workspace_changes(&root)
}

#[tauri::command]
fn get_workspace_file_diff(
    workspace_path: String,
    relative_path: String,
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

    let (before, after, comparison) = if change.unstaged {
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

#[tauri::command]
fn get_workspace_info(workspace_path: String) -> Result<WorkspaceInfo, String> {
    let path = PathBuf::from(&workspace_path);
    if !path.is_dir() {
        return Err("The selected workspace folder is unavailable.".to_string());
    }

    let display_name = path
        .file_name()
        .filter(|name| !name.is_empty())
        .map(|name| name.to_string_lossy().into_owned())
        .unwrap_or(workspace_path);

    let is_git_repository = Command::new("git")
        .arg("-C")
        .arg(&path)
        .args(["rev-parse", "--is-inside-work-tree"])
        .output()
        .map_err(|error| format!("Could not check the workspace Git status: {error}"))?
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

pub fn run() {
    let app = tauri::Builder::default()
        .plugin(tauri_plugin_clipboard_manager::init())
        .plugin(tauri_plugin_dialog::init())
        .plugin(tauri_plugin_notification::init())
        .plugin(tauri_plugin_opener::init())
        .setup(|app| {
            window::fit_main_window(app)?;
            Ok(())
        })
        .manage(daemon::DaemonManager::new())
        .manage(AppState::new())
        .invoke_handler(tauri::generate_handler![
            daemon_bootstrap,
            daemon_install,
            daemon_check_update,
            daemon_update,
            prepare_application_uninstall,
            exit_application,
            daemon_health,
            daemon_version,
            list_agents,
            create_chat,
            list_chats,
            get_chat,
            get_attachment,
            delete_chat,
            prompt,
            set_session_mode,
            cancel,
            respond_permission,
            respond_input,
            subscribe_events,
            list_workspace_files,
            get_workspace_info,
            list_workspace_changes,
            get_workspace_file_diff,
        ])
        .build(tauri::generate_context!())
        .expect("error while building amarcode");

    // The native service manager exclusively owns the daemon lifecycle. The
    // desktop application has no daemon child process to stop on exit.
    app.run(|_, _| {});
}
