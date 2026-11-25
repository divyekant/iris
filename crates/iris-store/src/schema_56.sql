-- Issue 910: a tombstone (an account in state `removing` or `removed`)
-- keeps its ID, provider, address and state only
-- (docs/specs/2026-11-24-remove-account.md). A sync worker that kept the
-- account ID and its token can still run after the removal. These triggers
-- refuse each sync write to a tombstone, on each path: a new `sync_log`
-- or `sync_seen` row, and a write of a column that the removal set to NULL.
--
-- The update trigger lets a tombstone change only in two ways: the move
-- from `removing` to `removed` (the end of the delete), and the new address
-- of a removed tombstone when the person adds the address again. Each other
-- column must stay NULL. A test (`crates/iris-store/tests/remove.rs`) fails
-- if a later migration adds an account column that this list does not name.
--
-- A late sync before this migration can have left sync columns or rows on
-- a tombstone. The migration clears them first, so the delete of a
-- `removing` account can still end. The sync rows of a `removed` account
-- are all late writes: its delete removed the rows that it had.

UPDATE account SET history_id = NULL, token = NULL, mirror_ready_at = NULL,
  sync_kind = NULL, sync_start = NULL, sync_step = NULL, imap_host = NULL,
  imap_port = NULL, imap_user = NULL, categories_off_at = NULL
  WHERE state IN ('removing', 'removed');
DELETE FROM sync_log WHERE account_id IN (SELECT id FROM account WHERE state = 'removed');
DELETE FROM sync_seen WHERE account_id IN (SELECT id FROM account WHERE state = 'removed');

CREATE TRIGGER IF NOT EXISTS removed_account_sync_log BEFORE INSERT ON sync_log
  WHEN (SELECT state FROM account WHERE id = NEW.account_id) IN ('removing', 'removed')
BEGIN
  SELECT RAISE(ABORT, 'iris: the account is removed');
END;

CREATE TRIGGER IF NOT EXISTS removed_account_sync_seen BEFORE INSERT ON sync_seen
  WHEN (SELECT state FROM account WHERE id = NEW.account_id) IN ('removing', 'removed')
BEGIN
  SELECT RAISE(ABORT, 'iris: the account is removed');
END;

CREATE TRIGGER IF NOT EXISTS removed_account_update BEFORE UPDATE ON account
  WHEN OLD.state IN ('removing', 'removed')
   AND (NEW.state IS NOT 'removed' AND NOT (OLD.state = 'removing' AND NEW.state IS 'removing')
        OR NEW.id IS NOT OLD.id
        OR NEW.provider IS NOT OLD.provider
        OR NEW.history_id IS NOT NULL
        OR NEW.token IS NOT NULL
        OR NEW.mirror_ready_at IS NOT NULL
        OR NEW.sync_kind IS NOT NULL
        OR NEW.sync_start IS NOT NULL
        OR NEW.sync_step IS NOT NULL
        OR NEW.imap_host IS NOT NULL
        OR NEW.imap_port IS NOT NULL
        OR NEW.imap_user IS NOT NULL
        OR NEW.categories_off_at IS NOT NULL)
BEGIN
  SELECT RAISE(ABORT, 'iris: the account is removed');
END;
