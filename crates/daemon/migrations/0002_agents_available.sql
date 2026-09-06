-- 0002_agents_available.sql
-- Replace is_preset with a persisted available flag.
-- Applied once via the migrations table in Store::open.

ALTER TABLE agents ADD COLUMN available INTEGER NOT NULL DEFAULT 0 CHECK (available IN (0, 1));
ALTER TABLE agents DROP COLUMN is_preset;
