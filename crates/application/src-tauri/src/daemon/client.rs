//! Typed daemon RPC facade, independent of Tauri command arguments.
//! All requests use shared protocol parameter types. The bridge alone encodes
//! JSON envelopes. Each call gets a separate connection so a pending prompt
//! cannot prevent cancellation, reads, or permission responses.

use super::{bridge::DaemonBridge, EventSubscription};
use amarcode_protocol::rpc::{
    methods, AmarcodeAcpConfigResult, AuthenticateAgentParams, AuthenticateAgentResult,
    CancelParams, CancelResult, CommitWorkspaceChangesParams, CreateChatParams, DaemonConfigResult,
    DeleteChatParams, DeleteChatResult, DiscoverCommandsParams, GetAttachmentParams,
    GetAttachmentResult, GetChatParams, GetMessagePartsParams, GetWorkspaceFileDiffParams,
    HealthResult, InstallAgentParams, InstallAgentResult, ListAcpEventsForChatParams,
    ListAcpEventsResult, ListAgentRunsForChatParams, ListAgentRunsResult, ListAgentsResult,
    ListChatsParams, ListChatsResult, ListWorkspaceFilesParams, PromptParams, PromptResultDto,
    RespondAgentParams, RespondAgentResult, SaveWorkspaceFileParams, SetAmarcodeAcpConfigParams,
    SetDaemonConfigParams, SetSessionConfigOptionParams, SetSessionConfigOptionResult,
    SubscribeEventsParams, TranscribeAudioParams, TranscribeAudioResult, UpdateChatParams,
    VacuumDatabaseResult, VersionResult, WorkspaceFileParams, WorkspaceParams,
};
use amarcode_protocol::{
    AcpEvent, AgentInfo, AgentRun, Chat, GetChatResult, MessagePart, SessionCommands,
    WorkspaceChange, WorkspaceDiff, WorkspaceInfo,
};
use serde::{de::DeserializeOwned, Serialize};

#[derive(Default)]
pub struct DaemonClient;

impl DaemonClient {
    pub fn new() -> Self {
        Self
    }

    pub async fn health(&self) -> Result<HealthResult, String> {
        let response: HealthResult = self.call(methods::HEALTH, ()).await?;
        ensure_compatible_protocol(response.protocol_version)?;
        Ok(response)
    }

    pub async fn version(&self) -> Result<VersionResult, String> {
        let response: VersionResult = self.call(methods::VERSION, ()).await?;
        ensure_compatible_protocol(response.protocol_version)?;
        Ok(response)
    }

    pub async fn daemon_config(&self) -> Result<DaemonConfigResult, String> {
        self.call(methods::GET_DAEMON_CONFIG, ()).await
    }

    pub async fn set_daemon_config(
        &self,
        params: SetDaemonConfigParams,
    ) -> Result<DaemonConfigResult, String> {
        self.call(methods::SET_DAEMON_CONFIG, params).await
    }

    pub async fn vacuum_database(&self) -> Result<VacuumDatabaseResult, String> {
        self.call(methods::VACUUM_DATABASE, ()).await
    }

    pub async fn list_agents(&self) -> Result<Vec<AgentInfo>, String> {
        Ok(self
            .call::<ListAgentsResult>(methods::LIST_AGENTS, ())
            .await?
            .agents)
    }

    pub async fn install_agent(
        &self,
        params: InstallAgentParams,
    ) -> Result<InstallAgentResult, String> {
        self.call(methods::INSTALL_AGENT, params).await
    }

    pub async fn authenticate_agent(
        &self,
        params: AuthenticateAgentParams,
    ) -> Result<AuthenticateAgentResult, String> {
        self.call(methods::AUTHENTICATE_AGENT, params).await
    }

    pub async fn amarcode_acp_config(&self) -> Result<AmarcodeAcpConfigResult, String> {
        self.call(methods::GET_AMARCODE_ACP_CONFIG, ()).await
    }

    pub async fn set_amarcode_acp_config(
        &self,
        params: SetAmarcodeAcpConfigParams,
    ) -> Result<AmarcodeAcpConfigResult, String> {
        self.call(methods::SET_AMARCODE_ACP_CONFIG, params).await
    }

    pub async fn create_chat(&self, params: CreateChatParams) -> Result<Chat, String> {
        self.call(methods::CREATE_CHAT, params).await
    }

    pub async fn list_chats(&self, params: ListChatsParams) -> Result<Vec<Chat>, String> {
        Ok(self
            .call::<ListChatsResult>(methods::LIST_CHATS, params)
            .await?
            .chats)
    }

    pub async fn get_chat(&self, params: GetChatParams) -> Result<GetChatResult, String> {
        self.call(methods::GET_CHAT, params).await
    }

    pub async fn get_message_parts(
        &self,
        params: GetMessagePartsParams,
    ) -> Result<Vec<MessagePart>, String> {
        self.call(methods::GET_MESSAGE_PARTS, params).await
    }

    pub async fn list_acp_events_for_chat(
        &self,
        params: ListAcpEventsForChatParams,
    ) -> Result<Vec<AcpEvent>, String> {
        Ok(self
            .call::<ListAcpEventsResult>(methods::LIST_ACP_EVENTS_FOR_CHAT, params)
            .await?
            .events)
    }

    pub async fn list_agent_runs_for_chat(
        &self,
        params: ListAgentRunsForChatParams,
    ) -> Result<Vec<AgentRun>, String> {
        Ok(self
            .call::<ListAgentRunsResult>(methods::LIST_AGENT_RUNS_FOR_CHAT, params)
            .await?
            .runs)
    }

    pub async fn update_chat(&self, params: UpdateChatParams) -> Result<Chat, String> {
        self.call(methods::UPDATE_CHAT, params).await
    }

    pub async fn delete_chat(&self, params: DeleteChatParams) -> Result<DeleteChatResult, String> {
        self.call(methods::DELETE_CHAT, params).await
    }

    pub async fn get_attachment(
        &self,
        params: GetAttachmentParams,
    ) -> Result<GetAttachmentResult, String> {
        self.call(methods::GET_ATTACHMENT, params).await
    }

    pub async fn discover_commands(
        &self,
        params: DiscoverCommandsParams,
    ) -> Result<SessionCommands, String> {
        self.call(methods::DISCOVER_COMMANDS, params).await
    }

    pub async fn transcribe_audio(
        &self,
        params: TranscribeAudioParams,
    ) -> Result<TranscribeAudioResult, String> {
        self.call(methods::TRANSCRIBE_AUDIO, params).await
    }

    pub async fn prompt(&self, params: PromptParams) -> Result<PromptResultDto, String> {
        self.call(methods::PROMPT, params).await
    }

    pub async fn set_session_config_option(
        &self,
        params: SetSessionConfigOptionParams,
    ) -> Result<SetSessionConfigOptionResult, String> {
        self.call(methods::SET_SESSION_CONFIG_OPTION, params).await
    }

    pub async fn cancel(&self, params: CancelParams) -> Result<CancelResult, String> {
        self.call(methods::CANCEL, params).await
    }

    pub async fn respond_permission(
        &self,
        params: RespondAgentParams,
    ) -> Result<RespondAgentResult, String> {
        self.call(methods::RESPOND_PERMISSION, params).await
    }

    pub async fn respond_input(
        &self,
        params: RespondAgentParams,
    ) -> Result<RespondAgentResult, String> {
        self.call(methods::RESPOND_INPUT, params).await
    }

    pub async fn list_workspace_files(
        &self,
        params: ListWorkspaceFilesParams,
    ) -> Result<Vec<String>, String> {
        self.call(methods::LIST_WORKSPACE_FILES, params).await
    }

    pub async fn get_workspace_info(
        &self,
        params: WorkspaceParams,
    ) -> Result<WorkspaceInfo, String> {
        self.call(methods::GET_WORKSPACE_INFO, params).await
    }

    pub async fn list_workspace_changes(
        &self,
        params: WorkspaceParams,
    ) -> Result<Vec<WorkspaceChange>, String> {
        self.call(methods::LIST_WORKSPACE_CHANGES, params).await
    }

    pub async fn stage_workspace_file(&self, params: WorkspaceFileParams) -> Result<(), String> {
        self.call(methods::STAGE_WORKSPACE_FILE, params).await
    }

    pub async fn unstage_workspace_file(&self, params: WorkspaceFileParams) -> Result<(), String> {
        self.call(methods::UNSTAGE_WORKSPACE_FILE, params).await
    }

    pub async fn stage_all_workspace_files(&self, params: WorkspaceParams) -> Result<(), String> {
        self.call(methods::STAGE_ALL_WORKSPACE_FILES, params).await
    }

    pub async fn commit_workspace_changes(
        &self,
        params: CommitWorkspaceChangesParams,
    ) -> Result<(), String> {
        self.call(methods::COMMIT_WORKSPACE_CHANGES, params).await
    }

    pub async fn get_workspace_file_diff(
        &self,
        params: GetWorkspaceFileDiffParams,
    ) -> Result<WorkspaceDiff, String> {
        self.call(methods::GET_WORKSPACE_FILE_DIFF, params).await
    }

    pub async fn read_workspace_file(&self, params: WorkspaceFileParams) -> Result<String, String> {
        self.call(methods::READ_WORKSPACE_FILE, params).await
    }

    pub async fn save_workspace_file(&self, params: SaveWorkspaceFileParams) -> Result<(), String> {
        self.call(methods::SAVE_WORKSPACE_FILE, params).await
    }

    /// Subscribe on a dedicated connection and recheck protocol compatibility
    /// after each reconnect, including when the daemon was restarted.
    pub async fn subscribe_events(
        &self,
        filter: SubscribeEventsParams,
    ) -> Result<EventSubscription, String> {
        self.health().await?;
        DaemonBridge::subscribe(filter).await
    }

    async fn call<T: DeserializeOwned>(
        &self,
        method: &str,
        params: impl Serialize,
    ) -> Result<T, String> {
        DaemonBridge::new().call(method, params).await
    }
}

fn ensure_compatible_protocol(actual: u32) -> Result<(), String> {
    let expected = amarcode_protocol::PROTOCOL_VERSION;
    if actual == expected {
        Ok(())
    } else {
        Err(format!(
            "incompatible daemon protocol: application requires {expected}, daemon provides {actual}"
        ))
    }
}

#[cfg(test)]
mod tests {
    use super::ensure_compatible_protocol;

    #[test]
    fn rejects_a_different_daemon_protocol_version() {
        assert!(ensure_compatible_protocol(amarcode_protocol::PROTOCOL_VERSION).is_ok());
        let error = ensure_compatible_protocol(amarcode_protocol::PROTOCOL_VERSION + 1)
            .expect_err("different version must be rejected");
        assert!(error.contains("incompatible daemon protocol"));
    }
}
