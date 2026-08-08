-- 0001_initial.sql
-- First schema migration for amarcode-daemon.
-- Applied on store open (in order by filename). Keep migrations append-only.
--
-- Tables:
--   agents        — agent definitions (presets + user-defined)
--   chats         — user-visible conversations in a workspace
--   agent_runs    — one agent execution inside a chat
--   messages      — chat messages
--   message_parts — structured pieces of a message
--   acp_events    — raw ACP JSON-RPC traffic

PRAGMA foreign_keys = ON;

CREATE TABLE IF NOT EXISTS agents (
    id TEXT PRIMARY KEY,
    name TEXT NOT NULL,
    command TEXT NOT NULL,
    arguments_json TEXT NOT NULL DEFAULT '[]',
    environment_json TEXT NOT NULL DEFAULT '[]',
    is_preset INTEGER NOT NULL DEFAULT 0 CHECK (is_preset IN (0, 1)),
    created_at TEXT NOT NULL,
    updated_at TEXT NOT NULL
) STRICT;

CREATE TABLE IF NOT EXISTS chats (
    id TEXT PRIMARY KEY,
    workspace_path TEXT NOT NULL,
    title TEXT NOT NULL DEFAULT 'New chat',
    created_at TEXT NOT NULL,
    updated_at TEXT NOT NULL,
    archived_at TEXT
) STRICT;

CREATE TABLE IF NOT EXISTS agent_runs (
    id TEXT PRIMARY KEY,
    chat_id TEXT NOT NULL REFERENCES chats(id) ON DELETE CASCADE,
    agent_id TEXT NOT NULL REFERENCES agents(id) ON DELETE RESTRICT,
    acp_session_id TEXT,
    status TEXT NOT NULL CHECK (status IN ('starting', 'running', 'completed', 'stopped', 'failed')),
    started_at TEXT NOT NULL,
    finished_at TEXT,
    error_message TEXT
) STRICT;

CREATE TABLE IF NOT EXISTS messages (
    id TEXT PRIMARY KEY,
    chat_id TEXT NOT NULL REFERENCES chats(id) ON DELETE CASCADE,
    agent_run_id TEXT REFERENCES agent_runs(id) ON DELETE SET NULL,
    role TEXT NOT NULL CHECK (role IN ('system', 'user', 'assistant', 'tool')),
    content TEXT NOT NULL DEFAULT '',
    status TEXT NOT NULL DEFAULT 'complete' CHECK (status IN ('streaming', 'complete', 'interrupted', 'failed')),
    created_at TEXT NOT NULL,
    updated_at TEXT NOT NULL
) STRICT;

CREATE TABLE IF NOT EXISTS message_parts (
    message_id TEXT NOT NULL REFERENCES messages(id) ON DELETE CASCADE,
    ordinal INTEGER NOT NULL,
    kind TEXT NOT NULL CHECK (kind IN ('text', 'tool_call', 'tool_result', 'thinking', 'file', 'image')),
    content_json TEXT NOT NULL DEFAULT '{}',
    PRIMARY KEY (message_id, ordinal)
) STRICT;

CREATE TABLE IF NOT EXISTS acp_events (
    id INTEGER PRIMARY KEY AUTOINCREMENT,
    agent_run_id TEXT NOT NULL REFERENCES agent_runs(id) ON DELETE CASCADE,
    direction TEXT NOT NULL CHECK (direction IN ('sent', 'received')),
    method TEXT NOT NULL DEFAULT '',
    payload_json TEXT NOT NULL,
    created_at TEXT NOT NULL
) STRICT;

CREATE INDEX IF NOT EXISTS idx_chats_workspace ON chats(workspace_path);
CREATE INDEX IF NOT EXISTS idx_agent_runs_chat ON agent_runs(chat_id);
CREATE INDEX IF NOT EXISTS idx_messages_chat ON messages(chat_id);
CREATE INDEX IF NOT EXISTS idx_message_parts_message ON message_parts(message_id);
CREATE INDEX IF NOT EXISTS idx_acp_events_run ON acp_events(agent_run_id);
