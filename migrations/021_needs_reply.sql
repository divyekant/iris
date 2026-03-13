ALTER TABLE messages ADD COLUMN ai_needs_reply INTEGER DEFAULT 0;
INSERT OR IGNORE INTO schema_version (version) VALUES (21);
