//! CLI test client for the amarcode-daemon TCP JSON-line protocol.
//!
//! ```text
//! cargo run -p amarcode-daemon --bin daemon-test-cli -- health
//! cargo run -p amarcode-daemon --bin daemon-test-cli -- slice \
//!   --mock-agent ./target/debug/mock-acp-agent --text "hello"
//! ```

use std::{
    io::{self, Write},
    path::PathBuf,
    process::ExitCode,
    time::Duration,
};

use amarcode_daemon::{
    config::DEFAULT_DAEMON_ADDR,
    protocol::rpc::methods,
    store::{AgentDefinition, Store},
    Config,
};
use clap::{Parser, Subcommand};
use serde_json::{json, Value};
use tokio::{
    io::{AsyncBufReadExt, AsyncWriteExt, BufReader},
    net::TcpStream,
    time::timeout,
};

#[derive(Debug, Parser)]
#[command(
    name = "daemon-test-cli",
    about = "TCP client for amarcode-daemon (exercise the JSON-line RPC without the desktop app)",
    version
)]
struct Cli {
    /// Daemon address (`AMARCODE_DAEMON_ADDR` or default).
    #[arg(
        long,
        global = true,
        env = "AMARCODE_DAEMON_ADDR",
        default_value = DEFAULT_DAEMON_ADDR
    )]
    addr: String,

    #[command(subcommand)]
    command: Command,
}

#[derive(Debug, Subcommand)]
enum Command {
    /// Health check.
    Health,
    /// Package version from the daemon.
    Version,
    /// List agent definitions.
    #[command(visible_alias = "agents")]
    ListAgents,
    /// Create a chat in a workspace.
    CreateChat {
        #[arg(long, short = 'w')]
        workspace: PathBuf,
        #[arg(long, short = 't')]
        title: Option<String>,
    },
    /// List chats (optionally filtered by workspace).
    #[command(visible_alias = "chats")]
    ListChats {
        #[arg(long, short = 'w')]
        workspace: Option<PathBuf>,
    },
    /// Load a chat (messages included by default).
    GetChat {
        #[arg(long = "chat-id", short = 'c')]
        chat_id: String,
        /// Omit message history.
        #[arg(long = "no-messages")]
        no_messages: bool,
    },
    /// Send a prompt (starts a run if needed).
    Prompt {
        #[arg(long = "chat-id", short = 'c')]
        chat_id: String,
        #[arg(long, short = 'a')]
        agent: String,
        #[arg(long, short = 't')]
        text: String,
    },
    /// Cancel the live run for a chat.
    Cancel {
        #[arg(long = "chat-id", short = 'c')]
        chat_id: String,
    },
    /// Answer an ApprovalRequired request from the agent.
    RespondPermission {
        #[arg(long = "request-id", short = 'r')]
        request_id: String,
        /// JSON result payload (default: `{"allow":true}`).
        #[arg(long)]
        result: Option<String>,
        /// If set, answer with a JSON-RPC error instead.
        #[arg(long)]
        error: Option<String>,
        #[arg(long, default_value_t = -1)]
        code: i64,
    },
    /// Answer a QuestionRequired request from the agent.
    RespondInput {
        #[arg(long = "request-id", short = 'r')]
        request_id: String,
        /// JSON result payload (default: `{}`).
        #[arg(long)]
        result: Option<String>,
        #[arg(long)]
        error: Option<String>,
        #[arg(long, default_value_t = -1)]
        code: i64,
    },
    /// Stream live EditorEvent lines until Ctrl-C.
    Subscribe {
        #[arg(long = "chat-id", short = 'c')]
        chat_id: Option<String>,
        #[arg(long = "run-id")]
        run_id: Option<String>,
    },
    /// Raw RPC: method name + optional JSON params.
    Call {
        method: String,
        /// JSON params object (default: null / omitted).
        params: Option<String>,
    },
    /// Insert/update an agent row in the local SQLite DB (same AMARCODE_APPDIR as daemon).
    RegisterAgent {
        #[arg(long)]
        id: String,
        #[arg(long)]
        command: PathBuf,
        #[arg(long)]
        name: Option<String>,
    },
    /// Vertical slice: create_chat → subscribe → prompt → get_chat.
    Slice {
        #[arg(long, short = 'w', default_value = "/tmp/amarcode-slice-ws")]
        workspace: PathBuf,
        #[arg(long, short = 'a', default_value = "mock-acp")]
        agent: String,
        #[arg(long, short = 't', default_value = "hello from daemon-test-cli")]
        text: String,
        #[arg(long, default_value = "slice")]
        title: String,
        /// Register this executable as `--agent` before prompting (e.g. mock-acp-agent).
        #[arg(long = "mock-agent")]
        mock_agent: Option<PathBuf>,
        /// Seconds to wait for events after prompt.
        #[arg(long = "events-for", default_value_t = 2)]
        events_for: u64,
    },
    /// Interactive REPL.
    Repl,
}

#[tokio::main]
async fn main() -> ExitCode {
    if let Err(err) = run().await {
        eprintln!("error: {err}");
        return ExitCode::FAILURE;
    }
    ExitCode::SUCCESS
}

async fn run() -> Result<(), String> {
    let cli = Cli::parse();
    let addr = cli.addr.as_str();

    match cli.command {
        Command::Health => {
            print_response(rpc(addr, methods::HEALTH, Value::Null).await?);
        }
        Command::Version => {
            print_response(rpc(addr, methods::VERSION, Value::Null).await?);
        }
        Command::ListAgents => {
            print_response(rpc(addr, methods::LIST_AGENTS, Value::Null).await?);
        }
        Command::CreateChat { workspace, title } => {
            let mut params = json!({ "workspace_path": workspace });
            if let Some(title) = title {
                params
                    .as_object_mut()
                    .unwrap()
                    .insert("title".into(), json!(title));
            }
            print_response(rpc(addr, methods::CREATE_CHAT, params).await?);
        }
        Command::ListChats { workspace } => {
            let params = match workspace {
                Some(w) => json!({ "workspace_path": w }),
                None => json!({}),
            };
            print_response(rpc(addr, methods::LIST_CHATS, params).await?);
        }
        Command::GetChat {
            chat_id,
            no_messages,
        } => {
            print_response(
                rpc(
                    addr,
                    methods::GET_CHAT,
                    json!({
                        "chat_id": chat_id,
                        "include_messages": !no_messages,
                    }),
                )
                .await?,
            );
        }
        Command::Prompt {
            chat_id,
            agent,
            text,
        } => {
            print_response(
                rpc(
                    addr,
                    methods::PROMPT,
                    json!({
                        "chat_id": chat_id,
                        "agent_id": agent,
                        "text": text,
                    }),
                )
                .await?,
            );
        }
        Command::Cancel { chat_id } => {
            print_response(rpc(addr, methods::CANCEL, json!({ "chat_id": chat_id })).await?);
        }
        Command::RespondPermission {
            request_id,
            result,
            error,
            code,
        } => {
            print_response(
                respond_agent(
                    addr,
                    methods::RESPOND_PERMISSION,
                    request_id,
                    result,
                    error,
                    code,
                )
                .await?,
            );
        }
        Command::RespondInput {
            request_id,
            result,
            error,
            code,
        } => {
            print_response(
                respond_agent(
                    addr,
                    methods::RESPOND_INPUT,
                    request_id,
                    result,
                    error,
                    code,
                )
                .await?,
            );
        }
        Command::Subscribe { chat_id, run_id } => {
            let mut params = json!({});
            if let Some(chat_id) = chat_id {
                params
                    .as_object_mut()
                    .unwrap()
                    .insert("chat_id".into(), json!(chat_id));
            }
            if let Some(run_id) = run_id {
                params
                    .as_object_mut()
                    .unwrap()
                    .insert("run_id".into(), json!(run_id));
            }
            subscribe_loop(addr, params).await?;
        }
        Command::Call { method, params } => {
            let params = match params {
                Some(raw) => parse_json(&raw)?,
                None => Value::Null,
            };
            print_response(rpc(addr, &method, params).await?);
        }
        Command::RegisterAgent { id, command, name } => {
            let name = name.unwrap_or_else(|| id.clone());
            register_agent(&id, &name, &command)?;
            println!("registered agent {id} → {}", command.display());
        }
        Command::Slice {
            workspace,
            agent,
            text,
            title,
            mock_agent,
            events_for,
        } => {
            run_slice(addr, workspace, agent, text, title, mock_agent, events_for).await?;
        }
        Command::Repl => repl(addr).await?,
    }
    Ok(())
}

async fn respond_agent(
    addr: &str,
    method: &str,
    request_id: String,
    result: Option<String>,
    error: Option<String>,
    code: i64,
) -> Result<Value, String> {
    let params = if let Some(message) = error {
        json!({
            "request_id": request_id,
            "error": { "code": code, "message": message }
        })
    } else {
        let result = match result {
            Some(raw) => parse_json(&raw)?,
            None if method == methods::RESPOND_PERMISSION => json!({ "allow": true }),
            None => json!({}),
        };
        json!({
            "request_id": request_id,
            "result": result,
        })
    };
    rpc(addr, method, params).await
}

async fn run_slice(
    addr: &str,
    workspace: PathBuf,
    agent: String,
    text: String,
    title: String,
    mock_agent: Option<PathBuf>,
    events_for: u64,
) -> Result<(), String> {
    if let Some(cmd) = mock_agent {
        let resolved_cmd = resolve_existing_path(&cmd)?;
        register_agent(&agent, "Mock ACP (cli)", &resolved_cmd)?;
        println!("registered agent {agent} → {}", resolved_cmd.display());
    }

    std::fs::create_dir_all(&workspace)
        .map_err(|e| format!("create workspace {}: {e}", workspace.display()))?;

    println!("→ create_chat workspace={}", workspace.display());
    let create = rpc(
        addr,
        methods::CREATE_CHAT,
        json!({
            "workspace_path": workspace,
            "title": title,
        }),
    )
    .await?;
    print_response(create.clone());
    let chat_id = create
        .pointer("/result/id")
        .and_then(|v| v.as_str())
        .ok_or_else(|| format!("create_chat missing id: {create}"))?
        .to_owned();

    let sub_addr = addr.to_owned();
    let sub_chat = chat_id.clone();
    let sub_handle = tokio::spawn(async move {
        if let Err(err) = subscribe_for(
            &sub_addr,
            json!({ "chat_id": sub_chat }),
            Duration::from_secs(events_for + 5),
        )
        .await
        {
            eprintln!("subscribe: {err}");
        }
    });

    tokio::time::sleep(Duration::from_millis(150)).await;

    println!("→ prompt chat={chat_id} agent={agent}");
    let prompt = rpc(
        addr,
        methods::PROMPT,
        json!({
            "chat_id": chat_id,
            "agent_id": agent,
            "text": text,
        }),
    )
    .await?;
    print_response(prompt.clone());

    if prompt.get("error").is_some() {
        sub_handle.abort();
        return Err(format!(
            "prompt failed (is agent '{agent}' registered and executable?)"
        ));
    }

    println!("… waiting {events_for}s for events");
    tokio::time::sleep(Duration::from_secs(events_for)).await;
    sub_handle.abort();

    println!("→ get_chat");
    print_response(
        rpc(
            addr,
            methods::GET_CHAT,
            json!({ "chat_id": chat_id, "include_messages": true }),
        )
        .await?,
    );

    println!("slice complete. chat_id={chat_id}");
    Ok(())
}

async fn repl(addr: &str) -> Result<(), String> {
    println!("amarcode daemon-test-cli repl  (addr={addr})");
    println!("type help for commands, quit to exit");
    let stdin = io::stdin();
    loop {
        print!("rpc> ");
        let _ = io::stdout().flush();
        let mut line = String::new();
        if stdin.read_line(&mut line).map_err(|e| e.to_string())? == 0 {
            break;
        }
        let line = line.trim();
        if line.is_empty() {
            continue;
        }
        if matches!(line, "quit" | "exit" | ":q") {
            break;
        }
        if matches!(line, "help" | "?") {
            println!(
                "health | version | agents | chats [workspace] | create <workspace> [title]\n\
                 get <chat_id> | prompt <chat_id> <agent_id> <text...> | cancel <chat_id>\n\
                 subscribe | call <method> [json] | quit"
            );
            continue;
        }

        let parts = shell_split(line);
        if parts.is_empty() {
            continue;
        }

        let result = match parts[0].as_str() {
            "health" => rpc(addr, methods::HEALTH, Value::Null).await,
            "version" => rpc(addr, methods::VERSION, Value::Null).await,
            "agents" => rpc(addr, methods::LIST_AGENTS, Value::Null).await,
            "chats" => {
                let params = match parts.get(1) {
                    Some(w) => json!({ "workspace_path": w }),
                    None => json!({}),
                };
                rpc(addr, methods::LIST_CHATS, params).await
            }
            "create" => {
                let workspace = parts
                    .get(1)
                    .ok_or_else(|| "create <workspace> [title]".to_string())?;
                let mut params = json!({ "workspace_path": workspace });
                if let Some(title) = parts.get(2) {
                    params
                        .as_object_mut()
                        .unwrap()
                        .insert("title".into(), json!(title));
                }
                rpc(addr, methods::CREATE_CHAT, params).await
            }
            "get" => {
                let chat_id = parts.get(1).ok_or_else(|| "get <chat_id>".to_string())?;
                rpc(
                    addr,
                    methods::GET_CHAT,
                    json!({ "chat_id": chat_id, "include_messages": true }),
                )
                .await
            }
            "prompt" => {
                if parts.len() < 4 {
                    Err("prompt <chat_id> <agent_id> <text...>".into())
                } else {
                    let text = parts[3..].join(" ");
                    rpc(
                        addr,
                        methods::PROMPT,
                        json!({
                            "chat_id": parts[1],
                            "agent_id": parts[2],
                            "text": text,
                        }),
                    )
                    .await
                }
            }
            "cancel" => {
                let chat_id = parts.get(1).ok_or_else(|| "cancel <chat_id>".to_string())?;
                rpc(addr, methods::CANCEL, json!({ "chat_id": chat_id })).await
            }
            "subscribe" => {
                println!("(streaming; Ctrl-C to stop this process)");
                subscribe_loop(addr, json!({})).await?;
                continue;
            }
            "call" => {
                let method = parts
                    .get(1)
                    .ok_or_else(|| "call <method> [json]".to_string())?;
                let params = if parts.len() > 2 {
                    parse_json(&parts[2..].join(" "))?
                } else {
                    Value::Null
                };
                rpc(addr, method, params).await
            }
            other => Err(format!("unknown repl command: {other}")),
        };

        match result {
            Ok(v) => print_response(v),
            Err(err) => eprintln!("error: {err}"),
        }
    }
    Ok(())
}

// --- transport -------------------------------------------------------------

async fn rpc(addr: &str, method: &str, params: Value) -> Result<Value, String> {
    let mut stream = TcpStream::connect(addr)
        .await
        .map_err(|e| format!("connect {addr}: {e}"))?;
    let request = if params.is_null() {
        json!({ "method": method })
    } else {
        json!({ "method": method, "params": params })
    };
    write_line(&mut stream, &request).await?;
    let mut lines = BufReader::new(stream).lines();
    let line = timeout(Duration::from_secs(120), lines.next_line())
        .await
        .map_err(|_| "rpc response timeout".to_string())?
        .map_err(|e| format!("read: {e}"))?
        .ok_or_else(|| "connection closed before response".to_string())?;
    serde_json::from_str(&line).map_err(|e| format!("invalid json response: {e} ({line})"))
}

async fn write_line(stream: &mut TcpStream, value: &Value) -> Result<(), String> {
    let mut line = serde_json::to_string(value).map_err(|e| e.to_string())?;
    line.push('\n');
    stream
        .write_all(line.as_bytes())
        .await
        .map_err(|e| format!("write: {e}"))?;
    stream.flush().await.map_err(|e| format!("flush: {e}"))
}

async fn subscribe_loop(addr: &str, params: Value) -> Result<(), String> {
    println!("subscribing on {addr} … (Ctrl-C to quit)");
    let mut stream = TcpStream::connect(addr)
        .await
        .map_err(|e| format!("connect {addr}: {e}"))?;
    write_line(
        &mut stream,
        &json!({ "method": methods::SUBSCRIBE_EVENTS, "params": params }),
    )
    .await?;
    let mut lines = BufReader::new(stream).lines();
    while let Some(line) = lines.next_line().await.map_err(|e| format!("read: {e}"))? {
        if line.trim().is_empty() {
            continue;
        }
        match serde_json::from_str::<Value>(&line) {
            Ok(v) => print_response(v),
            Err(_) => println!("{line}"),
        }
    }
    Ok(())
}

async fn subscribe_for(addr: &str, params: Value, max: Duration) -> Result<(), String> {
    let mut stream = TcpStream::connect(addr)
        .await
        .map_err(|e| format!("connect {addr}: {e}"))?;
    write_line(
        &mut stream,
        &json!({ "method": methods::SUBSCRIBE_EVENTS, "params": params }),
    )
    .await?;
    let mut lines = BufReader::new(stream).lines();
    let deadline = tokio::time::Instant::now() + max;
    loop {
        let left = deadline.saturating_duration_since(tokio::time::Instant::now());
        if left.is_zero() {
            break;
        }
        match timeout(left, lines.next_line()).await {
            Ok(Ok(Some(line))) => {
                if line.trim().is_empty() {
                    continue;
                }
                match serde_json::from_str::<Value>(&line) {
                    Ok(v) => {
                        if v.pointer("/result/subscribed").is_some() {
                            eprintln!("(subscribed)");
                        } else {
                            print_response(v);
                        }
                    }
                    Err(_) => println!("{line}"),
                }
            }
            Ok(Ok(None)) => break,
            Ok(Err(err)) => return Err(format!("read: {err}")),
            Err(_) => break,
        }
    }
    Ok(())
}

fn register_agent(id: &str, name: &str, command: &std::path::Path) -> Result<(), String> {
    let config = Config::from_env().map_err(|e| e.to_string())?;
    let store = Store::open(&config.db_path).map_err(|e| e.to_string())?;
    let now = chrono::Utc::now().to_rfc3339();
    store
        .save_agent(&AgentDefinition {
            id: id.into(),
            name: name.into(),
            command: command.to_string_lossy().into_owned(),
            arguments: vec![],
            environment: vec![],
            is_preset: false,
            created_at: now.clone(),
            updated_at: now,
        })
        .map_err(|e| e.to_string())?;
    Ok(())
}

fn resolve_existing_path(path: &std::path::Path) -> Result<PathBuf, String> {
    std::fs::canonicalize(path).map_err(|error| format!("resolve {}: {error}", path.display()))
}

fn print_response(value: Value) {
    match serde_json::to_string_pretty(&value) {
        Ok(s) => println!("{s}"),
        Err(_) => println!("{value}"),
    }
}

fn parse_json(s: &str) -> Result<Value, String> {
    serde_json::from_str(s).map_err(|e| format!("invalid json: {e}"))
}

fn shell_split(line: &str) -> Vec<String> {
    let mut out = Vec::new();
    let mut cur = String::new();
    let mut in_quotes = false;
    for ch in line.chars() {
        match ch {
            '"' => in_quotes = !in_quotes,
            c if c.is_whitespace() && !in_quotes => {
                if !cur.is_empty() {
                    out.push(std::mem::take(&mut cur));
                }
            }
            c => cur.push(c),
        }
    }
    if !cur.is_empty() {
        out.push(cur);
    }
    out
}
