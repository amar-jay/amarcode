mod config;
mod daemon;
mod protocol;
mod state;
mod window;

use std::path::{Path, PathBuf};

use serde::Serialize;
use tauri::{ipc::Channel, AppHandle, State};

use crate::{
    protocol::{
        events::EditorEvent,
        rpc::{
            CancelResult, DeleteChatResult, HealthResult, PromptResultDto, RespondAgentParams,
            RespondAgentResult, VersionResult,
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
async fn prompt(
    state: State<'_, AppState>,
    chat_id: String,
    agent_id: String,
    text: String,
    session_mode: Option<String>,
) -> Result<PromptResultDto, String> {
    state.prompt(chat_id, agent_id, text, session_mode).await
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
    let app = tauri::Builder::default()
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
            delete_chat,
            prompt,
            set_session_mode,
            cancel,
            respond_permission,
            respond_input,
            subscribe_events,
            list_workspace_files,
        ])
        .build(tauri::generate_context!())
        .expect("error while building amarcode");

    // The native service manager exclusively owns the daemon lifecycle. The
    // desktop application has no daemon child process to stop on exit.
    app.run(|_, _| {});
}
