CREATE TABLE prompt_queue_state (
  chat_id TEXT PRIMARY KEY REFERENCES chats(id) ON DELETE CASCADE,
  paused INTEGER NOT NULL DEFAULT 0,
  reason TEXT
) STRICT;
CREATE TABLE prompt_jobs (
  sequence INTEGER PRIMARY KEY AUTOINCREMENT,
  id TEXT NOT NULL UNIQUE,
  chat_id TEXT NOT NULL REFERENCES chats(id) ON DELETE CASCADE,
  agent_id TEXT NOT NULL,
  text TEXT NOT NULL,
  attachment_count INTEGER NOT NULL,
  payload_json TEXT,
  status TEXT NOT NULL CHECK(status IN ('queued','running','failed','interrupted','done','removed')),
  error TEXT,
  created_at TEXT NOT NULL
) STRICT;
CREATE INDEX idx_prompt_jobs_chat ON prompt_jobs(chat_id,status,sequence);
