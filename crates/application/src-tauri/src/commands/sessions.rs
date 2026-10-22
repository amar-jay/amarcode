//! Tauri IPC adapters for sessions operations.
use crate::daemon::DaemonClient;
use amarcode_protocol::rpc::{
    CancelParams, CancelResult, DiscoverCommandsParams, PromptAttachment, PromptParams,
    PromptResultDto, RespondAgentParams, RespondAgentResult, SetSessionConfigOptionParams,
    SetSessionConfigOptionResult, TranscribeAudioParams, TranscribeAudioResult,
};
use amarcode_protocol::{SessionCommands, SessionConfigAssignment, SessionConfigValue};
use tauri::State;

#[tauri::command]
pub async fn discover_commands(
    client: State<'_, DaemonClient>,
    chat_id: String,
    agent_id: String,
) -> Result<SessionCommands, String> {
    client
        .discover_commands(DiscoverCommandsParams { chat_id, agent_id })
        .await
}

#[tauri::command]
pub async fn transcribe_audio(
    client: State<'_, DaemonClient>,
    params: TranscribeAudioParams,
) -> Result<TranscribeAudioResult, String> {
    client.transcribe_audio(params).await
}

#[tauri::command]
pub async fn prompt(
    client: State<'_, DaemonClient>,
    chat_id: String,
    agent_id: String,
    text: String,
    attachments: Vec<PromptAttachment>,
    config_values: Vec<SessionConfigAssignment>,
) -> Result<PromptResultDto, String> {
    client
        .prompt(PromptParams {
            chat_id,
            agent_id,
            text,
            attachments,
            config_values,
        })
        .await
}

#[tauri::command]
pub async fn set_session_config_option(
    client: State<'_, DaemonClient>,
    chat_id: String,
    config_id: String,
    value: SessionConfigValue,
) -> Result<SetSessionConfigOptionResult, String> {
    client
        .set_session_config_option(SetSessionConfigOptionParams {
            chat_id,
            config_id,
            value,
        })
        .await
}

#[tauri::command]
pub async fn cancel(
    client: State<'_, DaemonClient>,
    chat_id: String,
) -> Result<CancelResult, String> {
    client.cancel(CancelParams { chat_id }).await
}

#[tauri::command]
pub async fn respond_permission(
    client: State<'_, DaemonClient>,
    params: RespondAgentParams,
) -> Result<RespondAgentResult, String> {
    client.respond_permission(params).await
}

#[tauri::command]
pub async fn respond_input(
    client: State<'_, DaemonClient>,
    params: RespondAgentParams,
) -> Result<RespondAgentResult, String> {
    client.respond_input(params).await
}
