-- migrations/020_unsubscribe.sql
-- Feature #68: One-click unsubscribe from mailing lists

ALTER TABLE messages ADD COLUMN list_unsubscribe TEXT;
ALTER TABLE messages ADD COLUMN list_unsubscribe_post INTEGER DEFAULT 0;

INSERT OR IGNORE INTO schema_version (version) VALUES (20);
