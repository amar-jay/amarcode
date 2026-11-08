use std::{
    collections::HashMap,
    io::Read,
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
        if *self.full_access.lock().ok()? {
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

    pub fn set_full_access(&self, enabled: bool) {
        *self.full_access.lock().expect("permission lock") = enabled;
        self.remembered.lock().expect("permission lock").clear();
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
    // Reading is always non-sensitive; it never needs a permission dialog.
    let sensitive = if call.name == "file_command"
        && arguments.get("operation").and_then(Value::as_str) == Some("read")
    {
        false
    } else {
        arguments
            .get("sensitive")
            .and_then(Value::as_bool)
            .unwrap_or(true)
    };
    // Whether this call was auto-approved without showing the host permission
    // dialog. When true the daemon must not require a recorded terminal
    // permission; when false a permission dialog was shown (or is needed), so
    // the daemon keeps enforcing its exact-command permission.
    let mut auto_approved = !sensitive;
    if let Some(permission_key) = permission_key(call, &arguments).filter(|_| sensitive) {
        let allowed = if let Some(allowed) = permissions.decision(&permission_key) {
            auto_approved = allowed;
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
        if arguments.get("operation").and_then(Value::as_str) == Some("read") {
            let workspace = workspace.to_owned();
            let result =
                tokio::task::spawn_blocking(move || read_workspace_file(&workspace, &arguments))
                    .await
                    .unwrap_or_else(|error| Err(format!("file worker failed: {error}")));
            return finish(connection, session_id, call, result);
        }
        let workspace = workspace.to_owned();
        let result =
            tokio::task::spawn_blocking(move || apply_file_command(&workspace, &arguments))
                .await
                .unwrap_or_else(|error| Err(format!("file worker failed: {error}")));
        return finish_file_change(connection, session_id, call, result);
    }
    return run_command(
        workspace,
        &arguments,
        connection,
        session_id,
        call,
        cancellation,
        auto_approved,
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

/// Read a workspace file or list a workspace directory without a shell.
/// The model can use this to inspect files even when terminal access is
/// restricted. Returns the file contents or a directory listing, bounded to
/// [`MAX_OUTPUT`] bytes.
fn read_workspace_file(workspace: &Path, args: &Value) -> Result<String, String> {
    let path = existing_path(workspace, required_str(args, "path")?)?;
    if path.is_dir() {
        let mut entries = Vec::new();
        let mut directory = std::fs::read_dir(&path)
            .map_err(|error| format!("failed to list {}: {error}", path.display()))?;
        for entry in directory.by_ref() {
            let entry =
                entry.map_err(|error| format!("failed to read directory entry: {error}"))?;
            let file_name = entry.file_name().to_string_lossy().into_owned();
            let kind = if entry.file_type().map(|t| t.is_dir()).unwrap_or(false) {
                "dir"
            } else {
                "file"
            };
            entries.push(format!("{kind}\t{file_name}"));
        }
        if entries.is_empty() {
            return Ok(format!("(empty directory: {})", path.display()));
        }
        entries.sort();
        return Ok(truncate(format!(
            "{}\n{}",
            path.display(),
            entries.join("\n")
        )));
    }
    if !path.is_file() {
        return Err(format!("not a file or directory: {}", path.display()));
    }
    let file = std::fs::File::open(&path)
        .map_err(|error| format!("failed to read {}: {error}", path.display()))?;
    let metadata = file
        .metadata()
        .map_err(|error| format!("failed to inspect {}: {error}", path.display()))?;
    if !metadata.is_file() {
        return Err(format!("not a regular file: {}", path.display()));
    }
    // Read one extra byte to detect truncation without allocating the whole
    // file, even if it grows after metadata was read.
    let mut contents = Vec::new();
    file.take(MAX_OUTPUT as u64 + 1)
        .read_to_end(&mut contents)
        .map_err(|error| format!("failed to read {}: {error}", path.display()))?;
    // A bounded read can end inside a UTF-8 character. Preserve the valid
    // prefix rather than misclassifying that text file as binary.
    match std::str::from_utf8(&contents) {
        Ok(text) => Ok(truncate(text.to_owned())),
        Err(error) if contents.len() > MAX_OUTPUT && error.error_len().is_none() => {
            let text = std::str::from_utf8(&contents[..error.valid_up_to()])
                .expect("UTF-8 error identifies a valid prefix");
            Ok(truncate(format!("{text}\n[truncated]")))
        }
        Err(_) => Ok(truncate(format!(
            "(binary file {}: {} bytes)",
            path.display(),
            metadata.len()
        ))),
    }
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
            if let Ok(metadata) = std::fs::symlink_metadata(&path) {
                if metadata.file_type().is_symlink() {
                    return Err("destination must not be a symlink".into());
                }
                let canonical_target = path
                    .canonicalize()
                    .map_err(|e| format!("invalid destination: {e}"))?;
                if !canonical_target.starts_with(&canonical_root) {
                    return Err("path escapes workspace".into());
                }
            }
            if path.exists() && args.get("overwrite").and_then(Value::as_bool) != Some(true) {
                return Err(
                    "file already exists; use edit or explicitly set overwrite=true".into(),
                );
            }
            let old_text = if path.is_file() {
                Some(
                    std::fs::read_to_string(&path)
                        .map_err(|e| format!("failed to read {}: {e}", path.display()))?,
                )
            } else {
                None
            };
            atomic_write(&path, content.as_bytes())
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
            atomic_write(&path, updated.as_bytes())
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

// Replace only after the complete contents have been written and synced.
fn atomic_write(path: &Path, content: &[u8]) -> std::io::Result<()> {
    use std::io::Write;
    let temporary = path.with_file_name(format!(".amarcode-{}.tmp", uuid::Uuid::new_v4()));
    let result = (|| {
        let mut options = std::fs::OpenOptions::new();
        options.write(true).create_new(true);
        #[cfg(unix)]
        {
            use std::os::unix::fs::OpenOptionsExt;
            options.mode(0o600);
        }
        let mut file = options.open(&temporary)?;
        if let Ok(metadata) = std::fs::metadata(path) {
            file.set_permissions(metadata.permissions())?;
        }
        file.write_all(content)?;
        file.sync_all()?;
        drop(file);
        std::fs::rename(&temporary, path)
    })();
    if result.is_err() {
        let _ = std::fs::remove_file(&temporary);
    }
    result
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
    auto_approved: bool,
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

    let mut meta = serde_json::Map::new();
    meta.insert("amarcode".into(), json!({ "sensitive": !auto_approved }));
    let create = CreateTerminalRequest::new(session_id.clone(), command)
        .args(command_args)
        .cwd(cwd)
        .output_byte_limit(MAX_OUTPUT as u64)
        .meta(meta);
    let creating = connection.send_request(create).block_task();
    tokio::pin!(creating);
    let created = tokio::select! {
        result = &mut creating => result,
        _ = cancellation.changed() => {
            // Obtain the ID if creation is already in flight, then kill below.
            match tokio::time::timeout(std::time::Duration::from_secs(1), &mut creating).await {
                Ok(result) => result,
                Err(_) => return finish(connection, session_id, call, Err("cancelled; terminal creation did not acknowledge cancellation".into())),
            }
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
        _ = async { if !*cancellation.borrow() { let _ = cancellation.changed().await; } } => {
            let _ = tokio::time::timeout(std::time::Duration::from_secs(1), connection
                .send_request(KillTerminalRequest::new(session_id.clone(), terminal_id.clone()))
                .block_task()).await;
            Err("cancelled".into())
        }
    };

    let output = tokio::time::timeout(
        std::time::Duration::from_secs(1),
        connection
            .send_request(TerminalOutputRequest::new(
                session_id.clone(),
                terminal_id.clone(),
            ))
            .block_task(),
    )
    .await;
    let _ = tokio::time::timeout(
        std::time::Duration::from_secs(1),
        connection
            .send_request(ReleaseTerminalRequest::new(session_id.clone(), terminal_id))
            .block_task(),
    )
    .await;
    let output = match output {
        Ok(result) => result.map_err(|error| error.to_string()),
        Err(_) => Err("terminal output timed out".to_owned()),
    };

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
    fn file_create_edit_and_overwrite_are_explicit() {
        let root = std::env::temp_dir().join(uuid::Uuid::new_v4().to_string());
        std::fs::create_dir(&root).unwrap();
        let create = json!({"operation":"create","path":"note.txt","content":"hello world"});
        assert!(apply_file_command(&root, &create).is_ok());
        assert!(apply_file_command(&root, &create).is_err());
        let edit =
            json!({"operation":"edit","path":"note.txt","old_text":"world","content":"friend"});
        let change = apply_file_command(&root, &edit).unwrap();
        assert_eq!(change.old_text.as_deref(), Some("hello world"));
        assert_eq!(
            std::fs::read_to_string(root.join("note.txt")).unwrap(),
            "hello friend"
        );
        assert!(apply_file_command(&root, &edit).is_err());
        let replace =
            json!({"operation":"create","path":"note.txt","content":"aa aa","overwrite":true});
        apply_file_command(&root, &replace).unwrap();
        assert!(apply_file_command(
            &root,
            &json!({"operation":"edit","path":"note.txt","old_text":"aa","content":"b"})
        )
        .is_err());
        assert!(apply_file_command(
            &root,
            &json!({"operation":"create","path":"../escape","content":"x"})
        )
        .is_err());
        assert_eq!(std::fs::read_dir(&root).unwrap().count(), 1);
        std::fs::remove_dir_all(root).unwrap();
    }

    #[test]
    fn file_read_returns_file_and_lists_directory() {
        let root = std::env::temp_dir().join(uuid::Uuid::new_v4().to_string());
        std::fs::create_dir(&root).unwrap();
        std::fs::create_dir(root.join("sub")).unwrap();
        std::fs::write(root.join("note.txt"), "hello world").unwrap();
        std::fs::write(root.join("sub").join("nested.md"), "# Hi").unwrap();

        let file = read_workspace_file(&root, &json!({"operation":"read","path":"note.txt"}))
            .expect("read file");
        assert_eq!(file, "hello world");

        let directory = read_workspace_file(&root, &json!({"operation":"read","path":"."}))
            .expect("list directory");
        assert!(directory.contains("note.txt"));
        assert!(directory.contains("sub"));

        // Reading never requires a sensitive declaration and is bounded.
        assert!(read_workspace_file(&root, &json!({"operation":"read","path":"missing"})).is_err());
        assert!(
            read_workspace_file(&root, &json!({"operation":"read","path":"../escape"})).is_err()
        );

        std::fs::remove_dir_all(root).unwrap();
    }

    #[test]
    fn file_reads_stop_at_the_output_limit_and_preserve_split_utf8() {
        use std::io::{Seek, SeekFrom, Write};

        let root = std::env::temp_dir().join(uuid::Uuid::new_v4().to_string());
        std::fs::create_dir(&root).unwrap();
        let path = root.join("large.txt");
        let mut file = std::fs::File::create(&path).unwrap();
        file.write_all("a".repeat(MAX_OUTPUT - 1).as_bytes())
            .unwrap();
        // The bounded read ends partway through this character.
        file.write_all("€".as_bytes()).unwrap();
        let size = 16 * 1024 * 1024;
        file.set_len(size).unwrap();
        file.seek(SeekFrom::End(-1)).unwrap();
        // Reading the whole file would encounter this invalid UTF-8 byte and
        // return a binary summary instead of the bounded text prefix.
        file.write_all(&[0xff]).unwrap();
        drop(file);
        let args = json!({"operation":"read","path":"large.txt"});
        let output = read_workspace_file(&root, &args).unwrap();
        assert!(output.starts_with("aaaa"));
        assert!(output.ends_with("\n[truncated]"));
        assert!(output.len() <= MAX_OUTPUT);

        let mut file = std::fs::OpenOptions::new().write(true).open(&path).unwrap();
        file.write_all(&[0xff]).unwrap();
        drop(file);
        let output = read_workspace_file(&root, &args).unwrap();
        assert!(output.starts_with("(binary file "));
        assert!(output.ends_with(&format!(": {size} bytes)")));

        // An incomplete character at a real EOF is still invalid UTF-8.
        std::fs::write(&path, [0xe2, 0x82]).unwrap();
        assert!(read_workspace_file(&root, &args)
            .unwrap()
            .starts_with("(binary file "));
        std::fs::remove_dir_all(root).unwrap();
    }

    #[cfg(unix)]
    #[test]
    fn file_commands_reject_symlink_escapes() {
        use std::os::unix::fs::symlink;
        let root = std::env::temp_dir().join(uuid::Uuid::new_v4().to_string());
        let outside = std::env::temp_dir().join(uuid::Uuid::new_v4().to_string());
        std::fs::create_dir(&root).unwrap();
        std::fs::create_dir(&outside).unwrap();
        symlink(outside.join("missing"), root.join("dangling")).unwrap();
        symlink(&outside, root.join("escape")).unwrap();
        for path in ["dangling", "escape/new"] {
            assert!(apply_file_command(
                &root,
                &json!({"operation":"create","path":path,"content":"x"})
            )
            .is_err());
        }
        std::fs::write(outside.join("existing"), "original").unwrap();
        assert!(apply_file_command(&root, &json!({"operation":"edit","path":"escape/existing","old_text":"original","content":"changed"})).is_err());
        assert_eq!(
            std::fs::read_to_string(outside.join("existing")).unwrap(),
            "original"
        );
        std::fs::remove_dir_all(root).unwrap();
        std::fs::remove_dir_all(outside).unwrap();
    }

    #[test]
    fn permission_mode_can_be_revoked() {
        let permissions = PermissionState::default();
        permissions.set_full_access(true);
        assert!(permissions.has_full_access());
        permissions.set_full_access(false);
        let key = PermissionKey::FileWrite { path: "a".into() };
        assert_eq!(permissions.decision(&key), None);
    }

    #[test]
    fn only_command_and_file_tools_have_permission_keys() {
        let command = ModelToolCall {
            id: "call-1".into(),
            name: "run_command".into(),
            arguments: String::new(),
        };
        assert!(permission_key(&command, &json!({ "command": "cargo" })).is_some());
        assert!(permission_key(&model_call("file_command"), &json!({ "path": "a.txt" })).is_some());
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
