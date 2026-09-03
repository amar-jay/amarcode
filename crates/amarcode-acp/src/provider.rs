use futures_util::StreamExt;
use reqwest::Client;
use serde::Deserialize;
use serde_json::{json, Value};
use tokio::io::{AsyncWrite, AsyncWriteExt};

#[derive(Debug, Clone, Deserialize)]
pub struct Config {
    #[serde(alias = "baseUrl")]
    pub base_url: String,
    #[serde(alias = "apiKey")]
    pub api_key: String,
    pub model: String,
}

impl Config {
    pub fn from_file(path: &std::path::Path) -> Result<Self, String> {
        let contents = std::fs::read_to_string(path)
            .map_err(|error| format!("failed to read {}: {error}", path.display()))?;
        let mut config: Self = serde_json::from_str(&contents)
            .map_err(|error| format!("invalid JSON in {}: {error}", path.display()))?;
        config.base_url = config.base_url.trim_end_matches('/').to_owned();
        if config.base_url.is_empty()
            || config.api_key.trim().is_empty()
            || config.model.trim().is_empty()
        {
            return Err("base_url, api_key, and model must not be empty".into());
        }
        Ok(config)
    }

    fn endpoint(&self) -> String {
        format!("{}/chat/completions", self.base_url)
    }
}

#[derive(Debug, Clone)]
pub struct Message {
    pub role: &'static str,
    pub content: String,
}

pub async fn stream_completion(
    client: &Client,
    config: &Config,
    history: &[Message],
    session_id: &str,
    stdout: &mut (impl AsyncWrite + Unpin),
) -> Result<String, String> {
    let body = json!({
        "model": config.model,
        "messages": history.iter().map(|message| json!({
            "role": message.role,
            "content": message.content,
        })).collect::<Vec<_>>(),
        "stream": true,
    });
    let response = client
        .post(config.endpoint())
        .bearer_auth(&config.api_key)
        .json(&body)
        .send()
        .await
        .map_err(|error| format!("provider request failed: {error}"))?;
    let status = response.status();
    if !status.is_success() {
        let body = response.text().await.unwrap_or_default();
        return Err(format!(
            "provider returned {status}: {}",
            error_message(&body)
        ));
    }

    let mut stream = response.bytes_stream();
    let mut buffer = String::new();
    let mut answer = String::new();
    while let Some(chunk) = stream.next().await {
        let chunk = chunk.map_err(|error| format!("provider stream failed: {error}"))?;
        buffer.push_str(&String::from_utf8_lossy(&chunk));
        while let Some(index) = buffer.find('\n') {
            let line = buffer[..index].trim_end_matches('\r').to_owned();
            buffer.drain(..=index);
            if let Some(data) = line.strip_prefix("data:") {
                let data = data.trim();
                if data == "[DONE]" {
                    continue;
                }
                let value: Value = serde_json::from_str(data)
                    .map_err(|error| format!("invalid provider event: {error}"))?;
                if let Some(error) = value.get("error") {
                    return Err(error_message(&error.to_string()));
                }
                let text = value["choices"][0]["delta"]["content"]
                    .as_str()
                    .unwrap_or_default();
                if !text.is_empty() {
                    answer.push_str(text);
                    write_line(
                        stdout,
                        &json!({
                            "jsonrpc": "2.0",
                            "method": "session/update",
                            "params": {
                                "sessionId": session_id,
                                "update": {
                                    "sessionUpdate": "agent_message_chunk",
                                    "messageId": "amarcode-response",
                                    "content": { "type": "text", "text": text }
                                }
                            }
                        }),
                    )
                    .await
                    .map_err(|error| format!("ACP write failed: {error}"))?;
                }
            }
        }
    }
    Ok(answer)
}

fn error_message(body: &str) -> String {
    serde_json::from_str::<Value>(body)
        .ok()
        .and_then(|value| {
            value
                .pointer("/error/message")
                .and_then(Value::as_str)
                .map(str::to_owned)
        })
        .unwrap_or_else(|| body.trim().to_owned())
}

async fn write_line(stdout: &mut (impl AsyncWrite + Unpin), value: &Value) -> std::io::Result<()> {
    let line = serde_json::to_string(value).expect("ACP message must serialize");
    stdout.write_all(line.as_bytes()).await?;
    stdout.write_all(b"\n").await?;
    stdout.flush().await
}
