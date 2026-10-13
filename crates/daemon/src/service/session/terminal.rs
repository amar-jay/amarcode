//! Workspace-scoped implementation of ACP's client-owned terminal lifecycle.

use std::{
    collections::HashMap,
    io::Read,
    path::Path,
    process::{Child, Command, ExitStatus, Stdio},
    sync::{
        atomic::{AtomicBool, Ordering},
        Arc, Mutex, Weak,
    },
    thread,
    time::Duration,
};

use agent_client_protocol::schema::v1::{
    CreateTerminalRequest, CreateTerminalResponse, KillTerminalRequest, KillTerminalResponse,
    ReleaseTerminalRequest, ReleaseTerminalResponse, TerminalExitStatus, TerminalOutputRequest,
    TerminalOutputResponse, WaitForTerminalExitRequest, WaitForTerminalExitResponse,
};
use serde_json::{json, Value};

use crate::{
    acp::{AcpClient, RpcId},
    Error, Result,
};

use super::types::SessionInner;

const DEFAULT_OUTPUT_LIMIT: usize = 64 * 1024;
const MAX_OUTPUT_LIMIT: usize = 1024 * 1024;

#[derive(Default)]
pub(super) struct TerminalManager {
    processes: Mutex<HashMap<String, Arc<TerminalProcess>>>,
    permissions: Mutex<Vec<CommandPermission>>,
}

struct TerminalProcess {
    terminal_id: String,
    run_id: String,
    session_id: String,
    child: Mutex<Child>,
    output: Arc<Mutex<CapturedOutput>>,
    readers: Mutex<Vec<thread::JoinHandle<()>>>,
    command: String,
    args: Vec<String>,
    cwd: String,
    events: Mutex<TerminalEvents>,
    released: AtomicBool,
}

#[derive(Default)]
struct TerminalEvents {
    binding: Option<ToolBinding>,
    client: Option<Weak<AcpClient>>,
    last_snapshot: Option<Value>,
}

struct ToolBinding {
    message_id: String,
    tool_call_id: String,
}

#[derive(Default)]
struct CapturedOutput {
    bytes: Vec<u8>,
    truncated: bool,
}

#[derive(Debug, Clone, PartialEq, Eq)]
struct CommandPermission {
    run_id: String,
    chat_id: String,
    command: String,
    args: Vec<String>,
    cwd: Option<String>,
    reusable: bool,
}

pub(super) fn is_terminal_method(method: &str) -> bool {
    matches!(
        method,
        "terminal/create"
            | "terminal/output"
            | "terminal/wait_for_exit"
            | "terminal/kill"
            | "terminal/release"
    )
}

pub(super) fn spawn_terminal_request(
    inner: Arc<SessionInner>,
    run_id: String,
    chat_id: String,
    id: RpcId,
    method: String,
    params: Value,
) {
    thread::Builder::new()
        .name(format!("acp-terminal-{id}"))
        .spawn(move || {
            let response = handle_terminal_request(&inner, &run_id, &chat_id, &method, params);
            let client = inner.by_chat.lock().ok().and_then(|runs| {
                runs.get(&chat_id)
                    .filter(|live| live.run_id == run_id)
                    .map(|live| Arc::clone(&live.client))
            });
            let Some(client) = client else {
                inner.terminals.release_run(&run_id);
                return;
            };
            if method == "terminal/create" {
                if let Ok(value) = &response {
                    if let Some(id) = value.get("terminalId").and_then(Value::as_str) {
                        inner.terminals.observe(&run_id, id, &client);
                    }
                }
            }
            match response {
                Ok(value) => {
                    let _ = client.respond(id, value);
                }
                Err(error) => {
                    let _ = client.respond_error(id, -32602, &error.to_string(), None);
                }
            }
        })
        .expect("spawn ACP terminal request worker");
}

fn handle_terminal_request(
    inner: &SessionInner,
    run_id: &str,
    chat_id: &str,
    method: &str,
    params: Value,
) -> Result<Value> {
    let expected_session = {
        let runs = inner
            .by_chat
            .lock()
            .map_err(|_| Error::msg("session lock poisoned"))?;
        let live = runs
            .get(chat_id)
            .filter(|live| live.run_id == run_id)
            .ok_or_else(|| Error::msg("terminal request belongs to a replaced run"))?;
        live.acp_session_id
            .clone()
            .ok_or_else(|| Error::msg("ACP session is not initialized"))?
    };
    let workspace = inner
        .store
        .get_chat(chat_id)?
        .ok_or_else(|| Error::msg("chat not found"))?
        .workspace_path;

    match method {
        "terminal/create" => {
            let request: CreateTerminalRequest = parse(params)?;
            require_session(&request.session_id.to_string(), &expected_session)?;
            let response =
                inner
                    .terminals
                    .create(run_id, chat_id, Path::new(&workspace), request)?;
            encode(response)
        }
        "terminal/output" => {
            let request: TerminalOutputRequest = parse(params)?;
            require_session(&request.session_id.to_string(), &expected_session)?;
            encode(inner.terminals.output(run_id, request)?)
        }
        "terminal/wait_for_exit" => {
            let request: WaitForTerminalExitRequest = parse(params)?;
            require_session(&request.session_id.to_string(), &expected_session)?;
            encode(inner.terminals.wait(run_id, request)?)
        }
        "terminal/kill" => {
            let request: KillTerminalRequest = parse(params)?;
            require_session(&request.session_id.to_string(), &expected_session)?;
            inner.terminals.kill(run_id, request)?;
            encode(KillTerminalResponse::new())
        }
        "terminal/release" => {
            let request: ReleaseTerminalRequest = parse(params)?;
            require_session(&request.session_id.to_string(), &expected_session)?;
            inner.terminals.release(run_id, request)?;
            encode(ReleaseTerminalResponse::new())
        }
        _ => Err(Error::msg(format!("unsupported terminal method: {method}"))),
    }
}

impl TerminalManager {
    pub(super) fn record_permission(
        &self,
        run_id: &str,
        chat_id: &str,
        request: &Value,
        response: &Value,
    ) {
        let selected_id = response
            .pointer("/outcome/optionId")
            .and_then(Value::as_str);
        let Some(selected_id) = selected_id else {
            return;
        };
        let selected_kind = request
            .get("options")
            .and_then(Value::as_array)
            .and_then(|options| {
                options.iter().find(|option| {
                    option
                        .get("optionId")
                        .or_else(|| option.get("option_id"))
                        .and_then(Value::as_str)
                        == Some(selected_id)
                })
            })
            .and_then(|option| option.get("kind"))
            .and_then(Value::as_str);
        let allowed = selected_kind
            .is_some_and(|kind| matches!(kind, "allow_once" | "allow_always"))
            || (selected_kind.is_none() && selected_id.starts_with("allow"));
        if !allowed {
            return;
        }
        let Some(raw) = request
            .get("toolCall")
            .and_then(|tool| tool.get("rawInput"))
        else {
            return;
        };
        let Some(command) = raw.get("command").and_then(Value::as_str) else {
            return;
        };
        let Some(args) = string_array(raw.get("args")) else {
            return;
        };
        let permission = CommandPermission {
            run_id: run_id.to_owned(),
            chat_id: chat_id.to_owned(),
            command: command.to_owned(),
            args,
            cwd: raw.get("cwd").and_then(Value::as_str).map(str::to_owned),
            reusable: selected_kind == Some("allow_always"),
        };
        if let Ok(mut permissions) = self.permissions.lock() {
            permissions.push(permission);
        }
    }

    fn create(
        &self,
        run_id: &str,
        chat_id: &str,
        workspace: &Path,
        request: CreateTerminalRequest,
    ) -> Result<CreateTerminalResponse> {
        let root = workspace
            .canonicalize()
            .map_err(|error| Error::msg(format!("invalid workspace: {error}")))?;
        let cwd = request.cwd.clone().unwrap_or_else(|| root.clone());
        let cwd = cwd
            .canonicalize()
            .map_err(|error| Error::msg(format!("invalid terminal cwd: {error}")))?;
        if !cwd.is_dir() || !cwd.starts_with(&root) {
            return Err(Error::msg(
                "terminal cwd must be a directory inside the workspace",
            ));
        }
        if !request.env.is_empty() {
            return Err(Error::msg(
                "terminal environment overrides are not permitted without explicit authorization",
            ));
        }
        self.consume_permission(run_id, chat_id, &root, &cwd, &request)?;

        let mut command = Command::new(&request.command);
        #[cfg(windows)]
        {
            use std::os::windows::process::CommandExt;
            command.creation_flags(0x0800_0000); // CREATE_NO_WINDOW
        }
        command
            .args(&request.args)
            .current_dir(&cwd)
            .stdin(Stdio::null())
            .stdout(Stdio::piped())
            .stderr(Stdio::piped());
        for variable in &request.env {
            command.env(&variable.name, &variable.value);
        }
        #[cfg(unix)]
        {
            use std::os::unix::process::CommandExt as _;
            command.process_group(0);
        }
        let mut child = command
            .spawn()
            .map_err(|error| Error::msg(format!("failed to start command: {error}")))?;
        let stdout = child.stdout.take();
        let stderr = child.stderr.take();
        let output = Arc::new(Mutex::new(CapturedOutput::default()));
        let output_limit = request
            .output_byte_limit
            .and_then(|limit| usize::try_from(limit).ok())
            .unwrap_or(DEFAULT_OUTPUT_LIMIT)
            .min(MAX_OUTPUT_LIMIT);
        let mut readers = Vec::new();
        if let Some(stdout) = stdout {
            readers.push(drain_output(stdout, Arc::clone(&output), output_limit));
        }
        if let Some(stderr) = stderr {
            readers.push(drain_output(stderr, Arc::clone(&output), output_limit));
        }

        let terminal_id = uuid::Uuid::new_v4().to_string();
        let process = Arc::new(TerminalProcess {
            terminal_id: terminal_id.clone(),
            run_id: run_id.to_owned(),
            session_id: request.session_id.to_string(),
            child: Mutex::new(child),
            output,
            readers: Mutex::new(readers),
            command: request.command.clone(),
            args: request.args.clone(),
            cwd: cwd.to_string_lossy().into_owned(),
            events: Mutex::new(TerminalEvents::default()),
            released: AtomicBool::new(false),
        });
        self.processes
            .lock()
            .map_err(|_| Error::msg("terminal lock poisoned"))?
            .insert(terminal_id.clone(), process);
        Ok(CreateTerminalResponse::new(terminal_id))
    }

    /// Link the client-owned terminal reference to the exact assistant message
    /// and tool call, including commands that finished before this update.
    pub(super) fn bind_tool(&self, run_id: &str, message_id: &str, update: &Value) {
        let Some(tool_call_id) = update.get("toolCallId").and_then(Value::as_str) else {
            return;
        };
        let Some(content) = update.get("content").and_then(Value::as_array) else {
            return;
        };
        for terminal_id in content
            .iter()
            .filter(|item| item["type"] == "terminal")
            .filter_map(|item| item.get("terminalId").and_then(Value::as_str))
        {
            let Ok(process) = self.process(run_id, terminal_id) else {
                continue;
            };
            if let Ok(mut events) = process.events.lock() {
                if events.binding.is_none() {
                    events.binding = Some(ToolBinding {
                        message_id: message_id.to_owned(),
                        tool_call_id: tool_call_id.to_owned(),
                    });
                }
            }
            let _ = publish_snapshot(&process);
        }
    }

    fn observe(&self, run_id: &str, terminal_id: &str, client: &Arc<AcpClient>) {
        let Ok(process) = self.process(run_id, terminal_id) else {
            return;
        };
        if let Ok(mut events) = process.events.lock() {
            events.client = Some(Arc::downgrade(client));
        }
        let process = Arc::downgrade(&process);
        thread::spawn(move || {
            while let Some(process) = process.upgrade() {
                if process.released.load(Ordering::Acquire) {
                    break;
                }
                match publish_snapshot(&process) {
                    Ok(true) | Err(_) => break,
                    Ok(false) => {}
                }
                drop(process);
                thread::sleep(Duration::from_millis(100));
            }
        });
    }

    fn consume_permission(
        &self,
        run_id: &str,
        chat_id: &str,
        root: &Path,
        cwd: &Path,
        request: &CreateTerminalRequest,
    ) -> Result<()> {
        let mut permissions = self
            .permissions
            .lock()
            .map_err(|_| Error::msg("terminal permission lock poisoned"))?;
        let position = permissions.iter().position(|permission| {
            let permitted_cwd = permission
                .cwd
                .as_deref()
                .map(|value| root.join(value))
                .unwrap_or_else(|| root.to_owned())
                .canonicalize()
                .ok();
            permission.run_id == run_id
                && permission.chat_id == chat_id
                && permission.command == request.command
                && permission.args == request.args
                && permitted_cwd.as_deref() == Some(cwd)
        });
        position
            .map(|index| {
                if permissions[index].reusable {
                    permissions[index].clone()
                } else {
                    permissions.remove(index)
                }
            })
            .ok_or_else(|| {
                Error::msg(
                    "terminal command was not approved with an exact command-specific permission",
                )
            })?;
        Ok(())
    }

    fn process(&self, run_id: &str, terminal_id: &str) -> Result<Arc<TerminalProcess>> {
        let process = self
            .processes
            .lock()
            .map_err(|_| Error::msg("terminal lock poisoned"))?
            .get(terminal_id)
            .cloned()
            .ok_or_else(|| Error::msg("terminal not found"))?;
        if process.run_id != run_id {
            return Err(Error::msg("terminal belongs to another run"));
        }
        Ok(process)
    }

    fn output(
        &self,
        run_id: &str,
        request: TerminalOutputRequest,
    ) -> Result<TerminalOutputResponse> {
        let process = self.process(run_id, &request.terminal_id.to_string())?;
        require_session(&request.session_id.to_string(), &process.session_id)?;
        let output = process
            .output
            .lock()
            .map_err(|_| Error::msg("terminal output lock poisoned"))?;
        let text = String::from_utf8_lossy(&output.bytes).into_owned();
        let truncated = output.truncated;
        drop(output);
        let exit = process
            .child
            .lock()
            .map_err(|_| Error::msg("terminal process lock poisoned"))?
            .try_wait()
            .map_err(|error| Error::msg(format!("failed checking terminal: {error}")))?
            .map(exit_status);
        Ok(TerminalOutputResponse::new(text, truncated).exit_status(exit))
    }

    fn wait(
        &self,
        run_id: &str,
        request: WaitForTerminalExitRequest,
    ) -> Result<WaitForTerminalExitResponse> {
        let process = self.process(run_id, &request.terminal_id.to_string())?;
        require_session(&request.session_id.to_string(), &process.session_id)?;
        loop {
            let status = process
                .child
                .lock()
                .map_err(|_| Error::msg("terminal process lock poisoned"))?
                .try_wait()
                .map_err(|error| Error::msg(format!("failed waiting for terminal: {error}")))?;
            if let Some(status) = status {
                join_readers(&process);
                publish_snapshot(&process)?;
                return Ok(WaitForTerminalExitResponse::new(exit_status(status)));
            }
            thread::sleep(Duration::from_millis(25));
        }
    }

    fn kill(&self, run_id: &str, request: KillTerminalRequest) -> Result<()> {
        let process = self.process(run_id, &request.terminal_id.to_string())?;
        require_session(&request.session_id.to_string(), &process.session_id)?;
        kill_process(&process)
    }

    fn release(&self, run_id: &str, request: ReleaseTerminalRequest) -> Result<()> {
        let id = request.terminal_id.to_string();
        let process = self.process(run_id, &id)?;
        require_session(&request.session_id.to_string(), &process.session_id)?;
        let _ = kill_process(&process);
        let _ = process.child.lock().map(|mut child| child.wait());
        join_readers(&process);
        let snapshot_result = publish_snapshot(&process);
        process.released.store(true, Ordering::Release);
        self.processes
            .lock()
            .map_err(|_| Error::msg("terminal lock poisoned"))?
            .remove(&id);
        snapshot_result.map(|_| ())
    }

    pub(super) fn release_run(&self, run_id: &str) {
        if let Ok(mut permissions) = self.permissions.lock() {
            permissions.retain(|permission| permission.run_id != run_id);
        }
        let removed = if let Ok(mut processes) = self.processes.lock() {
            let ids = processes
                .iter()
                .filter(|(_, process)| process.run_id == run_id)
                .map(|(id, _)| id.clone())
                .collect::<Vec<_>>();
            ids.into_iter()
                .filter_map(|id| processes.remove(&id))
                .collect::<Vec<_>>()
        } else {
            Vec::new()
        };
        for process in removed {
            let _ = kill_process(&process);
            let _ = process.child.lock().map(|mut child| child.wait());
            let _ = publish_snapshot(&process);
            process.released.store(true, Ordering::Release);
        }
    }
}

/// Publish only changed, bounded snapshots. The inbound worker persists them
/// before notifying the editor; no invented ACP events are sent to the agent.
fn publish_snapshot(process: &TerminalProcess) -> Result<bool> {
    let mut events = process
        .events
        .lock()
        .map_err(|_| Error::msg("terminal event lock poisoned"))?;
    let Some(binding) = events.binding.as_ref() else {
        return Ok(false);
    };
    let Some(client) = events.client.as_ref().and_then(Weak::upgrade) else {
        return Ok(false);
    };
    let output = process
        .output
        .lock()
        .map_err(|_| Error::msg("terminal output lock poisoned"))?;
    let text = String::from_utf8_lossy(&output.bytes).into_owned();
    let truncated = output.truncated;
    drop(output);
    let exit = process
        .child
        .lock()
        .map_err(|_| Error::msg("terminal process lock poisoned"))?
        .try_wait()
        .map_err(|error| Error::msg(format!("failed checking terminal: {error}")))?
        .map(exit_status);
    let mut metadata = json!({
        "amarcode_terminal_snapshot": process.terminal_id,
        "terminal_info": { "terminal_id":process.terminal_id, "cwd":process.cwd },
        "terminal_output": { "terminal_id":process.terminal_id, "data":text, "truncated":truncated },
    });
    if let Some(exit) = &exit {
        metadata["terminal_exit"] = json!({ "terminal_id":process.terminal_id, "exit_code":exit.exit_code, "signal":exit.signal });
    }
    let payload = json!({ "sessionUpdate":"tool_call_update", "toolCallId":binding.tool_call_id,
        "kind":"execute", "title":process.command,
        "rawInput":{"command":process.command,"args":process.args,"cwd":process.cwd},
        "content":[{"type":"terminal","terminalId":process.terminal_id}], "_meta":metadata });
    if events.last_snapshot.as_ref() != Some(&payload) {
        client
            .publish_local_tool_update(binding.message_id.clone(), payload.clone())
            .map_err(|error| Error::msg(error.to_string()))?;
        events.last_snapshot = Some(payload);
    }
    Ok(exit.is_some()
        && process
            .readers
            .lock()
            .map(|readers| readers.iter().all(thread::JoinHandle::is_finished))
            .unwrap_or(false))
}

fn drain_output(
    mut reader: impl Read + Send + 'static,
    output: Arc<Mutex<CapturedOutput>>,
    limit: usize,
) -> thread::JoinHandle<()> {
    thread::spawn(move || {
        let mut buffer = [0_u8; 8192];
        while let Ok(count) = reader.read(&mut buffer) {
            if count == 0 {
                break;
            }
            let Ok(mut captured) = output.lock() else {
                break;
            };
            captured.bytes.extend_from_slice(&buffer[..count]);
            if captured.bytes.len() > limit {
                let excess = captured.bytes.len() - limit;
                captured.bytes.drain(..excess);
                captured.truncated = true;
            }
        }
    })
}

fn join_readers(process: &TerminalProcess) {
    let readers = process
        .readers
        .lock()
        .map(|mut readers| readers.drain(..).collect::<Vec<_>>())
        .unwrap_or_default();
    for reader in readers {
        let _ = reader.join();
    }
}

fn kill_process(process: &TerminalProcess) -> Result<()> {
    let mut child = process
        .child
        .lock()
        .map_err(|_| Error::msg("terminal process lock poisoned"))?;
    let exited = child
        .try_wait()
        .map_err(|error| Error::msg(format!("failed checking terminal: {error}")))?
        .is_some();
    #[cfg(unix)]
    {
        use nix::{
            sys::signal::{killpg, Signal},
            unistd::Pid,
        };
        let _ = killpg(Pid::from_raw(child.id() as i32), Signal::SIGKILL);
    }
    if exited {
        return Ok(());
    }
    child
        .kill()
        .map_err(|error| Error::msg(format!("failed killing terminal: {error}")))
}

fn exit_status(status: ExitStatus) -> TerminalExitStatus {
    let result = TerminalExitStatus::new()
        .exit_code(status.code().and_then(|code| u32::try_from(code).ok()));
    #[cfg(unix)]
    {
        use std::os::unix::process::ExitStatusExt as _;
        result.signal(status.signal().map(|signal| signal.to_string()))
    }
    #[cfg(not(unix))]
    result
}

fn require_session(actual: &str, expected: &str) -> Result<()> {
    if actual == expected {
        Ok(())
    } else {
        Err(Error::msg(
            "terminal request belongs to another ACP session",
        ))
    }
}

fn string_array(value: Option<&Value>) -> Option<Vec<String>> {
    match value {
        None => Some(Vec::new()),
        Some(Value::Array(values)) => values
            .iter()
            .map(|value| value.as_str().map(str::to_owned))
            .collect(),
        Some(_) => None,
    }
}

fn parse<T: serde::de::DeserializeOwned>(value: Value) -> Result<T> {
    serde_json::from_value(value)
        .map_err(|error| Error::msg(format!("invalid terminal request: {error}")))
}

fn encode<T: serde::Serialize>(value: T) -> Result<Value> {
    serde_json::to_value(value).map_err(Error::from)
}

#[cfg(test)]
mod tests {
    use super::*;
    use agent_client_protocol::schema::v1::SessionId;
    use serde_json::json;
    use std::path::PathBuf;

    fn temp_workspace() -> PathBuf {
        let path = std::env::temp_dir().join(format!("amarcode-terminal-{}", uuid::Uuid::new_v4()));
        std::fs::create_dir(&path).expect("create test workspace");
        path.canonicalize().expect("canonicalize test workspace")
    }

    fn allow(manager: &TerminalManager, command: &str, args: &[&str]) {
        manager.record_permission(
            "run",
            "chat",
            &json!({ "toolCall": { "rawInput": { "command": command, "args": args, "cwd": "." } } }),
            &json!({ "outcome": { "outcome": "selected", "optionId": "allow-once" } }),
        );
    }

    #[test]
    fn records_only_exact_allowed_command_permissions() {
        let manager = TerminalManager::default();
        manager.record_permission(
            "run",
            "chat",
            &json!({ "toolCall": { "rawInput": { "command": "cargo", "args": ["test"], "cwd": "." } } }),
            &json!({ "outcome": { "outcome": "selected", "optionId": "allow-once" } }),
        );
        let permissions = manager.permissions.lock().expect("permissions");
        assert_eq!(permissions.len(), 1);
        assert_eq!(permissions[0].command, "cargo");
        assert_eq!(permissions[0].args, ["test"]);
    }

    #[test]
    fn denied_permission_is_not_recorded() {
        let manager = TerminalManager::default();
        manager.record_permission(
            "run",
            "chat",
            &json!({ "toolCall": { "rawInput": { "command": "cargo", "args": ["test"] } } }),
            &json!({ "outcome": { "outcome": "selected", "optionId": "reject-once" } }),
        );
        assert!(manager.permissions.lock().expect("permissions").is_empty());
    }

    #[test]
    fn allow_always_permission_is_reusable() {
        let workspace = temp_workspace();
        let manager = TerminalManager::default();
        manager.record_permission(
            "run",
            "chat",
            &json!({
                "toolCall": { "rawInput": { "command": "/bin/echo", "args": ["ok"], "cwd": "." } },
                "options": [
                    { "optionId": "allow-session", "name": "Always allow", "kind": "allow_always" }
                ]
            }),
            &json!({ "outcome": { "outcome": "selected", "optionId": "allow-session" } }),
        );
        let request = CreateTerminalRequest::new("session", "/bin/echo")
            .args(vec!["ok".into()])
            .cwd(workspace.clone());

        manager
            .consume_permission("run", "chat", &workspace, &workspace, &request)
            .expect("first use");
        manager
            .consume_permission("run", "chat", &workspace, &workspace, &request)
            .expect("reused permission");
        assert_eq!(manager.permissions.lock().expect("permissions").len(), 1);
        let _ = std::fs::remove_dir_all(workspace);
    }

    #[cfg(unix)]
    #[test]
    fn executes_waits_reads_and_releases_terminal() {
        let workspace = temp_workspace();
        let manager = TerminalManager::default();
        allow(&manager, "/bin/sh", &["-c", "printf terminal-ok"]);
        let session = SessionId::new("session");
        let created = manager
            .create(
                "run",
                "chat",
                &workspace,
                CreateTerminalRequest::new(session.clone(), "/bin/sh")
                    .args(vec!["-c".into(), "printf terminal-ok".into()])
                    .cwd(workspace.clone()),
            )
            .expect("create terminal");
        let terminal = created.terminal_id;
        let exit = manager
            .wait(
                "run",
                WaitForTerminalExitRequest::new(session.clone(), terminal.clone()),
            )
            .expect("wait terminal");
        assert_eq!(exit.exit_status.exit_code, Some(0));
        let output = manager
            .output(
                "run",
                TerminalOutputRequest::new(session.clone(), terminal.clone()),
            )
            .expect("terminal output");
        assert_eq!(output.output, "terminal-ok");
        manager
            .release(
                "run",
                ReleaseTerminalRequest::new(session, terminal.clone()),
            )
            .expect("release terminal");
        assert!(manager.process("run", &terminal.to_string()).is_err());
        let _ = std::fs::remove_dir_all(workspace);
    }

    #[cfg(windows)]
    #[test]
    fn windows_terminal_captures_output_without_allocating_a_console() {
        let workspace = temp_workspace();
        let manager = TerminalManager::default();
        // Check the terminal process itself, rather than its window title.
        // A normal console-subsystem process would report a non-null HWND.
        let script = "Add-Type -TypeDefinition 'using System; using System.Runtime.InteropServices; public static class ConsoleProbe { [DllImport(\"kernel32.dll\")] public static extern IntPtr GetConsoleWindow(); }'; if ([ConsoleProbe]::GetConsoleWindow() -ne [IntPtr]::Zero) { exit 1 }; [Console]::Out.Write('terminal-ok')";
        let args = [
            "-NoLogo",
            "-NoProfile",
            "-NonInteractive",
            "-Command",
            script,
        ];
        allow(&manager, "powershell.exe", &args);
        let session = SessionId::new("session");
        let terminal = manager
            .create(
                "run",
                "chat",
                &workspace,
                CreateTerminalRequest::new(session.clone(), "powershell.exe")
                    .args(args.iter().map(|arg| (*arg).to_owned()).collect::<Vec<_>>())
                    .cwd(workspace.clone()),
            )
            .expect("create hidden Windows terminal")
            .terminal_id;
        let exit = manager
            .wait(
                "run",
                WaitForTerminalExitRequest::new(session.clone(), terminal.clone()),
            )
            .expect("wait for hidden terminal");
        assert_eq!(exit.exit_status.exit_code, Some(0));
        let output = manager
            .output(
                "run",
                TerminalOutputRequest::new(session.clone(), terminal.clone()),
            )
            .expect("read captured output");
        assert_eq!(output.output, "terminal-ok");
        manager
            .release("run", ReleaseTerminalRequest::new(session, terminal))
            .expect("release hidden terminal");
        let _ = std::fs::remove_dir_all(workspace);
    }

    #[cfg(unix)]
    #[test]
    fn exact_permission_cannot_authorize_changed_arguments() {
        let workspace = temp_workspace();
        let manager = TerminalManager::default();
        allow(&manager, "/bin/echo", &["approved"]);
        let result = manager.create(
            "run",
            "chat",
            &workspace,
            CreateTerminalRequest::new("session", "/bin/echo")
                .args(vec!["different".into()])
                .cwd(workspace.clone()),
        );
        assert!(result
            .expect_err("changed command must be rejected")
            .to_string()
            .contains("exact command-specific permission"));
        let _ = std::fs::remove_dir_all(workspace);
    }

    #[cfg(unix)]
    #[test]
    fn kill_terminates_running_terminal() {
        let workspace = temp_workspace();
        let manager = TerminalManager::default();
        allow(&manager, "/bin/sh", &["-c", "sleep 30"]);
        let session = SessionId::new("session");
        let created = manager
            .create(
                "run",
                "chat",
                &workspace,
                CreateTerminalRequest::new(session.clone(), "/bin/sh")
                    .args(vec!["-c".into(), "sleep 30".into()])
                    .cwd(workspace.clone()),
            )
            .expect("create terminal");
        let terminal = created.terminal_id;
        manager
            .kill(
                "run",
                KillTerminalRequest::new(session.clone(), terminal.clone()),
            )
            .expect("kill terminal");
        let exit = manager
            .wait(
                "run",
                WaitForTerminalExitRequest::new(session.clone(), terminal.clone()),
            )
            .expect("wait killed terminal");
        assert_ne!(exit.exit_status.exit_code, Some(0));
        manager
            .release("run", ReleaseTerminalRequest::new(session, terminal))
            .expect("release terminal");
        let _ = std::fs::remove_dir_all(workspace);
    }
}
