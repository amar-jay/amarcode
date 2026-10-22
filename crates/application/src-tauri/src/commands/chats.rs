//! Tauri IPC adapters for chats operations.
use crate::daemon::DaemonClient;
use amarcode_protocol::rpc::{
    CreateChatParams, DeleteChatParams, DeleteChatResult, GetAttachmentParams, GetAttachmentResult,
    GetChatParams, GetMessagePartsParams, ListAcpEventsForChatParams, ListAgentRunsForChatParams,
    ListChatsParams, UpdateChatParams,
};
use amarcode_protocol::{AcpEvent, AgentRun, Chat, GetChatResult, MessagePart};
use tauri::State;

#[tauri::command]
pub async fn create_chat(
    client: State<'_, DaemonClient>,
    workspace_path: String,
    title: Option<String>,
) -> Result<Chat, String> {
    client
        .create_chat(CreateChatParams {
            workspace_path,
            title,
        })
        .await
}

#[tauri::command]
pub async fn list_chats(
    client: State<'_, DaemonClient>,
    workspace_path: Option<String>,
) -> Result<Vec<Chat>, String> {
    client.list_chats(ListChatsParams { workspace_path }).await
}

#[tauri::command]
pub async fn get_chat(
    client: State<'_, DaemonClient>,
    chat_id: String,
    include_messages: bool,
    include_tool_content: bool,
) -> Result<GetChatResult, String> {
    client
        .get_chat(GetChatParams {
            chat_id,
            include_messages,
            include_tool_content,
        })
        .await
}

#[tauri::command]
pub async fn get_message_parts(
    client: State<'_, DaemonClient>,
    message_ids: Vec<String>,
) -> Result<Vec<MessagePart>, String> {
    client
        .get_message_parts(GetMessagePartsParams { message_ids })
        .await
}

#[tauri::command]
pub async fn list_acp_events_for_chat(
    client: State<'_, DaemonClient>,
    chat_id: String,
) -> Result<Vec<AcpEvent>, String> {
    client
        .list_acp_events_for_chat(ListAcpEventsForChatParams { chat_id })
        .await
}

#[tauri::command]
pub async fn list_agent_runs_for_chat(
    client: State<'_, DaemonClient>,
    chat_id: String,
) -> Result<Vec<AgentRun>, String> {
    client
        .list_agent_runs_for_chat(ListAgentRunsForChatParams { chat_id })
        .await
}

#[tauri::command]
pub async fn update_chat(
    client: State<'_, DaemonClient>,
    params: UpdateChatParams,
) -> Result<Chat, String> {
    client.update_chat(params).await
}

#[tauri::command]
pub async fn delete_chat(
    client: State<'_, DaemonClient>,
    chat_id: String,
) -> Result<DeleteChatResult, String> {
    client.delete_chat(DeleteChatParams { chat_id }).await
}

#[tauri::command]
pub async fn get_attachment(
    client: State<'_, DaemonClient>,
    chat_id: String,
    attachment_id: String,
) -> Result<GetAttachmentResult, String> {
    client
        .get_attachment(GetAttachmentParams {
            chat_id,
            attachment_id,
        })
        .await
}
