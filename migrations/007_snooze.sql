-- Migration 007: Email snooze support
ALTER TABLE messages ADD COLUMN snoozed_until INTEGER;
CREATE INDEX IF NOT EXISTS idx_messages_snoozed ON messages(snoozed_until) WHERE snoozed_until IS NOT NULL;
INSERT OR IGNORE INTO schema_version (version) VALUES (7);
