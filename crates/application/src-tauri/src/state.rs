//! Typed application-side facade for the daemon JSON-line RPC protocol.
//!
//! This module intentionally mirrors the daemon contract rather than any UI
//! model.  Tauri commands should delegate here; no command may invent an RPC
//! method or change a daemon parameter name.

use std::sync::Arc;

use serde::Deserialize;
use serde_json::{json, Value};
use tokio::sync::Mutex;

use crate::{
    daemon::{DaemonBridge, EventSubscription},
    protocol::{
        events::EditorEvent,
        rpc::{
            methods, CancelResult, HealthResult, ListAgentsResult, ListChatsResult,
            PromptResultDto, RespondAgentParams, RespondAgentResult, VersionResult,
        },
        AgentDefinition, Chat,
    },
};

pub struct AppState {
    bridge: Arc<Mutex<DaemonBridge>>,
}

impl AppState {
    pub fn new() -> Self {
        Self {
            bridge: Arc::new(Mutex::new(DaemonBridge::new())),
        }
    }

    pub async fn health(&self) -> Result<HealthResult, String> {
        self.call(methods::HEALTH, Value::Null).await
    }

    pub async fn version(&self) -> Result<VersionResult, String> {
        self.call(methods::VERSION, Value::Null).await
    }

    pub async fn list_agents(&self) -> Result<Vec<AgentDefinition>, String> {
        Ok(self
            .call::<ListAgentsResult>(methods::LIST_AGENTS, Value::Null)
            .await?
            .agents)
    }

    pub async fn create_chat(
        &self,
        workspace_path: String,
        title: Option<String>,
    ) -> Result<Chat, String> {
        self.call(
            methods::CREATE_CHAT,
            json!({ "workspace_path": workspace_path, "title": title }),
        )
        .await
    }

    pub async fn list_chats(&self, workspace_path: Option<String>) -> Result<Vec<Chat>, String> {
        Ok(self
            .call::<ListChatsResult>(
                methods::LIST_CHATS,
                json!({ "workspace_path": workspace_path }),
            )
            .await?
            .chats)
    }

    pub async fn get_chat(&self, chat_id: String, include_messages: bool) -> Result<Chat, String> {
        self.call(
            methods::GET_CHAT,
            json!({ "chat_id": chat_id, "include_messages": include_messages }),
        )
        .await
    }

    pub async fn prompt(
        &self,
        chat_id: String,
        agent_id: String,
        text: String,
    ) -> Result<PromptResultDto, String> {
        self.call(
            methods::PROMPT,
            json!({ "chat_id": chat_id, "agent_id": agent_id, "text": text }),
        )
        .await
    }

    pub async fn cancel(&self, chat_id: String) -> Result<CancelResult, String> {
        self.call(methods::CANCEL, json!({ "chat_id": chat_id }))
            .await
    }

    pub async fn respond_permission(
        &self,
        params: RespondAgentParams,
    ) -> Result<RespondAgentResult, String> {
        self.respond_agent(methods::RESPOND_PERMISSION, params)
            .await
    }

    pub async fn respond_input(
        &self,
        params: RespondAgentParams,
    ) -> Result<RespondAgentResult, String> {
        self.respond_agent(methods::RESPOND_INPUT, params).await
    }

    /// Opens a separate, read-only event connection.  The daemon turns this
    /// socket into a stream after the acknowledgement, so it must not share
    /// the regular request/response bridge.
    pub async fn subscribe_events(&self, filter: EditorEvent) -> Result<EventSubscription, String> {
        DaemonBridge::subscribe(serde_json::to_value(filter).map_err(|error| error.to_string())?)
            .await
    }

    async fn respond_agent(
        &self,
        method: &str,
        // request_id: String,
        params: RespondAgentParams,
    ) -> Result<RespondAgentResult, String> {
        self.call(
            method,
            json!({ "request_id": params.request_id, "result": params.result, "error": params.error }),
        )
        .await
    }

    async fn call<T: for<'de> Deserialize<'de>>(
        &self,
        method: &str,
        params: Value,
    ) -> Result<T, String> {
        self.bridge.lock().await.call(method, params).await
    }
}
