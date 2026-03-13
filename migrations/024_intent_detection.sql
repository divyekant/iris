-- migrations/024_intent_detection.sql
-- Feature #43: Intent Detection — finer-grained intent classification

ALTER TABLE messages ADD COLUMN intent TEXT;
ALTER TABLE messages ADD COLUMN intent_confidence REAL;
CREATE INDEX idx_messages_intent ON messages(intent);

INSERT OR IGNORE INTO schema_version (version) VALUES (24);
