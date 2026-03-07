-- migrations/006_dedup_messages.sql

-- Remove duplicate messages (keep the oldest by rowid for each account_id + message_id pair)
DELETE FROM messages WHERE rowid NOT IN (
    SELECT MIN(rowid) FROM messages
    WHERE message_id IS NOT NULL
    GROUP BY account_id, message_id, folder
) AND message_id IS NOT NULL
  AND id NOT IN (SELECT MIN(id) FROM messages WHERE message_id IS NOT NULL GROUP BY account_id, message_id, folder);

-- More precise: delete by rowid
DELETE FROM messages WHERE rowid IN (
    SELECT m.rowid FROM messages m
    WHERE m.message_id IS NOT NULL
      AND m.rowid NOT IN (
          SELECT MIN(m2.rowid) FROM messages m2
          WHERE m2.message_id IS NOT NULL
          GROUP BY m2.account_id, m2.message_id, m2.folder
      )
);

-- Add unique index to prevent future duplicates (only where message_id is not null)
CREATE UNIQUE INDEX IF NOT EXISTS idx_messages_unique_msgid
    ON messages(account_id, message_id, folder)
    WHERE message_id IS NOT NULL;

-- Rebuild FTS5 index after dedup
INSERT INTO fts_messages(fts_messages) VALUES('rebuild');

INSERT OR IGNORE INTO schema_version (version) VALUES (6);
