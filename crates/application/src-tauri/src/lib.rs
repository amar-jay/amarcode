mod app_update;
mod clipboard_files;
mod config;
mod daemon;
#[cfg(target_os = "linux")]
mod microphone;
mod protocol;
mod state;
mod window;
mod workspace;

use serde::Serialize;
use tauri::{ipc::Channel, AppHandle, State};

use crate::{
    protocol::{
        events::EditorEvent,
        rpc::{
            CancelResult, DeleteChatResult, GetAttachmentResult, HealthResult, PromptAttachment,
            PromptResultDto, RespondAgentParams, RespondAgentResult, VersionResult,
        },
        rpc::{DaemonConfigResult, SetDaemonConfigParams, VacuumDatabaseResult},
        AcpEvent, AgentInfo, AgentRun, Chat, GetChatResult, MessagePart,
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
async fn app_check_update(app: AppHandle) -> Result<app_update::AppUpdateCheck, String> {
    app_update::check(&app).await
}

#[tauri::command]
async fn app_install_update(
    app: AppHandle,
    on_status: Channel<app_update::AppUpdateStatus>,
) -> Result<(), String> {
    match app_update::install(app, on_status.clone()).await {
        Ok(()) => Ok(()),
        Err(error) => {
            let _ = on_status.send(app_update::AppUpdateStatus::Failed {
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
async fn read_pasted_files(paths: Vec<String>) -> Result<clipboard_files::PastedFiles, String> {
    tauri::async_runtime::spawn_blocking(move || clipboard_files::read_pasted_files(paths))
        .await
        .map_err(|error| error.to_string())?
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
async fn install_agent(
    state: State<'_, AppState>,
    agent_id: String,
) -> Result<amarcode_protocol::rpc::InstallAgentResult, String> {
    state.install_agent(agent_id).await
}

#[tauri::command]
async fn authenticate_agent(
    state: State<'_, AppState>,
    agent_id: String,
    method_id: Option<String>,
) -> Result<amarcode_protocol::rpc::AuthenticateAgentResult, String> {
    state.authenticate_agent(agent_id, method_id).await
}

#[tauri::command]
async fn get_amarcode_acp_config(
    state: State<'_, AppState>,
) -> Result<amarcode_protocol::rpc::AmarcodeAcpConfigResult, String> {
    state.amarcode_acp_config().await
}

#[tauri::command]
async fn set_amarcode_acp_config(
    state: State<'_, AppState>,
    base_url: String,
    model: String,
    api_key: String,
    clear_api_key: bool,
) -> Result<amarcode_protocol::rpc::AmarcodeAcpConfigResult, String> {
    state
        .set_amarcode_acp_config(base_url, model, api_key, clear_api_key)
        .await
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
async fn discover_commands(
    state: State<'_, AppState>,
    chat_id: String,
    agent_id: String,
) -> Result<crate::protocol::SessionCommands, String> {
    state.discover_commands(chat_id, agent_id).await
}

#[tauri::command]
async fn get_chat(
    state: State<'_, AppState>,
    chat_id: String,
    include_messages: bool,
    include_tool_content: bool,
) -> Result<GetChatResult, String> {
    state
        .get_chat(chat_id, include_messages, include_tool_content)
        .await
}

#[tauri::command]
async fn get_message_parts(
    state: State<'_, AppState>,
    message_ids: Vec<String>,
) -> Result<Vec<MessagePart>, String> {
    state.get_message_parts(message_ids).await
}

#[tauri::command]
async fn list_acp_events_for_chat(
    state: State<'_, AppState>,
    chat_id: String,
) -> Result<Vec<AcpEvent>, String> {
    state.list_acp_events_for_chat(chat_id).await
}

#[tauri::command]
async fn list_agent_runs_for_chat(
    state: State<'_, AppState>,
    chat_id: String,
) -> Result<Vec<AgentRun>, String> {
    state.list_agent_runs_for_chat(chat_id).await
}

#[tauri::command]
async fn get_daemon_config(state: State<'_, AppState>) -> Result<DaemonConfigResult, String> {
    state.daemon_config().await
}

#[tauri::command]
async fn set_daemon_config(
    state: State<'_, AppState>,
    params: SetDaemonConfigParams,
) -> Result<DaemonConfigResult, String> {
    state.set_daemon_config(params).await
}

#[tauri::command]
async fn vacuum_database(state: State<'_, AppState>) -> Result<VacuumDatabaseResult, String> {
    state.vacuum_database().await
}

#[tauri::command]
async fn transcribe_audio(
    state: State<'_, AppState>,
    params: crate::protocol::rpc::TranscribeAudioParams,
) -> Result<crate::protocol::rpc::TranscribeAudioResult, String> {
    state.transcribe_audio(params).await
}

#[tauri::command]
async fn update_chat(
    state: State<'_, AppState>,
    params: crate::protocol::rpc::UpdateChatParams,
) -> Result<Chat, String> {
    state.update_chat(params).await
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
    config_values: Vec<amarcode_protocol::SessionConfigAssignment>,
) -> Result<PromptResultDto, String> {
    state
        .prompt(chat_id, agent_id, text, attachments, config_values)
        .await
}

#[tauri::command]
async fn set_session_config_option(
    state: State<'_, AppState>,
    chat_id: String,
    config_id: String,
    value: amarcode_protocol::SessionConfigValue,
) -> Result<amarcode_protocol::rpc::SetSessionConfigOptionResult, String> {
    state
        .set_session_config_option(chat_id, config_id, value)
        .await
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

pub fn run() {
    let app = tauri::Builder::default()
        .plugin(tauri_plugin_clipboard_manager::init())
        .plugin(tauri_plugin_dialog::init())
        .plugin(tauri_plugin_notification::init())
        .plugin(tauri_plugin_opener::init())
        .plugin(tauri_plugin_updater::Builder::new().build())
        .setup(|app| {
            #[cfg(target_os = "linux")]
            microphone::configure(app)?;
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
            app_check_update,
            app_install_update,
            prepare_application_uninstall,
            exit_application,
            read_pasted_files,
            daemon_health,
            daemon_version,
            list_agents,
            install_agent,
            authenticate_agent,
            get_amarcode_acp_config,
            set_amarcode_acp_config,
            create_chat,
            list_chats,
            get_chat,
            discover_commands,
            get_message_parts,
            list_acp_events_for_chat,
            list_agent_runs_for_chat,
            get_daemon_config,
            set_daemon_config,
            vacuum_database,
            get_attachment,
            transcribe_audio,
            update_chat,
            delete_chat,
            prompt,
            set_session_config_option,
            cancel,
            respond_permission,
            respond_input,
            subscribe_events,
            workspace::list_workspace_files,
            workspace::get_workspace_info,
            workspace::list_workspace_changes,
            workspace::stage_workspace_file,
            workspace::unstage_workspace_file,
            workspace::stage_all_workspace_files,
            workspace::commit_workspace_changes,
            workspace::get_workspace_file_diff,
            workspace::read_workspace_file,
            workspace::save_workspace_file,
        ])
        .build(tauri::generate_context!())
        .expect("error while building amarcode");

    // The native service manager exclusively owns the daemon lifecycle. The
    // desktop application has no daemon child process to stop on exit.
    app.run(|_, _| {});
}
