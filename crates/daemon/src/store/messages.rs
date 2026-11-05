//! Persistence for `messages` and `message_parts`.

use rusqlite::params;

use super::{cell_parse, map_message, now, to_error, Message, MessagePart, Store};
use crate::{protocol::MessageStatus, Error, Result};

impl Store {
    pub fn create_message(&self, message: &Message) -> Result<()> {
        self.connection()?
            .execute(
                "INSERT INTO messages (id,chat_id,agent_run_id,role,content,status,created_at,updated_at)
                 VALUES (?1,?2,?3,?4,?5,?6,?7,?8)",
                params![
                    message.id,
                    message.chat_id,
                    message.agent_run_id,
                    message.role.as_str(),
                    message.content,
                    message.status.as_str(),
                    message.created_at,
                    message.updated_at,
                ],
            )
            .map_err(to_error)?;
        self.touch_chat(&message.chat_id)
    }

    pub fn update_message(&self, id: &str, content: &str, status: MessageStatus) -> Result<()> {
        self.connection()?
            .execute(
                "UPDATE messages SET content=?2, status=?3, updated_at=?4 WHERE id=?1",
                params![id, content, status.as_str(), now()],
            )
            .map_err(to_error)?;
        Ok(())
    }

    pub fn get_message(&self, id: &str) -> Result<Option<Message>> {
        let connection = self.connection()?;
        let mut statement = connection
            .prepare(
                "SELECT id,chat_id,agent_run_id,role,content,status,created_at,updated_at
                 FROM messages WHERE id=?1",
            )
            .map_err(to_error)?;
        let mut rows = statement
            .query_map(params![id], super::map_message)
            .map_err(to_error)?;
        match rows.next() {
            Some(row) => Ok(Some(row.map_err(to_error)?)),
            None => Ok(None),
        }
    }

    pub fn messages(&self, chat_id: &str) -> Result<Vec<Message>> {
        let connection = self.connection()?;
        let mut statement = connection
            .prepare(
                "SELECT id,chat_id,agent_run_id,role,content,status,created_at,updated_at
                 FROM messages WHERE chat_id=?1 ORDER BY created_at,id",
            )
            .map_err(to_error)?;
        let rows = statement
            .query_map(params![chat_id], map_message)
            .map_err(to_error)?;
        rows.collect::<std::result::Result<_, _>>()
            .map_err(to_error)
    }

    /// Cursor pagination uses the same stable ordering as full history reads.
    pub fn message_page(
        &self,
        chat_id: &str,
        limit: u32,
        before: Option<&str>,
    ) -> Result<(Vec<Message>, bool)> {
        if !(1..=200).contains(&limit) {
            return Err(Error::msg("message_limit must be between 1 and 200"));
        }
        let anchor = before
            .map(|id| {
                self.get_message(id)?
                    .filter(|message| message.chat_id == chat_id)
                    .ok_or_else(|| Error::msg("message cursor does not belong to this chat"))
            })
            .transpose()?;
        let connection = self.connection()?;
        let query = if anchor.is_some() {
            "SELECT id,chat_id,agent_run_id,role,content,status,created_at,updated_at
             FROM messages WHERE chat_id=?1 AND (created_at,id) < (?2,?3)
             ORDER BY created_at DESC,id DESC LIMIT ?4"
        } else {
            "SELECT id,chat_id,agent_run_id,role,content,status,created_at,updated_at
             FROM messages WHERE chat_id=?1 ORDER BY created_at DESC,id DESC LIMIT ?2"
        };
        let mut statement = connection.prepare(query).map_err(to_error)?;
        let rows = if let Some(anchor) = anchor {
            statement.query_map(
                params![chat_id, anchor.created_at, anchor.id, limit + 1],
                map_message,
            )
        } else {
            statement.query_map(params![chat_id, limit + 1], map_message)
        }
        .map_err(to_error)?;
        let mut messages = rows
            .collect::<std::result::Result<Vec<_>, _>>()
            .map_err(to_error)?;
        let has_older = messages.len() > limit as usize;
        messages.truncate(limit as usize);
        messages.reverse();
        Ok((messages, has_older))
    }

    pub fn replace_message_parts(&self, message_id: &str, parts: &[MessagePart]) -> Result<()> {
        let mut connection = self.connection()?;
        let transaction = connection.transaction().map_err(to_error)?;
        transaction
            .execute(
                "DELETE FROM message_parts WHERE message_id=?1",
                params![message_id],
            )
            .map_err(to_error)?;
        for part in parts {
            transaction
                .execute(
                    "INSERT INTO message_parts (message_id,ordinal,kind,content_json)
                     VALUES (?1,?2,?3,?4)",
                    params![
                        message_id,
                        part.ordinal,
                        part.kind.as_str(),
                        part.content_json
                    ],
                )
                .map_err(to_error)?;
        }
        transaction.commit().map_err(to_error)
    }

    /// Client projection: fold reasoning tool snapshots and adjacent thoughts.
    /// Keep raw parts intact so later tool updates can still address their IDs.
    pub fn display_message_parts(&self, message_id: &str) -> Result<Vec<MessagePart>> {
        Ok(coalesce_reasoning(self.message_parts(message_id)?))
    }

    pub fn message_parts(&self, message_id: &str) -> Result<Vec<MessagePart>> {
        let connection = self.connection()?;
        let mut statement = connection
            .prepare(
                "SELECT message_id,ordinal,kind,content_json
                 FROM message_parts WHERE message_id=?1 ORDER BY ordinal",
            )
            .map_err(to_error)?;
        let rows = statement
            .query_map(params![message_id], |row| {
                Ok(MessagePart {
                    message_id: row.get(0)?,
                    ordinal: row.get(1)?,
                    kind: cell_parse(row.get(2)?, crate::protocol::MessagePartKind::parse)?,
                    content_json: row.get(3)?,
                })
            })
            .map_err(to_error)?;
        rows.collect::<std::result::Result<_, _>>()
            .map_err(to_error)
    }

    /// Load only the small context-bearing parts used for semantic handoff.
    /// Tool completion updates can contain megabytes of output, so exclude
    /// them in SQLite rather than materializing and discarding them in Rust.
    pub fn message_context_parts(&self, message_id: &str) -> Result<Vec<MessagePart>> {
        let connection = self.connection()?;
        let mut statement = connection
            .prepare(
                "SELECT message_id,ordinal,kind,content_json
                 FROM message_parts
                 WHERE message_id=?1 AND (
                    kind='thinking' OR
                    (kind='tool_call' AND json_extract(content_json, '$.sessionUpdate')='tool_call')
                 )
                 ORDER BY ordinal",
            )
            .map_err(to_error)?;
        let rows = statement
            .query_map(params![message_id], |row| {
                Ok(MessagePart {
                    message_id: row.get(0)?,
                    ordinal: row.get(1)?,
                    kind: cell_parse(row.get(2)?, crate::protocol::MessagePartKind::parse)?,
                    content_json: row.get(3)?,
                })
            })
            .map_err(to_error)?;
        rows.collect::<std::result::Result<_, _>>()
            .map_err(to_error)
    }
}

/// Tool updates contain replacement snapshots, not text deltas. Resolve the
/// latest content first, then emit it at the tool's original position.
fn coalesce_reasoning(parts: Vec<MessagePart>) -> Vec<MessagePart> {
    use crate::protocol::MessagePartKind;
    use serde_json::{json, Value};
    use std::collections::{HashMap, HashSet};

    let mut tools: HashMap<String, Value> = HashMap::new();
    for part in &parts {
        if part.kind != MessagePartKind::ToolCall {
            continue;
        }
        let Ok(Value::Object(value)) = serde_json::from_str::<Value>(&part.content_json) else {
            continue;
        };
        let Some(id) = value.get("toolCallId").and_then(Value::as_str) else {
            continue;
        };
        let record = tools.entry(id.to_owned()).or_insert_with(|| json!({}));
        for (key, value) in value {
            record[&key] = value;
        }
    }
    let mut seen = HashSet::new();
    let mut result: Vec<MessagePart> = Vec::new();
    for mut part in parts {
        if part.kind == MessagePartKind::ToolCall {
            if let Ok(value) = serde_json::from_str::<Value>(&part.content_json) {
                if let Some(id) = value.get("toolCallId").and_then(Value::as_str) {
                    if let Some(record) = tools.get(id).filter(|record| {
                        matches!(
                            record.get("kind").and_then(Value::as_str),
                            Some("think" | "thinking")
                        )
                    }) {
                        if !seen.insert(id.to_owned()) {
                            continue;
                        }
                        let text = record
                            .get("content")
                            .and_then(Value::as_array)
                            .map(|content| {
                                content
                                    .iter()
                                    .filter_map(|entry| {
                                        let block = if entry.get("type").and_then(Value::as_str)
                                            == Some("content")
                                        {
                                            entry.get("content")?
                                        } else {
                                            entry
                                        };
                                        block.get("text").and_then(Value::as_str)
                                    })
                                    .collect::<Vec<_>>()
                                    .join("\n")
                            })
                            .unwrap_or_default();
                        // Preserve contentless thought tools (and their title) as-is.
                        if !text.is_empty() {
                            part.kind = MessagePartKind::Thinking;
                            part.content_json = json!({ "text": text }).to_string();
                        }
                    }
                }
            }
        }
        if part.kind == MessagePartKind::Thinking {
            if let Some(previous) = result
                .last_mut()
                .filter(|previous| previous.kind == MessagePartKind::Thinking)
            {
                if let (Ok(left), Ok(right)) = (
                    serde_json::from_str::<Value>(&previous.content_json),
                    serde_json::from_str::<Value>(&part.content_json),
                ) {
                    if let (Some(left), Some(right)) = (
                        left.get("text").and_then(Value::as_str),
                        right.get("text").and_then(Value::as_str),
                    ) {
                        previous.content_json =
                            json!({ "text": format!("{left}\n\n{right}") }).to_string();
                        continue;
                    }
                }
            }
        }
        result.push(part);
    }
    result
}

#[cfg(test)]
mod reasoning_tests {
    use super::*;
    use crate::protocol::MessagePartKind;
    use serde_json::{json, Value};

    fn part(ordinal: i64, kind: MessagePartKind, value: Value) -> MessagePart {
        MessagePart {
            message_id: "message".into(),
            ordinal,
            kind,
            content_json: value.to_string(),
        }
    }

    #[test]
    fn merges_adjacent_thoughts_using_latest_tool_snapshot() {
        use MessagePartKind::*;
        let parts = coalesce_reasoning(vec![
            part(0, Thinking, json!({"text":"First"})),
            part(
                1,
                ToolCall,
                json!({"toolCallId":"a", "kind":"think", "content":[{"type":"content", "content":{"type":"text", "text":"partial"}}]}),
            ),
            part(
                2,
                ToolCall,
                json!({"toolCallId":"a", "content":[{"type":"content", "content":{"type":"text", "text":"Complete"}}]}),
            ),
            part(3, Thinking, json!({"text":"Last"})),
        ]);
        assert_eq!(parts.len(), 1);
        assert_eq!(parts[0].ordinal, 0);
        assert_eq!(
            serde_json::from_str::<Value>(&parts[0].content_json).unwrap()["text"],
            "First\n\nComplete\n\nLast"
        );
    }

    #[test]
    fn text_and_real_tools_break_reasoning_groups() {
        use MessagePartKind::*;
        for boundary in [
            part(1, Text, json!({"text":"Answer"})),
            part(1, ToolCall, json!({"toolCallId":"read", "kind":"read"})),
        ] {
            let parts = coalesce_reasoning(vec![
                part(0, Thinking, json!({"text":"Before"})),
                boundary,
                part(2, Thinking, json!({"text":"After"})),
            ]);
            assert_eq!(parts.len(), 3);
        }
    }

    #[test]
    fn late_thought_updates_do_not_create_a_second_group_after_text() {
        use MessagePartKind::*;
        let parts = coalesce_reasoning(vec![
            part(
                0,
                ToolCall,
                json!({"toolCallId":"a", "kind":"think", "content":[{"type":"text", "text":"First"}]}),
            ),
            part(1, Text, json!({"text":"Answer"})),
            part(
                2,
                ToolCall,
                json!({"toolCallId":"a", "status":"completed", "content":[{"type":"text", "text":"Final"}]}),
            ),
            part(3, Thinking, json!({"text":"Next"})),
        ]);
        assert_eq!(parts.len(), 3);
        assert_eq!(
            serde_json::from_str::<Value>(&parts[0].content_json).unwrap()["text"],
            "Final"
        );
        assert_eq!(parts[1].kind, Text);
    }
}
