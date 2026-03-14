-- Performance indexes identified by full-codebase efficiency review

-- LOWER(from_address) is used in 20+ queries across contacts, relationship_intel, VIP detection, etc.
CREATE INDEX IF NOT EXISTS idx_messages_from_address_lower ON messages(LOWER(from_address));

-- Partial index for active (non-deleted) messages — virtually every query filters on is_deleted = 0
CREATE INDEX IF NOT EXISTS idx_messages_active ON messages(account_id, folder, date DESC) WHERE is_deleted = 0;

-- ai_needs_reply = 1 filter used by dedicated UI tab
CREATE INDEX IF NOT EXISTS idx_messages_needs_reply ON messages(account_id) WHERE ai_needs_reply = 1;

INSERT INTO schema_version (version) VALUES (51);
