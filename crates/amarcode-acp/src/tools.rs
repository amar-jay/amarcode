use std::{
    collections::HashMap,
    path::{Component, Path, PathBuf},
    sync::Mutex,
};

use crate::provider::ModelToolCall;
use agent_client_protocol::{
    schema::v1::{
        ContentBlock, CreateTerminalRequest, Diff, KillTerminalRequest, PermissionOption,
        PermissionOptionKind, ReleaseTerminalRequest, RequestPermissionOutcome,
        RequestPermissionRequest, SessionId, SessionNotification, SessionUpdate, Terminal,
        TerminalOutputRequest, ToolCall as AcpToolCall, ToolCallContent, ToolCallStatus,
        ToolCallUpdate, ToolCallUpdateFields, ToolKind, WaitForTerminalExitRequest,
    },
    Client, ConnectionTo,
};
use serde_json::{json, Value};
use tokio::sync::watch;

const MAX_OUTPUT: usize = 64 * 1024;

#[derive(Debug, Clone, PartialEq, Eq, Hash)]
enum PermissionKey {
    RunCommand {
        command: String,
        args: Vec<String>,
        cwd: String,
    },
    FileWrite {
        path: String,
    },
}

/// Permission decisions for one ACP session.
///
/// The dialog offers exactly two options: `full-access` remembers the grant
/// and disables every later prompt in the session, while `auto-approve`
/// allows one command and prompts again for the next sensitive action.
/// Dismissing the prompt denies that command once without remembering.
#[derive(Debug, Default)]
pub struct PermissionState {
    remembered: Mutex<HashMap<PermissionKey, bool>>,
    full_access: Mutex<bool>,
}

impl PermissionState {
    fn decision(&self, key: &PermissionKey) -> Option<bool> {
        if self.full_access.lock().ok()?.clone() {
            return Some(true);
        }
        self.remembered.lock().ok()?.get(key).copied()
    }

    fn remember(&self, key: PermissionKey, allowed: bool) {
        if let Ok(mut remembered) = self.remembered.lock() {
            remembered.insert(key, allowed);
        }
    }

    fn grant_full_access(&self) {
        if let Ok(mut full_access) = self.full_access.lock() {
            *full_access = true;
        }
    }

    #[allow(dead_code)]
    pub fn has_full_access(&self) -> bool {
        self.full_access.lock().ok().is_some_and(|flag| *flag)
    }
}

pub async fn execute(
    call: &ModelToolCall,
    workspace: &Path,
    permissions: &PermissionState,
    session_id: &SessionId,
    connection: &ConnectionTo<Client>,
    mut cancellation: watch::Receiver<bool>,
) -> String {
    let arguments = match serde_json::from_str::<Value>(&call.arguments) {
        Ok(value) => value,
        Err(error) => {
            return finish(
                connection,
                session_id,
                call,
                Err(format!("invalid arguments: {error}")),
            )
        }
    };
    let kind = tool_kind(&call.name);
    let title = tool_title(call, &arguments);
    let started = AcpToolCall::new(call.id.clone(), title)
        .kind(kind)
        .status(ToolCallStatus::Pending)
        .raw_input(arguments.clone());
    let _ = connection.send_notification(SessionNotification::new(
        session_id.clone(),
        SessionUpdate::ToolCall(started.clone()),
    ));

    if call.name != "run_command" && call.name != "file_command" {
        return finish(
            connection,
            session_id,
            call,
            Err(format!("unknown tool: {}", call.name)),
        );
    }
    if let Some(permission_key) = permission_key(call, &arguments) {
        let allowed = if let Some(allowed) = permissions.decision(&permission_key) {
            allowed
        } else {
            let permission = connection.send_request(RequestPermissionRequest::new(
                session_id.clone(),
                ToolCallUpdate::from(started),
                vec![
                    PermissionOption::new(
                        "full-access",
                        "Full access",
                        PermissionOptionKind::AllowAlways,
                    ),
                    PermissionOption::new(
                        "auto-approve",
                        "Auto approve",
                        PermissionOptionKind::AllowOnce,
                    ),
                ],
            ));
            let response = tokio::select! {
                result = permission.block_task() => result,
                _ = cancellation.changed() => return finish(connection, session_id, call, Err("cancelled".into())),
            };
            permission_outcome(response, permissions, permission_key)
        };
        if !allowed {
            return finish(
                connection,
                session_id,
                call,
                Err("permission denied".into()),
            );
        }
    }

    update(
        connection,
        session_id,
        call,
        ToolCallStatus::InProgress,
        None,
    );
    if *cancellation.borrow() {
        return finish(connection, session_id, call, Err("cancelled".into()));
    }
    if call.name == "file_command" {
        return finish_file_change(
            connection,
            session_id,
            call,
            apply_file_command(workspace, &arguments),
        );
    }
    return run_command(
        workspace,
        &arguments,
        connection,
        session_id,
        call,
        cancellation,
    )
    .await;
}

#[derive(Debug, Clone, PartialEq, Eq)]
struct FileChange {
    path: PathBuf,
    old_text: Option<String>,
    new_text: String,
    summary: String,
}

/// Create a new file or apply one exact, uniquely matched edit.
/// Pure workspace I/O shared by the ACP wrapper and unit tests.
fn apply_file_command(workspace: &Path, args: &Value) -> Result<FileChange, String> {
    let relative = safe_relative(required_str(args, "path")?)?;
    let operation = required_str(args, "operation")?;
    match operation {
        "create" => {
            let content = required_str(args, "content")?.to_owned();
            let path = workspace.join(&relative);
            let parent = path.parent().ok_or_else(|| "invalid path".to_string())?;
            let canonical_root = workspace
                .canonicalize()
                .map_err(|e| format!("invalid workspace: {e}"))?;
            let canonical_parent = parent
                .canonicalize()
                .map_err(|e| format!("parent directory does not exist: {e}"))?;
            if !canonical_parent.starts_with(&canonical_root) {
                return Err("path escapes workspace".into());
            }
            if let canonical_target = path
                    .canonicalize()
                    .map_err(|e| format!("invalid destination: {e}"))?;
                if !canonical_target.starts_with(&canonical_root) {
                    return Err("path escapes workspace".into());
                }
            }
            let old_text = if path.is_file() {
                Some(
                    std::fs::read_to_string(&path)
                        .map_err(|e| format!("failed to read {}: {e}", path.display()))?,
                )
            } else {
                None
            };
            std::fs::write(&path, &content)
                .map_err(|e| format!("failed to write {}: {e}", path.display()))?;
            Ok(FileChange {
                path: path.clone(),
                old_text,
                new_text: content.clone(),
                summary: format!("Wrote {} bytes to {}", content.len(), path.display()),
            })
        }
        "edit" => {
            let path = existing_path(workspace, required_str(args, "path")?)?;
            if !path.is_file() {
                return Err(format!("not a file: {}", path.display()));
            }
            let old_text = required_str(args, "old_text")?;
            if old_text.is_empty() {
                return Err("old_text must not be empty".into());
            }
            let new_text = required_str(args, "content")?;
            let contents = std::fs::read_to_string(&path)
                .map_err(|error| format!("failed to read {}: {error}", path.display()))?;
            let matches = contents.match_indices(old_text).count();
            if matches == 0 {
                return Err("old_text was not found; the file was not changed".into());
            }
            if matches > 1 {
                return Err(format!(
                    "old_text matched {matches} locations; include more surrounding context so it is unique"
                ));
            }
            let updated = contents.replacen(old_text, new_text, 1);
            std::fs::write(&path, updated.as_bytes())
                .map_err(|error| format!("failed to write {}: {error}", path.display()))?;
            Ok(FileChange {
                path: path.clone(),
                old_text: Some(contents),
                new_text: updated,
                summary: format!(
                    "Replaced {} bytes with {} bytes in {}",
                    old_text.len(),
                    new_text.len(),
                    path.display()
                ),
            })
        }
        _ => Err("operation must be \"create\" or \"edit\"".into()),
    }
}

fn finish_file_change(
    connection: &ConnectionTo<Client>,
    session_id: &SessionId,
    call: &ModelToolCall,
    result: Result<FileChange, String>,
) -> String {
    match result {
        Ok(change) => {
            let diff = Diff::new(change.path, change.new_text).old_text(change.old_text);
            let fields = ToolCallUpdateFields::new()
                .status(ToolCallStatus::Completed)
                .content(vec![ToolCallContent::Diff(diff)])
                .raw_output(json!({ "text": change.summary }));
            let _ = connection.send_notification(SessionNotification::new(
                session_id.clone(),
                SessionUpdate::ToolCallUpdate(ToolCallUpdate::new(call.id.clone(), fields)),
            ));
            change.summary
        }
        Err(error) => {
            update(
                connection,
                session_id,
                call,
                ToolCallStatus::Failed,
                Some(&error),
            );
            format!("Error: {error}")
        }
    }
}

fn permission_key(call: &ModelToolCall, arguments: &Value) -> Option<PermissionKey> {
    match call.name.as_str() {
        "run_command" => Some(PermissionKey::RunCommand {
            command: arguments.get("command")?.as_str()?.to_owned(),
            args: optional_string_array(arguments, "args").ok()?,
            cwd: arguments
                .get("cwd")
                .and_then(Value::as_str)
                .unwrap_or(".")
                .to_owned(),
        }),
        "file_command" => Some(PermissionKey::FileWrite {
            path: arguments.get("path")?.as_str()?.to_owned(),
        }),
        _ => None,
    }
}

fn permission_outcome<E>(
    response: Result<agent_client_protocol::schema::v1::RequestPermissionResponse, E>,
    permissions: &PermissionState,
    key: PermissionKey,
) -> bool {
    let Ok(response) = response else { return false };
    let RequestPermissionOutcome::Selected(selected) = response.outcome else {
        return false;
    };
    match selected.option_id.to_string().as_str() {
        "auto-approve" => true,
        "full-access" => {
            permissions.remember(key, true);
            permissions.grant_full_access();
            true
        }
        _ => false,
    }
}

async fn run_command(
    workspace: &Path,
    args: &Value,
    connection: &ConnectionTo<Client>,
    session_id: &SessionId,
    call: &ModelToolCall,
    mut cancellation: watch::Receiver<bool>,
) -> String {
    let command = match required_str(args, "command") {
        Ok(command) if !command.is_empty() => command,
        Ok(_) => {
            return finish(
                connection,
                session_id,
                call,
                Err("command must not be empty".into()),
            )
        }
        Err(error) => return finish(connection, session_id, call, Err(error)),
    };
    let command_args = match optional_string_array(args, "args") {
        Ok(args) => args,
        Err(error) => return finish(connection, session_id, call, Err(error)),
    };
    let cwd = match existing_path(
        workspace,
        args.get("cwd").and_then(Value::as_str).unwrap_or("."),
    ) {
        Ok(path) if path.is_dir() => path,
        Ok(path) => {
            return finish(
                connection,
                session_id,
                call,
                Err(format!(
                    "working directory is not a directory: {}",
                    path.display()
                )),
            )
        }
        Err(error) => return finish(connection, session_id, call, Err(error)),
    };

    let create = CreateTerminalRequest::new(session_id.clone(), command)
        .args(command_args)
        .cwd(cwd)
        .output_byte_limit(MAX_OUTPUT as u64);
    let created = tokio::select! {
        result = connection.send_request(create).block_task() => result,
        _ = cancellation.changed() => {
            return finish(connection, session_id, call, Err("cancelled".into()));
        }
    };
    let terminal_id = match created {
        Ok(response) => response.terminal_id,
        Err(error) => {
            return finish(
                connection,
                session_id,
                call,
                Err(format!("failed to create terminal: {error}")),
            )
        }
    };

    terminal_update(connection, session_id, call, terminal_id.clone());
    let wait = connection.send_request(WaitForTerminalExitRequest::new(
        session_id.clone(),
        terminal_id.clone(),
    ));
    let exit = tokio::select! {
        result = wait.block_task() => result.map_err(|error| format!("failed waiting for terminal: {error}")),
        _ = cancellation.changed() => {
            let _ = connection
                .send_request(KillTerminalRequest::new(session_id.clone(), terminal_id.clone()))
                .block_task()
                .await;
            Err("cancelled".into())
        }
    };

    let output = connection
        .send_request(TerminalOutputRequest::new(
            session_id.clone(),
            terminal_id.clone(),
        ))
        .block_task()
        .await;
    let _ = connection
        .send_request(ReleaseTerminalRequest::new(session_id.clone(), terminal_id))
        .block_task()
        .await;

    let result = match (exit, output) {
        (Ok(exit), Ok(output)) => {
            let status = exit.exit_status;
            let status_text = status.exit_code.map_or_else(
                || format!("signal {}", status.signal.as_deref().unwrap_or("unknown")),
                |code| format!("exit code {code}"),
            );
            let rendered = truncate(format!("{}\n[{status_text}]", output.output));
            if status.exit_code == Some(0) {
                Ok(rendered)
            } else {
                Err(rendered)
            }
        }
        (Err(error), Ok(output)) if error == "cancelled" => {
            Err(truncate(format!("cancelled\n{}", output.output)))
        }
        (Err(error), Ok(output)) if !output.output.is_empty() => {
            Err(truncate(format!("{error}\n{}", output.output)))
        }
        (Err(error), _) => Err(error),
        (_, Err(error)) => Err(format!("failed reading terminal output: {error}")),
    };
    finish(connection, session_id, call, result)
}

fn required_str<'a>(args: &'a Value, key: &str) -> Result<&'a str, String> {
    args.get(key)
        .and_then(Value::as_str)
        .ok_or_else(|| format!("missing string argument: {key}"))
}

fn optional_string_array(args: &Value, key: &str) -> Result<Vec<String>, String> {
    let Some(value) = args.get(key) else {
        return Ok(Vec::new());
    };
    value
        .as_array()
        .ok_or_else(|| format!("{key} must be an array of strings"))?
        .iter()
        .map(|value| {
            value
                .as_str()
                .map(str::to_owned)
                .ok_or_else(|| format!("{key} must contain only strings"))
        })
        .collect()
}

fn tool_kind(name: &str) -> ToolKind {
    match name {
        "run_command" => ToolKind::Execute,
        "file_command" => ToolKind::Edit,
        _ => ToolKind::Other,
    }
}

fn existing_path(workspace: &Path, value: &str) -> Result<PathBuf, String> {
    let root = workspace
        .canonicalize()
        .map_err(|e| format!("invalid workspace: {e}"))?;
    let path = workspace
        .join(safe_relative(value)?)
        .canonicalize()
        .map_err(|e| format!("path not found: {e}"))?;
    if !path.starts_with(&root) {
        return Err("path escapes workspace".into());
    }
    Ok(path)
}

fn safe_relative(value: &str) -> Result<PathBuf, String> {
    let path = Path::new(value);
    if path.is_absolute()
        || path.components().any(|part| {
            matches!(
                part,
                Component::ParentDir | Component::RootDir | Component::Prefix(_)
            )
        })
    {
        return Err("path must be relative and remain inside the workspace".into());
    }
    Ok(path.to_owned())
}

fn tool_title(call: &ModelToolCall, args: &Value) -> String {
    if call.name == "run_command" {
        let command = args
            .get("command")
            .and_then(Value::as_str)
            .unwrap_or("<invalid command>");
        let suffix = args
            .get("args")
            .and_then(Value::as_array)
            .into_iter()
            .flatten()
            .filter_map(Value::as_str)
            .map(quote_command_part)
            .collect::<Vec<_>>()
            .join(" ");
        return if suffix.is_empty() {
            command.to_owned()
        } else {
            format!("{command} {suffix}")
        };
    }
    if call.name == "file_command" {
        return args
            .get("path")
            .and_then(Value::as_str)
            .map_or_else(|| call.name.clone(), |path| format!("file {path}"));
    }
    call.name.clone()
}

fn quote_command_part(value: &str) -> String {
    if value
        .chars()
        .all(|character| character.is_ascii_alphanumeric() || "-._/:=@+".contains(character))
    {
        value.to_owned()
    } else {
        format!("{:?}", value)
    }
}

fn terminal_update(
    connection: &ConnectionTo<Client>,
    session_id: &SessionId,
    call: &ModelToolCall,
    terminal_id: agent_client_protocol::schema::v1::TerminalId,
) {
    let fields = ToolCallUpdateFields::new()
        .status(ToolCallStatus::InProgress)
        .content(vec![ToolCallContent::Terminal(Terminal::new(terminal_id))]);
    let _ = connection.send_notification(SessionNotification::new(
        session_id.clone(),
        SessionUpdate::ToolCallUpdate(ToolCallUpdate::new(call.id.clone(), fields)),
    ));
}

fn update(
    connection: &ConnectionTo<Client>,
    session_id: &SessionId,
    call: &ModelToolCall,
    status: ToolCallStatus,
    result: Option<&str>,
) {
    let mut fields = ToolCallUpdateFields::new().status(status);
    if let Some(result) = result {
        fields = fields
            .content(vec![ToolCallContent::from(ContentBlock::Text(
                agent_client_protocol::schema::v1::TextContent::new(result),
            ))])
            .raw_output(json!({ "text": result }));
    }
    let _ = connection.send_notification(SessionNotification::new(
        session_id.clone(),
        SessionUpdate::ToolCallUpdate(ToolCallUpdate::new(call.id.clone(), fields)),
    ));
}

fn finish(
    connection: &ConnectionTo<Client>,
    session_id: &SessionId,
    call: &ModelToolCall,
    result: Result<String, String>,
) -> String {
    match result {
        Ok(output) => {
            update(
                connection,
                session_id,
                call,
                ToolCallStatus::Completed,
                Some(&output),
            );
            output
        }
        Err(error) => {
            update(
                connection,
                session_id,
                call,
                ToolCallStatus::Failed,
                Some(&error),
            );
            format!("Error: {error}")
        }
    }
}

fn truncate(mut value: String) -> String {
    if value.len() > MAX_OUTPUT {
        const MARKER: &str = "\n[truncated]";

        // `String::truncate` requires a UTF-8 character boundary. Tool output
        // is limited in bytes, including the marker, so walk backward from the
        // content limit when it happens to split a multi-byte character.
        let mut boundary = MAX_OUTPUT - MARKER.len();
        while !value.is_char_boundary(boundary) {
            boundary -= 1;
        }
        value.truncate(boundary);
        value.push_str(MARKER);
    }
    value
}

#[cfg(test)]
mod tests {
    use super::*;
    use agent_client_protocol::schema::v1::{RequestPermissionResponse, SelectedPermissionOutcome};

    fn model_call(name: &str) -> ModelToolCall {
        ModelToolCall {
            id: "call-1".into(),
            name: name.into(),
            arguments: "{}".into(),
        }
    }

    fn selected(option: &'static str) -> Result<RequestPermissionResponse, ()> {
        Ok(RequestPermissionResponse::new(
            RequestPermissionOutcome::Selected(SelectedPermissionOutcome::new(option)),
        ))
    }

    #[test]
    fn only_command_and_file_tools_have_permission_keys() {
        let command = ModelToolCall {
            id: "call-1".into(),
            name: "run_command".into(),
            arguments: String::new(),
        };
        assert!(permission_key(&command, &json!({ "command": "cargo" })).is_some());
        assert!(
            permission_key(&model_call("file_command"), &json!({ "path": "a.txt" })).is_some()
        );
        assert!(permission_key(&model_call("read_file"), &json!({})).is_none());
    }

    #[test]
    fn truncation_is_safe_when_limit_splits_a_utf8_character() {
        let content_limit = MAX_OUTPUT - "\n[truncated]".len();
        let mut input = "a".repeat(content_limit - 1);
        input.push('🙂');
        input.push_str("unreachable output beyond the limit");

        let output = truncate(input);

        assert!(output.starts_with('a'));
        assert!(output.ends_with("\n[truncated]"));
        assert!(output.len() <= MAX_OUTPUT);
        assert!(output.is_char_boundary(output.len()));
    }

    #[test]
    fn truncation_keeps_output_at_or_below_the_byte_limit() {
        let input = format!("{}extra", "a".repeat(MAX_OUTPUT));
        let output = truncate(input);
        assert_eq!(output.len(), MAX_OUTPUT);
        assert!(output.ends_with("\n[truncated]"));
    }

    #[test]
    fn rejects_parent_and_absolute_paths() {
        assert!(safe_relative("../secret").is_err());
        assert!(safe_relative("/etc/passwd").is_err());
        assert!(safe_relative("src/lib.rs").is_ok());
    }

    #[test]
    fn permission_keys_are_exact_and_stable() {
        let call = ModelToolCall {
            id: "call-1".into(),
            name: "run_command".into(),
            arguments: String::new(),
        };
        let first = permission_key(
            &call,
            &json!({ "command": "cargo", "args": ["test"], "cwd": "." }),
        );
        let same = permission_key(
            &call,
            &json!({ "cwd": ".", "args": ["test"], "command": "cargo" }),
        );
        let different = permission_key(
            &call,
            &json!({ "command": "cargo", "args": ["build"], "cwd": "." }),
        );
        assert_eq!(first, same);
        assert_ne!(first, different);
    }

    #[test]
    fn auto_approve_allows_once_without_remembering() {
        let permissions = PermissionState::default();
        let key = PermissionKey::RunCommand {
            command: "cargo".into(),
            args: vec!["test".into()],
            cwd: ".".into(),
        };
        assert!(permission_outcome(
            selected("auto-approve"),
            &permissions,
            key.clone()
        ));
        assert!(!permissions.has_full_access());
        assert_eq!(permissions.decision(&key), None);
    }

    #[test]
    fn full_access_remembers_and_disables_later_prompts() {
        let permissions = PermissionState::default();
        let key = PermissionKey::RunCommand {
            command: "cargo".into(),
            args: vec!["test".into()],
            cwd: ".".into(),
        };
        assert!(permission_outcome(
            selected("full-access"),
            &permissions,
            key
        ));
        assert!(permissions.has_full_access());
        let other = PermissionKey::RunCommand {
            command: "rm".into(),
            args: vec!["-rf".into()],
            cwd: ".".into(),
        };
        assert_eq!(permissions.decision(&other), Some(true));
    }

    #[test]
    fn dismissal_denies_without_remembering() {
        let permissions = PermissionState::default();
        let key = PermissionKey::RunCommand {
            command: "cargo".into(),
            args: vec!["test".into()],
            cwd: ".".into(),
        };
        assert!(!permission_outcome(
            selected("reject-once"),
            &permissions,
            key.clone()
        ));
        assert!(!permissions.has_full_access());
        assert_eq!(permissions.decision(&key), None);
    }
}
