-- migrations/033_tracking_pixels.sql

ALTER TABLE messages ADD COLUMN has_tracking_pixels INTEGER NOT NULL DEFAULT 0;

INSERT OR IGNORE INTO schema_version (version) VALUES (33);
