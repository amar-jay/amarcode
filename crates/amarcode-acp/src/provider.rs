use std::{collections::BTreeMap, path::Path};

use agent_client_protocol::{
    schema::v1::{
        ContentBlock, ContentChunk, MessageId, SessionId, SessionNotification, SessionUpdate,
        TextContent,
    },
    Client as AcpClient, ConnectionTo,
};
use futures_util::StreamExt;
use reqwest::Client as HttpClient;
use serde::Deserialize;
use serde_json::{json, Value};
use tokio::sync::watch;

#[derive(Debug, Clone, Deserialize)]
pub struct Config {
    pub name: String,
    pub provider: ProviderConfig,
}

impl Config {
    pub fn title(&self) -> String {
        self.name
            .split(['.', '_', '-'])
            .filter(|part| !part.is_empty())
            .map(|part| {
                let mut chars = part.chars();
                chars.next().map_or_else(String::new, |first| {
                    let mut word = first.to_ascii_uppercase().to_string();
                    word.extend(chars);
                    word
                })
            })
            .collect::<Vec<_>>()
            .join(" ")
    }

    pub fn from_file(path: &Path) -> Result<Self, String> {
        let contents = std::fs::read_to_string(path)
            .map_err(|error| format!("failed to read {}: {error}", path.display()))?;
        let mut config: Self = serde_json::from_str(&contents)
            .map_err(|error| format!("invalid JSON in {}: {error}", path.display()))?;
        config.name = config.name.trim().to_owned();
        config.provider.base_url = config.provider.base_url.trim_end_matches('/').to_owned();
        if !valid_agent_name(&config.name) {
            return Err("name must start with an ASCII lowercase letter or digit and contain only lowercase letters, digits, '.', '_', or '-'".into());
        }
        if config.provider.base_url.is_empty()
            || config.provider.api_key.trim().is_empty()
            || config.provider.model.trim().is_empty()
        {
            return Err(
                "provider.base_url, provider.api_key, and provider.model must not be empty".into(),
            );
        }
        Ok(config)
    }
}

#[derive(Debug, Clone, Deserialize)]
pub struct ProviderConfig {
    #[serde(alias = "baseUrl")]
    pub base_url: String,
    #[serde(alias = "apiKey")]
    pub api_key: String,
    pub model: String,
}

impl ProviderConfig {
    fn endpoint(&self) -> String {
        format!("{}/chat/completions", self.base_url)
    }
}

fn valid_agent_name(name: &str) -> bool {
    let mut chars = name.chars();
    chars
        .next()
        .is_some_and(|c| c.is_ascii_lowercase() || c.is_ascii_digit())
        && chars
            .all(|c| c.is_ascii_lowercase() || c.is_ascii_digit() || matches!(c, '.' | '_' | '-'))
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ModelToolCall {
    pub id: String,
    pub name: String,
    pub arguments: String,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ModelTurn {
    pub text: String,
    pub tool_calls: Vec<ModelToolCall>,
}

pub enum Completion<T = ModelTurn> {
    Completed(T),
    Cancelled,
}

pub fn tool_definitions() -> Vec<Value> {
    vec![
        function_tool(
            "read_file",
            "Read a UTF-8 text file inside the workspace.",
            json!({
                "type": "object", "properties": { "path": { "type": "string" } }, "required": ["path"], "additionalProperties": false
            }),
        ),
        function_tool(
            "list_directory",
            "List entries in a workspace directory.",
            json!({
                "type": "object", "properties": { "path": { "type": "string" } }, "required": ["path"], "additionalProperties": false
            }),
        ),
        function_tool(
            "search_text",
            "Search UTF-8 workspace files for literal text.",
            json!({
                "type": "object", "properties": { "query": { "type": "string" }, "path": { "type": "string" } }, "required": ["query"], "additionalProperties": false
            }),
        ),
        function_tool(
            "write_file",
            "Create or replace a UTF-8 text file inside the workspace. Requires user approval.",
            json!({
                "type": "object", "properties": { "path": { "type": "string" }, "content": { "type": "string" } }, "required": ["path", "content"], "additionalProperties": false
            }),
        ),
    ]
}

fn function_tool(name: &str, description: &str, parameters: Value) -> Value {
    json!({ "type": "function", "function": { "name": name, "description": description, "parameters": parameters } })
}

pub async fn stream_completion(
    client: &HttpClient,
    config: &Config,
    history: &[Value],
    session_id: SessionId,
    message_id: MessageId,
    connection: ConnectionTo<AcpClient>,
    mut cancellation: watch::Receiver<bool>,
) -> Result<Completion, String> {
    if *cancellation.borrow() {
        return Ok(Completion::Cancelled);
    }
    let request = client
        .post(config.provider.endpoint())
        .bearer_auth(&config.provider.api_key)
        .json(&json!({
            "model": config.provider.model,
            "messages": history,
            "tools": tool_definitions(),
            "tool_choice": "auto",
            "stream": true
        }))
        .send();
    let response = tokio::select! {
        response = request => response.map_err(|error| format!("provider request failed: {error}"))?,
        _ = cancellation.changed() => return Ok(Completion::Cancelled),
    };
    let status = response.status();
    if !status.is_success() {
        let body = response.text().await.unwrap_or_default();
        return Err(format!(
            "provider returned {status}: {}",
            error_message(&body)
        ));
    }

    let mut stream = Box::pin(response.bytes_stream());
    let mut buffer = String::new();
    let mut turn = ModelTurn {
        text: String::new(),
        tool_calls: Vec::new(),
    };
    let mut calls = BTreeMap::<usize, ModelToolCall>::new();
    loop {
        let chunk = tokio::select! {
            chunk = stream.next() => chunk,
            _ = cancellation.changed() => return Ok(Completion::Cancelled),
        };
        let Some(chunk) = chunk else { break };
        buffer.push_str(&String::from_utf8_lossy(
            &chunk.map_err(|e| format!("provider stream failed: {e}"))?,
        ));
        while let Some(index) = buffer.find('\n') {
            let line = buffer[..index].trim_end_matches('\r').to_owned();
            buffer.drain(..=index);
            process_sse_line(
                &line,
                &mut turn,
                &mut calls,
                &session_id,
                &message_id,
                &connection,
            )?;
        }
    }
    if !buffer.is_empty() {
        process_sse_line(
            buffer.trim_end_matches('\r'),
            &mut turn,
            &mut calls,
            &session_id,
            &message_id,
            &connection,
        )?;
    }
    turn.tool_calls = calls.into_values().collect();
    Ok(Completion::Completed(turn))
}

fn process_sse_line(
    line: &str,
    turn: &mut ModelTurn,
    calls: &mut BTreeMap<usize, ModelToolCall>,
    session_id: &SessionId,
    message_id: &MessageId,
    connection: &ConnectionTo<AcpClient>,
) -> Result<(), String> {
    let Some(data) = line.strip_prefix("data:") else {
        return Ok(());
    };
    let data = data.trim();
    if data.is_empty() || data == "[DONE]" {
        return Ok(());
    }
    let value: Value =
        serde_json::from_str(data).map_err(|e| format!("invalid provider event: {e}"))?;
    if let Some(error) = value.get("error") {
        return Err(error_message(&error.to_string()));
    }
    let delta = &value["choices"][0]["delta"];
    if let Some(text) = delta["content"].as_str().filter(|text| !text.is_empty()) {
        turn.text.push_str(text);
        connection
            .send_notification(SessionNotification::new(
                session_id.clone(),
                SessionUpdate::AgentMessageChunk(
                    ContentChunk::new(ContentBlock::Text(TextContent::new(text)))
                        .message_id(message_id.clone()),
                ),
            ))
            .map_err(|e| format!("ACP write failed: {e}"))?;
    }
    if let Some(tool_calls) = delta["tool_calls"].as_array() {
        for chunk in tool_calls {
            let index = chunk["index"].as_u64().unwrap_or(0) as usize;
            let call = calls.entry(index).or_insert_with(|| ModelToolCall {
                id: String::new(),
                name: String::new(),
                arguments: String::new(),
            });
            if let Some(id) = chunk["id"].as_str() {
                call.id.push_str(id);
            }
            if let Some(name) = chunk["function"]["name"].as_str() {
                call.name.push_str(name);
            }
            if let Some(arguments) = chunk["function"]["arguments"].as_str() {
                call.arguments.push_str(arguments);
            }
        }
    }
    Ok(())
}

pub fn assistant_message(turn: &ModelTurn) -> Value {
    let calls = turn.tool_calls.iter().map(|call| json!({
        "id": call.id, "type": "function", "function": { "name": call.name, "arguments": call.arguments }
    })).collect::<Vec<_>>();
    json!({ "role": "assistant", "content": if turn.text.is_empty() { Value::Null } else { Value::String(turn.text.clone()) }, "tool_calls": calls })
}

pub fn tool_message(call: &ModelToolCall, output: &str) -> Value {
    json!({ "role": "tool", "tool_call_id": call.id, "name": call.name, "content": output })
}

fn error_message(body: &str) -> String {
    serde_json::from_str::<Value>(body)
        .ok()
        .and_then(|v| {
            v.pointer("/error/message")
                .and_then(Value::as_str)
                .map(str::to_owned)
        })
        .unwrap_or_else(|| body.trim().to_owned())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn assembles_split_tool_call_chunks() {
        let mut turn = ModelTurn {
            text: String::new(),
            tool_calls: Vec::new(),
        };
        let mut calls = BTreeMap::new();
        fn apply(data: Value, turn: &mut ModelTurn, calls: &mut BTreeMap<usize, ModelToolCall>) {
            let delta = &data["choices"][0]["delta"];
            for chunk in delta["tool_calls"].as_array().unwrap() {
                let index = chunk["index"].as_u64().unwrap() as usize;
                let call = calls.entry(index).or_insert_with(|| ModelToolCall {
                    id: String::new(),
                    name: String::new(),
                    arguments: String::new(),
                });
                if let Some(v) = chunk["id"].as_str() {
                    call.id.push_str(v);
                }
                if let Some(v) = chunk["function"]["name"].as_str() {
                    call.name.push_str(v);
                }
                if let Some(v) = chunk["function"]["arguments"].as_str() {
                    call.arguments.push_str(v);
                }
            }
            let _ = turn;
        }
        apply(
            json!({"choices":[{"delta":{"tool_calls":[{"index":0,"id":"call-1","function":{"name":"read_file","arguments":"{\"pa"}}]}}]}),
            &mut turn,
            &mut calls,
        );
        apply(
            json!({"choices":[{"delta":{"tool_calls":[{"index":0,"function":{"arguments":"th\":\"a.rs\"}"}}]}}]}),
            &mut turn,
            &mut calls,
        );
        assert_eq!(calls[&0].arguments, "{\"path\":\"a.rs\"}");
    }
}
