-- Codex initially publishes the prompt itself as session metadata, followed by
-- its generated title. Migration 0006 claimed that prompt echo too early.
-- Repair affected chats from the ACP events that are already persisted.
WITH title_updates AS (
    SELECT
        r.chat_id,
        e.id,
        trim(json_extract(e.payload_json, '$.update.title')) AS title
    FROM acp_events e
    JOIN agent_runs r ON r.id = e.agent_run_id
    WHERE json_extract(e.payload_json, '$.update.sessionUpdate') = 'session_info_update'
      AND json_type(e.payload_json, '$.update.title') = 'text'
      AND trim(json_extract(e.payload_json, '$.update.title')) <> ''
),
first_titles AS (
    SELECT title_updates.chat_id, title_updates.title
    FROM title_updates
    JOIN (
        SELECT chat_id, min(id) AS id
        FROM title_updates
        GROUP BY chat_id
    ) first_update USING (chat_id, id)
),
generated_titles AS (
    SELECT title_updates.chat_id, title_updates.title
    FROM title_updates
    JOIN first_titles USING (chat_id)
    JOIN (
        SELECT updates.chat_id, min(updates.id) AS id
        FROM title_updates updates
        JOIN first_titles first ON first.chat_id = updates.chat_id
        WHERE updates.title <> first.title
        GROUP BY updates.chat_id
    ) generated_update
      ON generated_update.chat_id = title_updates.chat_id
     AND generated_update.id = title_updates.id
)
UPDATE chats
SET title = (SELECT title FROM generated_titles WHERE chat_id = chats.id),
    agent_title_set = 1
WHERE agent_title_set = 1
  AND title = (SELECT title FROM first_titles WHERE chat_id = chats.id)
  AND EXISTS (SELECT 1 FROM generated_titles WHERE chat_id = chats.id);
