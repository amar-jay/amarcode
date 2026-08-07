mod daemon;
mod models;

use daemon::DaemonBridge;
use models::{AgentDefinition, AgentEvent, SessionSummary};
use serde_json::json;
use std::path::{Path, PathBuf};
use tauri::{ipc::Channel, Manager, State};

struct AppState;

#[tauri::command]
async fn list_agents() -> Result<Vec<AgentDefinition>, String> {
    DaemonBridge::call("list_agents", json!({})).await
}
#[tauri::command]
async fn save_agent(agent: AgentDefinition) -> Result<(), String> {
    DaemonBridge::call("save_agent", json!({ "agent": agent })).await
}
#[tauri::command]
async fn list_sessions() -> Result<Vec<SessionSummary>, String> {
    DaemonBridge::call("list_sessions", json!({})).await
}
#[tauri::command]
async fn session_events(session_id: String) -> Result<Vec<AgentEvent>, String> {
    DaemonBridge::call("session_events", json!({ "sessionId": session_id })).await
}
#[tauri::command]
async fn start_session(
    workspace_path: String,
    agent: AgentDefinition,
    on_event: Channel<AgentEvent>,
) -> Result<SessionSummary, String> {
    tauri::async_runtime::spawn(async move {
        let _ = DaemonBridge::forward_events(on_event).await;
    });
    DaemonBridge::call(
        "start_session",
        json!({ "workspacePath": workspace_path, "agent": agent }),
    )
    .await
}
#[tauri::command]
async fn send_prompt(
    session_id: String,
    prompt: String,
    display_text: String,
) -> Result<(), String> {
    DaemonBridge::call(
        "send_prompt",
        json!({ "sessionId": session_id, "prompt": prompt, "displayText": display_text }),
    )
    .await
}
#[tauri::command]
async fn cancel_session(session_id: String) -> Result<(), String> {
    DaemonBridge::call("cancel_session", json!({ "sessionId": session_id })).await
}
#[tauri::command]
async fn respond_to_request(
    session_id: String,
    request_id: serde_json::Value,
    result: serde_json::Value,
) -> Result<(), String> {
    DaemonBridge::call(
        "respond_to_request",
        json!({ "sessionId": session_id, "requestId": request_id, "result": result }),
    )
    .await
}
#[tauri::command]
async fn save_secret(secret_ref: String, value: String) -> Result<(), String> {
    DaemonBridge::call(
        "save_secret",
        json!({ "secretRef": secret_ref, "value": value }),
    )
    .await
}

#[tauri::command]
fn list_workspace_files(
    workspace_path: String,
    _state: State<'_, AppState>,
) -> Result<Vec<String>, String> {
    fn visit(
        root: &Path,
        path: &Path,
        depth: usize,
        files: &mut Vec<String>,
    ) -> Result<(), String> {
        if depth > 6 || files.len() >= 500 {
            return Ok(());
        }
        for entry in std::fs::read_dir(path).map_err(|error| error.to_string())? {
            let entry = entry.map_err(|error| error.to_string())?;
            let child = entry.path();
            let name = entry.file_name();
            if [".git", "node_modules", "target", "dist"]
                .iter()
                .any(|ignored| name == *ignored)
            {
                continue;
            }
            if child.is_dir() {
                visit(root, &child, depth + 1, files)?;
            } else if child.is_file() {
                if let Ok(relative) = child.strip_prefix(root) {
                    files.push(relative.to_string_lossy().to_string());
                }
            }
        }
        Ok(())
    }
    let root = PathBuf::from(workspace_path);
    let mut files = Vec::new();
    visit(&root, &root, 0, &mut files)?;
    files.sort();
    Ok(files)
}

pub fn run() {
    tauri::Builder::default()
        .plugin(tauri_plugin_dialog::init())
        .plugin(tauri_plugin_opener::init())
        .setup(|app| {
            app.manage(AppState);
            DaemonBridge::launch_if_needed();
            Ok(())
        })
        .invoke_handler(tauri::generate_handler![
            list_agents,
            save_agent,
            list_sessions,
            session_events,
            list_workspace_files,
            start_session,
            send_prompt,
            cancel_session,
            respond_to_request,
            save_secret
        ])
        .run(tauri::generate_context!())
        .expect("error while running AMARCODE");
}
