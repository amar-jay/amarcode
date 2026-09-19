use std::path::{Component, Path, PathBuf};

use serde_json::Value;
use walkdir::WalkDir;

use super::{required_str, truncate, MAX_OUTPUT};

const DEFAULT_READ_LIMIT: usize = 60 * 1024;

pub(super) fn read_file(workspace: &Path, args: &Value) -> Result<String, String> {
    let path = existing_path(workspace, required_str(args, "path")?)?;
    let contents = std::fs::read_to_string(&path)
        .map_err(|error| format!("failed to read {}: {error}", path.display()))?;
    let offset = optional_usize(args, "offset", 0)?;
    let limit = optional_usize(args, "limit", DEFAULT_READ_LIMIT)?;
    if limit == 0 || limit > DEFAULT_READ_LIMIT {
        return Err(format!(
            "limit must be between 1 and {DEFAULT_READ_LIMIT} bytes"
        ));
    }
    if offset > contents.len() {
        return Err(format!(
            "offset {offset} is beyond end of file ({} bytes)",
            contents.len()
        ));
    }
    if !contents.is_char_boundary(offset) {
        return Err(format!("offset {offset} is not a UTF-8 character boundary"));
    }
    let requested_end = offset.saturating_add(limit).min(contents.len());
    let mut end = requested_end;
    while end > offset && !contents.is_char_boundary(end) {
        end -= 1;
    }
    if end == offset && offset < contents.len() {
        return Err("limit is too small to include the next UTF-8 character".into());
    }
    let page = &contents[offset..end];
    if end == contents.len() {
        return Ok(page.to_owned());
    }
    Ok(format!(
        "{page}\n[partial read: bytes {offset}..{end} of {}; next_offset={end}]",
        contents.len()
    ))
}

fn optional_usize(args: &Value, key: &str, default: usize) -> Result<usize, String> {
    match args.get(key) {
        None => Ok(default),
        Some(value) => value
            .as_u64()
            .ok_or_else(|| format!("{key} must be a non-negative integer"))
            .and_then(|value| {
                usize::try_from(value).map_err(|_| format!("{key} is too large for this platform"))
            }),
    }
}

pub(super) fn list_directory(workspace: &Path, args: &Value) -> Result<String, String> {
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

pub(super) fn search_text(workspace: &Path, args: &Value) -> Result<String, String> {
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

pub(super) fn write_file(workspace: &Path, args: &Value) -> Result<String, String> {
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

pub(super) fn edit_file(workspace: &Path, args: &Value) -> Result<String, String> {
    let path = existing_path(workspace, required_str(args, "path")?)?;
    if !path.is_file() {
        return Err(format!("not a file: {}", path.display()));
    }
    let old_text = required_str(args, "old_text")?;
    if old_text.is_empty() {
        return Err("old_text must not be empty".into());
    }
    let new_text = required_str(args, "new_text")?;
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
    Ok(format!(
        "Replaced {} bytes with {} bytes in {}",
        old_text.len(),
        new_text.len(),
        path.display()
    ))
}

pub(super) fn move_file(workspace: &Path, args: &Value) -> Result<String, String> {
    let source = existing_entry_path(workspace, required_str(args, "source_path")?)?;
    let metadata = std::fs::symlink_metadata(&source)
        .map_err(|error| format!("failed to inspect {}: {error}", source.display()))?;
    if metadata.file_type().is_dir() {
        return Err("source_path must be a file, not a directory".into());
    }
    let destination = new_entry_path(workspace, required_str(args, "destination_path")?)?;
    if std::fs::symlink_metadata(&destination).is_ok() {
        return Err(format!(
            "destination already exists: {}",
            destination.display()
        ));
    }
    std::fs::rename(&source, &destination).map_err(|error| {
        format!(
            "failed to move {} to {}: {error}",
            source.display(),
            destination.display()
        )
    })?;
    Ok(format!(
        "Moved {} to {}",
        source.display(),
        destination.display()
    ))
}

pub(super) fn delete_file(workspace: &Path, args: &Value) -> Result<String, String> {
    let path = existing_entry_path(workspace, required_str(args, "path")?)?;
    let metadata = std::fs::symlink_metadata(&path)
        .map_err(|error| format!("failed to inspect {}: {error}", path.display()))?;
    if metadata.file_type().is_dir() {
        return Err("path must be a file, not a directory".into());
    }
    std::fs::remove_file(&path)
        .map_err(|error| format!("failed to delete {}: {error}", path.display()))?;
    Ok(format!("Deleted {}", path.display()))
}

fn existing_entry_path(workspace: &Path, value: &str) -> Result<PathBuf, String> {
    let path = workspace.join(safe_relative(value)?);
    let canonical_root = workspace
        .canonicalize()
        .map_err(|error| format!("invalid workspace: {error}"))?;
    let canonical_target = path
        .canonicalize()
        .map_err(|error| format!("path not found: {error}"))?;
    if !canonical_target.starts_with(&canonical_root) {
        return Err("path escapes workspace".into());
    }
    Ok(path)
}

fn new_entry_path(workspace: &Path, value: &str) -> Result<PathBuf, String> {
    let path = workspace.join(safe_relative(value)?);
    let parent = path.parent().ok_or_else(|| "invalid path".to_string())?;
    let canonical_root = workspace
        .canonicalize()
        .map_err(|error| format!("invalid workspace: {error}"))?;
    let canonical_parent = parent
        .canonicalize()
        .map_err(|error| format!("destination parent directory does not exist: {error}"))?;
    if !canonical_parent.starts_with(&canonical_root) {
        return Err("path escapes workspace".into());
    }
    Ok(path)
}

pub(super) fn existing_path(workspace: &Path, value: &str) -> Result<PathBuf, String> {
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

pub(super) fn safe_relative(value: &str) -> Result<PathBuf, String> {
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
