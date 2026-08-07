use crate::{
    models::{AgentDefinition, AgentEvent},
    store::Store,
};
use serde_json::{json, Value};
use std::{collections::HashMap, process::Stdio, sync::Arc};
use tauri::ipc::Channel;
use tokio::{
    io::{AsyncBufReadExt, AsyncWriteExt, BufReader},
    process::{ChildStdin, Command},
    sync::Mutex,
};

pub struct LiveSession {
    pub stdin: Mutex<ChildStdin>,
}

pub struct SessionManager {
    sessions: Mutex<HashMap<String, Arc<LiveSession>>>,
    store: Arc<Store>,
}

impl SessionManager {
    pub fn new(store: Arc<Store>) -> Self {
        Self {
            sessions: Mutex::new(HashMap::new()),
            store,
        }
    }

    pub async fn start(
        &self,
        session_id: String,
        workspace_path: String,
        agent: AgentDefinition,
        channel: Channel<AgentEvent>,
    ) -> Result<(), String> {
        let mut command = Command::new(&agent.command);
        command
            .args(&agent.arguments)
            .current_dir(&workspace_path)
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::piped());
        for variable in agent.environment {
            let value = match (variable.value, variable.secret_ref) {
                (Some(value), _) => value,
                (None, Some(secret_ref)) => keyring::Entry::new("acp-workbench", &secret_ref)
                    .map_err(|error| error.to_string())?
                    .get_password()
                    .map_err(|_| {
                        format!("Secret '{secret_ref}' is not available in the OS keychain")
                    })?,
                (None, None) => continue,
            };
            command.env(variable.name, value);
        }
        let mut child = command
            .spawn()
            .map_err(|error| format!("Could not start {}: {error}", agent.name))?;
        let stdin = child
            .stdin
            .take()
            .ok_or("Agent process did not provide stdin")?;
        let stdout = child
            .stdout
            .take()
            .ok_or("Agent process did not provide stdout")?;
        let stderr = child
            .stderr
            .take()
            .ok_or("Agent process did not provide stderr")?;
        let live = Arc::new(LiveSession {
            stdin: Mutex::new(stdin),
        });
        self.sessions.lock().await.insert(session_id.clone(), live);

        let starting = AgentEvent::Status {
            session_id: session_id.clone(),
            status: "starting".into(),
            detail: Some(agent.name.clone()),
        };
        self.store.save_event(&starting)?;
        channel.send(starting).map_err(|error| error.to_string())?;
        self.write_request(&session_id, 1, "initialize", json!({ "protocolVersion": 1, "clientCapabilities": {}, "clientInfo": { "name": "ACP Workbench", "version": "0.1.0" } })).await?;
        self.write_request(
            &session_id,
            2,
            "session/new",
            json!({ "cwd": workspace_path, "mcpServers": [] }),
        )
        .await?;

        let store = self.store.clone();
        let output_channel = channel.clone();
        let output_session_id = session_id.clone();
        tauri::async_runtime::spawn(async move {
            let mut lines = BufReader::new(stdout).lines();
            while let Ok(Some(line)) = lines.next_line().await {
                let event = normalize_message(&output_session_id, &line);
                let _ = store.save_event(&event);
                let _ = output_channel.send(event);
            }
            let event = AgentEvent::Status {
                session_id: output_session_id.clone(),
                status: "stopped".into(),
                detail: Some("Agent closed its output stream".into()),
            };
            let _ = store.update_session_status(&output_session_id, "stopped");
            let _ = store.save_event(&event);
            let _ = output_channel.send(event);
        });
        let error_store = self.store.clone();
        let error_channel = channel;
        let error_session_id = session_id;
        tauri::async_runtime::spawn(async move {
            let mut lines = BufReader::new(stderr).lines();
            while let Ok(Some(line)) = lines.next_line().await {
                let event = AgentEvent::Activity {
                    session_id: error_session_id.clone(),
                    label: "stderr".into(),
                    payload: json!({ "text": line }),
                };
                let _ = error_store.save_event(&event);
                let _ = error_channel.send(event);
            }
        });
        Ok(())
    }

    pub async fn prompt(&self, session_id: &str, prompt: String) -> Result<(), String> {
        self.write_request(
            session_id,
            3,
            "session/prompt",
            json!({ "prompt": [{ "type": "text", "text": prompt }] }),
        )
        .await
    }

    pub async fn respond(
        &self,
        session_id: &str,
        request_id: &str,
        result: Value,
    ) -> Result<(), String> {
        self.write(
            session_id,
            json!({ "jsonrpc": "2.0", "id": request_id, "result": result }),
        )
        .await
    }

    pub async fn cancel(&self, session_id: &str) -> Result<(), String> {
        self.write_request(session_id, 4, "session/cancel", json!({}))
            .await
    }

    async fn write_request(
        &self,
        session_id: &str,
        id: u64,
        method: &str,
        params: Value,
    ) -> Result<(), String> {
        self.write(
            session_id,
            json!({ "jsonrpc": "2.0", "id": id, "method": method, "params": params }),
        )
        .await
    }

    async fn write(&self, session_id: &str, message: Value) -> Result<(), String> {
        let session = self
            .sessions
            .lock()
            .await
            .get(session_id)
            .cloned()
            .ok_or("Session is not running")?;
        let mut stdin = session.stdin.lock().await;
        stdin
            .write_all(format!("{}\n", message).as_bytes())
            .await
            .map_err(|error| error.to_string())?;
        stdin.flush().await.map_err(|error| error.to_string())
    }
}

fn normalize_message(session_id: &str, line: &str) -> AgentEvent {
    let message: Value = match serde_json::from_str(line) {
        Ok(value) => value,
        Err(error) => {
            return AgentEvent::ProtocolError {
                session_id: session_id.into(),
                message: format!("Invalid JSON-RPC from agent: {error}"),
            }
        }
    };
    let method = message.get("method").and_then(Value::as_str);
    let params = message.get("params").cloned().unwrap_or(Value::Null);
    match method {
        Some("session/update") => {
            if let Some(text) = params
                .pointer("/update/content/text")
                .and_then(Value::as_str)
                .or_else(|| params.pointer("/content/text").and_then(Value::as_str))
            {
                AgentEvent::Message {
                    session_id: session_id.into(),
                    role: "agent".into(),
                    text: text.into(),
                }
            } else {
                AgentEvent::Activity {
                    session_id: session_id.into(),
                    label: "session update".into(),
                    payload: params,
                }
            }
        }
        Some(method) if message.get("id").is_some() => AgentEvent::Request {
            session_id: session_id.into(),
            request_id: message.get("id").map(Value::to_string).unwrap_or_default(),
            method: method.into(),
            params,
        },
        Some(method) => AgentEvent::Activity {
            session_id: session_id.into(),
            label: method.into(),
            payload: params,
        },
        None => AgentEvent::Activity {
            session_id: session_id.into(),
            label: "response".into(),
            payload: message,
        },
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn normalizes_agent_text_updates() {
        let event = normalize_message(
            "s1",
            r#"{"jsonrpc":"2.0","method":"session/update","params":{"update":{"content":{"text":"Hello"}}}}"#,
        );
        assert!(matches!(event, AgentEvent::Message { text, .. } if text == "Hello"));
    }
    #[test]
    fn keeps_agent_requests_actionable() {
        let event = normalize_message(
            "s1",
            r#"{"jsonrpc":"2.0","id":7,"method":"fs/read","params":{"path":"a"}}"#,
        );
        assert!(
            matches!(event, AgentEvent::Request { method, request_id, .. } if method == "fs/read" && request_id == "7")
        );
    }
}
