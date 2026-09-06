-- 0003_chat_session_config.sql
-- Last ACP session config snapshot for a chat.

ALTER TABLE chats ADD COLUMN session_config_json TEXT NOT NULL DEFAULT '[]';
