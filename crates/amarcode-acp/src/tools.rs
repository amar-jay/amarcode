use std::path::{Component, Path, PathBuf};

use agent_client_protocol::{
    schema::v1::{
        ContentBlock, PermissionOption, PermissionOptionKind, RequestPermissionOutcome,
        RequestPermissionRequest, SessionId, SessionNotification, SessionUpdate,
        ToolCall as AcpToolCall, ToolCallContent, ToolCallStatus, ToolCallUpdate,
        ToolCallUpdateFields, ToolKind,
    },
    Client, ConnectionTo,
};
use serde_json::{json, Value};
use tokio::sync::watch;
use walkdir::WalkDir;

use crate::provider::ModelToolCall;

const MAX_OUTPUT: usize = 64 * 1024;

pub async fn execute(
    call: &ModelToolCall,
    workspace: &Path,
    mode: &str,
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

    if call.name == "write_file" {
        if mode != "code" {
            return finish(
                connection,
                session_id,
                call,
                Err("write_file is only available in code mode".into()),
            );
        }
        let permission = connection.send_request(RequestPermissionRequest::new(
            session_id.clone(),
            ToolCallUpdate::from(started),
            vec![
                PermissionOption::new("allow-once", "Allow once", PermissionOptionKind::AllowOnce),
                PermissionOption::new("reject-once", "Reject", PermissionOptionKind::RejectOnce),
            ],
        ));
        let response = tokio::select! {
            result = permission.block_task() => result,
            _ = cancellation.changed() => return finish(connection, session_id, call, Err("cancelled".into())),
        };
        let allowed = matches!(response,
            Ok(response) if matches!(&response.outcome, RequestPermissionOutcome::Selected(selected) if selected.option_id.to_string() == "allow-once")
        );
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
    let result = match call.name.as_str() {
        "read_file" => read_file(workspace, &arguments),
        "list_directory" => list_directory(workspace, &arguments),
        "search_text" => search_text(workspace, &arguments),
        "write_file" => write_file(workspace, &arguments),
        other => Err(format!("unknown tool: {other}")),
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

fn tool_kind(name: &str) -> ToolKind {
    match name {
        "read_file" => ToolKind::Read,
        "list_directory" | "search_text" => ToolKind::Search,
        "write_file" => ToolKind::Edit,
        _ => ToolKind::Other,
    }
}

fn tool_title(call: &ModelToolCall, args: &Value) -> String {
    args.get("path").and_then(Value::as_str).map_or_else(
        || call.name.clone(),
        |path| format!("{} {path}", call.name.replace('_', " ")),
    )
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

    #[test]
    fn rejects_parent_and_absolute_paths() {
        assert!(safe_relative("../secret").is_err());
        assert!(safe_relative("/etc/passwd").is_err());
        assert!(safe_relative("src/lib.rs").is_ok());
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
