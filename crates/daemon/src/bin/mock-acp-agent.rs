//! Minimal ACP agent for end-to-end tests (real ACP method names).
//!
//! Speaks newline-delimited JSON-RPC on stdio:
//! - `initialize`
//! - `session/new` → `{ "sessionId": "..." }`
//! - `session/prompt` → streams `session/update` chunks, then result
//! - `session/cancel` / `session/close` → ack

use std::io::{self, BufRead, Write};

use serde_json::{Value, json};

fn main() {
    let stdin = io::stdin();
    let mut stdout = io::stdout();
    let mut session_id = String::from("mock-session-1");

    for line in stdin.lock().lines() {
        let line = match line {
            Ok(l) => l,
            Err(err) => {
                eprintln!("mock-acp-agent: read error: {err}");
                break;
            }
        };
        if line.trim().is_empty() {
            continue;
        }

        let msg: Value = match serde_json::from_str(&line) {
            Ok(v) => v,
            Err(err) => {
                eprintln!("mock-acp-agent: bad json: {err}");
                continue;
            }
        };

        let method = msg.get("method").and_then(|m| m.as_str());
        let id = msg.get("id").cloned();
        let params = msg.get("params").cloned().unwrap_or(Value::Null);

        match method {
            Some("initialize") => {
                reply(
                    &mut stdout,
                    id,
                    json!({
                        "protocolVersion": 1,
                        "agentCapabilities": {
                            "loadSession": false,
                            "promptCapabilities": { "image": false, "audio": false, "embeddedContext": false }
                        },
                        "agentInfo": {
                            "name": "mock-acp-agent",
                            "title": "Mock ACP Agent",
                            "version": "0.1.0"
                        },
                        "authMethods": []
                    }),
                );
            }
            Some("session/new") => {
                if let Some(cwd) = params.get("cwd").and_then(|v| v.as_str()) {
                    session_id = format!("mock-session-{}", simple_hash(cwd));
                }
                reply(&mut stdout, id, json!({ "sessionId": session_id }));
            }
            Some("session/prompt") => {
                let text = extract_prompt_text(&params);
                let reply_text = format!("mock echo: {text}");

                // Use separate ACP message ids to model a progress/commentary
                // message followed by a final answer.
                notify(
                    &mut stdout,
                    "session/update",
                    json!({
                        "sessionId": session_id,
                        "update": {
                            "sessionUpdate": "agent_message_chunk",
                            "messageId": "mock-commentary",
                            "content": { "type": "text", "text": "mock progress." }
                        }
                    }),
                );
                notify(
                    &mut stdout,
                    "session/update",
                    json!({
                        "sessionId": session_id,
                        "update": {
                            "sessionUpdate": "tool_call",
                            "toolCallId": "mock-tool",
                            "title": "Mock tool"
                        }
                    }),
                );
                notify(
                    &mut stdout,
                    "session/update",
                    json!({
                        "sessionId": session_id,
                        "update": {
                            "sessionUpdate": "agent_message_chunk",
                            "messageId": "mock-final",
                            "content": { "type": "text", "text": reply_text }
                        }
                    }),
                );
                reply(
                    &mut stdout,
                    id,
                    json!({ "stopReason": "end_turn" }),
                );
            }
            Some("session/cancel") | Some("session/close") | Some("session/load") => {
                reply(&mut stdout, id, json!({}));
            }
            Some(other) => {
                if id.is_some() {
                    reply_error(
                        &mut stdout,
                        id,
                        -32601,
                        &format!("Method not found: {other}"),
                    );
                }
            }
            None => {}
        }
    }
}

fn extract_prompt_text(params: &Value) -> String {
    if let Some(arr) = params.get("prompt").and_then(|v| v.as_array()) {
        return arr
            .iter()
            .filter_map(|block| block.get("text")?.as_str())
            .collect::<Vec<_>>()
            .join("");
    }
    params
        .get("prompt")
        .and_then(|v| v.as_str())
        .unwrap_or("")
        .to_owned()
}

fn reply(stdout: &mut impl Write, id: Option<Value>, result: Value) {
    let Some(id) = id else {
        return;
    };
    write_line(
        stdout,
        &json!({
            "jsonrpc": "2.0",
            "id": id,
            "result": result,
        }),
    );
}

fn reply_error(stdout: &mut impl Write, id: Option<Value>, code: i64, message: &str) {
    let Some(id) = id else {
        return;
    };
    write_line(
        stdout,
        &json!({
            "jsonrpc": "2.0",
            "id": id,
            "error": { "code": code, "message": message },
        }),
    );
}

fn notify(stdout: &mut impl Write, method: &str, params: Value) {
    write_line(
        stdout,
        &json!({
            "jsonrpc": "2.0",
            "method": method,
            "params": params,
        }),
    );
}

fn write_line(stdout: &mut impl Write, value: &Value) {
    if let Ok(line) = serde_json::to_string(value) {
        let _ = writeln!(stdout, "{line}");
        let _ = stdout.flush();
    }
}

fn simple_hash(s: &str) -> u32 {
    s.bytes()
        .fold(0u32, |acc, b| acc.wrapping_mul(31).wrapping_add(b as u32))
}
