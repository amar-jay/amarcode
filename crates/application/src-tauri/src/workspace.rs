//! Desktop IPC forwarding for daemon-owned workspace operations.
use crate::{
    protocol::{WorkspaceChange, WorkspaceDiff, WorkspaceInfo},
    state::AppState,
};
use tauri::State;

#[tauri::command]
pub async fn list_workspace_files(
    state: State<'_, AppState>,
    workspace_path: String,
    query: Option<String>,
) -> Result<Vec<String>, String> {
    state.list_workspace_files(workspace_path, query).await
}

#[tauri::command]
pub async fn get_workspace_info(
    state: State<'_, AppState>,
    workspace_path: String,
) -> Result<WorkspaceInfo, String> {
    state.get_workspace_info(workspace_path).await
}

#[tauri::command]
pub async fn list_workspace_changes(
    state: State<'_, AppState>,
    workspace_path: String,
) -> Result<Vec<WorkspaceChange>, String> {
    state.list_workspace_changes(workspace_path).await
}

#[tauri::command]
pub async fn stage_workspace_file(
    state: State<'_, AppState>,
    workspace_path: String,
    relative_path: String,
) -> Result<(), String> {
    state
        .stage_workspace_file(workspace_path, relative_path)
        .await
}

#[tauri::command]
pub async fn unstage_workspace_file(
    state: State<'_, AppState>,
    workspace_path: String,
    relative_path: String,
) -> Result<(), String> {
    state
        .unstage_workspace_file(workspace_path, relative_path)
        .await
}

#[tauri::command]
pub async fn stage_all_workspace_files(
    state: State<'_, AppState>,
    workspace_path: String,
) -> Result<(), String> {
    state.stage_all_workspace_files(workspace_path).await
}

#[tauri::command]
pub async fn commit_workspace_changes(
    state: State<'_, AppState>,
    workspace_path: String,
    message: String,
) -> Result<(), String> {
    state
        .commit_workspace_changes(workspace_path, message)
        .await
}

#[tauri::command]
pub async fn get_workspace_file_diff(
    state: State<'_, AppState>,
    workspace_path: String,
    relative_path: String,
    staged: Option<bool>,
) -> Result<WorkspaceDiff, String> {
    state
        .get_workspace_file_diff(workspace_path, relative_path, staged)
        .await
}

#[tauri::command]
pub async fn read_workspace_file(
    state: State<'_, AppState>,
    workspace_path: String,
    relative_path: String,
) -> Result<String, String> {
    state
        .read_workspace_file(workspace_path, relative_path)
        .await
}

#[tauri::command]
pub async fn save_workspace_file(
    state: State<'_, AppState>,
    workspace_path: String,
    relative_path: String,
    expected_content: String,
    content: String,
) -> Result<(), String> {
    state
        .save_workspace_file(workspace_path, relative_path, expected_content, content)
        .await
}
