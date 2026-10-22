//! Tauri IPC adapters for agents operations.
use crate::daemon::DaemonClient;
use amarcode_protocol::rpc::{
    AmarcodeAcpConfigResult, AuthenticateAgentParams, AuthenticateAgentResult, InstallAgentParams,
    InstallAgentResult, SetAmarcodeAcpConfigParams,
};
use amarcode_protocol::AgentInfo;
use tauri::State;

#[tauri::command]
pub async fn list_agents(client: State<'_, DaemonClient>) -> Result<Vec<AgentInfo>, String> {
    client.list_agents().await
}

#[tauri::command]
pub async fn install_agent(
    client: State<'_, DaemonClient>,
    agent_id: String,
) -> Result<InstallAgentResult, String> {
    client.install_agent(InstallAgentParams { agent_id }).await
}

#[tauri::command]
pub async fn authenticate_agent(
    client: State<'_, DaemonClient>,
    agent_id: String,
    method_id: Option<String>,
) -> Result<AuthenticateAgentResult, String> {
    client
        .authenticate_agent(AuthenticateAgentParams {
            agent_id,
            method_id,
        })
        .await
}

#[tauri::command]
pub async fn get_amarcode_acp_config(
    client: State<'_, DaemonClient>,
) -> Result<AmarcodeAcpConfigResult, String> {
    client.amarcode_acp_config().await
}

#[tauri::command]
pub async fn set_amarcode_acp_config(
    client: State<'_, DaemonClient>,
    base_url: String,
    model: String,
    api_key: String,
    clear_api_key: bool,
) -> Result<AmarcodeAcpConfigResult, String> {
    client
        .set_amarcode_acp_config(SetAmarcodeAcpConfigParams {
            base_url,
            model,
            api_key,
            clear_api_key,
        })
        .await
}
