use std::{
    collections::HashMap,
    path::{Component, Path, PathBuf},
    sync::Mutex,
};

use agent_client_protocol::{
    schema::v1::{
        ContentBlock, CreateTerminalRequest, KillTerminalRequest, PermissionOption,
        PermissionOptionKind, ReleaseTerminalRequest, RequestPermissionOutcome,
        RequestPermissionRequest, SessionId, SessionNotification, SessionUpdate, Terminal,
        TerminalOutputRequest, ToolCall as AcpToolCall, ToolCallContent, ToolCallStatus,
        ToolCallUpdate, ToolCallUpdateFields, ToolKind, WaitForTerminalExitRequest,
    },
    Client, ConnectionTo,
};
use serde_json::{json, Value};
use tokio::sync::watch;
use walkdir::WalkDir;

use crate::provider::ModelToolCall;

const MAX_OUTPUT: usize = 64 * 1024;

#[derive(Debug, Clone, PartialEq, Eq, Hash)]
enum PermissionKey {
    WriteFile {
        path: String,
    },
    RunCommand {
        command: String,
        args: Vec<String>,
        cwd: String,
    },
}

/// Decisions selected with an ACP `*_always` option, scoped to one session.
#[derive(Debug, Default)]
pub struct PermissionState {
    remembered: Mutex<HashMap<PermissionKey, bool>>,
}

impl PermissionState {
    fn decision(&self, key: &PermissionKey) -> Option<bool> {
        self.remembered.lock().ok()?.get(key).copied()
    }

    fn remember(&self, key: PermissionKey, allowed: bool) {
        if let Ok(mut remembered) = self.remembered.lock() {
            remembered.insert(key, allowed);
        }
    }
}

pub async fn execute(
    call: &ModelToolCall,
    workspace: &Path,
    mode: &str,
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

    if let Some(permission_key) = permission_key(call, &arguments) {
        if mode != "code" {
            return finish(
                connection,
                session_id,
                call,
                Err(format!("{} is only available in code mode", call.name)),
            );
        }
        let allowed = if let Some(allowed) = permissions.decision(&permission_key) {
            allowed
        } else {
            let permission = connection.send_request(RequestPermissionRequest::new(
                session_id.clone(),
                ToolCallUpdate::from(started),
                vec![
                    PermissionOption::new(
                        "allow-once",
                        "Allow once",
                        PermissionOptionKind::AllowOnce,
                    ),
                    PermissionOption::new(
                        "allow-always",
                        "Allow for this session",
                        PermissionOptionKind::AllowAlways,
                    ),
                    PermissionOption::new(
                        "reject-once",
                        "Reject",
                        PermissionOptionKind::RejectOnce,
                    ),
                    PermissionOption::new(
                        "reject-always",
                        "Reject for this session",
                        PermissionOptionKind::RejectAlways,
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
    if call.name == "run_command" {
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
    let result = match call.name.as_str() {
        "read_file" => read_file(workspace, &arguments),
        "list_directory" => list_directory(workspace, &arguments),
        "search_text" => search_text(workspace, &arguments),
        "write_file" => write_file(workspace, &arguments),
        other => Err(format!("unknown tool: {other}")),
    };
    finish(connection, session_id, call, result)
}

fn permission_key(call: &ModelToolCall, arguments: &Value) -> Option<PermissionKey> {
    match call.name.as_str() {
        "write_file" => Some(PermissionKey::WriteFile {
            path: arguments.get("path")?.as_str()?.to_owned(),
        }),
        "run_command" => Some(PermissionKey::RunCommand {
            command: arguments.get("command")?.as_str()?.to_owned(),
            args: optional_string_array(arguments, "args").ok()?,
            cwd: arguments
                .get("cwd")
                .and_then(Value::as_str)
                .unwrap_or(".")
                .to_owned(),
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
        "allow-once" => true,
        "allow-always" => {
            permissions.remember(key, true);
            true
        }
        "reject-always" => {
            permissions.remember(key, false);
            false
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

fn read_file(workspace: &Path, args: &Value) -> Result<String, String> {
    let path = existing_path(workspace, required_str(args, "path")?)?;
    std::fs::read_to_string(&path)
        .map(truncate)
        .map_err(|e| format!("failed to read {}: {e}", path.display()))
}

fn list_directory(workspace: &Path, args: &Value) -> Result<String, String> {
    let path = existing_path(workspace, required_str(args, "path")?)?;
    let mut entries = std::fs::read_dir(&path)
        .map_err(|e| format!("failed to list {}: {e}", path.display()))?
        .map(|entry| {
            entry
                .map(|entry| entry.file_name().to_string_lossy().into_owned())
                .map_err(|e| e.to_string())
        })
        .collect::<Result<Vec<_>, _>>()?;
    entries.sort();
    Ok(truncate(entries.join("\n")))
}

fn search_text(workspace: &Path, args: &Value) -> Result<String, String> {
    let query = required_str(args, "query")?;
    if query.is_empty() {
        return Err("query must not be empty".into());
    }
    let start = existing_path(
        workspace,
        args.get("path").and_then(Value::as_str).unwrap_or("."),
    )?;
    let mut matches = String::new();
    for entry in WalkDir::new(start)
        .follow_links(false)
        .into_iter()
        .filter_map(Result::ok)
        .filter(|e| e.file_type().is_file())
        .take(10_000)
    {
        let Ok(text) = std::fs::read_to_string(entry.path()) else {
            continue;
        };
        for (line, content) in text.lines().enumerate() {
            if content.contains(query) {
                let relative = entry.path().strip_prefix(workspace).unwrap_or(entry.path());
                matches.push_str(&format!(
                    "{}:{}:{}\n",
                    relative.display(),
                    line + 1,
                    content
                ));
                if matches.len() >= MAX_OUTPUT {
                    return Ok(truncate(matches));
                }
            }
        }
    }
    Ok(if matches.is_empty() {
        "No matches found.".into()
    } else {
        matches
    })
}

fn write_file(workspace: &Path, args: &Value) -> Result<String, String> {
    let relative = safe_relative(required_str(args, "path")?)?;
    let path = workspace.join(relative);
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
    if path.exists() {
        let canonical_target = path
            .canonicalize()
            .map_err(|e| format!("invalid destination: {e}"))?;
        if !canonical_target.starts_with(&canonical_root) {
            return Err("path escapes workspace".into());
        }
    }
    let content = required_str(args, "content")?;
    std::fs::write(&path, content)
        .map_err(|e| format!("failed to write {}: {e}", path.display()))?;
    Ok(format!(
        "Wrote {} bytes to {}",
        content.len(),
        path.display()
    ))
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
        "read_file" => ToolKind::Read,
        "list_directory" | "search_text" => ToolKind::Search,
        "write_file" => ToolKind::Edit,
        "run_command" => ToolKind::Execute,
        _ => ToolKind::Other,
    }
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
    args.get("path").and_then(Value::as_str).map_or_else(
        || call.name.clone(),
        |path| format!("{} {path}", call.name.replace('_', " ")),
    )
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
        value.truncate(MAX_OUTPUT);
        value.push_str("\n[truncated]");
    }
    value
}

#[cfg(test)]
mod tests {
    use super::*;
    use agent_client_protocol::schema::v1::{RequestPermissionResponse, SelectedPermissionOutcome};

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
    fn always_outcomes_are_remembered() {
        let permissions = PermissionState::default();
        let allowed_key = PermissionKey::WriteFile {
            path: "src/lib.rs".into(),
        };
        let allowed = RequestPermissionResponse::new(RequestPermissionOutcome::Selected(
            SelectedPermissionOutcome::new("allow-always"),
        ));
        assert!(permission_outcome(
            Ok::<_, ()>(allowed),
            &permissions,
            allowed_key.clone(),
        ));
        assert_eq!(permissions.decision(&allowed_key), Some(true));

        let rejected_key = PermissionKey::WriteFile {
            path: "src/main.rs".into(),
        };
        let rejected = RequestPermissionResponse::new(RequestPermissionOutcome::Selected(
            SelectedPermissionOutcome::new("reject-always"),
        ));
        assert!(!permission_outcome(
            Ok::<_, ()>(rejected),
            &permissions,
            rejected_key.clone(),
        ));
        assert_eq!(permissions.decision(&rejected_key), Some(false));
    }

    #[cfg(unix)]
    #[test]
    fn write_rejects_symlink_that_targets_outside_workspace() {
        use std::os::unix::fs::symlink;

        let root =
            std::env::temp_dir().join(format!("amarcode-tool-root-{}", uuid::Uuid::new_v4()));
        let outside =
            std::env::temp_dir().join(format!("amarcode-tool-outside-{}", uuid::Uuid::new_v4()));
        std::fs::create_dir(&root).expect("create root");
        std::fs::write(&outside, "secret").expect("create outside file");
        symlink(&outside, root.join("escape.txt")).expect("create symlink");

        let result = write_file(
            &root,
            &json!({ "path": "escape.txt", "content": "overwrite" }),
        );
        assert_eq!(result.expect_err("reject escape"), "path escapes workspace");
        assert_eq!(
            std::fs::read_to_string(&outside).expect("read outside"),
            "secret"
        );

        let _ = std::fs::remove_dir_all(root);
        let _ = std::fs::remove_file(outside);
    }
}
