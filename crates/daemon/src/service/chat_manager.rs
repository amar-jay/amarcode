//! Chat use-cases.
//!
//! Responsibilities:
//! - create / list / archive chats for a workspace
//! - update titles
//! - load message history for UI restore (via store)
//!
//! Starting an agent run belongs to `session_manager`. Mutations that change
//! durable chat state emit `EditorEvent::ChatUpdated` **after** the store write.

use std::sync::Arc;

use tokio::sync::broadcast;

use crate::{
    Error, Result,
    protocol::EditorEvent,
    store::{Chat, Message, MessagePart, Store},
};

/// Chat plus ordered messages (each with parts) for restore.
#[derive(Debug, Clone)]
pub struct ChatDetail {
    pub chat: Chat,
    pub messages: Vec<MessageDetail>,
}

#[derive(Debug, Clone)]
pub struct MessageDetail {
    pub message: Message,
    pub parts: Vec<MessagePart>,
}

#[derive(Clone)]
pub struct ChatManager {
    store: Arc<Store>,
    events: broadcast::Sender<EditorEvent>,
}

impl ChatManager {
    pub fn new(store: Arc<Store>, events: broadcast::Sender<EditorEvent>) -> Self {
        Self { store, events }
    }

    pub fn create(
        &self,
        workspace_path: impl Into<String>,
        title: Option<String>,
    ) -> Result<Chat> {
        let now = timestamp();
        let chat = Chat {
            id: uuid::Uuid::new_v4().to_string(),
            workspace_path: workspace_path.into(),
            title: title.unwrap_or_else(|| "New chat".into()),
            created_at: now.clone(),
            updated_at: now,
            archived_at: None,
        };
        self.store.create_chat(&chat)?;
        self.emit_chat_updated(&chat.id);
        Ok(chat)
    }

    pub fn list(&self, workspace_path: Option<&str>) -> Result<Vec<Chat>> {
        self.store.chats(workspace_path)
    }

    pub fn get(&self, id: &str) -> Result<Option<Chat>> {
        self.store.get_chat(id)
    }

    pub fn get_required(&self, id: &str) -> Result<Chat> {
        self.store
            .get_chat(id)?
            .ok_or_else(|| Error::msg(format!("chat not found: {id}")))
    }

    /// Load chat row + messages + parts for UI restore.
    pub fn get_with_messages(&self, id: &str) -> Result<ChatDetail> {
        let chat = self.get_required(id)?;
        let messages = self.store.messages(id)?;
        let mut detailed = Vec::with_capacity(messages.len());
        for message in messages {
            let parts = self.store.message_parts(&message.id)?;
            detailed.push(MessageDetail { message, parts });
        }
        Ok(ChatDetail {
            chat,
            messages: detailed,
        })
    }

    pub fn update_title(&self, id: &str, title: impl AsRef<str>) -> Result<Chat> {
        let title = title.as_ref();
        if title.trim().is_empty() {
            return Err(Error::msg("chat title must not be empty"));
        }
        self.get_required(id)?;
        self.store.update_title(id, title)?;
        self.emit_chat_updated(id);
        self.get_required(id)
    }

    pub fn archive(&self, id: &str) -> Result<Chat> {
        self.get_required(id)?;
        self.store.archive_chat(id)?;
        self.emit_chat_updated(id);
        self.get_required(id)
    }

    fn emit_chat_updated(&self, chat_id: &str) {
        let _ = self.events.send(EditorEvent::ChatUpdated {
            chat_id: chat_id.to_owned(),
        });
    }
}

fn timestamp() -> String {
    chrono::Utc::now().to_rfc3339()
}
