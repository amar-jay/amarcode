use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct AgentDefinition {
    pub id: String,
    pub name: String,
    pub command: String,
    #[serde(default)]
    pub arguments: Vec<String>,
    #[serde(default)]
    pub environment: Vec<EnvironmentVariable>,
    pub is_preset: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct EnvironmentVariable {
    pub name: String,
    pub secret_ref: Option<String>,
    pub value: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct SessionSummary {
    pub id: String,
    pub workspace_path: String,
    pub agent_id: String,
    pub status: String,
    pub created_at: String,
    pub updated_at: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "kind", content = "data", rename_all = "camelCase")]
pub enum AgentEvent {
    Status {
        session_id: String,
        status: String,
        detail: Option<String>,
    },
    Message {
        session_id: String,
        role: String,
        text: String,
    },
    Activity {
        session_id: String,
        label: String,
        payload: serde_json::Value,
    },
    Request {
        session_id: String,
        request_id: serde_json::Value,
        method: String,
        params: serde_json::Value,
    },
    ProtocolError {
        session_id: String,
        message: String,
    },
    TurnComplete {
        session_id: String,
    },
}
