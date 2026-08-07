mod acp;
mod agent_runtime;
mod models;
mod store;

use acp::SessionManager;
use agent_runtime::AgentRuntime;
use models::{AgentDefinition, AgentEvent};
use serde::Deserialize;
use serde_json::{json, Value};
use std::{io, path::PathBuf, sync::Arc};
use store::Store;
use tokio::{
    io::{AsyncBufReadExt, AsyncWriteExt, BufReader},
    net::{TcpListener, TcpStream},
    sync::broadcast,
};

const SERVICE_NAME: &str = "acp-workbench-daemon";

#[derive(Debug, Deserialize)]
struct Request {
    method: String,
    #[serde(default)]
    params: Value,
}

struct Daemon {
    store: Arc<Store>,
    sessions: Arc<SessionManager>,
    events: broadcast::Sender<AgentEvent>,
}

fn data_dir() -> PathBuf {
    std::env::var_os("ACP_WORKBENCH_DATA_DIR")
        .map(PathBuf::from)
        .unwrap_or_else(|| {
            dirs::data_local_dir()
                .unwrap_or_else(std::env::temp_dir)
                .join("acp-workbench")
        })
}
fn daemon_addr() -> String {
    std::env::var("ACP_WORKBENCH_DAEMON_ADDR").unwrap_or_else(|_| "127.0.0.1:43821".into())
}

#[tokio::main]
async fn main() -> io::Result<()> {
    let dir = data_dir();
    std::fs::create_dir_all(&dir)?;
    let store = Arc::new(Store::open(&dir.join("workbench.sqlite3")).map_err(io::Error::other)?);
    store.seed_presets().map_err(io::Error::other)?;
    store
        .stop_interrupted_sessions()
        .map_err(io::Error::other)?;
    let (events, _) = broadcast::channel(512);
    let daemon = Arc::new(Daemon {
        sessions: Arc::new(SessionManager::new(
            store.clone(),
            events.clone(),
            Arc::new(AgentRuntime::new(dir.join("tools"))),
        )),
        store,
        events,
    });
    let address = daemon_addr();
    let listener = TcpListener::bind(&address).await?;
    eprintln!("{SERVICE_NAME} listening on {address}");
    loop {
        tokio::select! {
            accepted = listener.accept() => { let (stream, _) = accepted?; tokio::spawn(handle_connection(stream, daemon.clone())); }
            signal = tokio::signal::ctrl_c() => { signal?; return Ok(()); }
        }
    }
}

async fn handle_connection(stream: TcpStream, daemon: Arc<Daemon>) -> io::Result<()> {
    let (reader, mut writer) = stream.into_split();
    let mut lines = BufReader::new(reader).lines();
    while let Some(line) = lines.next_line().await? {
        let request: Request = match serde_json::from_str(&line) {
            Ok(request) => request,
            Err(_) => {
                write_line(&mut writer, &json!({"error":"invalid request"})).await?;
                continue;
            }
        };
        if request.method == "subscribe_events" {
            let session_id = request
                .params
                .get("sessionId")
                .and_then(Value::as_str)
                .map(str::to_owned);
            write_line(&mut writer, &json!({"result":{"subscribed":true}})).await?;
            let mut events = daemon.events.subscribe();
            while let Ok(event) = events.recv().await {
                if session_id
                    .as_deref()
                    .is_none_or(|id| id == event.session_id())
                {
                    write_line(&mut writer, &json!({"event":event})).await?;
                }
            }
            return Ok(());
        }
        let response = dispatch(&daemon, request).await;
        write_line(
            &mut writer,
            &match response {
                Ok(result) => json!({"result":result}),
                Err(error) => json!({"error":error}),
            },
        )
        .await?;
    }
    Ok(())
}

async fn dispatch(daemon: &Daemon, request: Request) -> Result<Value, String> {
    match request.method.as_str() {
        "health" => Ok(
            json!({"service": SERVICE_NAME, "status":"ready", "version": env!("CARGO_PKG_VERSION")}),
        ),
        "list_agents" => serde_json::to_value(daemon.store.agents()?).map_err(|e| e.to_string()),
        "save_agent" => {
            let agent: AgentDefinition = serde_json::from_value(
                request
                    .params
                    .get("agent")
                    .cloned()
                    .ok_or("missing agent")?,
            )
            .map_err(|e| e.to_string())?;
            daemon.store.save_agent(&agent)?;
            Ok(Value::Null)
        }
        "list_sessions" => serde_json::to_value(daemon.sessions.session_summaries().await?)
            .map_err(|e| e.to_string()),
        "session_events" => {
            let id = required_string(&request.params, "sessionId")?;
            serde_json::to_value(daemon.store.events(id)?).map_err(|e| e.to_string())
        }
        "start_session" => {
            let workspace_path = required_string(&request.params, "workspacePath")?.to_owned();
            let agent: AgentDefinition = serde_json::from_value(
                request
                    .params
                    .get("agent")
                    .cloned()
                    .ok_or("missing agent")?,
            )
            .map_err(|e| e.to_string())?;
            serde_json::to_value(daemon.sessions.start(workspace_path, agent).await?)
                .map_err(|e| e.to_string())
        }
        "send_prompt" => {
            daemon
                .sessions
                .prompt(
                    required_string(&request.params, "sessionId")?,
                    required_string(&request.params, "prompt")?.to_owned(),
                )
                .await?;
            Ok(Value::Null)
        }
        "cancel_session" => {
            daemon
                .sessions
                .cancel(required_string(&request.params, "sessionId")?)
                .await?;
            Ok(Value::Null)
        }
        "respond_to_request" => {
            daemon
                .sessions
                .respond(
                    required_string(&request.params, "sessionId")?,
                    required_string(&request.params, "requestId")?,
                    request.params.get("result").cloned().unwrap_or(Value::Null),
                )
                .await?;
            Ok(Value::Null)
        }
        "save_secret" => {
            let reference = required_string(&request.params, "secretRef")?;
            let value = required_string(&request.params, "value")?;
            keyring::Entry::new("acp-workbench", reference)
                .map_err(|e| e.to_string())?
                .set_password(value)
                .map_err(|e| e.to_string())?;
            Ok(Value::Null)
        }
        _ => Err("unknown method".into()),
    }
}

fn required_string<'a>(params: &'a Value, key: &str) -> Result<&'a str, String> {
    params
        .get(key)
        .and_then(Value::as_str)
        .ok_or_else(|| format!("missing {key}"))
}
async fn write_line(writer: &mut tokio::net::tcp::OwnedWriteHalf, value: &Value) -> io::Result<()> {
    writer.write_all(format!("{value}\n").as_bytes()).await
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn daemon_address_can_be_overridden() {
        std::env::set_var("ACP_WORKBENCH_DAEMON_ADDR", "127.0.0.1:43822");
        assert_eq!(daemon_addr(), "127.0.0.1:43822");
        std::env::remove_var("ACP_WORKBENCH_DAEMON_ADDR");
    }
}
