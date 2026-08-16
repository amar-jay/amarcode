use std::{fs, path::PathBuf};

use base64::{engine::general_purpose::STANDARD, Engine as _};

use crate::{protocol::rpc::PromptAttachment, Error, Result};

const MAX_IMAGE_BYTES: usize = 10 * 1024 * 1024;
const MAX_TEXT_BYTES: usize = 1024 * 1024;
pub const MAX_PROMPT_IMAGES: usize = 4;
pub const MAX_PROMPT_ATTACHMENTS: usize = 4;

#[derive(Debug, Clone)]
pub struct StoredPromptAttachment {
    pub id: String,
    pub filename: Option<String>,
    pub media_type: String,
    pub data: String,
}

#[derive(Debug, Clone)]
pub struct AttachmentStore {
    root: PathBuf,
}

impl AttachmentStore {
    pub fn new(root: PathBuf) -> Self {
        Self { root }
    }

    pub fn save_prompt_attachment(
        &self,
        chat_id: &str,
        attachment: PromptAttachment,
    ) -> Result<StoredPromptAttachment> {
        if attachment.mime_type == "text/plain" {
            return self.save_prompt_text(chat_id, attachment);
        }

        validate_component("chat id", chat_id)?;
        if attachment.data.len() > encoded_size_limit() {
            return Err(Error::msg("pasted image exceeds the 10 MB limit"));
        }
        let bytes = STANDARD
            .decode(&attachment.data)
            .map_err(|_| Error::msg("pasted image is not valid base64"))?;
        if bytes.len() > MAX_IMAGE_BYTES {
            return Err(Error::msg("pasted image exceeds the 10 MB limit"));
        }

        let (media_type, extension) = detect_image_type(&bytes)
            .ok_or_else(|| Error::msg("only PNG, JPEG, WebP, and GIF images are supported"))?;
        if attachment.mime_type != media_type {
            return Err(Error::msg(format!(
                "pasted image content does not match its media type ({})",
                attachment.mime_type
            )));
        }

        let id = uuid::Uuid::new_v4().to_string();
        let directory = self.root.join(chat_id);
        fs::create_dir_all(&directory)?;
        let final_path = directory.join(format!("{id}.{extension}"));
        let temporary_path = directory.join(format!(".{id}.tmp"));
        fs::write(&temporary_path, &bytes)?;
        if let Err(error) = fs::rename(&temporary_path, &final_path) {
            let _ = fs::remove_file(&temporary_path);
            return Err(error.into());
        }

        Ok(StoredPromptAttachment {
            id,
            filename: clean_filename(attachment.filename),
            media_type: media_type.to_owned(),
            data: attachment.data,
        })
    }

    fn save_prompt_text(
        &self,
        chat_id: &str,
        attachment: PromptAttachment,
    ) -> Result<StoredPromptAttachment> {
        validate_component("chat id", chat_id)?;
        if attachment.data.len() > encoded_text_size_limit() {
            return Err(Error::msg("pasted text exceeds the 1 MB limit"));
        }
        let bytes = STANDARD
            .decode(&attachment.data)
            .map_err(|_| Error::msg("pasted text is not valid base64"))?;
        if bytes.len() > MAX_TEXT_BYTES {
            return Err(Error::msg("pasted text exceeds the 1 MB limit"));
        }
        std::str::from_utf8(&bytes).map_err(|_| Error::msg("pasted text must be UTF-8"))?;

        let id = uuid::Uuid::new_v4().to_string();
        let directory = self.root.join(chat_id);
        fs::create_dir_all(&directory)?;
        let final_path = directory.join(format!("{id}.txt"));
        let temporary_path = directory.join(format!(".{id}.tmp"));
        fs::write(&temporary_path, &bytes)?;
        if let Err(error) = fs::rename(&temporary_path, &final_path) {
            let _ = fs::remove_file(&temporary_path);
            return Err(error.into());
        }

        Ok(StoredPromptAttachment {
            id,
            filename: clean_filename(attachment.filename)
                .or_else(|| Some("pasted-text.txt".to_owned())),
            media_type: "text/plain".to_owned(),
            data: attachment.data,
        })
    }

    pub fn read(&self, chat_id: &str, attachment_id: &str) -> Result<(String, String)> {
        validate_component("chat id", chat_id)?;
        uuid::Uuid::parse_str(attachment_id).map_err(|_| Error::msg("invalid attachment id"))?;

        for (media_type, extension) in supported_types() {
            let path = self
                .root
                .join(chat_id)
                .join(format!("{attachment_id}.{extension}"));
            if path.is_file() {
                let bytes = fs::read(path)?;
                return Ok((media_type.to_owned(), STANDARD.encode(bytes)));
            }
        }
        Err(Error::msg("attachment not found"))
    }

    pub fn remove(&self, chat_id: &str, attachment_id: &str) {
        if validate_component("chat id", chat_id).is_err()
            || uuid::Uuid::parse_str(attachment_id).is_err()
        {
            return;
        }
        for (_, extension) in supported_types() {
            let _ = fs::remove_file(
                self.root
                    .join(chat_id)
                    .join(format!("{attachment_id}.{extension}")),
            );
        }
    }

    pub fn delete_chat(&self, chat_id: &str) -> Result<()> {
        validate_component("chat id", chat_id)?;
        let directory = self.root.join(chat_id);
        if directory.exists() {
            fs::remove_dir_all(directory)?;
        }
        Ok(())
    }
}

fn encoded_size_limit() -> usize {
    MAX_IMAGE_BYTES.div_ceil(3) * 4 + 4
}

fn encoded_text_size_limit() -> usize {
    MAX_TEXT_BYTES.div_ceil(3) * 4 + 4
}

fn supported_types() -> [(&'static str, &'static str); 5] {
    [
        ("image/png", "png"),
        ("image/jpeg", "jpg"),
        ("image/webp", "webp"),
        ("image/gif", "gif"),
        ("text/plain", "txt"),
    ]
}

fn detect_image_type(bytes: &[u8]) -> Option<(&'static str, &'static str)> {
    if bytes.starts_with(b"\x89PNG\r\n\x1a\n") {
        Some(("image/png", "png"))
    } else if bytes.starts_with(&[0xff, 0xd8, 0xff]) {
        Some(("image/jpeg", "jpg"))
    } else if bytes.len() >= 12 && bytes.starts_with(b"RIFF") && &bytes[8..12] == b"WEBP" {
        Some(("image/webp", "webp"))
    } else if bytes.starts_with(b"GIF87a") || bytes.starts_with(b"GIF89a") {
        Some(("image/gif", "gif"))
    } else {
        None
    }
}

fn validate_component(label: &str, value: &str) -> Result<()> {
    if value.is_empty()
        || !value
            .bytes()
            .all(|byte| byte.is_ascii_alphanumeric() || matches!(byte, b'-' | b'_'))
    {
        return Err(Error::msg(format!("invalid {label}")));
    }
    Ok(())
}

fn clean_filename(filename: Option<String>) -> Option<String> {
    filename.and_then(|name| {
        let name = std::path::Path::new(&name)
            .file_name()?
            .to_string_lossy()
            .chars()
            .filter(|character| !character.is_control())
            .take(255)
            .collect::<String>();
        (!name.trim().is_empty()).then_some(name)
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn detects_supported_image_signatures() {
        assert_eq!(
            detect_image_type(b"\x89PNG\r\n\x1a\nrest"),
            Some(("image/png", "png"))
        );
        assert_eq!(
            detect_image_type(&[0xff, 0xd8, 0xff, 0]),
            Some(("image/jpeg", "jpg"))
        );
        assert_eq!(detect_image_type(b"not an image"), None);
    }

    #[test]
    fn stores_utf8_text_attachments() {
        let root = std::env::temp_dir().join(uuid::Uuid::new_v4().to_string());
        let store = AttachmentStore::new(root.clone());
        let stored = store
            .save_prompt_attachment(
                "chat-1",
                PromptAttachment {
                    filename: Some("pasted.txt".into()),
                    mime_type: "text/plain".into(),
                    data: STANDARD.encode("A long pasted note"),
                },
            )
            .expect("text attachment should be stored");

        assert_eq!(stored.media_type, "text/plain");
        assert!(root
            .join("chat-1")
            .join(format!("{}.txt", stored.id))
            .is_file());
        store
            .delete_chat("chat-1")
            .expect("chat cleanup should succeed");
        let _ = fs::remove_dir(root);
    }
}
