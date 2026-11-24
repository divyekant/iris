-- Migration 46 (decision D34, issue #626, docs/specs/2026-11-24-remove-account.md):
-- IF NOT EXISTS: a test that sets an older user_version runs this again.
-- an account in state `removing` or `removed` gets no new mail. A sync that
-- started before the removal fails at its next write, so it cannot add mail
-- after the removal. The states need no column: `account.state` is TEXT.

CREATE TRIGGER IF NOT EXISTS removed_account_message BEFORE INSERT ON message
  WHEN (SELECT state FROM account WHERE id = NEW.account_id) IN ('removing', 'removed')
BEGIN
  SELECT RAISE(ABORT, 'iris: the account is removed');
END;

CREATE TRIGGER IF NOT EXISTS removed_account_thread BEFORE INSERT ON thread
  WHEN (SELECT state FROM account WHERE id = NEW.account_id) IN ('removing', 'removed')
BEGIN
  SELECT RAISE(ABORT, 'iris: the account is removed');
END;

CREATE TRIGGER IF NOT EXISTS removed_account_label BEFORE INSERT ON label
  WHEN (SELECT state FROM account WHERE id = NEW.account_id) IN ('removing', 'removed')
BEGIN
  SELECT RAISE(ABORT, 'iris: the account is removed');
END;

CREATE TRIGGER IF NOT EXISTS removed_account_item BEFORE INSERT ON item
  WHEN (SELECT state FROM account WHERE id = NEW.account_id) IN ('removing', 'removed')
BEGIN
  SELECT RAISE(ABORT, 'iris: the account is removed');
END;

CREATE TRIGGER IF NOT EXISTS removed_account_body BEFORE INSERT ON body
  WHEN (SELECT a.state FROM message m JOIN account a ON a.id = m.account_id
        WHERE m.id = NEW.message_id) IN ('removing', 'removed')
BEGIN
  SELECT RAISE(ABORT, 'iris: the account is removed');
END;

-- The jobs whose workspace can hold mail of a removed account. The removal
-- writes them before it deletes the items, because after that no other
-- row links a job to the account. The delete builds each workspace again
-- from an empty folder (other than notes.md), then deletes the row.
CREATE TABLE IF NOT EXISTS removed_workspace (
  account_id INTEGER NOT NULL REFERENCES account(id),
  job_id INTEGER NOT NULL,
  PRIMARY KEY (account_id, job_id)
) WITHOUT ROWID;

-- The sender addresses of the deleted messages of a removed account. Each
-- batch of the delete writes them before it deletes the headers, so the
-- last batch deletes only the senders of this account, also after a crash.
CREATE TABLE IF NOT EXISTS removed_sender (
  account_id INTEGER NOT NULL REFERENCES account(id),
  address TEXT NOT NULL,
  PRIMARY KEY (account_id, address)
) WITHOUT ROWID;
