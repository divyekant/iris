ALTER TABLE messages ADD COLUMN ai_sentiment TEXT;

INSERT OR IGNORE INTO schema_version (version) VALUES (19);
