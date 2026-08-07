use crate::{
    agent_runtime::AgentRuntime,
    models::{AgentDefinition, AgentEvent},
    store::Store,
};
use serde_json::{json, Value};
use std::{
    collections::HashMap,
    process::Stdio,
    sync::{
        atomic::{AtomicU64, Ordering},
        Arc,
    },
};
use tokio::{
    io::{AsyncBufReadExt, AsyncWriteExt, BufReader},
    process::{ChildStdin, ChildStdout, Command},
    sync::{broadcast, Mutex},
};

pub struct LiveSession {
    stdin: Mutex<ChildStdin>,
    acp_session_id: String,
}

pub struct SessionManager {
    sessions: Arc<Mutex<HashMap<String, Arc<LiveSession>>>>,
    store: Arc<Store>,
    events: broadcast::Sender<AgentEvent>,
    request_ids: AtomicU64,
    runtime: Arc<AgentRuntime>,
}

impl SessionManager {
    pub fn new(
        store: Arc<Store>,
        events: broadcast::Sender<AgentEvent>,
        runtime: Arc<AgentRuntime>,
    ) -> Self {
        Self {
            sessions: Arc::new(Mutex::new(HashMap::new())),
            store,
            events,
            request_ids: AtomicU64::new(3),
            runtime,
        }
    }

    pub async fn start(
        &self,
        workspace_path: String,
        agent: AgentDefinition,
    ) -> Result<crate::models::SessionSummary, String> {
        let now = chrono::Utc::now().to_rfc3339();
        let summary = crate::models::SessionSummary {
            id: uuid::Uuid::new_v4().to_string(),
            workspace_path: workspace_path.clone(),
            agent_id: agent.id.clone(),
            status: "running".into(),
            created_at: now.clone(),
            updated_at: now,
        };
        self.store.create_session(&summary)?;
        let command_path = match self.runtime.command_for(&agent).await {
            Ok(command_path) => command_path,
            Err(error) => {
                let _ = self.store.update_session_status(&summary.id, "failed");
                return Err(error);
            }
        };
        let mut command = Command::new(command_path);
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
        let mut child = match command.spawn() {
            Ok(child) => child,
            Err(error) => {
                let _ = self.store.update_session_status(&summary.id, "failed");
                return Err(format!("Could not start {}: {error}", agent.name));
            }
        };
        let mut stdin = child
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
        self.emit(AgentEvent::Status {
            session_id: summary.id.clone(),
            status: "starting".into(),
            detail: Some(agent.name),
        })?;
        let mut stdout = BufReader::new(stdout).lines();
        let setup = async {
            write_to_stdin(&mut stdin, json!({
                "jsonrpc": "2.0",
                "id": 1,
                "method": "initialize",
                "params": {
                    "protocolVersion": 1,
                    "clientCapabilities": {},
                    "clientInfo": { "name": "ACP Workbench", "version": env!("CARGO_PKG_VERSION") }
                }
            })).await?;
            wait_for_response(&mut stdout, 1, &summary.id, &self.store, &self.events).await?;
            write_to_stdin(&mut stdin, json!({
                "jsonrpc": "2.0",
                "id": 2,
                "method": "session/new",
                "params": { "cwd": workspace_path, "additionalDirectories": [], "mcpServers": [] }
            })).await?;
            let response =
                wait_for_response(&mut stdout, 2, &summary.id, &self.store, &self.events).await?;
            response
                .get("sessionId")
                .and_then(Value::as_str)
                .map(str::to_owned)
                .ok_or_else(|| "ACP agent did not return a sessionId".to_string())
        }
        .await;
        let acp_session_id = match setup {
            Ok(id) => id,
            Err(error) => {
                let _ = self.store.update_session_status(&summary.id, "failed");
                self.emit(AgentEvent::Status {
                    session_id: summary.id.clone(),
                    status: "failed".into(),
                    detail: Some(error.clone()),
                })?;
                return Err(error);
            }
        };
        self.sessions.lock().await.insert(
            summary.id.clone(),
            Arc::new(LiveSession {
                stdin: Mutex::new(stdin),
                acp_session_id,
            }),
        );
        self.emit(AgentEvent::Status {
            session_id: summary.id.clone(),
            status: "running".into(),
            detail: None,
        })?;

        let output_sessions = self.sessions.clone();
        let output_store = self.store.clone();
        let output_events = self.events.clone();
        let output_session_id = summary.id.clone();
        tokio::spawn(async move {
            while let Ok(Some(line)) = stdout.next_line().await {
                emit_to(
                    &output_store,
                    &output_events,
                    normalize_message(&output_session_id, &line),
                );
            }
            let event = AgentEvent::Status {
                session_id: output_session_id.clone(),
                status: "stopped".into(),
                detail: Some("Agent closed its output stream".into()),
            };
            let _ = output_store.update_session_status(&output_session_id, "stopped");
            emit_to(&output_store, &output_events, event);
            output_sessions.lock().await.remove(&output_session_id);
        });
        let error_store = self.store.clone();
        let error_events = self.events.clone();
        let error_session_id = summary.id.clone();
        tokio::spawn(async move {
            let mut lines = BufReader::new(stderr).lines();
            while let Ok(Some(line)) = lines.next_line().await {
                emit_to(
                    &error_store,
                    &error_events,
                    AgentEvent::Activity {
                        session_id: error_session_id.clone(),
                        label: "stderr".into(),
                        payload: json!({ "text": line }),
                    },
                );
            }
        });
        Ok(summary)
    }

    pub async fn prompt(&self, session_id: &str, prompt: String) -> Result<(), String> {
        let acp_session_id = self.acp_session_id(session_id).await?;
        self.write_request(
            session_id,
            "session/prompt",
            json!({ "sessionId": acp_session_id, "prompt": [{ "type": "text", "text": prompt }] }),
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
        let acp_session_id = self.acp_session_id(session_id).await?;
        self.write(session_id, json!({ "jsonrpc": "2.0", "method": "session/cancel", "params": { "sessionId": acp_session_id } })).await
    }

    pub async fn session_summaries(&self) -> Result<Vec<crate::models::SessionSummary>, String> {
        let mut summaries = self.store.sessions()?;
        let live_sessions = self.sessions.lock().await;
        for summary in &mut summaries {
            if summary.status == "running" && !live_sessions.contains_key(&summary.id) {
                self.store.update_session_status(&summary.id, "stopped")?;
                summary.status = "stopped".into();
            }
        }
        Ok(summaries)
    }

    async fn acp_session_id(&self, session_id: &str) -> Result<String, String> {
        self.sessions
            .lock()
            .await
            .get(session_id)
            .map(|session| session.acp_session_id.clone())
            .ok_or("Session is not running".into())
    }

    async fn write_request(
        &self,
        session_id: &str,
        method: &str,
        params: Value,
    ) -> Result<(), String> {
        self.write(session_id, json!({ "jsonrpc": "2.0", "id": self.request_ids.fetch_add(1, Ordering::Relaxed), "method": method, "params": params })).await
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
            .write_all(format!("{message}\n").as_bytes())
            .await
            .map_err(|error| error.to_string())?;
        stdin.flush().await.map_err(|error| error.to_string())
    }
    fn emit(&self, event: AgentEvent) -> Result<(), String> {
        emit_to(&self.store, &self.events, event);
        Ok(())
    }
}

fn emit_to(store: &Store, events: &broadcast::Sender<AgentEvent>, event: AgentEvent) {
    let _ = store.save_event(&event);
    let _ = events.send(event);
}

async fn write_to_stdin(stdin: &mut ChildStdin, message: Value) -> Result<(), String> {
    stdin
        .write_all(format!("{message}\n").as_bytes())
        .await
        .map_err(|error| error.to_string())?;
    stdin.flush().await.map_err(|error| error.to_string())
}

async fn wait_for_response(
    lines: &mut tokio::io::Lines<BufReader<ChildStdout>>,
    id: u64,
    session_id: &str,
    store: &Store,
    events: &broadcast::Sender<AgentEvent>,
) -> Result<Value, String> {
    while let Some(line) = lines.next_line().await.map_err(|error| error.to_string())? {
        let message: Value = serde_json::from_str(&line)
            .map_err(|error| format!("Invalid JSON-RPC from agent: {error}"))?;
        if message.get("id") == Some(&json!(id)) && message.get("method").is_none() {
            if let Some(error) = message.get("error") {
                return Err(format!("ACP request failed: {error}"));
            }
            return Ok(message.get("result").cloned().unwrap_or(Value::Null));
        }
        emit_to(store, events, normalize_message(session_id, &line));
    }
    Err("Agent closed its output stream during initialization".into())
}

pub fn normalize_message(session_id: &str, line: &str) -> AgentEvent {
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
        Some("session/update") => params
            .pointer("/update/content/text")
            .and_then(Value::as_str)
            .or_else(|| params.pointer("/content/text").and_then(Value::as_str))
            .map(|text| AgentEvent::Message {
                session_id: session_id.into(),
                role: "agent".into(),
                text: text.into(),
            })
            .unwrap_or_else(|| AgentEvent::Activity {
                session_id: session_id.into(),
                label: "session update".into(),
                payload: params,
            }),
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
        assert!(
            matches!(normalize_message("s1", r#"{"jsonrpc":"2.0","method":"session/update","params":{"update":{"content":{"text":"Hello"}}}}"#), AgentEvent::Message { text, .. } if text == "Hello")
        );
    }
    #[test]
    fn keeps_agent_requests_actionable() {
        assert!(
            matches!(normalize_message("s1", r#"{"jsonrpc":"2.0","id":7,"method":"fs/read","params":{"path":"a"}}"#), AgentEvent::Request { method, request_id, .. } if method == "fs/read" && request_id == "7")
        );
    }
}
