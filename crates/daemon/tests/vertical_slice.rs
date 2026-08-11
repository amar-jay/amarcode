//! End-to-end vertical slice:
//!
//! ```text
//! create_chat → prompt → mock ACP → store → EditorEvent → subscribe_events
//! ```
//!
//! Proves store-first ordering for a real TCP client against a live daemon
//! and a mock agent binary (`mock-acp-agent`).

use std::{
    path::PathBuf,
    process::{Command, Stdio},
    time::Duration,
};

use serde_json::{json, Value};
use tokio::{
    io::{AsyncBufReadExt, AsyncWriteExt, BufReader},
    net::TcpStream,
    time::timeout,
};

const AGENT_ID: &str = "mock-acp";

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn create_chat_prompt_store_and_events() {
    let mock_agent = env_bin("mock-acp-agent");
    let daemon_bin = env_bin("amarcode-daemon");

    let app_dir = std::env::temp_dir().join(format!("amarcode-slice-{}", uuid_simple()));
    std::fs::create_dir_all(&app_dir).expect("app dir");
    let workspace = app_dir.join("workspace");
    std::fs::create_dir_all(&workspace).expect("workspace");

    // Free port chosen by binding to port 0 in a throwaway socket.
    let addr = {
        let listener = std::net::TcpListener::bind("127.0.0.1:0").expect("bind free port");
        let port = listener.local_addr().unwrap().port();
        drop(listener);
        format!("127.0.0.1:{port}")
    };

    let mut daemon = Command::new(&daemon_bin)
        .env("AMARCODE_APPDIR", &app_dir)
        .env("AMARCODE_DAEMON_ADDR", &addr)
        .env("AMARCODE_LOG", "amarcode_daemon=info")
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .spawn()
        .expect("spawn daemon");

    // Wait until TCP accepts.
    wait_for_tcp(&addr, Duration::from_secs(5))
        .await
        .expect("daemon did not start");

    // Register mock agent (command = built mock binary).
    let now = chrono_now();
    let health = rpc(
        &addr,
        // No create_agent RPC — use list_agents then we need another path.
        // We'll open the store file directly to insert the agent.
        json!({"method": "health"}),
    )
    .await
    .expect("health");
    assert_eq!(
        health.pointer("/result/protocol_version").and_then(Value::as_u64),
        Some(u64::from(amarcode_protocol::PROTOCOL_VERSION)),
        "health must advertise the shared protocol version"
    );

    insert_mock_agent(&app_dir, &mock_agent, &now);

    // Subscriber connection (stays open for events).
    let mut sub = TcpStream::connect(&addr).await.expect("subscribe connect");
    write_rpc(
        &mut sub,
        json!({"method": "subscribe_events", "params": {}}),
    )
    .await;
    let sub_ack = read_rpc(&mut sub).await.expect("subscribe ack");
    assert_eq!(
        sub_ack
            .pointer("/result/subscribed")
            .and_then(|v| v.as_bool()),
        Some(true),
        "subscribe ack: {sub_ack}"
    );

    let (event_tx, mut event_rx) = tokio::sync::mpsc::unbounded_channel::<Value>();
    let sub_task = tokio::spawn(async move {
        let mut lines = BufReader::new(sub).lines();
        while let Ok(Some(line)) = lines.next_line().await {
            if line.trim().is_empty() {
                continue;
            }
            if let Ok(v) = serde_json::from_str::<Value>(&line) {
                let _ = event_tx.send(v);
            }
        }
    });

    // Command connection: create chat + prompt.
    let create = rpc(
        &addr,
        json!({
            "method": "create_chat",
            "params": {
                "workspace_path": workspace.to_string_lossy(),
                "title": "slice"
            }
        }),
    )
    .await
    .expect("create_chat");
    let chat_id = create["result"]["id"].as_str().expect("chat id").to_owned();

    let prompt = rpc(
        &addr,
        json!({
            "method": "prompt",
            "params": {
                "chat_id": chat_id,
                "agent_id": AGENT_ID,
                "text": "hello mock",
                "session_mode": "build"
            }
        }),
    )
    .await
    .expect("prompt");

    assert!(
        prompt.get("result").is_some(),
        "prompt should succeed with mock agent: {prompt}"
    );
    let run_id = prompt["result"]["run_id"]
        .as_str()
        .expect("run_id")
        .to_owned();
    let user_message_id = prompt["result"]["user_message_id"]
        .as_str()
        .expect("user_message_id")
        .to_owned();

    // Collect events for a short window (store-first means events trail ACP).
    let mut saw_chat_updated = false;
    let mut saw_message_updated = false;
    let mut saw_run_updated = false;
    let mut saw_turn_started = false;
    let mut saw_turn_completed = false;
    let deadline = tokio::time::Instant::now() + Duration::from_secs(3);
    while tokio::time::Instant::now() < deadline {
        match timeout(Duration::from_millis(200), event_rx.recv()).await {
            Ok(Some(line)) => {
                let event_type = line
                    .pointer("/event/type")
                    .and_then(|v| v.as_str())
                    .unwrap_or("");
                match event_type {
                    "chatUpdated" => saw_chat_updated = true,
                    "messageUpdated" => saw_message_updated = true,
                    "runUpdated" => saw_run_updated = true,
                    "turnUpdated" => {
                        let status = line
                            .pointer("/event/payload/status")
                            .and_then(|v| v.as_str())
                            .unwrap_or("");
                        if status == "started" {
                            saw_turn_started = true;
                        }
                        if status == "completed" {
                            saw_turn_completed = true;
                        }
                    }
                    _ => {}
                }
            }
            Ok(None) => break,
            Err(_) => continue,
        }
        if saw_turn_started && saw_turn_completed && (saw_message_updated || saw_run_updated) {
            break;
        }
    }

    assert!(
        saw_chat_updated || saw_message_updated || saw_run_updated || saw_turn_completed,
        "expected at least one EditorEvent on subscribe socket"
    );
    assert!(
        saw_turn_started,
        "expected turnUpdated started after prompt begins"
    );
    assert!(
        saw_turn_completed,
        "expected turnUpdated completed when the agent returns stopReason"
    );

    // Store truth: reopen SQLite and check durable rows.
    let db_path = app_dir.join("workspace.sqlite3");
    let store = amarcode_daemon::store::Store::open(&db_path).expect("reopen store");

    // The ACP reader persists notifications on its worker thread. The prompt
    // response can be observed a moment before its final queued update.
    let messages = {
        let deadline = tokio::time::Instant::now() + Duration::from_secs(1);
        loop {
            let messages = store.messages(&chat_id).expect("messages");
            let assistant_count = messages
                .iter()
                .filter(|message| message.role == amarcode_daemon::protocol::MessageRole::Assistant)
                .count();
            if assistant_count >= 2 || tokio::time::Instant::now() >= deadline {
                break messages;
            }
            tokio::time::sleep(Duration::from_millis(10)).await;
        }
    };
    assert!(
        messages.iter().any(|m| m.id == user_message_id),
        "user message must be stored before/with prompt result"
    );
    assert!(
        messages
            .iter()
            .any(|m| m.role == amarcode_daemon::protocol::MessageRole::User),
        "user role message present"
    );
    // Mock streams assistant content → at least one assistant message row.
    assert!(
        messages
            .iter()
            .any(|m| m.role == amarcode_daemon::protocol::MessageRole::Assistant),
        "assistant message should be stored from ACP notifications: {messages:?}"
    );
    let assistant_messages: Vec<_> = messages
        .iter()
        .filter(|m| m.role == amarcode_daemon::protocol::MessageRole::Assistant)
        .collect();
    assert_eq!(
        assistant_messages.len(),
        2,
        "distinct ACP message ids must create distinct messages"
    );
    assert_eq!(assistant_messages[0].content, "mock progress.");
    assert_eq!(assistant_messages[1].content, "mock echo: hello mock");
    assert_eq!(
        messages
            .iter()
            .filter(|message| message.role == amarcode_daemon::protocol::MessageRole::User)
            .count(),
        1,
        "an ACP user_message_chunk echo must not create a duplicate message",
    );
    let commentary_parts = store
        .message_parts(&assistant_messages[0].id)
        .expect("commentary parts");
    assert!(
        commentary_parts
            .iter()
            .any(|part| part.kind == amarcode_daemon::protocol::MessagePartKind::ToolCall),
        "a tool call without an ACP message id should attach to commentary"
    );

    let acp_events = store.acp_events(&run_id).expect("acp_events");
    assert!(
        !acp_events.is_empty(),
        "raw ACP traffic must be logged (store-first log)"
    );
    assert!(
        acp_events.iter().any(|e| e.method.contains("prompt")
            || e.method == "agent.prompt"
            || e.method == "rpc.result"
            || e.method.starts_with("message.")),
        "expected prompt or message methods in acp_events: {:?}",
        acp_events.iter().map(|e| &e.method).collect::<Vec<_>>()
    );
    assert!(
        acp_events
            .iter()
            .any(|event| event.method == "session/set_config_option"),
        "a non-Codex agent's advertised mode should be configured: {:?}",
        acp_events
            .iter()
            .map(|event| &event.method)
            .collect::<Vec<_>>()
    );

    let run = store
        .get_run(&run_id)
        .expect("get_run")
        .expect("run exists");
    assert_eq!(run.chat_id, chat_id);
    assert_eq!(run.agent_id, AGENT_ID);
    assert!(
        run.acp_session_id.is_some(),
        "createSession should have set acp_session_id"
    );

    // Cleanup
    sub_task.abort();
    let _ = daemon.kill();
    let _ = daemon.wait();
    let _ = std::fs::remove_dir_all(&app_dir);
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn concurrent_prompts_share_one_chat_run() {
    let mock_agent = env_bin("mock-acp-agent");
    let daemon_bin = env_bin("amarcode-daemon");

    let app_dir = std::env::temp_dir().join(format!("amarcode-concurrent-{}", uuid_simple()));
    std::fs::create_dir_all(&app_dir).expect("app dir");
    let workspace = app_dir.join("workspace");
    std::fs::create_dir_all(&workspace).expect("workspace");

    let addr = {
        let listener = std::net::TcpListener::bind("127.0.0.1:0").expect("bind free port");
        let port = listener.local_addr().expect("free port address").port();
        drop(listener);
        format!("127.0.0.1:{port}")
    };

    let mut daemon = Command::new(&daemon_bin)
        .env("AMARCODE_APPDIR", &app_dir)
        .env("AMARCODE_DAEMON_ADDR", &addr)
        .env("AMARCODE_LOG", "amarcode_daemon=info")
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .spawn()
        .expect("spawn daemon");

    wait_for_tcp(&addr, Duration::from_secs(5))
        .await
        .expect("daemon did not start");
    rpc(&addr, json!({"method": "health"}))
        .await
        .expect("health");
    insert_mock_agent_with_environment(
        &app_dir,
        &mock_agent,
        &chrono_now(),
        vec![("AMARCODE_MOCK_INITIALIZE_DELAY_MS".into(), "250".into())],
    );

    let create = rpc(
        &addr,
        json!({
            "method": "create_chat",
            "params": {
                "workspace_path": workspace.to_string_lossy(),
                "title": "concurrent prompts"
            }
        }),
    )
    .await
    .expect("create_chat");
    let chat_id = create["result"]["id"].as_str().expect("chat id").to_owned();

    let first = rpc(
        &addr,
        json!({
            "method": "prompt",
            "params": { "chat_id": chat_id, "agent_id": AGENT_ID, "text": "first" }
        }),
    );
    let second = rpc(
        &addr,
        json!({
            "method": "prompt",
            "params": { "chat_id": chat_id, "agent_id": AGENT_ID, "text": "second" }
        }),
    );
    let (first, second) = tokio::join!(first, second);
    let first = first.expect("first prompt");
    let second = second.expect("second prompt");
    assert!(
        first.get("result").is_some(),
        "first prompt failed: {first}"
    );
    assert!(
        second.get("result").is_some(),
        "second prompt failed: {second}"
    );
    assert_eq!(
        first.pointer("/result/run_id"),
        second.pointer("/result/run_id"),
        "serialized prompts should reuse the same live run"
    );

    let store = amarcode_daemon::store::Store::open(&app_dir.join("workspace.sqlite3"))
        .expect("reopen store");
    let runs = store.list_runs_for_chat(&chat_id).expect("list chat runs");
    assert_eq!(runs.len(), 1, "concurrent prompts created orphan runs");
    let user_messages = store
        .messages(&chat_id)
        .expect("chat messages")
        .into_iter()
        .filter(|message| message.role == amarcode_daemon::protocol::MessageRole::User)
        .count();
    assert_eq!(user_messages, 2, "both prompts must be stored exactly once");

    let _ = daemon.kill();
    let _ = daemon.wait();
    let _ = std::fs::remove_dir_all(&app_dir);
}

/// Insert mock agent into the live daemon DB (no create_agent RPC yet).
fn insert_mock_agent(app_dir: &std::path::Path, mock_agent: &std::path::Path, now: &str) {
    insert_mock_agent_with_environment(app_dir, mock_agent, now, vec![]);
}

fn insert_mock_agent_with_environment(
    app_dir: &std::path::Path,
    mock_agent: &std::path::Path,
    now: &str,
    environment: Vec<(String, String)>,
) {
    // Daemon may still be writing; open with short retry.
    let db_path = app_dir.join("workspace.sqlite3");
    for _ in 0..20 {
        if db_path.exists() {
            break;
        }
        std::thread::sleep(Duration::from_millis(50));
    }
    let store = amarcode_daemon::store::Store::open(&db_path).expect("open store for agent insert");
    store
        .save_agent(&amarcode_daemon::store::AgentDefinition {
            id: AGENT_ID.into(),
            name: "Mock ACP".into(),
            command: mock_agent.to_string_lossy().into_owned(),
            arguments: vec![],
            environment,
            is_preset: false,
            created_at: now.into(),
            updated_at: now.into(),
        })
        .expect("save mock agent");
}

async fn wait_for_tcp(addr: &str, overall: Duration) -> Result<(), String> {
    let start = std::time::Instant::now();
    loop {
        if TcpStream::connect(addr).await.is_ok() {
            return Ok(());
        }
        if start.elapsed() > overall {
            return Err(format!("timeout waiting for {addr}"));
        }
        tokio::time::sleep(Duration::from_millis(50)).await;
    }
}

async fn rpc(addr: &str, request: Value) -> Result<Value, String> {
    let mut stream = TcpStream::connect(addr)
        .await
        .map_err(|e| format!("connect: {e}"))?;
    write_rpc(&mut stream, request).await;
    read_rpc(&mut stream).await
}

async fn write_rpc(stream: &mut TcpStream, request: Value) {
    let mut line = serde_json::to_string(&request).expect("serialize");
    line.push('\n');
    stream.write_all(line.as_bytes()).await.expect("write rpc");
    stream.flush().await.expect("flush");
}

async fn read_rpc(stream: &mut TcpStream) -> Result<Value, String> {
    let mut lines = BufReader::new(stream).lines();
    let line = timeout(Duration::from_secs(10), lines.next_line())
        .await
        .map_err(|_| "read timeout".to_string())?
        .map_err(|e| format!("read: {e}"))?
        .ok_or_else(|| "eof".to_string())?;
    serde_json::from_str(&line).map_err(|e| format!("json: {e} ({line})"))
}

fn env_bin(name: &str) -> PathBuf {
    // Cargo normally sets CARGO_BIN_EXE_<name> (hyphens → underscores). Some
    // workspace layouts omit it; fall back to the built artifact path.
    let underscored = name.replace('-', "_");
    for key in [
        format!("CARGO_BIN_EXE_{underscored}"),
        format!("CARGO_BIN_EXE_{name}"),
    ] {
        if let Some(path) = std::env::var_os(&key) {
            return PathBuf::from(path);
        }
    }

    let manifest_dir = PathBuf::from(env!("CARGO_MANIFEST_DIR"));
    for candidate in [
        manifest_dir.join("../../target/debug").join(name),
        manifest_dir.join("target/debug").join(name),
    ] {
        if candidate.exists() {
            return candidate.canonicalize().unwrap_or(candidate);
        }
    }

    panic!(
        "could not find binary {name}; set CARGO_BIN_EXE_{underscored} or build with cargo test"
    );
}

fn uuid_simple() -> String {
    uuid::Uuid::new_v4().simple().to_string()
}

fn chrono_now() -> String {
    use std::time::{SystemTime, UNIX_EPOCH};
    let secs = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap()
        .as_secs();
    // RFC3339-ish; only used for agent row timestamps in the test insert.
    format!("2020-01-01T00:00:00Z+{secs}")
}
