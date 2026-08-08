//! Persistence for the `chats` table.

use rusqlite::params;

use super::{map_chat, to_error, Chat, Store};
use crate::Result;

impl Store {
    pub fn create_chat(&self, chat: &Chat) -> Result<()> {
        self.connection()?
            .execute(
                "INSERT INTO chats (id,workspace_path,title,created_at,updated_at,archived_at)
                 VALUES (?1,?2,?3,?4,?5,?6)",
                params![
                    chat.id,
                    chat.workspace_path,
                    chat.title,
                    chat.created_at,
                    chat.updated_at,
                    chat.archived_at,
                ],
            )
            .map_err(to_error)?;
        Ok(())
    }

    pub fn chats(&self, workspace_path: Option<&str>) -> Result<Vec<Chat>> {
        let connection = self.connection()?;
        let mut statement = connection
            .prepare(
                "SELECT id,workspace_path,title,created_at,updated_at,archived_at FROM chats
                 WHERE (?1 IS NULL OR workspace_path = ?1) ORDER BY updated_at DESC",
            )
            .map_err(to_error)?;
        let rows = statement
            .query_map(params![workspace_path], map_chat)
            .map_err(to_error)?;
        rows.collect::<std::result::Result<_, _>>()
            .map_err(to_error)
    }

    pub fn get_chat(&self, id: &str) -> Result<Option<Chat>> {
        let connection = self.connection()?;
        let mut statement = connection
            .prepare(
                "SELECT id,workspace_path,title,created_at,updated_at,archived_at
                 FROM chats WHERE id=?1",
            )
            .map_err(to_error)?;
        let mut rows = statement
            .query_map(params![id], map_chat)
            .map_err(to_error)?;
        match rows.next() {
            Some(row) => Ok(Some(row.map_err(to_error)?)),
            None => Ok(None),
        }
    }

    pub fn update_title(&self, id: &str, title: &str) -> Result<()> {
        self.connection()?
            .execute(
                "UPDATE chats SET title=?2, updated_at=?3 WHERE id=?1",
                params![id, title, super::now()],
            )
            .map_err(to_error)?;
        Ok(())
    }

    pub fn archive_chat(&self, id: &str) -> Result<()> {
        let now = super::now();
        self.connection()?
            .execute(
                "UPDATE chats SET archived_at=?2, updated_at=?2 WHERE id=?1",
                params![id, now],
            )
            .map_err(to_error)?;
        Ok(())
    }
}
