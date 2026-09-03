//! Workspace-scoped implementation of ACP's client-owned terminal lifecycle.

use std::{
    collections::HashMap,
    io::Read,
    path::Path,
    process::{Child, Command, ExitStatus, Stdio},
    sync::{Arc, Mutex},
    thread,
    time::Duration,
};

use agent_client_protocol::schema::v1::{
    CreateTerminalRequest, CreateTerminalResponse, KillTerminalRequest, KillTerminalResponse,
    ReleaseTerminalRequest, ReleaseTerminalResponse, TerminalExitStatus, TerminalOutputRequest,
    TerminalOutputResponse, WaitForTerminalExitRequest, WaitForTerminalExitResponse,
};
use serde_json::Value;

use crate::{acp::RpcId, Error, Result};

use super::types::SessionInner;

const DEFAULT_OUTPUT_LIMIT: usize = 64 * 1024;
const MAX_OUTPUT_LIMIT: usize = 1024 * 1024;

#[derive(Default)]
pub(super) struct TerminalManager {
    processes: Mutex<HashMap<String, Arc<TerminalProcess>>>,
    permissions: Mutex<Vec<CommandPermission>>,
}

struct TerminalProcess {
    run_id: String,
    session_id: String,
    child: Mutex<Child>,
    output: Arc<Mutex<CapturedOutput>>,
    readers: Mutex<Vec<thread::JoinHandle<()>>>,
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
                return;
            };
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
            run_id: run_id.to_owned(),
            session_id: request.session_id.to_string(),
            child: Mutex::new(child),
            output,
            readers: Mutex::new(readers),
        });
        self.processes
            .lock()
            .map_err(|_| Error::msg("terminal lock poisoned"))?
            .insert(terminal_id.clone(), process);
        Ok(CreateTerminalResponse::new(terminal_id))
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
        self.processes
            .lock()
            .map_err(|_| Error::msg("terminal lock poisoned"))?
            .remove(&id);
        Ok(())
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
        }
    }
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
    if child
        .try_wait()
        .map_err(|error| Error::msg(format!("failed checking terminal: {error}")))?
        .is_some()
    {
        return Ok(());
    }
    #[cfg(unix)]
    {
        use nix::{
            sys::signal::{killpg, Signal},
            unistd::Pid,
        };
        let _ = killpg(Pid::from_raw(child.id() as i32), Signal::SIGKILL);
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
        path
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
