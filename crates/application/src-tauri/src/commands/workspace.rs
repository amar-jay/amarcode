//! Tauri IPC adapters for workspace operations.
use crate::daemon::DaemonClient;
use amarcode_protocol::rpc::{
    CommitWorkspaceChangesParams, GetWorkspaceFileDiffParams, ListWorkspaceFilesParams,
    SaveWorkspaceFileParams, WorkspaceFileParams, WorkspaceParams,
};
use amarcode_protocol::{WorkspaceChange, WorkspaceDiff, WorkspaceInfo};
use tauri::State;

#[tauri::command]
pub async fn list_workspace_files(
    client: State<'_, DaemonClient>,
    workspace_path: String,
    query: Option<String>,
) -> Result<Vec<String>, String> {
    client
        .list_workspace_files(ListWorkspaceFilesParams {
            workspace_path,
            query,
        })
        .await
}

#[tauri::command]
pub async fn get_workspace_info(
    client: State<'_, DaemonClient>,
    workspace_path: String,
) -> Result<WorkspaceInfo, String> {
    client
        .get_workspace_info(WorkspaceParams { workspace_path })
        .await
}

#[tauri::command]
pub async fn list_workspace_changes(
    client: State<'_, DaemonClient>,
    workspace_path: String,
) -> Result<Vec<WorkspaceChange>, String> {
    client
        .list_workspace_changes(WorkspaceParams { workspace_path })
        .await
}

#[tauri::command]
pub async fn stage_workspace_file(
    client: State<'_, DaemonClient>,
    workspace_path: String,
    relative_path: String,
) -> Result<(), String> {
    client
        .stage_workspace_file(WorkspaceFileParams {
            workspace_path,
            relative_path,
        })
        .await
}

#[tauri::command]
pub async fn unstage_workspace_file(
    client: State<'_, DaemonClient>,
    workspace_path: String,
    relative_path: String,
) -> Result<(), String> {
    client
        .unstage_workspace_file(WorkspaceFileParams {
            workspace_path,
            relative_path,
        })
        .await
}

#[tauri::command]
pub async fn stage_all_workspace_files(
    client: State<'_, DaemonClient>,
    workspace_path: String,
) -> Result<(), String> {
    client
        .stage_all_workspace_files(WorkspaceParams { workspace_path })
        .await
}

#[tauri::command]
pub async fn commit_workspace_changes(
    client: State<'_, DaemonClient>,
    workspace_path: String,
    message: String,
) -> Result<(), String> {
    client
        .commit_workspace_changes(CommitWorkspaceChangesParams {
            workspace_path,
            message,
        })
        .await
}

#[tauri::command]
pub async fn get_workspace_file_diff(
    client: State<'_, DaemonClient>,
    workspace_path: String,
    relative_path: String,
    staged: Option<bool>,
) -> Result<WorkspaceDiff, String> {
    client
        .get_workspace_file_diff(GetWorkspaceFileDiffParams {
            workspace_path,
            relative_path,
            staged,
        })
        .await
}

#[tauri::command]
pub async fn read_workspace_file(
    client: State<'_, DaemonClient>,
    workspace_path: String,
    relative_path: String,
) -> Result<String, String> {
    client
        .read_workspace_file(WorkspaceFileParams {
            workspace_path,
            relative_path,
        })
        .await
}

#[tauri::command]
pub async fn save_workspace_file(
    client: State<'_, DaemonClient>,
    workspace_path: String,
    relative_path: String,
    expected_content: String,
    content: String,
) -> Result<(), String> {
    client
        .save_workspace_file(SaveWorkspaceFileParams {
            workspace_path,
            relative_path,
            expected_content,
            content,
        })
        .await
}
