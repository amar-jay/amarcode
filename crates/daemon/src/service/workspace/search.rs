//! Bounded, daemon-hosted content search, independent of an installed rg binary.
use amarcode_protocol::rpc::{
    SearchWorkspaceContentParams, WorkspaceSearchMatch, WorkspaceSearchResult,
};
use ignore::WalkBuilder;
use regex::RegexBuilder;
use std::{
    fs::File,
    io::Read,
    path::Path,
    time::{Duration, Instant},
};

const MAX_FILE_BYTES: u64 = 1_000_000;
const MAX_SCAN_BYTES: u64 = 64 * 1024 * 1024;
const MAX_RESULTS: usize = 500;

pub fn search_workspace_content(
    params: SearchWorkspaceContentParams,
) -> Result<WorkspaceSearchResult, String> {
    let root = Path::new(&params.workspace_path)
        .canonicalize()
        .map_err(|_| "The selected workspace folder is unavailable.".to_owned())?;
    if !root.is_dir() {
        return Err("The selected workspace folder is unavailable.".into());
    }
    let mut result = WorkspaceSearchResult::default();
    if params.query.is_empty() {
        return Ok(result);
    }
    if params.query.len() > 4096 {
        return Err("Search query exceeds 4 KB.".into());
    }
    let pattern = if params.regex {
        params.query
    } else {
        regex::escape(&params.query)
    };
    let matcher = RegexBuilder::new(&pattern)
        .case_insensitive(!params.case_sensitive)
        .size_limit(2 * 1024 * 1024)
        .build()
        .map_err(|error| format!("Invalid search pattern: {error}"))?;
    let started = Instant::now();
    let mut scanned = 0;
    let mut walker = WalkBuilder::new(&root);
    walker
        .require_git(false)
        .git_ignore(true)
        .git_global(false)
        .git_exclude(false)
        .ignore(false)
        .parents(false)
        .hidden(false)
        .follow_links(false)
        .sort_by_file_path(|a, b| a.cmp(b))
        .filter_entry(|entry| {
            !matches!(
                entry.file_name().to_str(),
                Some(".git" | "node_modules" | "target" | "dist")
            )
        });
    for entry in walker.build() {
        if started.elapsed() > Duration::from_secs(5) || scanned >= MAX_SCAN_BYTES {
            result.truncated = true;
            break;
        }
        let entry = match entry {
            Ok(entry) => entry,
            Err(_) => {
                result.skipped_files += 1;
                continue;
            }
        };
        if !entry.file_type().is_some_and(|kind| kind.is_file()) {
            continue;
        }
        // Revalidate containment before opening; do not follow links outside the workspace.
        let path = match entry.path().canonicalize() {
            Ok(path) if path.starts_with(&root) => path,
            _ => {
                result.skipped_files += 1;
                continue;
            }
        };
        let read = (|| -> std::io::Result<Vec<u8>> {
            let file = File::open(&path)?;
            let metadata = file.metadata()?;
            if !metadata.is_file() || metadata.len() > MAX_FILE_BYTES {
                return Err(std::io::Error::other("not an editable text file"));
            }
            let mut bytes = Vec::new();
            file.take(MAX_FILE_BYTES + 1).read_to_end(&mut bytes)?;
            Ok(bytes)
        })();
        let bytes = match read {
            Ok(bytes) => bytes,
            Err(_) => {
                result.skipped_files += 1;
                continue;
            }
        };
        scanned += bytes.len() as u64;
        if bytes.len() as u64 > MAX_FILE_BYTES || bytes.contains(&0) {
            result.skipped_files += 1;
            continue;
        }
        let text = match std::str::from_utf8(&bytes) {
            Ok(text) => text,
            Err(_) => {
                result.skipped_files += 1;
                continue;
            }
        };
        for (index, line) in text.lines().enumerate() {
            if started.elapsed() > Duration::from_secs(5) {
                result.truncated = true;
                return Ok(result);
            }
            if let Some(found) = matcher.find(line) {
                if result.matches.len() == MAX_RESULTS {
                    result.truncated = true;
                    return Ok(result);
                }
                // Center bounded previews near the match, including matches late in long lines.
                let mut start = found.start().saturating_sub(100);
                while !line.is_char_boundary(start) {
                    start -= 1;
                }
                let preview: String = line[start..].chars().take(300).collect();
                let preview = format!(
                    "{}{}{}",
                    if start > 0 { "…" } else { "" },
                    preview,
                    if start + preview.len() < line.len() {
                        "…"
                    } else {
                        ""
                    }
                );
                result.matches.push(WorkspaceSearchMatch {
                    path: entry
                        .path()
                        .strip_prefix(&root)
                        .map_err(|error| error.to_string())?
                        .to_string_lossy()
                        .replace('\\', "/"),
                    line: index as u32 + 1,
                    preview,
                });
            }
        }
    }
    Ok(result)
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;

    fn params(root: &Path, query: &str) -> SearchWorkspaceContentParams {
        SearchWorkspaceContentParams {
            workspace_path: root.to_string_lossy().into_owned(),
            query: query.into(),
            case_sensitive: false,
            regex: false,
        }
    }

    #[test]
    fn searches_literals_case_insensitively_and_regex_on_request() {
        let root = tempfile::tempdir().unwrap();
        fs::write(
            root.path().join("code.rs"),
            "nothing\r\nNeedle.foo\r\nneedleXfoo\r\n",
        )
        .unwrap();
        let result = search_workspace_content(params(root.path(), "needle.foo")).unwrap();
        assert_eq!(result.matches.len(), 1);
        assert_eq!(result.matches[0].path, "code.rs");
        assert_eq!(result.matches[0].line, 2);
        assert_eq!(result.matches[0].preview, "Needle.foo");
        let mut sensitive = params(root.path(), "needle.foo");
        sensitive.case_sensitive = true;
        assert!(search_workspace_content(sensitive)
            .unwrap()
            .matches
            .is_empty());
        let mut pattern = params(root.path(), "^needle.foo$");
        pattern.regex = true;
        assert_eq!(search_workspace_content(pattern).unwrap().matches.len(), 2);
        let mut invalid = params(root.path(), "[");
        invalid.regex = true;
        assert!(search_workspace_content(invalid)
            .unwrap_err()
            .contains("Invalid search pattern"));
    }

    #[test]
    fn respects_ignore_files_and_searches_hidden_and_deep_files() {
        let root = tempfile::tempdir().unwrap();
        fs::write(root.path().join(".gitignore"), "ignored.txt\n").unwrap();
        for directory in ["node_modules", "target", "dist", ".git"] {
            fs::create_dir(root.path().join(directory)).unwrap();
            fs::write(root.path().join(directory).join("file"), "NEEDLE").unwrap();
        }
        fs::write(root.path().join("ignored.txt"), "NEEDLE").unwrap();
        fs::write(root.path().join(".hidden"), "NEEDLE").unwrap();
        let deep = root.path().join("a/b/c/d/e/f/g/h");
        fs::create_dir_all(&deep).unwrap();
        fs::write(deep.join("deep.rs"), "NEEDLE").unwrap();
        let result = search_workspace_content(params(root.path(), "NEEDLE")).unwrap();
        assert_eq!(
            result
                .matches
                .iter()
                .map(|item| item.path.as_str())
                .collect::<Vec<_>>(),
            vec![".hidden", "a/b/c/d/e/f/g/h/deep.rs"]
        );
    }

    #[test]
    fn skips_uneditable_files_and_bounds_results_and_unicode_previews() {
        let root = tempfile::tempdir().unwrap();
        fs::write(root.path().join("binary"), b"needle\0").unwrap();
        fs::write(root.path().join("invalid"), [0xff, 0xfe]).unwrap();
        fs::write(
            root.path().join("large"),
            vec![b'x'; MAX_FILE_BYTES as usize + 1],
        )
        .unwrap();
        fs::write(
            root.path().join("unicode"),
            format!("{}needle{}", "界".repeat(500), "界".repeat(500)),
        )
        .unwrap();
        let result = search_workspace_content(params(root.path(), "needle")).unwrap();
        assert_eq!(result.skipped_files, 3);
        assert_eq!(result.matches.len(), 1);
        assert!(result.matches[0].preview.contains("needle"));
        assert!(result.matches[0].preview.chars().count() <= 302);
        fs::write(root.path().join("many"), "needle\n".repeat(MAX_RESULTS + 1)).unwrap();
        let result = search_workspace_content(params(root.path(), "needle")).unwrap();
        assert_eq!(result.matches.len(), MAX_RESULTS);
        assert!(result.truncated);
    }

    #[cfg(unix)]
    #[test]
    fn never_searches_symlink_targets_outside_workspace() {
        let root = tempfile::tempdir().unwrap();
        let outside = tempfile::tempdir().unwrap();
        fs::write(outside.path().join("private"), "needle").unwrap();
        std::os::unix::fs::symlink(outside.path(), root.path().join("escape")).unwrap();
        std::os::unix::fs::symlink(
            outside.path().join("private"),
            root.path().join("file-link"),
        )
        .unwrap();
        assert!(search_workspace_content(params(root.path(), "needle"))
            .unwrap()
            .matches
            .is_empty());
    }

    #[test]
    fn empty_query_does_not_match_every_line_and_missing_roots_error() {
        let root = tempfile::tempdir().unwrap();
        fs::write(root.path().join("file"), "content").unwrap();
        assert!(search_workspace_content(params(root.path(), ""))
            .unwrap()
            .matches
            .is_empty());
        assert!(search_workspace_content(params(&root.path().join("missing"), "query")).is_err());
    }
}
