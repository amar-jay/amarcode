use std::{fs, io::Write, path::Path, sync::Mutex};

// Serialize read/check/replace saves from every daemon client.
static SAVE_LOCK: Mutex<()> = Mutex::new(());
const MAX_BYTES: usize = 1_000_000;

fn read_text(path: &Path) -> Result<String, String> {
    use std::io::Read;
    let file = fs::File::open(path).map_err(|error| error.to_string())?;
    if !file
        .metadata()
        .map_err(|error| error.to_string())?
        .is_file()
    {
        return Err("Only regular text files can be edited.".into());
    }
    let mut bytes = Vec::new();
    file.take(MAX_BYTES as u64 + 1)
        .read_to_end(&mut bytes)
        .map_err(|error| error.to_string())?;
    if bytes.len() > MAX_BYTES {
        return Err("This file is too large to edit (maximum 1 MB).".into());
    }
    if bytes.contains(&0) {
        return Err("Binary files cannot be edited as text.".into());
    }
    String::from_utf8(bytes).map_err(|_| "Only UTF-8 text files can be edited.".into())
}

pub fn read_workspace_file(
    workspace_path: String,
    relative_path: String,
) -> Result<String, String> {
    let path = super::workspace_relative_path(Path::new(&workspace_path), &relative_path)?;
    read_text(&path)
}

fn save(root: &Path, relative_path: &str, expected: &str, content: &str) -> Result<(), String> {
    let _lock = SAVE_LOCK.lock().map_err(|error| error.to_string())?;
    if content.len() > MAX_BYTES || content.contains('\0') {
        return Err("Save requires a UTF-8 text file smaller than 1 MB.".into());
    }
    let path = super::workspace_relative_path(root, relative_path)?;
    // Resolve in-workspace symlinks so replacement preserves the link itself.
    let path = path.canonicalize().map_err(|error| error.to_string())?;
    if read_text(&path)? != expected {
        return Err("This file changed on disk. Your draft is safe. Reload the file to review the latest version before saving.".into());
    }
    let permissions = fs::metadata(&path)
        .map_err(|error| error.to_string())?
        .permissions();
    if permissions.readonly() {
        return Err("This file is read-only.".into());
    }
    let nonce = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map_err(|error| error.to_string())?
        .as_nanos();
    let temporary = path.with_file_name(format!(".amarcode-save-{}-{nonce}", std::process::id()));
    let result = (|| {
        let mut file = fs::OpenOptions::new()
            .write(true)
            .create_new(true)
            .open(&temporary)
            .map_err(|error| error.to_string())?;
        file.write_all(content.as_bytes())
            .map_err(|error| error.to_string())?;
        file.set_permissions(permissions)
            .map_err(|error| error.to_string())?;
        file.sync_all().map_err(|error| error.to_string())?;
        drop(file);
        // Recheck after preparing the replacement to catch intervening agent edits.
        if read_text(&path)? != expected {
            return Err("This file changed on disk. Reload it before saving.".into());
        }
        fs::rename(&temporary, &path).map_err(|error| error.to_string())
    })();
    let _ = fs::remove_file(temporary);
    result
}

pub fn save_workspace_file(
    workspace_path: String,
    relative_path: String,
    expected_content: String,
    content: String,
) -> Result<(), String> {
    save(
        Path::new(&workspace_path),
        &relative_path,
        &expected_content,
        &content,
    )
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn save_preserves_bytes_and_rejects_stale_drafts() {
        let root =
            std::env::temp_dir().join(format!("amarcode-editor-test-{}", std::process::id()));
        fs::create_dir_all(&root).unwrap();
        let path = root.join("file.txt");
        fs::write(&path, "original\r\n").unwrap();
        save(&root, "file.txt", "original\r\n", "edited\r\n").unwrap();
        assert_eq!(fs::read(&path).unwrap(), b"edited\r\n");
        assert!(save(&root, "file.txt", "original\r\n", "stale").is_err());
        assert_eq!(read_text(&path).unwrap(), "edited\r\n");
        assert!(save(&root, "../outside", "", "").is_err());
        fs::write(&path, [0xff, 0xfe]).unwrap();
        assert!(read_text(&path).is_err());
        fs::write(&path, [0]).unwrap();
        assert!(read_text(&path).is_err());
        fs::remove_dir_all(root).unwrap();
    }
}
