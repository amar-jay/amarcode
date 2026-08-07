mod acp;
mod models;
mod store;

use acp::SessionManager;
use chrono::Utc;
use models::{
    AgentDefinition, AgentEvent, RespondToRequestInput, SessionSummary, StartSessionInput,
};
use std::sync::Arc;
use std::path::{Path, PathBuf};
use store::Store;
use tauri::{ipc::Channel, Manager, State};
use uuid::Uuid;

struct AppState {
    store: Arc<Store>,
    sessions: Arc<SessionManager>,
}

#[tauri::command]
fn list_agents(state: State<'_, AppState>) -> Result<Vec<AgentDefinition>, String> {
    state.store.agents()
}

#[tauri::command]
fn save_agent(state: State<'_, AppState>, agent: AgentDefinition) -> Result<(), String> {
    state.store.save_agent(&agent)
}

#[tauri::command]
fn list_sessions(state: State<'_, AppState>) -> Result<Vec<SessionSummary>, String> {
    state.store.sessions()
}

#[tauri::command]
fn session_events(
    state: State<'_, AppState>,
    session_id: String,
) -> Result<Vec<AgentEvent>, String> {
    state.store.events(&session_id)
}

#[tauri::command]
fn list_workspace_files(workspace_path: String) -> Result<Vec<String>, String> {
    fn visit(root: &Path, path: &Path, depth: usize, files: &mut Vec<String>) -> Result<(), String> {
        if depth > 6 || files.len() >= 500 { return Ok(()); }
        for entry in std::fs::read_dir(path).map_err(|error| error.to_string())? {
            let entry = entry.map_err(|error| error.to_string())?;
            let child = entry.path(); let name = entry.file_name();
            if [".git", "node_modules", "target", "dist"].iter().any(|ignored| name == *ignored) { continue; }
            if child.is_dir() { visit(root, &child, depth + 1, files)?; }
            else if child.is_file() { if let Ok(relative) = child.strip_prefix(root) { files.push(relative.to_string_lossy().to_string()); } }
        }
        Ok(())
    }
    let root = PathBuf::from(workspace_path); let mut files = Vec::new(); visit(&root, &root, 0, &mut files)?; files.sort(); Ok(files)
}

#[tauri::command]
async fn start_session(
    state: State<'_, AppState>,
    input: StartSessionInput,
    on_event: Channel<AgentEvent>,
) -> Result<SessionSummary, String> {
    let now = Utc::now().to_rfc3339();
    let summary = SessionSummary {
        id: Uuid::new_v4().to_string(),
        workspace_path: input.workspace_path.clone(),
        agent_id: input.agent.id.clone(),
        status: "running".into(),
        created_at: now.clone(),
        updated_at: now,
    };
    state.store.create_session(&summary)?;
    if let Err(error) = state
        .sessions
        .start(
            summary.id.clone(),
            input.workspace_path,
            input.agent,
            on_event,
        )
        .await
    {
        state.store.update_session_status(&summary.id, "failed")?;
        return Err(error);
    }
    Ok(summary)
}

#[tauri::command]
async fn send_prompt(
    state: State<'_, AppState>,
    session_id: String,
    prompt: String,
) -> Result<(), String> {
    state.sessions.prompt(&session_id, prompt).await
}

#[tauri::command]
async fn cancel_session(state: State<'_, AppState>, session_id: String) -> Result<(), String> {
    state.sessions.cancel(&session_id).await
}

#[tauri::command]
async fn respond_to_request(
    state: State<'_, AppState>,
    input: RespondToRequestInput,
) -> Result<(), String> {
    state
        .sessions
        .respond(&input.session_id, &input.request_id, input.result)
        .await
}

#[tauri::command]
fn save_secret(secret_ref: String, value: String) -> Result<(), String> {
    keyring::Entry::new("acp-workbench", &secret_ref)
        .map_err(|error| error.to_string())?
        .set_password(&value)
        .map_err(|error| error.to_string())
}

pub fn run() {
    tauri::Builder::default()
        .plugin(tauri_plugin_dialog::init())
        .plugin(tauri_plugin_opener::init())
        .setup(|app| {
            let data_dir = app
                .path()
                .app_data_dir()
                .map_err(|error| error.to_string())?;
            std::fs::create_dir_all(&data_dir).map_err(|error| error.to_string())?;
            let store = Arc::new(Store::open(&data_dir.join("workbench.sqlite3"))?);
            store.seed_presets()?;
            app.manage(AppState {
                sessions: Arc::new(SessionManager::new(store.clone())),
                store,
            });
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
        .expect("error while running ACP Workbench");
}
