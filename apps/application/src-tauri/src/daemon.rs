use crate::models::AgentEvent;
use serde::de::DeserializeOwned;
use serde_json::{json, Value};
use tauri::ipc::Channel;
use tokio::{
    io::{AsyncBufReadExt, AsyncWriteExt, BufReader},
    net::TcpStream,
};

pub struct DaemonBridge;

impl DaemonBridge {
    pub fn launch_if_needed() {
        // The service is intentionally independent of the window. Deployments can provide a
        // launcher command; development defaults to the daemon executable on PATH.
        if std::net::TcpStream::connect(daemon_addr()).is_ok() {
            return;
        }
        let command = std::env::var("ACP_WORKBENCH_DAEMON_COMMAND")
            .unwrap_or_else(|_| "amarcode-daemon".into());
        let _ = std::process::Command::new(command).spawn();
    }

    pub async fn call<T: DeserializeOwned>(method: &str, params: Value) -> Result<T, String> {
        let mut stream = connect().await?;
        stream
            .write_all(format!("{}\n", json!({ "method": method, "params": params })).as_bytes())
            .await
            .map_err(|error| error.to_string())?;
        let mut lines = BufReader::new(stream).lines();
        let line = lines
            .next_line()
            .await
            .map_err(|error| error.to_string())?
            .ok_or("ACP daemon closed the connection")?;
        let response: Value = serde_json::from_str(&line).map_err(|error| error.to_string())?;
        if let Some(error) = response.get("error").and_then(Value::as_str) {
            return Err(error.into());
        }
        serde_json::from_value(response.get("result").cloned().unwrap_or(Value::Null))
            .map_err(|error| error.to_string())
    }

    pub async fn forward_events(channel: Channel<AgentEvent>) -> Result<(), String> {
        let mut stream = connect().await?;
        stream
            .write_all(
                format!(
                    "{}\n",
                    json!({ "method": "subscribe_events", "params": {} })
                )
                .as_bytes(),
            )
            .await
            .map_err(|error| error.to_string())?;
        let mut lines = BufReader::new(stream).lines();
        let acknowledgement = lines
            .next_line()
            .await
            .map_err(|error| error.to_string())?
            .ok_or("ACP daemon closed the event stream")?;
        let acknowledgement: Value =
            serde_json::from_str(&acknowledgement).map_err(|error| error.to_string())?;
        if let Some(error) = acknowledgement.get("error").and_then(Value::as_str) {
            return Err(error.into());
        }
        while let Some(line) = lines.next_line().await.map_err(|error| error.to_string())? {
            let message: Value = serde_json::from_str(&line).map_err(|error| error.to_string())?;
            if let Some(event) = message.get("event") {
                let event: AgentEvent =
                    serde_json::from_value(event.clone()).map_err(|error| error.to_string())?;
                channel.send(event).map_err(|error| error.to_string())?;
            }
        }
        Ok(())
    }
}

fn daemon_addr() -> String {
    std::env::var("ACP_WORKBENCH_DAEMON_ADDR").unwrap_or_else(|_| "127.0.0.1:43821".into())
}

async fn connect() -> Result<TcpStream, String> {
    let address = daemon_addr();
    let mut last_error = None;
    for _ in 0..20 {
        match TcpStream::connect(&address).await {
            Ok(stream) => return Ok(stream),
            Err(error) => last_error = Some(error),
        }
        tokio::time::sleep(std::time::Duration::from_millis(100)).await;
    }
    Err(format!(
        "ACP daemon is unavailable at {address}: {}",
        last_error.expect("connection attempt was made")
    ))
}
