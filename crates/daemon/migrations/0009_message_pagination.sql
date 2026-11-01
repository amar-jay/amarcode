CREATE INDEX IF NOT EXISTS idx_messages_chat_order ON messages(chat_id, created_at, id);
