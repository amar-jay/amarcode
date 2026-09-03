use std::{
    fs,
    io::{BufRead, BufReader, Read, Write},
    net::TcpListener,
    path::PathBuf,
    process::{Child, ChildStdin, Command, Stdio},
    sync::mpsc::{self, Receiver},
    thread,
    time::Duration,
};

use serde_json::{json, Value};
use uuid::Uuid;

const TIMEOUT: Duration = Duration::from_secs(5);

struct AgentProcess {
    child: Child,
    stdin: ChildStdin,
    output: Receiver<Value>,
    config_path: PathBuf,
}

impl AgentProcess {
    fn spawn(base_url: &str) -> Self {
        let config_path =
            std::env::temp_dir().join(format!("amarcode-acp-transcript-{}.json", Uuid::new_v4()));
        fs::write(
            &config_path,
            serde_json::to_vec(&json!({
                "name": "transcript-agent",
                "provider": {
                    "base_url": base_url,
                    "api_key": "test-key",
                    "model": "test-model"
                }
            }))
            .expect("serialize config"),
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
        Self {
            child,
            stdin,
            output,
            config_path,
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
    assert_eq!(result["agentCapabilities"]["loadSession"], false);
    assert_eq!(
        result["agentCapabilities"]["sessionCapabilities"],
        json!({ "close": {} })
    );

    let first = new_session(&mut agent, 2, "/same-workspace");
    let second = new_session(&mut agent, 3, "/same-workspace");
    assert_ne!(first, second);
    Uuid::parse_str(&first).expect("first UUID session id");
    Uuid::parse_str(&second).expect("second UUID session id");

    agent.send(json!({
        "jsonrpc": "2.0",
        "id": 4,
        "method": "session/set_config_option",
        "params": { "sessionId": first, "configId": "mode", "value": "code" }
    }));
    let first_options = agent.response(4);
    assert_eq!(
        first_options["result"]["configOptions"][0]["currentValue"],
        "code"
    );

    agent.send(json!({
        "jsonrpc": "2.0",
        "id": 5,
        "method": "session/set_config_option",
        "params": { "sessionId": second, "configId": "mode", "value": "plan" }
    }));
    let second_options = agent.response(5);
    assert_eq!(
        second_options["result"]["configOptions"][0]["currentValue"],
        "plan"
    );
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
        let (mut socket, _) = listener.accept().expect("provider connection");
        socket
            .set_read_timeout(Some(TIMEOUT))
            .expect("set read timeout");
        let mut request = [0_u8; 4096];
        let _ = socket.read(&mut request).expect("read HTTP request");
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
        let (mut socket, _) = listener.accept().expect("provider connection");
        let mut request = [0_u8; 4096];
        let _ = socket.read(&mut request).expect("read HTTP request");
        let body = "data: {\"choices\":[{\"delta\":{\"content\":\"hello\"}}]}\n\ndata: [DONE]\n\n";
        write!(
            socket,
            "HTTP/1.1 200 OK\r\nContent-Type: text/event-stream\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{body}",
            body.len()
        )
        .expect("write provider response");
        socket.flush().expect("flush provider response");
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
            "data: {\"choices\":[{\"delta\":{\"tool_calls\":[{\"index\":0,\"id\":\"call-read\",\"type\":\"function\",\"function\":{\"name\":\"read_file\",\"arguments\":\"{\\\"path\\\":\\\"hello.txt\\\"}\"}}]}}]}\n\ndata: [DONE]\n\n",
            "data: {\"choices\":[{\"delta\":{\"content\":\"I read the file.\"}}]}\n\ndata: [DONE]\n\n",
        ] {
            let (mut socket, _) = listener.accept().expect("provider connection");
            let mut request = [0_u8; 16_384];
            let size = socket.read(&mut request).expect("read HTTP request");
            request_sender
                .send(String::from_utf8_lossy(&request[..size]).into_owned())
                .expect("capture request");
            write!(
                socket,
                "HTTP/1.1 200 OK\r\nContent-Type: text/event-stream\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{body}",
                body.len()
            )
            .expect("write provider response");
            socket.flush().expect("flush provider response");
        }
    });

    let workspace = std::env::temp_dir().join(format!("amarcode-tools-{}", Uuid::new_v4()));
    fs::create_dir(&workspace).expect("create workspace");
    fs::write(workspace.join("hello.txt"), "hello from tool").expect("write fixture");

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

    let (response, messages) = agent.response_with_messages(3);
    assert_eq!(response["result"]["stopReason"], "end_turn");
    assert!(messages.iter().any(|message| {
        message["params"]["update"]["sessionUpdate"] == "tool_call"
            && message["params"]["update"]["toolCallId"] == "call-read"
    }));
    assert!(messages.iter().any(|message| {
        message["params"]["update"]["sessionUpdate"] == "tool_call_update"
            && message["params"]["update"]["toolCallId"] == "call-read"
            && message["params"]["update"]["status"] == "completed"
    }));
    assert!(messages.iter().any(|message| {
        message["params"]["update"]["sessionUpdate"] == "agent_message_chunk"
            && message["params"]["update"]["content"]["text"] == "I read the file."
    }));

    let _first_request = request_receiver
        .recv_timeout(TIMEOUT)
        .expect("first request");
    let second_request = request_receiver
        .recv_timeout(TIMEOUT)
        .expect("second request");
    assert!(second_request.contains("\"role\":\"tool\""));
    assert!(second_request.contains("hello from tool"));
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
            "data: {\"choices\":[{\"delta\":{\"content\":\"Command completed.\"}}]}\n\ndata: [DONE]\n\n",
        ] {
            let (mut socket, _) = listener.accept().expect("provider connection");
            let mut request = [0_u8; 16_384];
            let size = socket.read(&mut request).expect("read HTTP request");
            request_sender
                .send(String::from_utf8_lossy(&request[..size]).into_owned())
                .expect("capture request");
            write!(
                socket,
                "HTTP/1.1 200 OK\r\nContent-Type: text/event-stream\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{body}",
                body.len()
            )
            .expect("write provider response");
            socket.flush().expect("flush provider response");
        }
    });

    let workspace = std::env::temp_dir().join(format!("amarcode-terminal-{}", Uuid::new_v4()));
    fs::create_dir(&workspace).expect("create workspace");
    let mut agent = AgentProcess::spawn(&format!("http://{address}/v1"));
    initialize(&mut agent);
    let session_id = new_session(&mut agent, 2, workspace.to_str().expect("workspace path"));
    agent.send(json!({
        "jsonrpc": "2.0",
        "id": 3,
        "method": "session/set_config_option",
        "params": { "sessionId": session_id, "configId": "mode", "value": "code" }
    }));
    let _ = agent.response(3);
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
                json!({ "outcome": { "outcome": "selected", "optionId": "allow-once" } })
            }
            "terminal/create" => {
                assert_eq!(message["params"]["command"], "/bin/echo");
                assert_eq!(message["params"]["args"], json!(["hello terminal"]));
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
    let _ = fs::remove_dir_all(workspace);
}
