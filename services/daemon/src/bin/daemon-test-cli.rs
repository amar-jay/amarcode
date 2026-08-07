use serde_json::{json, Value};
use std::{env, process::ExitCode};
use tokio::{
    io::{AsyncBufReadExt, AsyncWriteExt, BufReader},
    net::TcpStream,
};

#[tokio::main]
async fn main() -> ExitCode {
    match run(env::args().skip(1).collect()).await {
        Ok(()) => ExitCode::SUCCESS,
        Err(error) => {
            eprintln!("error: {error}");
            ExitCode::FAILURE
        }
    }
}

async fn run(mut args: Vec<String>) -> Result<(), String> {
    let address = take_option(&mut args, "--address").unwrap_or_else(daemon_addr);
    let Some(command) = args.first().cloned() else {
        print_usage();
        return Ok(());
    };
    args.remove(0);
    match command.as_str() {
        "help" | "--help" | "-h" => print_usage(),
        "health" => print_value(call(&address, "health", json!({})).await?),
        "agents" => print_value(call(&address, "list_agents", json!({})).await?),
        "sessions" => print_value(call(&address, "list_sessions", json!({})).await?),
        "events" => print_value(
            call(
                &address,
                "session_events",
                json!({ "sessionId": required(&args, 0, "session id")? }),
            )
            .await?,
        ),
        "start" => {
            let workspace_path = required(&args, 0, "workspace path")?;
            let agent_id = required(&args, 1, "agent id")?;
            let agents = call(&address, "list_agents", json!({})).await?;
            let agent = agents
                .as_array()
                .and_then(|agents| {
                    agents
                        .iter()
                        .find(|agent| agent.get("id").and_then(Value::as_str) == Some(agent_id))
                })
                .cloned()
                .ok_or_else(|| format!("unknown agent '{agent_id}'"))?;
            print_value(
                call(
                    &address,
                    "start_session",
                    json!({ "workspacePath": workspace_path, "agent": agent }),
                )
                .await?,
            );
        }
        "prompt" => {
            let session_id = required(&args, 0, "session id")?;
            let prompt = args
                .get(1..)
                .filter(|parts| !parts.is_empty())
                .map(|parts| parts.join(" "))
                .ok_or("prompt text is required")?;
            print_value(
                call(
                    &address,
                    "send_prompt",
                    json!({ "sessionId": session_id, "prompt": prompt }),
                )
                .await?,
            );
        }
        "cancel" => print_value(
            call(
                &address,
                "cancel_session",
                json!({ "sessionId": required(&args, 0, "session id")? }),
            )
            .await?,
        ),
        "respond" => {
            let result: Value = serde_json::from_str(required(&args, 2, "JSON result")?)
                .map_err(|error| format!("invalid JSON result: {error}"))?;
            print_value(call(&address, "respond_to_request", json!({ "sessionId": required(&args, 0, "session id")?, "requestId": required(&args, 1, "request id")?, "result": result })).await?);
        }
        "watch" => watch(&address, args.first().map(String::as_str)).await?,
        other => return Err(format!("unknown command '{other}' (run 'help' for usage)")),
    }
    Ok(())
}

async fn call(address: &str, method: &str, params: Value) -> Result<Value, String> {
    let mut stream = TcpStream::connect(address)
        .await
        .map_err(|error| format!("could not connect to {address}: {error}"))?;
    stream
        .write_all(format!("{}\n", json!({ "method": method, "params": params })).as_bytes())
        .await
        .map_err(|error| error.to_string())?;
    let mut lines = BufReader::new(stream).lines();
    let line = lines
        .next_line()
        .await
        .map_err(|error| error.to_string())?
        .ok_or("daemon closed the connection")?;
    response_result(&line)
}

async fn watch(address: &str, session_id: Option<&str>) -> Result<(), String> {
    let mut stream = TcpStream::connect(address)
        .await
        .map_err(|error| format!("could not connect to {address}: {error}"))?;
    stream.write_all(format!("{}\n", json!({ "method": "subscribe_events", "params": session_id.map(|id| json!({ "sessionId": id })).unwrap_or_else(|| json!({})) })).as_bytes()).await.map_err(|error| error.to_string())?;
    let mut lines = BufReader::new(stream).lines();
    let acknowledgement = lines
        .next_line()
        .await
        .map_err(|error| error.to_string())?
        .ok_or("daemon closed the event stream")?;
    response_result(&acknowledgement)?;
    while let Some(line) = lines.next_line().await.map_err(|error| error.to_string())? {
        print_value(serde_json::from_str(&line).map_err(|error| error.to_string())?);
    }
    Ok(())
}

fn response_result(line: &str) -> Result<Value, String> {
    let response: Value = serde_json::from_str(line).map_err(|error| error.to_string())?;
    if let Some(error) = response.get("error").and_then(Value::as_str) {
        return Err(error.to_owned());
    }
    Ok(response.get("result").cloned().unwrap_or(Value::Null))
}

fn required<'a>(args: &'a [String], index: usize, description: &str) -> Result<&'a str, String> {
    args.get(index)
        .map(String::as_str)
        .ok_or_else(|| format!("{description} is required"))
}
fn take_option(args: &mut Vec<String>, name: &str) -> Option<String> {
    let index = args.iter().position(|argument| argument == name)?;
    if index + 1 >= args.len() {
        return None;
    }
    args.remove(index);
    Some(args.remove(index))
}
fn daemon_addr() -> String {
    env::var("ACP_WORKBENCH_DAEMON_ADDR").unwrap_or_else(|_| "127.0.0.1:43821".into())
}
fn print_value(value: Value) {
    println!(
        "{}",
        serde_json::to_string_pretty(&value).expect("JSON values are serializable")
    );
}
fn print_usage() {
    println!("ACP Workbench daemon test CLI\n\nUsage: cargo run -p acp-workbench-daemon --bin daemon-test-cli -- [--address HOST:PORT] <command>\n\nCommands:\n  health                         Check daemon availability\n  agents                         List installed agent definitions\n  sessions                       List persisted sessions and live status\n  events <session-id>            Print persisted events\n  start <workspace> <agent-id>   Start a configured agent\n  prompt <session-id> <text>     Send a prompt\n  cancel <session-id>            Cancel the current turn\n  respond <session> <request> <json>  Respond to an ACP request\n  watch [session-id]             Stream live daemon events\n");
}
