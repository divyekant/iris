-- Migration 48 (send content spec docs/specs/2026-11-24-send-content.md,
-- section 6.2): the Bcc gate of each provider. Iris reads only the copy of
-- the sender, so a send with Bcc leaves only after the owner checked the
-- delivery and the privacy of Bcc through the API path of the provider.
-- The migration adds no row: each gate is closed, also when the send switch
-- is on.
-- IF NOT EXISTS: the tests that set an older user_version run it again.
CREATE TABLE IF NOT EXISTS bcc_check (
  provider TEXT PRIMARY KEY CHECK (provider IN ('gmail', 'outlook')),
  test_to TEXT NOT NULL,
  test_bcc TEXT NOT NULL,
  -- NULL until the owner opens the gate with `iris sending bcc`.
  checked_at INTEGER
);
