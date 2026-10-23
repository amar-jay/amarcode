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
        // No create_agent RPC: use list_agents then we need another path.
        // We'll open the store file directly to insert the agent.
        json!({"method": "health"}),
    )
    .await
    .expect("health");
    assert_eq!(
        health
            .pointer("/result/protocol_version")
            .and_then(Value::as_u64),
        Some(u64::from(amarcode_protocol::PROTOCOL_VERSION)),
        "health must advertise the shared protocol version"
    );

    // Raw ACP traffic is opt-in. This test explicitly verifies the durable
    // ACP event log, so enable recording before starting the agent session.
    let daemon_config = rpc(
        &addr,
        json!({
            "method": "set_daemon_config",
            "params": {
                "store_acp_events": true,
                "acp_event_retention_days": 7
            }
        }),
    )
    .await
    .expect("enable ACP event recording");
    assert_eq!(
        daemon_config
            .pointer("/result/store_acp_events")
            .and_then(Value::as_bool),
        Some(true),
        "ACP event recording must be enabled for this test: {daemon_config}"
    );

    insert_mock_agent_with_environment(
        &app_dir,
        &mock_agent,
        &now,
        vec![
            ("AMARCODE_MOCK_IMAGE_SUPPORT".into(), "1".into()),
            ("AMARCODE_MOCK_COMMANDS".into(), "1".into()),
        ],
    );

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

    let discovery = rpc(&addr, json!({ "method": "discover_commands", "params": { "chat_id": chat_id, "agent_id": AGENT_ID } })).await.expect("discover commands");
    let discovered_run = discovery["result"]["run_id"]
        .as_str()
        .expect("discovery run")
        .to_owned();
    let initial_catalog = wait_for_command_catalog(&addr, &chat_id, 2).await;
    assert_eq!(
        initial_catalog["result"]["messages"],
        json!([]),
        "discovery must not send a prompt"
    );
    assert_eq!(
        initial_catalog["result"]["session_commands"]["commands"][0]["input"]["hint"],
        "task to plan"
    );
    let reused = rpc(&addr, json!({ "method": "discover_commands", "params": { "chat_id": chat_id, "agent_id": AGENT_ID } })).await.unwrap();
    assert_eq!(
        reused["result"]["run_id"], discovered_run,
        "discovery must reuse the session"
    );

    let prompt = rpc(
        &addr,
        json!({
            "method": "prompt",
            "params": {
                "chat_id": chat_id,
                "agent_id": AGENT_ID,
                "text": "hello mock",
                "attachments": [{
                    "filename": "pixel.png",
                    "mime_type": "image/png",
                    "data": "iVBORw0KGgoAAAANSUhEUgAAAAEAAAABCAQAAAC1HAwCAAAAC0lEQVR42mNk+A8AAQUBAScY42YAAAAASUVORK5CYII="
                }],
                "config_values": [{ "config_id": "mode", "value": { "type": "id", "value": "code" } }]
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

    assert_eq!(
        run_id, discovered_run,
        "command discovery must reuse the prompt's session"
    );
    let updated_catalog = wait_for_command_catalog(&addr, &chat_id, 1).await;
    assert_eq!(
        updated_catalog["result"]["session_commands"]["commands"][0]["name"], "review",
        "new advertisements replace the whole list"
    );

    let configured = rpc(
        &addr,
        json!({
            "method": "set_session_config_option",
            "params": {
                "chat_id": chat_id,
                "config_id": "brave_mode",
                "value": { "type": "boolean", "value": true }
            }
        }),
    )
    .await
    .expect("set_session_config_option");
    assert_eq!(
        configured["result"]["options"][1]["current_value"],
        json!(true),
        "boolean config should round-trip through ACP: {configured}"
    );

    // Collect events for a short window (store-first means events trail ACP).
    let mut saw_chat_updated = false;
    let mut saw_message_updated = false;
    let mut saw_run_updated = false;
    let mut saw_turn_started = false;
    let mut saw_turn_completed = false;
    let mut command_catalogs = Vec::new();
    let deadline = tokio::time::Instant::now() + Duration::from_secs(3);
    while tokio::time::Instant::now() < deadline {
        match timeout(Duration::from_millis(200), event_rx.recv()).await {
            Ok(Some(line)) => {
                let event_type = line
                    .pointer("/event/type")
                    .and_then(|v| v.as_str())
                    .unwrap_or("");
                match event_type {
                    "sessionCommandsUpdated" => command_catalogs
                        .push(line["event"]["payload"]["catalog"]["commands"].clone()),
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

    assert!(
        command_catalogs
            .iter()
            .any(|commands| commands[0]["name"] == "plan"),
        "initial command advertisements must reach event subscribers"
    );
    assert!(
        command_catalogs
            .iter()
            .any(|commands| commands[0]["name"] == "review"),
        "dynamic replacements must reach event subscribers"
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
    let user_parts = store
        .message_parts(&user_message_id)
        .expect("user message parts");
    assert!(
        user_parts
            .iter()
            .any(|part| part.kind == amarcode_daemon::protocol::MessagePartKind::Image),
        "pasted image metadata must be stored with the user message"
    );
    let attachment_id = user_parts
        .iter()
        .find(|part| part.kind == amarcode_daemon::protocol::MessagePartKind::Image)
        .and_then(|part| serde_json::from_str::<Value>(&part.content_json).ok())
        .and_then(|value| value["attachmentId"].as_str().map(str::to_owned))
        .expect("stored attachment id");
    let attachment = rpc(
        &addr,
        json!({
            "method": "get_attachment",
            "params": { "chat_id": chat_id, "attachment_id": attachment_id }
        }),
    )
    .await
    .expect("get_attachment");
    assert_eq!(attachment["result"]["media_type"], "image/png");
    assert!(
        attachment["result"]["data"]
            .as_str()
            .is_some_and(|data| !data.is_empty()),
        "stored attachment bytes must be available to the UI"
    );
    assert_eq!(
        std::fs::read_dir(app_dir.join("attachments").join(&chat_id))
            .expect("attachment directory")
            .count(),
        1,
        "the decoded image must be stored outside SQLite"
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
        acp_events.iter().any(|event| {
            event.method == "session/prompt"
                && serde_json::from_str::<Value>(&event.payload_json)
                    .ok()
                    .and_then(|payload| payload.pointer("/prompt").cloned())
                    .and_then(|prompt| prompt.as_array().cloned())
                    .is_some_and(|parts| parts.iter().any(|part| part["type"] == "image"))
        }),
        "ACP session/prompt must contain an image content block"
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

    let command = rpc(
        &addr,
        json!({ "method": "prompt", "params": {
        "chat_id": chat_id, "agent_id": AGENT_ID, "text": "/review src/main.rs"
    } }),
    )
    .await
    .expect("execute slash command");
    assert_eq!(command["result"]["run_id"], run_id);
    let cleared = wait_for_command_catalog(&addr, &chat_id, 0).await;
    assert!(
        cleared["result"]["messages"]
            .as_array()
            .unwrap()
            .iter()
            .any(|item| item["message"]["content"] == "mock echo: /review src/main.rs"),
        "slash command and arguments must reach ACP verbatim"
    );

    rpc(
        &addr,
        json!({ "method": "cancel", "params": { "chat_id": chat_id } }),
    )
    .await
    .unwrap();
    let stopped = rpc(
        &addr,
        json!({ "method": "get_chat", "params": { "chat_id": chat_id, "include_messages": true } }),
    )
    .await
    .unwrap();
    assert!(
        stopped["result"]["session_commands"].is_null(),
        "stopped catalogs must not be restored from storage"
    );
    let restored_command = rpc(
        &addr,
        json!({ "method": "prompt", "params": {
        "chat_id": chat_id, "agent_id": AGENT_ID, "text": "/plan next task"
    } }),
    )
    .await
    .unwrap();
    let restored_run = restored_command["result"]["run_id"].as_str().unwrap();
    assert_ne!(restored_run, run_id);
    let requests = store.acp_events(restored_run).unwrap();
    let command_payload = requests
        .iter()
        .filter(|event| event.method == "session/prompt")
        .filter_map(|event| serde_json::from_str::<Value>(&event.payload_json).ok())
        .find(|payload| payload.pointer("/prompt/0/text").is_some())
        .expect("recorded command prompt");
    assert_eq!(
        command_payload["prompt"][0]["text"], "/plan next task",
        "history must not hide the command prefix"
    );
    assert!(command_payload["prompt"][1]["text"]
        .as_str()
        .unwrap()
        .contains("hello mock"));
    let restored_history = command_payload["prompt"][1]["text"].as_str().unwrap();
    assert!(restored_history.contains("<message role=\"user\""));
    assert!(restored_history.contains("<message role=\"assistant\""));
    assert!(!restored_history.contains("/plan next task"));
    assert!(!restored_history.contains("<new-user-message>"));

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
            available: false,
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

/// Windows consoles may capture CRLF; preserve all other output for assertions.
fn terminal_output_text(value: &Value) -> String {
    value
        .as_str()
        .expect("terminal output must be a string")
        .replace("\r\n", "\n")
}

#[test]
fn terminal_output_comparison_accepts_lf_and_crlf() {
    for output in [
        "first output\nlast output\n",
        "first output\r\nlast output\r\n",
    ] {
        assert_eq!(
            terminal_output_text(&json!(output)),
            "first output\nlast output\n"
        );
    }
    assert_eq!(
        terminal_output_text(&json!("progress\rdone")),
        "progress\rdone"
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

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn client_terminals_stream_persist_and_survive_release_and_compaction() {
    let app_dir = std::env::temp_dir().join(format!("amarcode-terminal-slice-{}", uuid_simple()));
    let workspace = app_dir.join("workspace");
    std::fs::create_dir_all(&workspace).unwrap();
    let address = {
        let socket = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
        socket.local_addr().unwrap().to_string()
    };
    let child = Command::new(env_bin("amarcode-daemon"))
        .env("AMARCODE_APPDIR", &app_dir)
        .env("AMARCODE_DAEMON_ADDR", &address)
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .spawn()
        .unwrap();
    struct Cleanup {
        child: std::process::Child,
        path: PathBuf,
    }
    impl Drop for Cleanup {
        fn drop(&mut self) {
            let _ = self.child.kill();
            let _ = self.child.wait();
            let _ = std::fs::remove_dir_all(&self.path);
        }
    }
    let _cleanup = Cleanup {
        child,
        path: app_dir.clone(),
    };
    wait_for_tcp(&address, Duration::from_secs(5))
        .await
        .unwrap();
    rpc(&address, json!({"method":"health"})).await.unwrap();
    insert_mock_agent_with_environment(
        &app_dir,
        &env_bin("mock-acp-agent"),
        &chrono_now(),
        vec![("AMARCODE_MOCK_TERMINAL".into(), "1".into())],
    );
    let store = amarcode_daemon::store::Store::open(&app_dir.join("workspace.sqlite3")).unwrap();
    let mut sub = TcpStream::connect(&address).await.unwrap();
    write_rpc(&mut sub, json!({"method":"subscribe_events","params":{}})).await;
    read_rpc(&mut sub).await.unwrap();
    let mut events = BufReader::new(sub).lines();
    let created = rpc(&address, json!({"method":"create_chat","params":{"workspace_path":workspace.to_string_lossy(),"title":"terminal"}})).await.unwrap();
    let chat_id = created["result"]["id"].as_str().unwrap().to_owned();
    let prompt_addr = address.clone();
    let prompt_chat = chat_id.clone();
    let prompt = tokio::spawn(async move {
        rpc(&prompt_addr, json!({"method":"prompt","params":{"chat_id":prompt_chat,"agent_id":AGENT_ID,"text":"Run test command"}})).await
    });
    let mut approved = false;
    let mut saw_live_output = false;
    loop {
        let line = timeout(Duration::from_secs(10), events.next_line())
            .await
            .unwrap()
            .unwrap()
            .unwrap();
        let event: Value = serde_json::from_str(&line).unwrap();
        let event = &event["event"];
        if event["type"] == "approvalRequired" {
            assert!(
                !approved,
                "terminal RPCs must not become extra approval prompts"
            );
            let response = rpc(&address, json!({"method":"respond_permission","params":{"request_id":event["payload"]["request_id"],"result":{"allow":true}}})).await.unwrap();
            assert!(response.get("error").is_none(), "{response}");
            approved = true;
        }
        if event["type"] == "messagePartAdded" {
            let message = event["payload"]["message_id"].as_str().unwrap();
            for part in store.message_parts(message).unwrap() {
                let value: Value = serde_json::from_str(&part.content_json).unwrap();
                if let Some(output) = value
                    .pointer("/_meta/terminal_output/data")
                    .and_then(Value::as_str)
                {
                    saw_live_output |=
                        output.contains("first output") && !output.contains("last output");
                }
            }
        }
        if event["type"] == "turnUpdated" && event["payload"]["status"] == "completed" {
            break;
        }
    }
    let response = prompt.await.unwrap().unwrap();
    assert!(response.get("error").is_none(), "{response}");
    assert!(
        approved && saw_live_output,
        "command must have approval and visible output before exit"
    );
    let messages = store.messages(&chat_id).unwrap();
    let mut snapshots = Vec::new();
    for message in &messages {
        for part in store.message_parts(&message.id).unwrap() {
            let value: Value = serde_json::from_str(&part.content_json).unwrap();
            if value.pointer("/_meta/amarcode_terminal_snapshot").is_some() {
                snapshots.push(value);
            }
        }
    }
    assert_eq!(
        snapshots.len(),
        1,
        "output is coalesced, not appended indefinitely"
    );
    assert_eq!(snapshots[0]["_meta"]["terminal_exit"]["exit_code"], 0);
    assert_eq!(
        terminal_output_text(&snapshots[0]["_meta"]["terminal_output"]["data"]),
        "first output\nlast output\n"
    );
    let compact = rpc(
        &address,
        json!({"method":"get_chat","params":{"chat_id":chat_id,"include_tool_content":false}}),
    )
    .await
    .unwrap();
    let parts: Vec<Value> = compact["result"]["messages"]
        .as_array()
        .unwrap()
        .iter()
        .flat_map(|message| message["parts"].as_array().unwrap().iter())
        .map(|part| serde_json::from_str(part["content_json"].as_str().unwrap()).unwrap())
        .collect();
    let terminal = parts
        .iter()
        .find(|value| value.pointer("/_meta/terminal_info/terminal_id").is_some())
        .expect("compact terminal summary");
    assert_eq!(terminal["_deferred"], true);
    assert_eq!(terminal["_meta"]["terminal_exit"]["exit_code"], 0);
    assert!(terminal.pointer("/_meta/terminal_output/data").is_none());
    let ids: Vec<_> = messages.iter().map(|message| &message.id).collect();
    let full = rpc(
        &address,
        json!({"method":"get_message_parts","params":{"message_ids":ids}}),
    )
    .await
    .unwrap();
    assert!(
        full["result"].as_array().unwrap().iter().any(|part| {
            serde_json::from_str::<Value>(part["content_json"].as_str().unwrap())
                .unwrap()
                .pointer("/_meta/terminal_output/data")
                .map(terminal_output_text)
                .as_deref()
                == Some("first output\nlast output\n")
        }),
        "full output is recoverable after release"
    );
}

async fn wait_for_command_catalog(addr: &str, chat_id: &str, count: usize) -> Value {
    timeout(Duration::from_secs(3), async {
        loop {
            let detail = rpc(addr, json!({ "method": "get_chat", "params": { "chat_id": chat_id, "include_messages": true } })).await.unwrap();
            if detail.pointer("/result/session_commands/commands").and_then(Value::as_array).is_some_and(|commands| commands.len() == count) {
                return detail;
            }
            tokio::time::sleep(Duration::from_millis(10)).await;
        }
    }).await.expect("latest command catalog")
}
