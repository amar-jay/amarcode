-- The first title published through ACP session metadata owns the chat title.
-- Later sessions may publish their own title, but must not replace it.
ALTER TABLE chats ADD COLUMN agent_title_set INTEGER NOT NULL DEFAULT 0
    CHECK (agent_title_set IN (0, 1));
