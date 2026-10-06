-- Track catalog ownership so synchronizing the external registry cannot remove
-- built-in or user-created agent definitions.
-- Existing rows predate custom-agent RPCs and came from registry sync.
ALTER TABLE agents ADD COLUMN source TEXT NOT NULL DEFAULT 'registry'
    CHECK (source IN ('builtin', 'registry', 'custom'));
