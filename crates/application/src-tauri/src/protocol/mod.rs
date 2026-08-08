use serde::{Deserialize, Serialize};

pub mod events;
pub mod rpc;
pub mod types;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AgentDefinition {
    pub id: String,
    pub name: String,
    pub command: String,
    pub arguments: Vec<String>,
    pub environment: Vec<(String, String)>,
    pub is_preset: bool,
    pub created_at: String,
    pub updated_at: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Chat {
    pub id: String,
    pub workspace_path: String,
    pub title: String,
    pub created_at: String,
    pub updated_at: String,
    pub archived_at: Option<String>,
}
