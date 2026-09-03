use std::{path::Path, pin::Pin};

use agent_client_protocol::{
    schema::v1::{
        ContentBlock, ContentChunk, MessageId, SessionId, SessionNotification, SessionUpdate,
        TextContent,
    },
    Client as AcpClient, ConnectionTo,
};
use futures_util::{Stream, StreamExt};
use reqwest::{Client as HttpClient, Response};
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
                let mut characters = part.chars();
                match characters.next() {
                    Some(first) => {
                        let mut word = first.to_ascii_uppercase().to_string();
                        word.extend(characters);
                        word
                    }
                    None => String::new(),
                }
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
            return Err(
                "name must start with an ASCII lowercase letter or digit and contain only lowercase letters, digits, '.', '_', or '-'"
                    .into(),
            );
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
    let mut characters = name.chars();
    characters
        .next()
        .is_some_and(|character| character.is_ascii_lowercase() || character.is_ascii_digit())
        && characters.all(|character| {
            character.is_ascii_lowercase()
                || character.is_ascii_digit()
                || matches!(character, '.' | '_' | '-')
        })
}

#[derive(Debug, Clone)]
pub struct Message {
    pub role: &'static str,
    pub content: String,
}

pub enum Completion {
    Completed(String),
    Cancelled,
}

pub async fn stream_completion(
    client: &HttpClient,
    config: &Config,
    history: &[Message],
    session_id: SessionId,
    message_id: MessageId,
    connection: ConnectionTo<AcpClient>,
    mut cancellation: watch::Receiver<bool>,
) -> Result<Completion, String> {
    if *cancellation.borrow() {
        return Ok(Completion::Cancelled);
    }

    let body = json!({
        "model": config.provider.model,
        "messages": history.iter().map(|message| json!({
            "role": message.role,
            "content": message.content,
        })).collect::<Vec<_>>(),
        "stream": true,
    });
    let request = client
        .post(config.provider.endpoint())
        .bearer_auth(&config.provider.api_key)
        .json(&body)
        .send();
    let response = tokio::select! {
        response = request => response.map_err(|error| format!("provider request failed: {error}"))?,
        _ = cancellation.changed() => return Ok(Completion::Cancelled),
    };
    let response = check_response(response).await?;
    stream_response(
        Box::pin(response.bytes_stream()),
        session_id,
        message_id,
        connection,
        cancellation,
    )
    .await
}

async fn check_response(response: Response) -> Result<Response, String> {
    let status = response.status();
    if status.is_success() {
        return Ok(response);
    }
    let body = response.text().await.unwrap_or_default();
    Err(format!(
        "provider returned {status}: {}",
        error_message(&body)
    ))
}

async fn stream_response(
    mut stream: Pin<Box<dyn Stream<Item = Result<bytes::Bytes, reqwest::Error>> + Send>>,
    session_id: SessionId,
    message_id: MessageId,
    connection: ConnectionTo<AcpClient>,
    mut cancellation: watch::Receiver<bool>,
) -> Result<Completion, String> {
    let mut buffer = String::new();
    let mut answer = String::new();
    loop {
        let chunk = tokio::select! {
            chunk = stream.next() => chunk,
            _ = cancellation.changed() => return Ok(Completion::Cancelled),
        };
        let Some(chunk) = chunk else { break };
        let chunk = chunk.map_err(|error| format!("provider stream failed: {error}"))?;
        buffer.push_str(&String::from_utf8_lossy(&chunk));
        while let Some(index) = buffer.find('\n') {
            let line = buffer[..index].trim_end_matches('\r').to_owned();
            buffer.drain(..=index);
            process_sse_line(&line, &mut answer, &session_id, &message_id, &connection)?;
        }
    }
    if !buffer.is_empty() {
        process_sse_line(
            buffer.trim_end_matches('\r'),
            &mut answer,
            &session_id,
            &message_id,
            &connection,
        )?;
    }
    Ok(Completion::Completed(answer))
}

fn process_sse_line(
    line: &str,
    answer: &mut String,
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
        serde_json::from_str(data).map_err(|error| format!("invalid provider event: {error}"))?;
    if let Some(error) = value.get("error") {
        return Err(error_message(&error.to_string()));
    }
    let text = value["choices"][0]["delta"]["content"]
        .as_str()
        .unwrap_or_default();
    if text.is_empty() {
        return Ok(());
    }
    answer.push_str(text);
    connection
        .send_notification(SessionNotification::new(
            session_id.clone(),
            SessionUpdate::AgentMessageChunk(
                ContentChunk::new(ContentBlock::Text(TextContent::new(text)))
                    .message_id(message_id.clone()),
            ),
        ))
        .map_err(|error| format!("ACP write failed: {error}"))
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

#[cfg(test)]
mod tests {
    use super::*;

    fn test_config(name: &str) -> Config {
        Config {
            name: name.into(),
            provider: ProviderConfig {
                base_url: "https://example.test/v1".into(),
                api_key: "secret".into(),
                model: "test-model".into(),
            },
        }
    }

    #[test]
    fn derives_agent_title_from_name() {
        assert_eq!(
            test_config("local_qwen-coder.v2").title(),
            "Local Qwen Coder V2"
        );
    }

    #[test]
    fn parses_json_config() {
        let path =
            std::env::temp_dir().join(format!("amarcode-acp-config-{}.json", uuid::Uuid::new_v4()));
        std::fs::write(
            &path,
            r#"{
                "name":"test-agent",
                "provider":{"baseUrl":"https://example.test/v1/","apiKey":"secret","model":"test-model"}
            }"#,
        )
        .expect("write config");
        let config = Config::from_file(&path).expect("parse config");
        assert_eq!(config.name, "test-agent");
        assert_eq!(config.provider.base_url, "https://example.test/v1");
        assert_eq!(config.provider.model, "test-model");
        std::fs::remove_file(path).expect("remove config");
    }
}
