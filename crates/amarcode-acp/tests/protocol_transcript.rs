use std::{
    fs,
    io::{BufRead, BufReader, Read, Write},
    net::{TcpListener, TcpStream},
    path::PathBuf,
    process::{Child, ChildStdin, Command, Stdio},
    sync::mpsc::{self, Receiver},
    thread,
    time::Duration,
};

use serde_json::{json, Value};
use uuid::Uuid;

const TIMEOUT: Duration = Duration::from_secs(5);

fn read_http_request(socket: &mut TcpStream) -> Vec<u8> {
    socket
        .set_read_timeout(Some(TIMEOUT))
        .expect("set read timeout");
    let mut buffer = Vec::new();
    let mut chunk = [0_u8; 4096];
    loop {
        let size = socket.read(&mut chunk).expect("read HTTP request");
        if size == 0 {
            break;
        }
        buffer.extend_from_slice(&chunk[..size]);
        let Some(header_end) = buffer.windows(4).position(|window| window == b"\r\n\r\n") else {
            continue;
        };
        let headers = std::str::from_utf8(&buffer[..header_end]).unwrap_or("");
        let content_length = headers
            .lines()
            .find_map(|line| {
                let (name, value) = line.split_once(':')?;
                name.eq_ignore_ascii_case("content-length")
                    .then(|| value.trim().parse::<usize>().ok())
                    .flatten()
            })
            .unwrap_or(0);
        if buffer.len() >= header_end + 4 + content_length {
            break;
        }
    }
    buffer
}

// Existing completion fixtures do not implement model discovery.
fn accept_chat(listener: &TcpListener) -> std::io::Result<(TcpStream, std::net::SocketAddr)> {
    loop {
        let (mut socket, address) = listener.accept()?;
        socket.set_read_timeout(Some(TIMEOUT))?;
        let mut method = [0; 4];
        let size = socket.peek(&mut method)?;
        if size >= 4 && &method == b"GET " {
            let request = read_http_request(&mut socket);
            assert!(request.starts_with(b"GET /v1/models "));
            write_json_response(&mut socket, "404 Not Found", "", json!({}));
            continue;
        }
        return Ok((socket, address));
    }
}

fn write_sse(socket: &mut impl Write, body: &str) {
    write!(
        socket,
        "HTTP/1.1 200 OK\r\nContent-Type: text/event-stream\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{body}",
        body.len()
    )
    .expect("write provider response");
    socket.flush().expect("flush provider response");
}

struct AgentProcess {
    child: Child,
    stdin: ChildStdin,
    output: Receiver<Value>,
    config_path: PathBuf,
    session_store_path: PathBuf,
}

impl AgentProcess {
    fn spawn(base_url: &str) -> Self {
        Self::spawn_with_options(base_url, json!({}))
    }

    fn spawn_with_options(base_url: &str, options: Value) -> Self {
        let config_path =
            std::env::temp_dir().join(format!("amarcode-acp-transcript-{}.json", Uuid::new_v4()));
        let mut config = json!({
            "name": "transcript-agent",
            "provider": { "base_url": base_url, "api_key": "test-key", "model": "test-model" }
        });
        for (key, value) in options.as_object().expect("config options object") {
            config[key] = value.clone();
        }
        fs::write(
            &config_path,
            serde_json::to_vec(&config).expect("serialize config"),
        )
        .expect("write config");

        let mut child = Command::new(env!("CARGO_BIN_EXE_amarcode-acp"))
            .arg("--config")
            .arg(&config_path)
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::piped())
            .env("NO_PROXY", "127.0.0.1,localhost")
            .env("no_proxy", "127.0.0.1,localhost")
            .spawn()
            .expect("spawn amarcode-acp");
        let stdin = child.stdin.take().expect("child stdin");
        let stdout = child.stdout.take().expect("child stdout");
        let (sender, output) = mpsc::channel();
        thread::spawn(move || {
            for line in BufReader::new(stdout).lines().map_while(Result::ok) {
                if let Ok(value) = serde_json::from_str(&line) {
                    let _ = sender.send(value);
                }
            }
        });
        let session_store_path = config_path.with_extension("sessions.json");
        Self {
            child,
            stdin,
            output,
            config_path,
            session_store_path,
        }
    }

    fn send(&mut self, message: Value) {
        serde_json::to_writer(&mut self.stdin, &message).expect("write request");
        self.stdin.write_all(b"\n").expect("write newline");
        self.stdin.flush().expect("flush request");
    }

    fn response(&self, id: u64) -> Value {
        self.response_with_messages(id).0
    }

    fn response_with_messages(&self, id: u64) -> (Value, Vec<Value>) {
        let mut messages = Vec::new();
        loop {
            let message = self.output.recv_timeout(TIMEOUT).expect("ACP response");
            if message.get("id").and_then(Value::as_u64) == Some(id) {
                return (message, messages);
            }
            messages.push(message);
        }
    }
}

impl Drop for AgentProcess {
    fn drop(&mut self) {
        let _ = self.child.kill();
        let _ = self.child.wait();
        let _ = fs::remove_file(&self.config_path);
        let _ = fs::remove_file(&self.session_store_path);
    }
}

fn initialize(agent: &mut AgentProcess) -> Value {
    agent.send(json!({
        "jsonrpc": "2.0",
        "id": 1,
        "method": "initialize",
        "params": { "protocolVersion": 1, "clientCapabilities": {} }
    }));
    agent.response(1)
}

fn new_session(agent: &mut AgentProcess, id: u64, cwd: &str) -> String {
    agent.send(json!({
        "jsonrpc": "2.0",
        "id": id,
        "method": "session/new",
        "params": { "cwd": cwd, "mcpServers": [] }
    }));
    agent
        .response(id)
        .pointer("/result/sessionId")
        .and_then(Value::as_str)
        .expect("session id")
        .to_owned()
}

#[test]
fn transcript_advertises_truthful_capabilities_and_routes_by_session_id() {
    let mut agent = AgentProcess::spawn("http://127.0.0.1:9/v1");
    let initialized = initialize(&mut agent);
    let result = &initialized["result"];
    assert_eq!(result["protocolVersion"], 1);
    assert_eq!(result["agentInfo"]["name"], "transcript-agent");
    assert_eq!(result["authMethods"], json!([]));
    assert_eq!(result["agentCapabilities"]["loadSession"], true);
    assert_eq!(
        result["agentCapabilities"]["sessionCapabilities"],
        json!({ "list": {}, "delete": {}, "resume": {}, "close": {} })
    );

    let first = new_session(&mut agent, 2, "/same-workspace");
    let second = new_session(&mut agent, 3, "/same-workspace");
    assert_ne!(first, second);
    Uuid::parse_str(&first).expect("first UUID session id");
    Uuid::parse_str(&second).expect("second UUID session id");
}

#[test]
fn transcript_lists_resumes_and_deletes_persisted_sessions() {
    let mut agent = AgentProcess::spawn("http://127.0.0.1:9/v1");
    initialize(&mut agent);
    let session_id = new_session(&mut agent, 2, "/persistent-workspace");
    agent.send(json!({
        "jsonrpc": "2.0",
        "id": 4,
        "method": "session/list",
        "params": { "cwd": "/persistent-workspace" }
    }));
    let listed = agent.response(4);
    assert_eq!(listed["result"]["sessions"][0]["sessionId"], session_id);

    agent.send(json!({
        "jsonrpc": "2.0",
        "id": 5,
        "method": "session/close",
        "params": { "sessionId": session_id }
    }));
    agent.response(5);
    agent.send(json!({
        "jsonrpc": "2.0",
        "id": 6,
        "method": "session/resume",
        "params": {
            "sessionId": session_id,
            "cwd": "/persistent-workspace",
            "mcpServers": []
        }
    }));
    let resumed = agent.response(6);
    assert_eq!(
        resumed["result"]["configOptions"][0]["currentValue"],
        "test-model"
    );

    agent.send(json!({
        "jsonrpc": "2.0",
        "id": 7,
        "method": "session/delete",
        "params": { "sessionId": session_id }
    }));
    agent.response(7);
    agent.send(json!({
        "jsonrpc": "2.0",
        "id": 8,
        "method": "session/list",
        "params": {}
    }));
    assert_eq!(agent.response(8)["result"]["sessions"], json!([]));
}

#[test]
fn transcript_cancels_an_in_flight_provider_stream() {
    let listener = match TcpListener::bind("127.0.0.1:0") {
        Ok(listener) => listener,
        Err(error) if error.kind() == std::io::ErrorKind::PermissionDenied => {
            eprintln!("skipping socket transcript test: sandbox forbids loopback listeners");
            return;
        }
        Err(error) => panic!("bind provider: {error}"),
    };
    let address = listener.local_addr().expect("provider address");
    let (streaming_sender, streaming_receiver) = mpsc::channel();
    thread::spawn(move || {
        let (mut socket, _) = accept_chat(&listener).expect("provider connection");
        socket
            .set_read_timeout(Some(TIMEOUT))
            .expect("set read timeout");
        let _ = read_http_request(&mut socket);
        socket
            .write_all(
                b"HTTP/1.1 200 OK\r\nContent-Type: text/event-stream\r\nConnection: close\r\n\r\n",
            )
            .expect("write streaming headers");
        socket.flush().expect("flush streaming headers");
        streaming_sender.send(()).expect("signal open stream");
        thread::sleep(TIMEOUT);
    });

    let mut agent = AgentProcess::spawn(&format!("http://{address}/v1"));
    initialize(&mut agent);
    let session_id = new_session(&mut agent, 2, "/workspace");
    agent.send(json!({
        "jsonrpc": "2.0",
        "id": 3,
        "method": "session/prompt",
        "params": {
            "sessionId": session_id,
            "prompt": [{ "type": "text", "text": "wait forever" }]
        }
    }));
    streaming_receiver
        .recv_timeout(TIMEOUT)
        .expect("provider stream to open before cancellation");
    agent.send(json!({
        "jsonrpc": "2.0",
        "method": "session/cancel",
        "params": { "sessionId": session_id }
    }));

    let response = agent.response(3);
    assert_eq!(response["result"]["stopReason"], "cancelled");
}

#[test]
fn transcript_streams_a_uuid_message_id() {
    let listener = match TcpListener::bind("127.0.0.1:0") {
        Ok(listener) => listener,
        Err(error) if error.kind() == std::io::ErrorKind::PermissionDenied => {
            eprintln!("skipping socket transcript test: sandbox forbids loopback listeners");
            return;
        }
        Err(error) => panic!("bind provider: {error}"),
    };
    let address = listener.local_addr().expect("provider address");
    thread::spawn(move || {
        let (mut socket, _) = accept_chat(&listener).expect("provider connection");
        let _ = read_http_request(&mut socket);
        write_sse(
            &mut socket,
            "data: {\"choices\":[{\"delta\":{\"content\":\"hello\"}}]}\n\ndata: [DONE]\n\n",
        );
    });

    let mut agent = AgentProcess::spawn(&format!("http://{address}/v1"));
    initialize(&mut agent);
    let session_id = new_session(&mut agent, 2, "/workspace");
    agent.send(json!({
        "jsonrpc": "2.0",
        "id": 3,
        "method": "session/prompt",
        "params": {
            "sessionId": session_id,
            "prompt": [{ "type": "text", "text": "say hello" }]
        }
    }));

    let (response, messages) = agent.response_with_messages(3);
    assert_eq!(response["result"]["stopReason"], "end_turn");
    let update = messages
        .iter()
        .find(|message| message["method"] == "session/update")
        .expect("streamed session update");
    assert_eq!(update["params"]["sessionId"], session_id);
    assert_eq!(update["params"]["update"]["content"]["text"], "hello");
    let message_id = update["params"]["update"]["messageId"]
        .as_str()
        .expect("message id");
    Uuid::parse_str(message_id).expect("UUID message id");
}

#[test]
fn transcript_streams_reasoning_as_a_think_tool() {
    let listener = match TcpListener::bind("127.0.0.1:0") {
        Ok(listener) => listener,
        Err(error) if error.kind() == std::io::ErrorKind::PermissionDenied => {
            eprintln!("skipping socket transcript test: sandbox forbids loopback listeners");
            return;
        }
        Err(error) => panic!("bind provider: {error}"),
    };
    let address = listener.local_addr().expect("provider address");
    thread::spawn(move || {
        let (mut socket, _) = accept_chat(&listener).expect("provider connection");
        let _ = read_http_request(&mut socket);
        write_sse(
            &mut socket,
            concat!(
                "data: {\"choices\":[{\"delta\":{\"reasoning\":\"first \"}}]}\n\n",
                "data: {\"choices\":[{\"delta\":{\"reasoning\":\"second\"}}]}\n\n",
                "data: {\"choices\":[{\"delta\":{\"content\":\"answer\"}}]}\n\n",
                "data: [DONE]\n\n"
            ),
        );
    });

    let mut agent = AgentProcess::spawn(&format!("http://{address}/v1"));
    initialize(&mut agent);
    let session_id = new_session(&mut agent, 2, "/workspace");
    agent.send(json!({
        "jsonrpc": "2.0",
        "id": 3,
        "method": "session/prompt",
        "params": {
            "sessionId": session_id,
            "prompt": [{ "type": "text", "text": "think first" }]
        }
    }));

    let (response, messages) = agent.response_with_messages(3);
    assert_eq!(response["result"]["stopReason"], "end_turn");
    let updates = messages
        .iter()
        .filter_map(|message| message.pointer("/params/update"))
        .collect::<Vec<_>>();
    let started = updates
        .iter()
        .find(|update| update["sessionUpdate"] == "tool_call")
        .expect("think tool start");
    assert_eq!(started["kind"], "think");
    assert_eq!(started["status"], "in_progress");
    assert_eq!(started["title"], "Thinking");
    assert_eq!(started["content"][0]["content"]["text"], "first ");

    let think_id = started["toolCallId"].clone();
    let think_updates = updates
        .iter()
        .filter(|update| {
            update["sessionUpdate"] == "tool_call_update" && update["toolCallId"] == think_id
        })
        .collect::<Vec<_>>();
    assert_eq!(think_updates.len(), 2);
    assert_eq!(
        think_updates[0]["content"][0]["content"]["text"],
        "first second"
    );
    assert_eq!(think_updates[1]["status"], "completed");
    assert_eq!(
        think_updates[1]["content"][0]["content"]["text"],
        "first second"
    );
    assert!(updates
        .iter()
        .all(|update| update["sessionUpdate"] != "agent_thought_chunk"));
}

#[test]
fn transcript_executes_tool_and_returns_result_to_model() {
    let listener = match TcpListener::bind("127.0.0.1:0") {
        Ok(listener) => listener,
        Err(error) if error.kind() == std::io::ErrorKind::PermissionDenied => {
            eprintln!("skipping socket transcript test: sandbox forbids loopback listeners");
            return;
        }
        Err(error) => panic!("bind provider: {error}"),
    };
    let address = listener.local_addr().expect("provider address");
    let (request_sender, request_receiver) = mpsc::channel();
    thread::spawn(move || {
        for body in [
            "data: {\"choices\":[{\"delta\":{\"reasoning_details\":[{\"type\":\"reasoning.text\",\"text\":\"I should inspect the file.\",\"id\":\"reasoning-1\",\"index\":0}]}}]}\n\ndata: {\"choices\":[{\"delta\":{\"tool_calls\":[{\"index\":0,\"id\":\"call-read\",\"type\":\"function\",\"function\":{\"name\":\"run_command\",\"arguments\":\"{\\\"command\\\":\\\"/bin/cat\\\",\\\"args\\\":[\\\"hello.txt\\\"],\\\"cwd\\\":\\\".\\\"}\"}}]}}]}\n\ndata: [DONE]\n\n",
            "data: {\"choices\":[{\"delta\":{\"content\":\"I read the file.\"}}]}\n\ndata: [DONE]\n\n",
        ] {
            let (mut socket, _) = accept_chat(&listener).expect("provider connection");
            let request = read_http_request(&mut socket);
            request_sender
                .send(String::from_utf8_lossy(&request).into_owned())
                .expect("capture request");
            write_sse(&mut socket, body);
        }
    });

    let workspace = std::env::temp_dir().join(format!("amarcode-tools-{}", Uuid::new_v4()));
    fs::create_dir(&workspace).expect("create workspace");

    let mut agent = AgentProcess::spawn(&format!("http://{address}/v1"));
    initialize(&mut agent);
    let session_id = new_session(&mut agent, 2, workspace.to_str().expect("workspace path"));
    agent.send(json!({
        "jsonrpc": "2.0",
        "id": 3,
        "method": "session/prompt",
        "params": {
            "sessionId": session_id,
            "prompt": [{ "type": "text", "text": "Read hello.txt" }]
        }
    }));

    let mut messages = Vec::new();
    let response = loop {
        let message = agent.output.recv_timeout(TIMEOUT).expect("ACP message");
        if message.get("id").and_then(Value::as_u64) == Some(3) {
            break message;
        }
        let method = message.get("method").and_then(Value::as_str);
        if method == Some("session/update") || message.get("id").is_none() {
            messages.push(message);
            continue;
        }
        terminal_stub(&mut agent, &message, "hello from tool\n");
    };
    assert_eq!(response["result"]["stopReason"], "end_turn");
    let updates = messages
        .iter()
        .filter_map(|message| message.pointer("/params/update"))
        .collect::<Vec<_>>();
    let think = updates
        .iter()
        .find(|update| update["sessionUpdate"] == "tool_call" && update["kind"] == "think")
        .expect("think tool start");
    assert_eq!(
        think["content"][0]["content"]["text"],
        "I should inspect the file."
    );
    assert!(updates.iter().any(|update| {
        update["sessionUpdate"] == "tool_call" && update["toolCallId"] == "call-read"
    }));
    assert!(updates.iter().any(|update| {
        update["sessionUpdate"] == "tool_call_update"
            && update["toolCallId"] == "call-read"
            && update["status"] == "completed"
    }));
    assert!(updates.iter().any(|update| {
        update["sessionUpdate"] == "agent_message_chunk"
            && update["content"]["text"] == "I read the file."
    }));
    assert!(updates
        .iter()
        .all(|update| update["sessionUpdate"] != "agent_thought_chunk"));

    let _first_request = request_receiver
        .recv_timeout(TIMEOUT)
        .expect("first request");
    let second_request = request_receiver
        .recv_timeout(TIMEOUT)
        .expect("second request");
    assert!(second_request.contains("\"role\":\"tool\""));
    assert!(second_request.contains("hello from tool"));
    assert!(second_request.contains("\"reasoning_details\""));
    assert!(second_request.contains("I should inspect the file."));
    let _ = fs::remove_dir_all(workspace);
}

#[test]
fn transcript_runs_file_work_through_os_commands() {
    let listener = match TcpListener::bind("127.0.0.1:0") {
        Ok(listener) => listener,
        Err(error) if error.kind() == std::io::ErrorKind::PermissionDenied => {
            eprintln!("skipping socket transcript test: sandbox forbids loopback listeners");
            return;
        }
        Err(error) => panic!("bind provider: {error}"),
    };
    let address = listener.local_addr().expect("provider address");
    thread::spawn(move || {
        for body in [
            "data: {\"choices\":[{\"delta\":{\"tool_calls\":[{\"index\":0,\"id\":\"call-edit\",\"type\":\"function\",\"function\":{\"name\":\"run_command\",\"arguments\":\"{\\\"command\\\":\\\"sh\\\",\\\"args\\\":[\\\"-c\\\",\\\"printf new > note.txt\\\"],\\\"cwd\\\":\\\".\\\"}\"}}]}}]}\n\ndata: [DONE]\n\n",
            "data: {\"choices\":[{\"delta\":{\"content\":\"Edited the file.\"}}]}\n\ndata: [DONE]\n\n",
        ] {
            let (mut socket, _) = accept_chat(&listener).expect("provider connection");
            let _ = read_http_request(&mut socket);
            write_sse(&mut socket, body);
        }
    });

    let workspace = std::env::temp_dir().join(format!("amarcode-edit-{}", Uuid::new_v4()));
    fs::create_dir(&workspace).expect("create workspace");

    let mut agent = AgentProcess::spawn(&format!("http://{address}/v1"));
    initialize(&mut agent);
    let session_id = new_session(&mut agent, 2, workspace.to_str().expect("workspace path"));
    agent.send(json!({
        "jsonrpc": "2.0",
        "id": 4,
        "method": "session/prompt",
        "params": {
            "sessionId": session_id,
            "prompt": [{ "type": "text", "text": "Replace old with new in note.txt" }]
        }
    }));

    let mut completed_text = None;
    loop {
        let message = agent.output.recv_timeout(TIMEOUT).expect("ACP message");
        if message.get("id").and_then(Value::as_u64) == Some(4) {
            assert_eq!(message["result"]["stopReason"], "end_turn");
            break;
        }
        let method = message.get("method").and_then(Value::as_str);
        if method == Some("session/update") || message.get("id").is_none() {
            if let Some(update) = message.pointer("/params/update") {
                if update["toolCallId"] == "call-edit" && update["status"] == "completed" {
                    completed_text = update.pointer("/content/0/content/text").cloned();
                }
            }
            continue;
        }
        terminal_stub(&mut agent, &message, "wrote note.txt\n");
    }

    let text = completed_text.expect("completed command text");
    assert!(text.as_str().unwrap().contains("wrote note.txt"));
    let _ = fs::remove_dir_all(workspace);
}

#[test]
fn transcript_uses_permissioned_acp_terminal_lifecycle() {
    let listener = match TcpListener::bind("127.0.0.1:0") {
        Ok(listener) => listener,
        Err(error) if error.kind() == std::io::ErrorKind::PermissionDenied => {
            eprintln!("skipping socket transcript test: sandbox forbids loopback listeners");
            return;
        }
        Err(error) => panic!("bind provider: {error}"),
    };
    let address = listener.local_addr().expect("provider address");
    let (request_sender, request_receiver) = mpsc::channel();
    thread::spawn(move || {
        for body in [
            "data: {\"choices\":[{\"delta\":{\"tool_calls\":[{\"index\":0,\"id\":\"call-command\",\"type\":\"function\",\"function\":{\"name\":\"run_command\",\"arguments\":\"{\\\"command\\\":\\\"/bin/echo\\\",\\\"args\\\":[\\\"hello terminal\\\"],\\\"cwd\\\":\\\".\\\"}\"}}]}}]}\n\ndata: [DONE]\n\n",
            "data: {\"choices\":[{\"delta\":{\"tool_calls\":[{\"index\":0,\"id\":\"call-command-again\",\"type\":\"function\",\"function\":{\"name\":\"run_command\",\"arguments\":\"{\\\"command\\\":\\\"/bin/echo\\\",\\\"args\\\":[\\\"hello terminal\\\"],\\\"cwd\\\":\\\".\\\"}\"}}]}}]}\n\ndata: [DONE]\n\n",
            "data: {\"choices\":[{\"delta\":{\"content\":\"Command completed.\"}}]}\n\ndata: [DONE]\n\n",
        ] {
            let (mut socket, _) = accept_chat(&listener).expect("provider connection");
            let request = read_http_request(&mut socket);
            request_sender
                .send(String::from_utf8_lossy(&request).into_owned())
                .expect("capture request");
            write_sse(&mut socket, body);
        }
    });

    let workspace = std::env::temp_dir().join(format!("amarcode-terminal-{}", Uuid::new_v4()));
    fs::create_dir(&workspace).expect("create workspace");
    let mut agent = AgentProcess::spawn(&format!("http://{address}/v1"));
    initialize(&mut agent);
    let session_id = new_session(&mut agent, 2, workspace.to_str().expect("workspace path"));
    agent.send(json!({
        "jsonrpc": "2.0",
        "id": 4,
        "method": "session/prompt",
        "params": {
            "sessionId": session_id,
            "prompt": [{ "type": "text", "text": "Run echo" }]
        }
    }));

    let mut methods = Vec::new();
    let mut saw_terminal_content = false;
    let mut terminal_creates = 0;
    loop {
        let message = agent.output.recv_timeout(TIMEOUT).expect("ACP message");
        if message.get("id").and_then(Value::as_u64) == Some(4) {
            assert_eq!(message["result"]["stopReason"], "end_turn");
            break;
        }
        if message["method"] == "session/update" {
            saw_terminal_content |= message["params"]["update"]["content"]
                .as_array()
                .is_some_and(|content| {
                    content.iter().any(|item| {
                        item["type"] == "terminal" && item["terminalId"] == "terminal-1"
                    })
                });
            continue;
        }
        let Some(method) = message.get("method").and_then(Value::as_str) else {
            continue;
        };
        methods.push(method.to_owned());
        let id = message["id"].clone();
        let result = match method {
            "session/request_permission" => {
                assert_eq!(
                    message["params"]["toolCall"]["rawInput"]["command"],
                    "/bin/echo"
                );
                assert_eq!(
                    message["params"]["toolCall"]["rawInput"]["args"],
                    json!(["hello terminal"])
                );
                assert_two_permission_options(&message);
                json!({ "outcome": { "outcome": "selected", "optionId": "full-access" } })
            }
            "terminal/create" => {
                assert_eq!(message["params"]["command"], "/bin/echo");
                assert_eq!(message["params"]["args"], json!(["hello terminal"]));
                terminal_creates += 1;
                // First call: approved through the dialog, so the daemon
                // recorded an exact permission and _meta.sensitive is true.
                // Second call: the earlier full-access grant is remembered, so
                // the adapter signals non-sensitive and the daemon bypasses.
                assert_eq!(
                    message["params"]["_meta"]["amarcode"]["sensitive"],
                    json!(terminal_creates == 1)
                );
                json!({ "terminalId": "terminal-1" })
            }
            "terminal/wait_for_exit" => json!({ "exitCode": 0 }),
            "terminal/output" => json!({
                "output": "hello terminal\n",
                "truncated": false,
                "exitStatus": { "exitCode": 0 }
            }),
            "terminal/release" => json!({}),
            other => panic!("unexpected agent request: {other}"),
        };
        agent.send(json!({ "jsonrpc": "2.0", "id": id, "result": result }));
    }

    assert_eq!(
        methods,
        [
            "session/request_permission",
            "terminal/create",
            "terminal/wait_for_exit",
            "terminal/output",
            "terminal/release",
            "terminal/create",
            "terminal/wait_for_exit",
            "terminal/output",
            "terminal/release"
        ]
    );
    assert!(saw_terminal_content);
    let _first_request = request_receiver
        .recv_timeout(TIMEOUT)
        .expect("first request");
    let second_request = request_receiver
        .recv_timeout(TIMEOUT)
        .expect("second request");
    assert!(second_request.contains("hello terminal"));
    assert!(second_request.contains("exit code 0"));
    let third_request = request_receiver
        .recv_timeout(TIMEOUT)
        .expect("third request");
    assert!(third_request.contains("call-command-again"));
    assert!(third_request.matches("exit code 0").count() >= 2);
    let _ = fs::remove_dir_all(workspace);
}

fn provider_listener() -> Option<TcpListener> {
    match TcpListener::bind("127.0.0.1:0") {
        Ok(listener) => Some(listener),
        Err(error) if error.kind() == std::io::ErrorKind::PermissionDenied => {
            eprintln!("skipping socket transcript test: sandbox forbids loopback listeners");
            None
        }
        Err(error) => panic!("bind provider: {error}"),
    }
}

fn provider_request(socket: &mut TcpStream) -> Value {
    let bytes = read_http_request(socket);
    let start = bytes
        .windows(4)
        .position(|part| part == b"\r\n\r\n")
        .unwrap()
        + 4;
    serde_json::from_slice(&bytes[start..]).expect("provider JSON")
}

fn write_json_response(socket: &mut TcpStream, status: &str, headers: &str, body: Value) {
    let body = body.to_string();
    write!(socket, "HTTP/1.1 {status}\r\nContent-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n{headers}\r\n{body}", body.len()).unwrap();
    socket.flush().unwrap();
}

fn prompt(agent: &mut AgentProcess, id: u64, session: &str, text: &str) {
    agent.send(json!({"jsonrpc":"2.0","id":id,"method":"session/prompt",
        "params":{"sessionId":session,"prompt":[{"type":"text","text":text}]}}));
}

fn tool_sse(calls: Vec<Value>) -> String {
    format!(
        "data: {}\n\ndata: [DONE]\n\n",
        json!({"choices":[{"delta":{"tool_calls":calls}}]})
    )
}

fn command_call(id: &str, command: &str, args: &[&str]) -> Value {
    json!({"index":0,"id":id,"type":"function","function":{"name":"run_command","arguments":json!({"command":command,"args":args,"cwd":"."}).to_string()}})
}

fn assert_two_permission_options(message: &Value) {
    let options = message["params"]["options"]
        .as_array()
        .expect("two permission options");
    assert_eq!(options.len(), 2);
    assert!(options
        .iter()
        .any(|option| { option["optionId"] == "full-access" && option["kind"] == "allow_always" }));
    assert!(options
        .iter()
        .any(|option| { option["optionId"] == "auto-approve" && option["kind"] == "allow_once" }));
}

fn terminal_stub(agent: &mut AgentProcess, message: &Value, output: &str) {
    let method = message["method"].as_str().expect("agent method");
    let id = message["id"].clone();
    let result = match method {
        "session/request_permission" => {
            assert_two_permission_options(message);
            json!({ "outcome": { "outcome": "selected", "optionId": "auto-approve" } })
        }
        "terminal/create" => json!({ "terminalId": "terminal-1" }),
        "terminal/wait_for_exit" => json!({ "exitCode": 0 }),
        "terminal/output" => json!({
            "output": output,
            "truncated": false,
            "exitStatus": { "exitCode": 0 }
        }),
        "terminal/release" => json!({}),
        other => panic!("unexpected agent request: {other}"),
    };
    agent.send(json!({ "jsonrpc": "2.0", "id": id, "result": result }));
}

#[test]
fn transcript_retries_transient_statuses_and_reports_usage() {
    let Some(listener) = provider_listener() else {
        return;
    };
    let address = listener.local_addr().unwrap();
    let server = thread::spawn(move || {
        let mut requests = Vec::new();
        for status in ["429 Too Many Requests", "503 Service Unavailable", "200 OK"] {
            let (mut socket, _) = accept_chat(&listener).unwrap();
            requests.push(provider_request(&mut socket));
            if status == "200 OK" {
                write_sse(
                    &mut socket,
                    &format!(
                        "data: {}\n\ndata: [DONE]\n\n",
                        json!({"choices":[{"delta":{"content":"recovered"}}],"usage":{"prompt_tokens":123,"completion_tokens":7,"cost":0.01}})
                    ),
                );
            } else {
                write_json_response(
                    &mut socket,
                    status,
                    "Retry-After: 0\r\n",
                    json!({"error":{"message":"retry"}}),
                );
            }
        }
        requests
    });
    let mut agent = AgentProcess::spawn(&format!("http://{address}/v1"));
    initialize(&mut agent);
    let session = new_session(&mut agent, 2, "/workspace");
    prompt(&mut agent, 3, &session, "retry safely");
    let (response, messages) = agent.response_with_messages(3);
    assert_eq!(response["result"]["stopReason"], "end_turn");
    let usage = messages
        .iter()
        .find(|message| message["params"]["update"]["sessionUpdate"] == "usage_update")
        .unwrap();
    assert_eq!(usage["params"]["update"]["used"], 123);
    assert_eq!(usage["params"]["update"]["cost"]["amount"], 0.01);
    assert_eq!(
        usage["params"]["update"]["_meta"]["amarcode"]["turn_tokens"],
        130
    );
    let requests = server.join().unwrap();
    assert_eq!(requests.len(), 3);
    assert!(requests.windows(2).all(|pair| pair[0] == pair[1]));
    assert!(requests[0]["max_tokens"].as_u64().unwrap() > 0);
}

#[test]
fn transcript_cancels_during_retry_after_backoff() {
    let Some(listener) = provider_listener() else {
        return;
    };
    let address = listener.local_addr().unwrap();
    let (ready_tx, ready_rx) = mpsc::channel();
    thread::spawn(move || {
        let (mut socket, _) = accept_chat(&listener).unwrap();
        provider_request(&mut socket);
        write_json_response(
            &mut socket,
            "429 Too Many Requests",
            "Retry-After: 3600\r\n",
            json!({"error":{"message":"wait"}}),
        );
        ready_tx.send(()).unwrap();
    });
    let mut agent = AgentProcess::spawn(&format!("http://{address}/v1"));
    initialize(&mut agent);
    let session = new_session(&mut agent, 2, "/workspace");
    prompt(&mut agent, 3, &session, "wait");
    ready_rx.recv_timeout(TIMEOUT).unwrap();
    agent.send(json!({"jsonrpc":"2.0","method":"session/cancel","params":{"sessionId":session}}));
    assert_eq!(agent.response(3)["result"]["stopReason"], "cancelled");
}

#[test]
fn transcript_falls_back_only_after_explicit_streaming_rejection() {
    let Some(listener) = provider_listener() else {
        return;
    };
    let address = listener.local_addr().unwrap();
    let server = thread::spawn(move || {
        let (mut socket, _) = accept_chat(&listener).unwrap();
        assert_eq!(provider_request(&mut socket)["stream"], true);
        write_json_response(
            &mut socket,
            "400 Bad Request",
            "",
            json!({"error":{"message":"stream is unsupported"}}),
        );
        let (mut socket, _) = accept_chat(&listener).unwrap();
        let request = provider_request(&mut socket);
        assert_eq!(request["stream"], false);
        assert!(request.get("stream_options").is_none());
        write_json_response(
            &mut socket,
            "200 OK",
            "",
            json!({"choices":[{"message":{"role":"assistant","content":"fallback succeeded"}}]}),
        );
    });
    let mut agent = AgentProcess::spawn(&format!("http://{address}/v1"));
    initialize(&mut agent);
    let session = new_session(&mut agent, 2, "/workspace");
    prompt(&mut agent, 3, &session, "fallback");
    let (response, messages) = agent.response_with_messages(3);
    assert_eq!(response["result"]["stopReason"], "end_turn");
    assert!(messages
        .iter()
        .any(|message| message["params"]["update"]["content"]["text"] == "fallback succeeded"));
    assert!(messages
        .iter()
        .any(|message| message["params"]["update"]["_meta"]["amarcode"]["estimated"] == true));
    server.join().unwrap();
}

#[test]
fn transcript_does_not_retry_auth_errors_or_execute_truncated_tool_calls() {
    for auth_error in [true, false] {
        let Some(listener) = provider_listener() else {
            return;
        };
        let address = listener.local_addr().unwrap();
        thread::spawn(move || {
            let (mut socket, _) = accept_chat(&listener).unwrap();
            provider_request(&mut socket);
            if auth_error {
                write_json_response(
                    &mut socket,
                    "401 Unauthorized",
                    "",
                    json!({"error":{"message":"bad credentials"}}),
                );
            } else {
                let body = format!(
                    "data: {}\n\n",
                    json!({"choices":[{"delta":{"tool_calls":[command_call("partial", "cat", &["never-read"])]}}]})
                );
                write_sse(&mut socket, &body);
            }
        });
        let mut agent = AgentProcess::spawn(&format!("http://{address}/v1"));
        initialize(&mut agent);
        let session = new_session(&mut agent, 2, "/workspace");
        prompt(&mut agent, 3, &session, "fail safely");
        let (response, messages) = agent.response_with_messages(3);
        assert!(response.get("error").is_some(), "{response}");
        assert!(!messages
            .iter()
            .any(|message| message["params"]["update"]["sessionUpdate"] == "tool_call"));
        assert!(response.to_string().contains(if auth_error {
            "401"
        } else {
            "before a finish reason"
        }));
    }
}

#[test]
fn transcript_ignores_legacy_tool_limits_and_stops_on_repetition_with_balanced_history() {
    let options = json!({"agent":{"max_tool_rounds":1,"max_tool_calls":1,"repeated_tool_limit":3}});
    let rounds = 3;
    let expected_text = "no new results";
    let Some(listener) = provider_listener() else {
        return;
    };
    let address = listener.local_addr().unwrap();
    let server = thread::spawn(move || {
        for index in 0..rounds {
            let (mut socket, _) = accept_chat(&listener).unwrap();
            provider_request(&mut socket);
            let calls = vec![command_call(
                &format!("call-{index}"),
                "cat",
                &["missing-file"],
            )];
            write_sse(&mut socket, &tool_sse(calls));
        }
    });
    let mut agent = AgentProcess::spawn_with_options(&format!("http://{address}/v1"), options);
    initialize(&mut agent);
    let session = new_session(&mut agent, 2, "/workspace");
    prompt(&mut agent, 3, &session, "bounded work");
    let mut messages = Vec::new();
    let response = loop {
        let message = agent.output.recv_timeout(TIMEOUT).expect("ACP message");
        if message.get("id").and_then(Value::as_u64) == Some(3) {
            break message;
        }
        let method = message.get("method").and_then(Value::as_str);
        if method == Some("session/update") || message.get("id").is_none() {
            messages.push(message);
            continue;
        }
        let id = message["id"].clone();
        let result = match method {
            Some("session/request_permission") => {
                assert_two_permission_options(&message);
                json!({ "outcome": { "outcome": "selected", "optionId": "auto-approve" } })
            }
            Some("terminal/create") => json!({ "terminalId": "terminal-1" }),
            Some("terminal/wait_for_exit") => json!({ "exitCode": 1 }),
            Some("terminal/output") => json!({
                "output": "cat: missing-file: no such file\n",
                "truncated": false,
                "exitStatus": { "exitCode": 1 }
            }),
            Some("terminal/release") => json!({}),
            other => panic!("unexpected agent request: {other:?}"),
        };
        agent.send(json!({ "jsonrpc": "2.0", "id": id, "result": result }));
    };
    assert_eq!(
        response["result"]["stopReason"], "max_turn_requests",
        "{response}"
    );
    assert!(messages
        .iter()
        .any(|message| message["params"]["update"]["content"]["text"]
            .as_str()
            .is_some_and(|text| text.contains(expected_text))));
    let saved: Value =
        serde_json::from_slice(&fs::read(&agent.session_store_path).unwrap()).unwrap();
    let history = saved["sessions"][0]["history"].as_array().unwrap();
    assert_eq!(history[0]["content"], "bounded work");
    for (index, message) in history.iter().enumerate() {
        if let Some(calls) = message["tool_calls"].as_array() {
            for (offset, call) in calls.iter().enumerate() {
                assert_eq!(history[index + offset + 1]["tool_call_id"], call["id"]);
            }
        }
    }
    server.join().unwrap();
}

#[test]
fn transcript_time_budget_and_idle_timeout_interrupt_a_stalled_stream() {
    for time_budget in [true, false] {
        let Some(listener) = provider_listener() else {
            return;
        };
        let address = listener.local_addr().unwrap();
        thread::spawn(move || {
            let (mut socket, _) = accept_chat(&listener).unwrap();
            provider_request(&mut socket);
            socket.write_all(b"HTTP/1.1 200 OK\r\nContent-Type: text/event-stream\r\nConnection: close\r\n\r\n").unwrap();
            socket.flush().unwrap();
            thread::sleep(Duration::from_secs(3));
        });
        let options = if time_budget {
            json!({"agent":{"max_turn_seconds":1}})
        } else {
            json!({"reliability":{"stream_idle_timeout_seconds":1}})
        };
        let mut agent = AgentProcess::spawn_with_options(&format!("http://{address}/v1"), options);
        initialize(&mut agent);
        let session = new_session(&mut agent, 2, "/workspace");
        prompt(&mut agent, 3, &session, "stalled stream");
        let response = agent.response(3);
        if time_budget {
            assert_eq!(
                response["result"]["stopReason"], "max_turn_requests",
                "{response}"
            );
        } else {
            assert!(response.to_string().contains("idle timeout"), "{response}");
        }
    }
}

#[test]
fn transcript_compacts_and_continues_the_same_turn_past_legacy_token_budget() {
    let Some(listener) = provider_listener() else {
        return;
    };
    let address = listener.local_addr().unwrap();
    let workspace = std::env::temp_dir().join(format!("amarcode-continuation-{}", Uuid::new_v4()));
    fs::create_dir_all(&workspace).unwrap();
    fs::write(
        workspace.join("large.txt"),
        "historical evidence ".repeat(2000),
    )
    .unwrap();
    fs::write(workspace.join("verification.txt"), "verification passed").unwrap();
    let server = thread::spawn(move || {
        let mut round = 0;
        let mut summaries = 0;
        loop {
            let (mut socket, _) = accept_chat(&listener).unwrap();
            let request = provider_request(&mut socket);
            if request["stream"] == false {
                summaries += 1;
                assert!(request.get("tools").is_none());
                write_json_response(
                    &mut socket,
                    "200 OK",
                    "",
                    json!({
                        "choices":[{"message":{"content":"Completed read of large.txt; evidence retained. Next verify verification.txt; do not repeat the completed read."}}],
                        "usage":{"prompt_tokens":300000,"completion_tokens":50}
                    }),
                );
                continue;
            }
            assert!(request["max_tokens"].as_u64().unwrap() > 1);
            let delta = match round {
                0 => json!({"tool_calls":[command_call("read-large", "cat", &["large.txt"])]}),
                1 => {
                    let text = request["messages"].to_string();
                    assert!(text.contains("original objective"));
                    assert!(text.contains("Completed read of large.txt"));
                    assert!(!text.contains("historical evidence"));
                    json!({"tool_calls":[command_call("verify", "cat", &["verification.txt"])]})
                }
                2 => {
                    let messages = request["messages"].as_array().unwrap();
                    assert!(messages.iter().any(|message| message["role"] == "tool"
                        && message["tool_call_id"] == "verify"
                        && message["content"]
                            .as_str()
                            .is_some_and(|text| text.contains("verification passed"))));
                    json!({"content":"Completed and verified."})
                }
                _ => panic!("unexpected continuation request"),
            };
            write_sse(
                &mut socket,
                &format!(
                    "data: {}\n\ndata: [DONE]\n\n",
                    json!({"choices":[{"delta":delta}],"usage":{"prompt_tokens":300000,"completion_tokens":100}})
                ),
            );
            if round == 2 {
                assert!(summaries > 0, "the growing context was never compacted");
                break;
            }
            round += 1;
        }
    });
    let mut agent = AgentProcess::spawn_with_options(
        &format!("http://{address}/v1"),
        json!({"agent":{"max_turn_tokens":1},"context":{"context_length":8192,"max_tool_result_bytes":65536}}),
    );
    initialize(&mut agent);
    let session = new_session(&mut agent, 2, workspace.to_str().unwrap());
    prompt(&mut agent, 3, &session, "original objective");
    let large_output = "historical evidence ".repeat(2000);
    let mut outputs = 0;
    let mut messages = Vec::new();
    let response = loop {
        let message = agent.output.recv_timeout(TIMEOUT).expect("ACP message");
        if message.get("id").and_then(Value::as_u64) == Some(3) {
            break message;
        }
        let method = message.get("method").and_then(Value::as_str);
        if method == Some("session/update") || message.get("id").is_none() {
            messages.push(message);
            continue;
        }
        let id = message["id"].clone();
        let result = match method {
            Some("session/request_permission") => {
                assert_two_permission_options(&message);
                json!({ "outcome": { "outcome": "selected", "optionId": "auto-approve" } })
            }
            Some("terminal/create") => json!({ "terminalId": "terminal-1" }),
            Some("terminal/wait_for_exit") => json!({ "exitCode": 0 }),
            Some("terminal/output") => {
                outputs += 1;
                let text = if outputs == 1 {
                    large_output.clone()
                } else {
                    "verification passed".to_owned()
                };
                json!({
                    "output": text,
                    "truncated": false,
                    "exitStatus": { "exitCode": 0 }
                })
            }
            Some("terminal/release") => json!({}),
            other => panic!("unexpected agent request: {other:?}"),
        };
        agent.send(json!({ "jsonrpc": "2.0", "id": id, "result": result }));
    };
    assert_eq!(response["result"]["stopReason"], "end_turn", "{response}");
    assert!(!messages
        .iter()
        .any(|message| message.to_string().contains("Stopped:")));
    let saved: Value =
        serde_json::from_slice(&fs::read(&agent.session_store_path).unwrap()).unwrap();
    let history = saved["sessions"][0]["history"].as_array().unwrap();
    assert!(history.iter().any(|message| message["content"]
        .as_str()
        .is_some_and(|text| text.contains("Completed read of large.txt"))));
    assert_eq!(
        history.last().unwrap()["content"],
        "Completed and verified."
    );
    server.join().unwrap();
    fs::remove_dir_all(workspace).unwrap();
}

#[test]
fn transcript_compacts_old_exchanges_and_injects_workspace_instructions() {
    let Some(listener) = provider_listener() else {
        return;
    };
    let address = listener.local_addr().unwrap();
    let workspace = std::env::temp_dir().join(format!("amarcode-context-{}", Uuid::new_v4()));
    fs::create_dir_all(&workspace).unwrap();
    fs::write(
        workspace.join("AGENTS.md"),
        "Always preserve project-specific invariant ZEBRA.",
    )
    .unwrap();
    let server = thread::spawn(move || {
        let (mut socket, _) = accept_chat(&listener).unwrap();
        let first = provider_request(&mut socket);
        assert!(first["messages"][0]["content"]
            .as_str()
            .unwrap()
            .contains("ZEBRA"));
        let old = "obsolete detail ".repeat(2000);
        write_sse(
            &mut socket,
            &format!(
                "data: {}\n\ndata: [DONE]\n\n",
                json!({"choices":[{"delta":{"content":old}}]})
            ),
        );
        let (mut socket, _) = accept_chat(&listener).unwrap();
        let summary = provider_request(&mut socket);
        assert_eq!(summary["stream"], false);
        assert!(summary.get("tools").is_none());
        write_json_response(
            &mut socket,
            "200 OK",
            "",
            json!({"choices":[{"message":{"content":"Preserved decision: use ZEBRA; verification pending."}}]}),
        );
        let (mut socket, _) = accept_chat(&listener).unwrap();
        let request = provider_request(&mut socket);
        let text = request["messages"].to_string();
        assert!(
            text.contains("original objective")
                && text.contains("current follow-up")
                && text.contains("Preserved decision")
        );
        assert!(!text.contains("obsolete detail"));
        write_sse(
            &mut socket,
            "data: {\"choices\":[{\"delta\":{\"content\":\"continued\"}}]}\n\ndata: [DONE]\n\n",
        );
    });
    let mut agent = AgentProcess::spawn_with_options(
        &format!("http://{address}/v1"),
        json!({"context":{"context_length":8192}}),
    );
    initialize(&mut agent);
    let session = new_session(&mut agent, 2, workspace.to_str().unwrap());
    prompt(&mut agent, 3, &session, "original objective");
    assert_eq!(agent.response(3)["result"]["stopReason"], "end_turn");
    prompt(&mut agent, 4, &session, "current follow-up");
    assert_eq!(agent.response(4)["result"]["stopReason"], "end_turn");
    let saved: Value =
        serde_json::from_slice(&fs::read(&agent.session_store_path).unwrap()).unwrap();
    assert!(saved.to_string().contains("Preserved decision"));
    assert!(!saved.to_string().contains("obsolete detail"));
    server.join().unwrap();
    fs::remove_dir_all(workspace).unwrap();
}

#[test]
fn transcript_time_budget_kills_and_releases_an_active_terminal() {
    let Some(listener) = provider_listener() else {
        return;
    };
    let address = listener.local_addr().unwrap();
    thread::spawn(move || {
        let (mut socket, _) = accept_chat(&listener).unwrap();
        provider_request(&mut socket);
        write_sse(
            &mut socket,
            &tool_sse(vec![
                json!({"index":0,"id":"command","function":{"name":"run_command","arguments":json!({"command":"sleep","args":["300"]}).to_string()}}),
            ]),
        );
    });
    let workspace = std::env::temp_dir().join(format!("amarcode-deadline-{}", Uuid::new_v4()));
    fs::create_dir_all(&workspace).unwrap();
    let mut agent = AgentProcess::spawn_with_options(
        &format!("http://{address}/v1"),
        json!({"agent":{"max_turn_seconds":1}}),
    );
    initialize(&mut agent);
    let session = new_session(&mut agent, 2, workspace.to_str().unwrap());
    prompt(&mut agent, 3, &session, "run a long command");
    let mut methods = Vec::new();
    loop {
        let message = agent.output.recv_timeout(TIMEOUT).unwrap();
        if message["id"] == 3 {
            assert_eq!(
                message["result"]["stopReason"], "max_turn_requests",
                "{message}"
            );
            break;
        }
        let Some(method) = message["method"].as_str() else {
            continue;
        };
        if method == "session/update" || method == "$/cancel_request" {
            continue;
        }
        methods.push(method.to_owned());
        if method == "terminal/wait_for_exit" {
            continue;
        }
        let result = match method {
            "session/request_permission" => {
                json!({"outcome":{"outcome":"selected","optionId":"auto-approve"}})
            }
            "terminal/create" => json!({"terminalId":"long-running"}),
            "terminal/output" => {
                json!({"output":"","truncated":false,"exitStatus":{"exitCode":143}})
            }
            "terminal/kill" | "terminal/release" => json!({}),
            other => panic!("unexpected request: {other}"),
        };
        agent.send(json!({"jsonrpc":"2.0","id":message["id"],"result":result}));
    }
    assert_eq!(
        methods,
        [
            "session/request_permission",
            "terminal/create",
            "terminal/wait_for_exit",
            "terminal/kill",
            "terminal/output",
            "terminal/release"
        ]
    );
    fs::remove_dir_all(workspace).unwrap();
}

#[test]
fn transcript_cost_budget_stops_requests_and_persists_reported_session_cost() {
    let Some(listener) = provider_listener() else {
        return;
    };
    let address = listener.local_addr().unwrap();
    thread::spawn(move || {
        let (mut socket, _) = accept_chat(&listener).unwrap();
        provider_request(&mut socket);
        let body = json!({"choices":[{"delta":{"tool_calls":[command_call("cost", "cat", &["missing-file"])]}}],
            "usage":{"prompt_tokens":100,"completion_tokens":5,"cost":0.02}});
        write_sse(&mut socket, &format!("data: {body}\n\ndata: [DONE]\n\n"));
    });
    let mut agent = AgentProcess::spawn_with_options(
        &format!("http://{address}/v1"),
        json!({"agent":{"max_cost_usd":0.01}}),
    );
    initialize(&mut agent);
    let session = new_session(&mut agent, 2, "/workspace");
    prompt(&mut agent, 3, &session, "respect cost budget");
    let mut messages = Vec::new();
    let response = loop {
        let message = agent.output.recv_timeout(TIMEOUT).expect("ACP message");
        if message.get("id").and_then(Value::as_u64) == Some(3) {
            break message;
        }
        let method = message.get("method").and_then(Value::as_str);
        if method == Some("session/update") || message.get("id").is_none() {
            messages.push(message);
            continue;
        }
        terminal_stub(&mut agent, &message, "done\n");
    };
    assert_eq!(
        response["result"]["stopReason"], "max_turn_requests",
        "{response}"
    );
    assert!(messages
        .iter()
        .any(|message| message["params"]["update"]["content"]["text"]
            .as_str()
            .is_some_and(|text| text.contains("cost budget"))));
    let saved: Value =
        serde_json::from_slice(&fs::read(&agent.session_store_path).unwrap()).unwrap();
    assert_eq!(saved["sessions"][0]["cost_usd"], 0.02);
}

#[test]
fn transcript_model_output_limit_reports_max_tokens_without_executing_calls() {
    let Some(listener) = provider_listener() else {
        return;
    };
    let address = listener.local_addr().unwrap();
    thread::spawn(move || {
        let (mut socket, _) = accept_chat(&listener).unwrap();
        provider_request(&mut socket);
        write_sse(
            &mut socket,
            &format!(
                "data: {}\n\ndata: [DONE]\n\n",
                json!({"choices":[{"delta":{"tool_calls":[command_call("limited", "cat", &["never-read"])]},"finish_reason":"length"}]})
            ),
        );
    });
    let mut agent = AgentProcess::spawn(&format!("http://{address}/v1"));
    initialize(&mut agent);
    let session = new_session(&mut agent, 2, "/workspace");
    prompt(&mut agent, 3, &session, "output limit");
    let (response, messages) = agent.response_with_messages(3);
    assert_eq!(response["result"]["stopReason"], "max_tokens");
    assert!(!messages
        .iter()
        .any(|message| message["params"]["update"]["sessionUpdate"] == "tool_call"));
}

#[test]
fn transcript_preserves_split_utf8_and_finishes_at_done_without_waiting_for_eof() {
    let Some(listener) = provider_listener() else {
        return;
    };
    let address = listener.local_addr().unwrap();
    thread::spawn(move || {
        let (mut socket, _) = accept_chat(&listener).unwrap();
        provider_request(&mut socket);
        socket.write_all(b"HTTP/1.1 200 OK\r\nContent-Type: text/event-stream\r\nConnection: close\r\n\r\ndata: {\"choices\":[{\"delta\":{\"content\":\"").unwrap();
        socket.write_all(&[0xc3]).unwrap();
        socket.flush().unwrap();
        thread::sleep(Duration::from_millis(50));
        socket.write_all(&[0xb6]).unwrap();
        socket.write_all(b"\"}}]}\n\ndata: [DONE]\n\n").unwrap();
        socket.flush().unwrap();
        thread::sleep(Duration::from_secs(2));
    });
    let mut agent = AgentProcess::spawn_with_options(
        &format!("http://{address}/v1"),
        json!({"reliability":{"stream_idle_timeout_seconds":1}}),
    );
    initialize(&mut agent);
    let session = new_session(&mut agent, 2, "/workspace");
    prompt(&mut agent, 3, &session, "UTF-8 stream");
    let (response, messages) = agent.response_with_messages(3);
    assert_eq!(response["result"]["stopReason"], "end_turn", "{response}");
    assert!(messages
        .iter()
        .any(|message| message["params"]["update"]["content"]["text"] == "ö"));
}

#[test]
fn transcript_discovers_models_from_arbitrary_provider_and_falls_back_on_invalid_catalogs() {
    for (catalog, expected) in [
        (
            json!({"data":[{"id":"custom/model-b"},{"id":"custom/model-a"},{"id":"custom/model-a"}]}),
            json!(["test-model", "custom/model-a", "custom/model-b"]),
        ),
        (
            json!({"models":[{"id":"custom/model"}]}),
            json!(["test-model"]),
        ),
        (json!({"data":[]}), json!(["test-model"])),
    ] {
        let Some(listener) = provider_listener() else {
            return;
        };
        let address = listener.local_addr().unwrap();
        let server = thread::spawn(move || {
            let (mut socket, _) = listener.accept().unwrap();
            let request = read_http_request(&mut socket);
            assert!(request.starts_with(b"GET /v1/models "));
            write_json_response(&mut socket, "200 OK", "", catalog);
        });
        let mut agent = AgentProcess::spawn(&format!("http://{address}/v1"));
        initialize(&mut agent);
        for id in [2, 3] {
            agent.send(json!({"jsonrpc":"2.0", "id":id, "method":"session/new",
                "params":{"cwd":"/workspace", "mcpServers":[]}}));
            let response = agent.response(id);
            let options = response
                .pointer("/result/configOptions/0/options")
                .and_then(Value::as_array)
                .expect("model selector options");
            let models: Vec<_> = options
                .iter()
                .map(|option| option["value"].clone())
                .collect();
            assert_eq!(json!(models), expected);
        }
        server.join().unwrap();
    }
}

#[test]
fn transcript_file_command_creates_and_edits_with_diffs_without_routine_prompts() {
    let Some(listener) = provider_listener() else {
        return;
    };
    let address = listener.local_addr().unwrap();
    let server = thread::spawn(move || {
        for (index, args) in [
            json!({"operation":"create","path":"note.txt","content":"hello world","sensitive":false}),
            json!({"operation":"edit","path":"note.txt","old_text":"world","content":"friend","sensitive":false}),
        ].into_iter().enumerate() {
            let (mut socket, _) = accept_chat(&listener).unwrap();
            read_http_request(&mut socket);
            write_sse(&mut socket, &tool_sse(vec![json!({"index":0,"id":format!("file-{index}"),"type":"function","function":{"name":"file_command","arguments":args.to_string()}})]));
        }
        let (mut socket, _) = accept_chat(&listener).unwrap();
        let body = provider_request(&mut socket);
        assert!(body["messages"]
            .as_array()
            .unwrap()
            .iter()
            .any(|m| m["role"] == "tool"));
        write_sse(
            &mut socket,
            "data: {\"choices\":[{\"delta\":{\"content\":\"Done\"}}]}\n\ndata: [DONE]\n\n",
        );
    });
    let root = std::env::temp_dir().join(format!("acp-file-{}", Uuid::new_v4()));
    fs::create_dir(&root).unwrap();
    let mut agent = AgentProcess::spawn(&format!("http://{address}/v1"));
    initialize(&mut agent);
    let session = new_session(&mut agent, 2, root.to_str().unwrap());
    for (id, mode) in [(10, "full-access"), (11, "auto-approve")] {
        agent.send(json!({"jsonrpc":"2.0","id":id,"method":"session/set_config_option","params":{"sessionId":session,"configId":"permission","value":mode}}));
        let response = agent.response(id);
        assert!(response.get("error").is_none(), "{response}");
        let options = response["result"]["configOptions"].as_array().unwrap();
        assert!(
            options
                .iter()
                .any(|option| option["id"] == "permission" && option["currentValue"] == mode),
            "{response}"
        );
    }
    prompt(&mut agent, 3, &session, "Create and edit note.txt");
    let (response, messages) = agent.response_with_messages(3);
    assert_eq!(response["result"]["stopReason"], "end_turn");
    assert!(!messages
        .iter()
        .any(|m| m["method"] == "session/request_permission"));
    assert_eq!(
        messages
            .iter()
            .filter(|m| m.pointer("/params/update/content/0/type") == Some(&json!("diff")))
            .count(),
        2
    );
    assert_eq!(
        fs::read_to_string(root.join("note.txt")).unwrap(),
        "hello friend"
    );
    server.join().unwrap();
    fs::remove_dir_all(root).unwrap();
}

#[test]
fn transcript_file_command_read_returns_content_without_a_permission_prompt() {
    let Some(listener) = provider_listener() else {
        return;
    };
    let address = listener.local_addr().unwrap();
    let server = thread::spawn(move || {
        // The read call omits `sensitive` entirely; reading must still be
        // treated as non-sensitive and must not request permission.
        let (mut socket, _) = accept_chat(&listener).unwrap();
        read_http_request(&mut socket);
        write_sse(
            &mut socket,
            &tool_sse(vec![json!({
                "index": 0,
                "id": "read-0",
                "type": "function",
                "function": { "name": "file_command", "arguments": json!({
                    "operation": "read", "path": "note.txt"
                }).to_string() }
            })]),
        );
        let (mut socket, _) = accept_chat(&listener).unwrap();
        let body = provider_request(&mut socket);
        assert!(body["messages"]
            .as_array()
            .unwrap()
            .iter()
            .any(|m| m["role"] == "tool"));
        write_sse(
            &mut socket,
            "data: {\"choices\":[{\"delta\":{\"content\":\"Done\"}}]}\n\ndata: [DONE]\n\n",
        );
    });
    let root = std::env::temp_dir().join(format!("acp-file-read-{}", Uuid::new_v4()));
    fs::create_dir(&root).unwrap();
    fs::write(root.join("note.txt"), "hello readable").unwrap();
    let mut agent = AgentProcess::spawn(&format!("http://{address}/v1"));
    initialize(&mut agent);
    let session = new_session(&mut agent, 2, root.to_str().unwrap());
    prompt(&mut agent, 3, &session, "Read note.txt");
    let (response, messages) = agent.response_with_messages(3);
    assert_eq!(response["result"]["stopReason"], "end_turn");
    assert!(!messages
        .iter()
        .any(|m| m["method"] == "session/request_permission"));
    // The read result is delivered as text, not a diff.
    assert!(messages.iter().any(|m| {
        m.pointer("/params/update/content/0/content/type") == Some(&json!("text"))
            && m.pointer("/params/update/content/0/content/text")
                .and_then(Value::as_str)
                .is_some_and(|text| text.contains("hello readable"))
    }));
    server.join().unwrap();
    fs::remove_dir_all(root).unwrap();
}

#[test]
fn transcript_request_cancellation_kills_and_releases_an_active_terminal() {
    let Some(listener) = provider_listener() else {
        return;
    };
    let address = listener.local_addr().unwrap();
    thread::spawn(move || {
        let (mut socket, _) = accept_chat(&listener).unwrap();
        provider_request(&mut socket);
        write_sse(
            &mut socket,
            &tool_sse(vec![
                json!({"index":0,"id":"command","function":{"name":"run_command","arguments":json!({"command":"sleep","args":["300"]}).to_string()}}),
            ]),
        );
    });
    let workspace = std::env::temp_dir().join(format!("amarcode-deadline-{}", Uuid::new_v4()));
    fs::create_dir_all(&workspace).unwrap();
    let mut agent = AgentProcess::spawn_with_options(
        &format!("http://{address}/v1"),
        json!({"agent":{"max_turn_seconds":60}}),
    );
    initialize(&mut agent);
    let session = new_session(&mut agent, 2, workspace.to_str().unwrap());
    prompt(&mut agent, 3, &session, "run a long command");
    let mut methods = Vec::new();
    loop {
        let message = agent.output.recv_timeout(TIMEOUT).unwrap();
        if message["id"] == 3 {
            assert_eq!(message["result"]["stopReason"], "cancelled", "{message}");
            break;
        }
        let Some(method) = message["method"].as_str() else {
            continue;
        };
        if method == "session/update" || method == "$/cancel_request" {
            continue;
        }
        methods.push(method.to_owned());
        if method == "terminal/wait_for_exit" {
            agent.send(
                json!({"jsonrpc":"2.0","method":"$/cancel_request","params":{"requestId":3}}),
            );
            continue;
        }
        let result = match method {
            "session/request_permission" => {
                json!({"outcome":{"outcome":"selected","optionId":"auto-approve"}})
            }
            "terminal/create" => json!({"terminalId":"long-running"}),
            "terminal/output" => {
                json!({"output":"","truncated":false,"exitStatus":{"exitCode":143}})
            }
            "terminal/kill" | "terminal/release" => json!({}),
            other => panic!("unexpected request: {other}"),
        };
        agent.send(json!({"jsonrpc":"2.0","id":message["id"],"result":result}));
    }
    assert_eq!(
        methods,
        [
            "session/request_permission",
            "terminal/create",
            "terminal/wait_for_exit",
            "terminal/kill",
            "terminal/output",
            "terminal/release"
        ]
    );
    fs::remove_dir_all(workspace).unwrap();
}
