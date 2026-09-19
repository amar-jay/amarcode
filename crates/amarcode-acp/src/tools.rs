use std::{collections::HashMap, path::Path, sync::Mutex};

use crate::provider::ModelToolCall;
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

mod filesystem;

use filesystem::{edit_file, existing_path, list_directory, read_file, search_text, write_file};

const MAX_OUTPUT: usize = 64 * 1024;

pub fn is_search_tool(call: &ModelToolCall) -> bool {
    matches!(call.name.as_str(), "search_text" | "list_directory")
}

/// Execute a read-only search call without publishing its individual ACP
/// lifecycle. The runtime uses this for multi-call batches and publishes one
/// synthetic search lifecycle while retaining every real result in model
/// history.
pub fn execute_grouped_search(call: &ModelToolCall, workspace: &Path) -> Result<String, String> {
    let arguments = serde_json::from_str::<Value>(&call.arguments)
        .map_err(|error| format!("invalid arguments: {error}"))?;
    match call.name.as_str() {
        "search_text" => search_text(workspace, &arguments),
        "list_directory" => list_directory(workspace, &arguments),
        other => Err(format!("cannot group non-search tool: {other}")),
    }
}

pub fn begin_search_group(
    connection: &ConnectionTo<Client>,
    session_id: &SessionId,
    group_id: &str,
) {
    let started = AcpToolCall::new(group_id.to_owned(), "Searching")
        .kind(ToolKind::Search)
        .status(ToolCallStatus::InProgress)
        .raw_input(json!({ "grouped": true }));
    let _ = connection.send_notification(SessionNotification::new(
        session_id.clone(),
        SessionUpdate::ToolCall(started),
    ));
}

pub fn finish_search_group(
    connection: &ConnectionTo<Client>,
    session_id: &SessionId,
    group_id: &str,
    count: usize,
    failed: usize,
) {
    let status = if failed == 0 {
        ToolCallStatus::Completed
    } else {
        ToolCallStatus::Failed
    };
    let summary = if failed == 0 {
        format!("Completed {count} search operations")
    } else {
        format!("Completed {count} search operations with {failed} failure(s)")
    };
    let fields = ToolCallUpdateFields::new()
        .status(status)
        .content(vec![ToolCallContent::from(ContentBlock::Text(
            agent_client_protocol::schema::v1::TextContent::new(summary.clone()),
        ))])
        .raw_output(json!({
            "operationCount": count,
            "failedOperationCount": failed,
            "text": summary,
        }));
    let _ = connection.send_notification(SessionNotification::new(
        session_id.clone(),
        SessionUpdate::ToolCallUpdate(ToolCallUpdate::new(group_id.to_owned(), fields)),
    ));
}

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
        "edit_file" => edit_file(workspace, &arguments),
        "write_file" => write_file(workspace, &arguments),
        other => Err(format!("unknown tool: {other}")),
    };
    finish(connection, session_id, call, result)
}

fn permission_key(call: &ModelToolCall, arguments: &Value) -> Option<PermissionKey> {
    match call.name.as_str() {
        "write_file" | "edit_file" => Some(PermissionKey::WriteFile {
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
        "write_file" | "edit_file" => ToolKind::Edit,
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
    use super::filesystem::{edit_file, read_file, safe_relative, write_file};
    use super::*;
    use agent_client_protocol::schema::v1::{RequestPermissionResponse, SelectedPermissionOutcome};

    fn model_call(name: &str) -> ModelToolCall {
        ModelToolCall {
            id: "call-1".into(),
            name: name.into(),
            arguments: "{}".into(),
        }
    }

    #[test]
    fn groups_only_read_only_search_operations() {
        assert!(is_search_tool(&model_call("search_text")));
        assert!(is_search_tool(&model_call("list_directory")));
        assert!(!is_search_tool(&model_call("read_file")));
        assert!(!is_search_tool(&model_call("run_command")));
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

        let write = permission_key(
            &model_call("write_file"),
            &json!({ "path": "src/lib.rs", "content": "full" }),
        );
        let edit = permission_key(
            &model_call("edit_file"),
            &json!({ "path": "src/lib.rs", "old_text": "old", "new_text": "new" }),
        );
        assert_eq!(write, edit);
    }

    #[test]
    fn targeted_edit_replaces_one_unique_match() {
        let root =
            std::env::temp_dir().join(format!("amarcode-edit-root-{}", uuid::Uuid::new_v4()));
        std::fs::create_dir(&root).expect("create root");
        let path = root.join("example.txt");
        std::fs::write(&path, "before\nold block\nafter\n").expect("write fixture");

        let result = edit_file(
            &root,
            &json!({
                "path": "example.txt",
                "old_text": "old block",
                "new_text": "new block"
            }),
        )
        .expect("edit file");

        assert!(result.contains("Replaced 9 bytes with 9 bytes"));
        assert_eq!(
            std::fs::read_to_string(&path).expect("read result"),
            "before\nnew block\nafter\n"
        );
        let _ = std::fs::remove_dir_all(root);
    }

    #[test]
    fn targeted_edit_rejects_stale_and_ambiguous_matches_without_writing() {
        let root =
            std::env::temp_dir().join(format!("amarcode-edit-root-{}", uuid::Uuid::new_v4()));
        std::fs::create_dir(&root).expect("create root");
        let path = root.join("example.txt");
        let original = "repeat\nmiddle\nrepeat\n";
        std::fs::write(&path, original).expect("write fixture");

        let missing = edit_file(
            &root,
            &json!({ "path": "example.txt", "old_text": "stale", "new_text": "new" }),
        );
        assert!(missing
            .expect_err("reject stale edit")
            .contains("not found"));
        let ambiguous = edit_file(
            &root,
            &json!({ "path": "example.txt", "old_text": "repeat", "new_text": "new" }),
        );
        assert!(ambiguous
            .expect_err("reject ambiguous edit")
            .contains("matched 2 locations"));
        assert_eq!(
            std::fs::read_to_string(&path).expect("read unchanged file"),
            original
        );
        let _ = std::fs::remove_dir_all(root);
    }

    #[test]
    fn read_file_pages_without_losing_utf8_content() {
        let root =
            std::env::temp_dir().join(format!("amarcode-read-root-{}", uuid::Uuid::new_v4()));
        std::fs::create_dir(&root).expect("create root");
        let path = root.join("example.txt");
        std::fs::write(&path, "ab🙂cd").expect("write fixture");

        let first = read_file(
            &root,
            &json!({ "path": "example.txt", "offset": 0, "limit": 4 }),
        )
        .expect("read first page");
        assert_eq!(first, "ab\n[partial read: bytes 0..2 of 8; next_offset=2]");
        let second = read_file(
            &root,
            &json!({ "path": "example.txt", "offset": 2, "limit": 4 }),
        )
        .expect("read second page");
        assert_eq!(second, "🙂\n[partial read: bytes 2..6 of 8; next_offset=6]");
        let third = read_file(
            &root,
            &json!({ "path": "example.txt", "offset": 6, "limit": 4 }),
        )
        .expect("read final page");
        assert_eq!(third, "cd");
        let _ = std::fs::remove_dir_all(root);
    }

    #[test]
    fn read_file_rejects_invalid_offsets_and_limits() {
        let root =
            std::env::temp_dir().join(format!("amarcode-read-root-{}", uuid::Uuid::new_v4()));
        std::fs::create_dir(&root).expect("create root");
        std::fs::write(root.join("example.txt"), "🙂").expect("write fixture");

        let split_character = read_file(&root, &json!({ "path": "example.txt", "offset": 1 }));
        assert!(split_character
            .expect_err("reject split character")
            .contains("UTF-8 character boundary"));
        let beyond_end = read_file(&root, &json!({ "path": "example.txt", "offset": 5 }));
        assert!(beyond_end
            .expect_err("reject offset beyond end")
            .contains("beyond end of file"));
        let zero_limit = read_file(&root, &json!({ "path": "example.txt", "limit": 0 }));
        assert!(zero_limit
            .expect_err("reject zero limit")
            .contains("limit must be between"));
        let _ = std::fs::remove_dir_all(root);
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
