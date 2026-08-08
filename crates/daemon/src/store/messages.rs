//! Persistence for `messages` and `message_parts`.

use rusqlite::params;

use super::{
    Message, MessagePart, Store, cell_parse, map_message, now, to_error,
};
use crate::{Result, protocol::MessageStatus};

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

    pub fn update_message(
        &self,
        id: &str,
        content: &str,
        status: MessageStatus,
    ) -> Result<()> {
        self.connection()?
            .execute(
                "UPDATE messages SET content=?2, status=?3, updated_at=?4 WHERE id=?1",
                params![id, content, status.as_str(), now()],
            )
            .map_err(to_error)?;
        Ok(())
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

    pub fn replace_message_parts(
        &self,
        message_id: &str,
        parts: &[MessagePart],
    ) -> Result<()> {
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
}
