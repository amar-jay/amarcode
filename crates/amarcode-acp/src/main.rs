use std::{env, path::PathBuf};

use reqwest::Client;
use serde_json::{json, Value};
use tokio::io::{self, AsyncBufReadExt, AsyncWrite, AsyncWriteExt, BufReader};

mod provider;
use provider::{Config, Message};

const DEFAULT_CONFIG_PATH: &str = "amarcode-acp.json";

#[derive(Debug, Clone)]
struct Session {
    id: String,
    cwd: String,
    history: Vec<Message>,
    mode: String,
}

#[tokio::main]
async fn main() -> io::Result<()> {
    let config_path = config_path();
    let config = match Config::from_file(&config_path) {
        Ok(config) => config,
        Err(error) => {
            eprintln!("amarcode-acp: configuration error: {error}");
            return Ok(());
        }
    };

    let client = Client::new();
    let stdin = BufReader::new(io::stdin());
    let mut lines = stdin.lines();
    let mut stdout = io::BufWriter::new(io::stdout());
    let mut sessions: Vec<Session> = Vec::new();
    let mut active_session_id: Option<String> = None;

    while let Some(line) = lines.next_line().await? {
        if line.trim().is_empty() {
            continue;
        }
        let message: Value = match serde_json::from_str(&line) {
            Ok(message) => message,
            Err(error) => {
                eprintln!("amarcode-acp: invalid JSON: {error}");
                continue;
            }
        };
        let id = message.get("id").cloned();
        let Some(method) = message.get("method").and_then(Value::as_str) else {
            continue;
        };
        let params = message.get("params").cloned().unwrap_or(Value::Null);

        match method {
            "initialize" => {
                reply(
                    &mut stdout,
                    id,
                    json!({
                        "protocolVersion": 1,
                        "agentCapabilities": {
                            "loadSession": false,
                            "sessionCapabilities": {
                                "list": true,
                                "resume": true,
                                "close": true
                            },
                            "promptCapabilities": {
                                "image": false,
                                "audio": false,
                                "embeddedContext": false
                            }
                        },
                        "agentInfo": {
                            "name": "amarcode-acp",
                            "title": "OpenAI-compatible API",
                            "version": env!("CARGO_PKG_VERSION")
                        },
                        "authMethods": [{
                            "id": "api-key",
                            "name": "OpenAI-compatible API key"
                        }]
                    }),
                )
                .await?;
            }
            "authenticate" => {
                reply(&mut stdout, id, json!({ "authenticated": true })).await?;
            }
            "session/new" => {
                let cwd = params
                    .get("cwd")
                    .and_then(Value::as_str)
                    .unwrap_or(".")
                    .to_owned();
                let session = Session {
                    id: format!("amarcode-{}", stable_hash(&cwd)),
                    cwd,
                    history: Vec::new(),
                    mode: "ask".to_owned(),
                };
                let session_id = session.id.clone();
                sessions.retain(|existing| existing.id != session_id);
                sessions.push(session);
                active_session_id = Some(session_id.clone());
                reply(
                    &mut stdout,
                    id,
                    json!({
                        "sessionId": session_id,
                        "configOptions": config_options("ask", &config.model)
                    }),
                )
                .await?;
            }
            "session/load" | "session/resume" => {
                let requested = params.get("sessionId").and_then(Value::as_str);
                if let Some(session) = requested.and_then(|id| sessions.iter().find(|s| s.id == id))
                {
                    active_session_id = Some(session.id.clone());
                    reply(
                        &mut stdout,
                        id,
                        json!({
                            "sessionId": session.id,
                            "configOptions": config_options(&session.mode, &config.model)
                        }),
                    )
                    .await?;
                } else {
                    reply_error(&mut stdout, id, -32602, "Unknown session").await?;
                }
            }
            "session/list" => {
                let cwd = params.get("cwd").and_then(Value::as_str);
                let listed = sessions
                    .iter()
                    .filter(|session| cwd.is_none_or(|cwd| cwd == session.cwd))
                    .map(|session| json!({ "sessionId": session.id, "cwd": session.cwd }))
                    .collect::<Vec<_>>();
                reply(&mut stdout, id, json!({ "sessions": listed })).await?;
            }
            "session/delete" => {
                let requested = params.get("sessionId").and_then(Value::as_str);
                let deleted = requested.is_some_and(|requested| {
                    let before = sessions.len();
                    sessions.retain(|session| session.id != requested);
                    sessions.len() != before
                });
                if active_session_id.as_deref() == requested {
                    active_session_id = None;
                }
                reply(&mut stdout, id, json!({ "deleted": deleted })).await?;
            }
            "session/set_mode" => {
                let mode = params
                    .get("mode")
                    .and_then(Value::as_str)
                    .or_else(|| params.get("modeId").and_then(Value::as_str));
                if let (Some(session_id), Some(mode @ ("ask" | "code" | "plan"))) =
                    (active_session_id.as_deref(), mode)
                {
                    if let Some(session) = sessions.iter_mut().find(|s| s.id == session_id) {
                        session.mode = mode.to_owned();
                    }
                    reply(&mut stdout, id, json!({})).await?;
                } else {
                    reply_error(&mut stdout, id, -32602, "Unsupported session mode").await?;
                }
            }
            "session/set_config_option" => {
                let config_id = params.get("configId").and_then(Value::as_str);
                let value = params.get("value").and_then(Value::as_str);
                if let (Some("mode"), Some(mode @ ("ask" | "code" | "plan"))) = (config_id, value) {
                    if let Some(session_id) = active_session_id.as_deref() {
                        if let Some(session) = sessions.iter_mut().find(|s| s.id == session_id) {
                            session.mode = mode.to_owned();
                        }
                    }
                    reply(
                        &mut stdout,
                        id,
                        json!({ "configOptions": config_options(mode, &config.model) }),
                    )
                    .await?;
                } else {
                    reply_error(&mut stdout, id, -32602, "Unknown config option").await?;
                }
            }
            "session/prompt" => {
                let prompt = extract_prompt_text(&params);
                if prompt.is_empty() {
                    reply_error(&mut stdout, id, -32602, "Prompt must contain text").await?;
                    continue;
                }
                let Some(session_id) = active_session_id.clone() else {
                    reply_error(&mut stdout, id, -32000, "No active session").await?;
                    continue;
                };
                let history = {
                    let session = sessions
                        .iter_mut()
                        .find(|s| s.id == session_id)
                        .expect("active session");
                    session.history.push(Message {
                        role: "user",
                        content: prompt,
                    });
                    session.history.clone()
                };
                match provider::stream_completion(
                    &client,
                    &config,
                    &history,
                    &session_id,
                    &mut stdout,
                )
                .await
                {
                    Ok(answer) => {
                        if let Some(session) = sessions.iter_mut().find(|s| s.id == session_id) {
                            session.history.push(Message {
                                role: "assistant",
                                content: answer,
                            });
                        }
                        reply(&mut stdout, id, json!({ "stopReason": "end_turn" })).await?;
                    }
                    Err(error) => {
                        if let Some(session) = sessions.iter_mut().find(|s| s.id == session_id) {
                            let _ = session.history.pop();
                        }
                        reply_error(&mut stdout, id, -32000, &error).await?;
                    }
                }
            }
            "session/cancel" => {
                reply(&mut stdout, id, json!({})).await?;
            }
            "session/close" => {
                if let Some(session_id) = params.get("sessionId").and_then(Value::as_str) {
                    sessions.retain(|session| session.id != session_id);
                    if active_session_id.as_deref() == Some(session_id) {
                        active_session_id = None;
                    }
                }
                reply(&mut stdout, id, json!({})).await?;
            }
            "logout" => {
                reply(&mut stdout, id, json!({})).await?;
            }
            other if id.is_some() => {
                reply_error(
                    &mut stdout,
                    id,
                    -32601,
                    &format!("Method not found: {other}"),
                )
                .await?;
            }
            _ => {}
        }
    }
    Ok(())
}

fn config_path() -> PathBuf {
    let mut arguments = env::args().skip(1);
    while let Some(argument) = arguments.next() {
        if argument == "--config" {
            if let Some(path) = arguments.next() {
                return PathBuf::from(path);
            }
            eprintln!("amarcode-acp: --config requires a file path");
            break;
        }
    }
    PathBuf::from(DEFAULT_CONFIG_PATH)
}

fn extract_prompt_text(params: &Value) -> String {
    params
        .get("prompt")
        .and_then(|prompt| {
            prompt
                .as_array()
                .map(|blocks| {
                    blocks
                        .iter()
                        .filter_map(|block| block.get("text")?.as_str())
                        .collect::<String>()
                })
                .or_else(|| prompt.as_str().map(ToOwned::to_owned))
        })
        .unwrap_or_default()
}

fn config_options(mode: &str, model: &str) -> Value {
    json!([
        {
            "id": "mode",
            "name": "Session mode",
            "category": "mode",
            "type": "select",
            "currentValue": mode,
            "options": [
                { "value": "ask", "name": "Ask" },
                { "value": "code", "name": "Code" },
                { "value": "plan", "name": "Plan" }
            ]
        },
        {
            "id": "model",
            "name": "Model",
            "category": "model",
            "type": "select",
            "currentValue": model,
            "options": [{ "value": model, "name": model }]
        }
    ])
}

async fn reply(
    stdout: &mut (impl AsyncWrite + Unpin),
    id: Option<Value>,
    result: Value,
) -> io::Result<()> {
    let Some(id) = id else { return Ok(()) };
    write_line(
        stdout,
        &json!({ "jsonrpc": "2.0", "id": id, "result": result }),
    )
    .await
}

async fn reply_error(
    stdout: &mut (impl AsyncWrite + Unpin),
    id: Option<Value>,
    code: i64,
    message: &str,
) -> io::Result<()> {
    let Some(id) = id else { return Ok(()) };
    write_line(
        stdout,
        &json!({ "jsonrpc": "2.0", "id": id, "error": { "code": code, "message": message } }),
    )
    .await
}

async fn write_line(stdout: &mut (impl AsyncWrite + Unpin), value: &Value) -> io::Result<()> {
    let line = serde_json::to_string(value).expect("ACP message must serialize");
    stdout.write_all(line.as_bytes()).await?;
    stdout.write_all(b"\n").await?;
    stdout.flush().await
}

fn stable_hash(value: &str) -> u32 {
    value.bytes().fold(0u32, |hash, byte| {
        hash.wrapping_mul(31).wrapping_add(byte as u32)
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn extracts_text_from_acp_blocks() {
        let params = json!({ "prompt": [
            { "type": "text", "text": "hello " },
            { "type": "text", "text": "world" }
        ]});
        assert_eq!(extract_prompt_text(&params), "hello world");
    }

    #[test]
    fn extracts_string_prompt() {
        assert_eq!(extract_prompt_text(&json!({ "prompt": "hello" })), "hello");
    }

    #[test]
    fn parses_json_config() {
        let path =
            std::env::temp_dir().join(format!("amarcode-acp-config-{}.json", std::process::id()));
        std::fs::write(
            &path,
            r#"{"baseUrl":"https://example.test/v1/","apiKey":"secret","model":"test-model"}"#,
        )
        .expect("write config");
        let config = Config::from_file(&path).expect("parse config");
        assert_eq!(config.base_url, "https://example.test/v1");
        assert_eq!(config.model, "test-model");
        std::fs::remove_file(path).expect("remove config");
    }
}
