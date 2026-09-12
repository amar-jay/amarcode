CREATE TABLE IF NOT EXISTS daemon_config (
    id INTEGER PRIMARY KEY CHECK (id = 1),
    store_acp_events INTEGER NOT NULL DEFAULT 0 CHECK (store_acp_events IN (0, 1)),
    acp_event_retention_days INTEGER NOT NULL DEFAULT 7 CHECK (acp_event_retention_days BETWEEN 1 AND 365)
) STRICT;

INSERT OR IGNORE INTO daemon_config (id, store_acp_events, acp_event_retention_days)
VALUES (1, 0, 7);

CREATE INDEX IF NOT EXISTS idx_acp_events_created_at ON acp_events(created_at);
