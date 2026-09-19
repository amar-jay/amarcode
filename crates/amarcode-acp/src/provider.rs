use std::collections::BTreeMap;

use agent_client_protocol::{
    schema::v1::{
        ContentBlock, ContentChunk, MessageId, SessionId, SessionNotification, SessionUpdate,
        TextContent, ToolCall as AcpToolCall, ToolCallContent, ToolCallStatus, ToolCallUpdate,
        ToolCallUpdateFields, ToolKind,
    },
    Client as AcpClient, ConnectionTo,
};
use futures_util::StreamExt;
use reqwest::Client as HttpClient;
use serde_json::{json, Value};
use tokio::sync::watch;

use crate::config::{Config, ProviderConfig};

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ModelToolCall {
    pub id: String,
    pub name: String,
    pub arguments: String,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ModelTurn {
    pub text: String,
    pub reasoning: String,
    pub reasoning_details: Vec<Value>,
    pub tool_calls: Vec<ModelToolCall>,
}

pub enum Completion<T = ModelTurn> {
    Completed(T),
    Cancelled,
}

/// ACP state associated with one streamed model response.
///
/// Keeping this together prevents the transport API from growing one argument
/// for every piece of protocol context needed while translating the stream.
pub struct StreamContext {
    pub session_id: SessionId,
    pub message_id: MessageId,
    pub connection: ConnectionTo<AcpClient>,
    pub cancellation: watch::Receiver<bool>,
}

struct ReasoningToolCall {
    id: String,
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
            "edit_file",
            "Replace one exact, uniquely matching text block in an existing UTF-8 workspace file. Include enough unchanged surrounding text in old_text to make the match unique. Use this instead of write_file for targeted edits.",
            json!({
                "type": "object",
                "properties": {
                    "path": { "type": "string" },
                    "old_text": { "type": "string", "description": "Exact text currently present in the file." },
                    "new_text": { "type": "string", "description": "Replacement text; may be empty to delete the matched block." }
                },
                "required": ["path", "old_text", "new_text"],
                "additionalProperties": false
            }),
        ),
        function_tool(
            "write_file",
            "Create or fully replace a UTF-8 text file inside the workspace. Prefer edit_file when changing part of an existing file. Invoke the tool directly when needed; the host handles any required approval.",
            json!({
                "type": "object", "properties": { "path": { "type": "string" }, "content": { "type": "string" } }, "required": ["path", "content"], "additionalProperties": false
            }),
        ),
        function_tool(
            "run_command",
            "Run an executable in the workspace through the ACP client's terminal service. Pass the executable and arguments separately; shell syntax is not interpreted. Invoke the tool directly when needed; the host handles any required approval.",
            json!({
                "type": "object",
                "properties": {
                    "command": { "type": "string", "description": "Executable name or absolute path." },
                    "args": { "type": "array", "items": { "type": "string" }, "default": [] },
                    "cwd": { "type": "string", "description": "Optional workspace-relative working directory.", "default": "." }
                },
                "required": ["command"],
                "additionalProperties": false
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
    mode: &str,
    mut context: StreamContext,
) -> Result<Completion, String> {
    if *context.cancellation.borrow() {
        return Ok(Completion::Cancelled);
    }
    let mut request_body = json!({
        "model": config.provider.model,
        "messages": model_messages(history, mode),
        "tools": tool_definitions(),
        "tool_choice": "auto",
        "stream": true
    });
    if let Some(reasoning) = reasoning_request(&config.provider) {
        request_body["reasoning"] = reasoning;
    }
    let request = client
        .post(config.provider.endpoint())
        .bearer_auth(&config.provider.api_key)
        .json(&request_body)
        .send();
    let response = tokio::select! {
        response = request => response.map_err(|error| format!("provider request failed: {error}"))?,
        _ = context.cancellation.changed() => return Ok(Completion::Cancelled),
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
        reasoning: String::new(),
        reasoning_details: Vec::new(),
        tool_calls: Vec::new(),
    };
    let mut calls = BTreeMap::<usize, ModelToolCall>::new();
    let mut reasoning_call = None;
    loop {
        let chunk = tokio::select! {
            chunk = stream.next() => chunk,
            _ = context.cancellation.changed() => {
                finish_reasoning_tool(&context, reasoning_call.as_ref(), &turn.reasoning, ToolCallStatus::Failed)?;
                return Ok(Completion::Cancelled);
            },
        };
        let Some(chunk) = chunk else { break };
        let chunk = match chunk {
            Ok(chunk) => chunk,
            Err(error) => {
                let _ = finish_reasoning_tool(
                    &context,
                    reasoning_call.as_ref(),
                    &turn.reasoning,
                    ToolCallStatus::Failed,
                );
                return Err(format!("provider stream failed: {error}"));
            }
        };
        buffer.push_str(&String::from_utf8_lossy(&chunk));
        while let Some(index) = buffer.find('\n') {
            let line = buffer[..index].trim_end_matches('\r').to_owned();
            buffer.drain(..=index);
            if let Err(error) = process_sse_line(
                &line,
                &mut turn,
                &mut calls,
                &context.session_id,
                &context.message_id,
                &context.connection,
                &mut reasoning_call,
            ) {
                let _ = finish_reasoning_tool(
                    &context,
                    reasoning_call.as_ref(),
                    &turn.reasoning,
                    ToolCallStatus::Failed,
                );
                return Err(error);
            }
        }
    }
    if !buffer.is_empty() {
        if let Err(error) = process_sse_line(
            buffer.trim_end_matches('\r'),
            &mut turn,
            &mut calls,
            &context.session_id,
            &context.message_id,
            &context.connection,
            &mut reasoning_call,
        ) {
            let _ = finish_reasoning_tool(
                &context,
                reasoning_call.as_ref(),
                &turn.reasoning,
                ToolCallStatus::Failed,
            );
            return Err(error);
        }
    }
    finish_reasoning_tool(
        &context,
        reasoning_call.as_ref(),
        &turn.reasoning,
        ToolCallStatus::Completed,
    )?;
    turn.tool_calls = calls.into_values().collect();
    Ok(Completion::Completed(turn))
}

fn reasoning_request(config: &ProviderConfig) -> Option<Value> {
    config.reasoning.clone().or_else(|| {
        reqwest::Url::parse(&config.base_url).ok().and_then(|url| {
            (url.host_str() == Some("openrouter.ai")).then_some(json!({
                "enabled": true
            }))
        })
    })
}

fn model_messages(history: &[Value], mode: &str) -> Vec<Value> {
    let mode_instruction = match mode {
        "code" => {
            "You may use every provided tool, including tools that modify files or run commands."
        }
        "plan" => "Inspect with read-only tools as needed. Do not modify files or run commands.",
        _ => "Inspect with read-only tools as needed. Do not modify files or run commands.",
    };
    let system = format!(
        "You are a workspace coding agent. Use the provided tools proactively whenever they help answer or complete the user's request. If the user asks you to inspect files, list a directory, search, or use an installed CLI, invoke the appropriate tool immediately. Never ask for confirmation before invoking a tool and never offer to invoke it later. The host application performs any required approval after the tool call, so do not request approval in chat. Do not claim that an executable is unavailable merely because it is not a named tool; use run_command for installed workspace commands when the current mode permits it. {mode_instruction}"
    );
    std::iter::once(json!({ "role": "system", "content": system }))
        .chain(history.iter().cloned())
        .collect()
}

fn process_sse_line(
    line: &str,
    turn: &mut ModelTurn,
    calls: &mut BTreeMap<usize, ModelToolCall>,
    session_id: &SessionId,
    message_id: &MessageId,
    connection: &ConnectionTo<AcpClient>,
    reasoning_call: &mut Option<ReasoningToolCall>,
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
    let thought = apply_reasoning_delta(delta, turn);
    if !thought.is_empty() {
        publish_reasoning_tool(
            connection,
            session_id,
            message_id,
            reasoning_call,
            &turn.reasoning,
        )?;
    }
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

fn publish_reasoning_tool(
    connection: &ConnectionTo<AcpClient>,
    session_id: &SessionId,
    message_id: &MessageId,
    reasoning_call: &mut Option<ReasoningToolCall>,
    reasoning: &str,
) -> Result<(), String> {
    let content = vec![ToolCallContent::from(ContentBlock::Text(TextContent::new(
        reasoning,
    )))];
    let update = if let Some(call) = reasoning_call {
        SessionUpdate::ToolCallUpdate(ToolCallUpdate::new(
            call.id.clone(),
            ToolCallUpdateFields::new().content(content),
        ))
    } else {
        let id = format!("think-{message_id}");
        *reasoning_call = Some(ReasoningToolCall { id: id.clone() });
        SessionUpdate::ToolCall(
            AcpToolCall::new(id, "Thinking")
                .kind(ToolKind::Think)
                .status(ToolCallStatus::InProgress)
                .content(content),
        )
    };
    connection
        .send_notification(SessionNotification::new(session_id.clone(), update))
        .map_err(|error| format!("ACP write failed: {error}"))
}

fn finish_reasoning_tool(
    context: &StreamContext,
    reasoning_call: Option<&ReasoningToolCall>,
    reasoning: &str,
    status: ToolCallStatus,
) -> Result<(), String> {
    let Some(call) = reasoning_call else {
        return Ok(());
    };
    let fields = ToolCallUpdateFields::new()
        .status(status)
        .content(vec![ToolCallContent::from(ContentBlock::Text(
            TextContent::new(reasoning),
        ))]);
    context
        .connection
        .send_notification(SessionNotification::new(
            context.session_id.clone(),
            SessionUpdate::ToolCallUpdate(ToolCallUpdate::new(call.id.clone(), fields)),
        ))
        .map_err(|error| format!("ACP write failed: {error}"))
}

fn apply_reasoning_delta(delta: &Value, turn: &mut ModelTurn) -> String {
    let mut visible = String::new();
    if let Some(details) = delta.get("reasoning_details").and_then(Value::as_array) {
        for detail in details {
            if let Some(text) = detail
                .get("text")
                .or_else(|| detail.get("summary"))
                .and_then(Value::as_str)
            {
                visible.push_str(text);
            }
            merge_reasoning_detail(&mut turn.reasoning_details, detail);
        }
    }

    // Several OpenAI-compatible providers use a plain string rather than
    // OpenRouter's structured reasoning_details. Prefer structured text when
    // both are present so the same thought is never displayed twice.
    if visible.is_empty() {
        if let Some(text) = delta
            .get("reasoning")
            .or_else(|| delta.get("reasoning_content"))
            .and_then(Value::as_str)
        {
            visible.push_str(text);
        }
    }
    turn.reasoning.push_str(&visible);
    visible
}

fn merge_reasoning_detail(details: &mut Vec<Value>, chunk: &Value) {
    let Some(chunk_object) = chunk.as_object() else {
        details.push(chunk.clone());
        return;
    };
    let index = chunk_object.get("index").and_then(Value::as_u64);
    let kind = chunk_object.get("type").and_then(Value::as_str);
    let existing = details.iter_mut().find(|detail| {
        detail.get("index").and_then(Value::as_u64) == index
            && detail.get("type").and_then(Value::as_str) == kind
    });
    let Some(existing) = existing.and_then(Value::as_object_mut) else {
        details.push(chunk.clone());
        return;
    };

    for (key, value) in chunk_object {
        if matches!(key.as_str(), "text" | "summary") {
            if let (Some(current), Some(fragment)) =
                (existing.get(key).and_then(Value::as_str), value.as_str())
            {
                let combined = format!("{current}{fragment}");
                existing.insert(key.clone(), Value::String(combined));
                continue;
            }
        }
        if !value.is_null() {
            existing.insert(key.clone(), value.clone());
        }
    }
}

pub fn assistant_message(turn: &ModelTurn) -> Value {
    let calls = turn.tool_calls.iter().map(|call| json!({
        "id": call.id, "type": "function", "function": { "name": call.name, "arguments": call.arguments }
    })).collect::<Vec<_>>();
    let mut message = json!({
        "role": "assistant",
        "content": if turn.text.is_empty() { Value::Null } else { Value::String(turn.text.clone()) },
        "tool_calls": calls
    });
    if !turn.reasoning_details.is_empty() {
        message["reasoning_details"] = Value::Array(turn.reasoning_details.clone());
    }
    message
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
            reasoning: String::new(),
            reasoning_details: Vec::new(),
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

    #[test]
    fn assembles_and_preserves_structured_reasoning_chunks() {
        let mut turn = ModelTurn {
            text: String::new(),
            reasoning: String::new(),
            reasoning_details: Vec::new(),
            tool_calls: Vec::new(),
        };
        let first = json!({
            "reasoning_details": [{
                "type": "reasoning.text", "text": "Let me ", "id": "r1",
                "format": "test-v1", "index": 0
            }]
        });
        let second = json!({
            "reasoning_details": [{
                "type": "reasoning.text", "text": "think.", "signature": "signed",
                "index": 0
            }]
        });
        assert_eq!(apply_reasoning_delta(&first, &mut turn), "Let me ");
        assert_eq!(apply_reasoning_delta(&second, &mut turn), "think.");
        assert_eq!(turn.reasoning, "Let me think.");
        assert_eq!(turn.reasoning_details[0]["text"], "Let me think.");
        assert_eq!(turn.reasoning_details[0]["signature"], "signed");

        let message = assistant_message(&turn);
        assert_eq!(message["reasoning_details"], json!(turn.reasoning_details));
    }

    #[test]
    fn accepts_plain_reasoning_without_duplicating_structured_text() {
        let mut turn = ModelTurn {
            text: String::new(),
            reasoning: String::new(),
            reasoning_details: Vec::new(),
            tool_calls: Vec::new(),
        };
        let plain = json!({ "reasoning_content": "plain thought" });
        assert_eq!(apply_reasoning_delta(&plain, &mut turn), "plain thought");

        let both = json!({
            "reasoning": "duplicate",
            "reasoning_details": [{
                "type": "reasoning.text", "text": "structured", "index": 0
            }]
        });
        assert_eq!(apply_reasoning_delta(&both, &mut turn), "structured");
        assert_eq!(turn.reasoning, "plain thoughtstructured");
    }

    #[test]
    fn enables_openrouter_reasoning_by_default_but_not_other_providers() {
        let config = ProviderConfig {
            base_url: "https://openrouter.ai/api/v1".into(),
            api_key: "secret".into(),
            model: "test-model".into(),
            reasoning: None,
        };
        assert_eq!(reasoning_request(&config), Some(json!({ "enabled": true })));

        let generic = ProviderConfig {
            base_url: "https://example.test/v1".into(),
            ..config.clone()
        };
        assert_eq!(reasoning_request(&generic), None);

        let disabled = ProviderConfig {
            reasoning: Some(json!({ "enabled": false })),
            ..config
        };
        assert_eq!(
            reasoning_request(&disabled),
            Some(json!({ "enabled": false }))
        );
    }

    #[test]
    fn model_policy_requires_direct_tool_use_and_delegates_approval_to_host() {
        let messages = model_messages(&[json!({ "role": "user", "content": "use gog" })], "code");
        let policy = messages[0]["content"].as_str().expect("system policy");
        assert!(policy.contains("invoke the appropriate tool immediately"));
        assert!(policy.contains("Never ask for confirmation"));
        assert!(policy.contains("host application performs any required approval"));
        assert!(policy.contains("use run_command"));
        assert_eq!(messages[1]["content"], "use gog");
    }

    #[test]
    fn non_code_modes_forbid_mutating_tools_without_discouraging_inspection() {
        let messages = model_messages(&[], "ask");
        let policy = messages[0]["content"].as_str().expect("system policy");
        assert!(policy.contains("Inspect with read-only tools as needed"));
        assert!(policy.contains("Do not modify files or run commands"));
    }
}
