//! The removal of an account and the delete of its mail from the store
//! (decision D34, docs/specs/2026-11-24-remove-account.md). Nothing here
//! writes to the provider.

use std::fs;
use std::io;
use std::path::{Path, PathBuf};
use std::time::{Duration, Instant, SystemTime};

use rusqlite::{params, Connection, OptionalExtension, TransactionBehavior};

use crate::{content, lock, Error, FileLock, Result, Store};

/// `account.state` after the removal, while the delete runs.
pub const STATE_REMOVING: &str = "removing";
/// `account.state` after the delete: a tombstone with no mail.
pub const STATE_REMOVED: &str = "removed";

/// How long a waiting delete tries a busy checkpoint again.
const CHECKPOINT_WAIT: Duration = Duration::from_secs(10);

/// The most messages in one transaction of the delete.
const BATCH: i64 = 500;

/// What [`Store::delete_removed`] deleted.
#[derive(Debug, Default, PartialEq, Eq)]
pub struct Deleted {
    /// Accounts whose delete ended in this call.
    pub accounts: u64,
    pub messages: u64,
    /// Raw message files.
    pub files: u64,
}

/// The items that cite a message of `account`, and the items of the
/// account. A SQL condition on `item.id`, with `?1` the account.
const ITEMS: &str = "item.account_id = ?1 OR item.id IN (
    SELECT s.item_id FROM item_source s JOIN message m ON m.id = s.message_id
    WHERE m.account_id = ?1)";

/// The jobs whose scope has mail of `account` (`?1`): a thread of the
/// account, or a listed message of the account.
const JOBS: &str = "SELECT DISTINCT s.job_id FROM job_scope s
    WHERE s.thread_id IN (SELECT id FROM thread WHERE account_id = ?1)
       OR s.item_id IN (SELECT id FROM item WHERE account_id = ?1)
       OR EXISTS (SELECT 1 FROM job_scope_message l JOIN message m ON m.id = l.message_id
                  WHERE l.scope_id = s.id AND m.account_id = ?1)";

impl Store {
    /// The file of the sync lock of `account`. Each sync and each apply of
    /// the account holds it (`iris_sync::lock`), and so does the delete.
    pub fn sync_lock_path(&self, account: i64) -> PathBuf {
        self.dir().join(format!("sync-{account}.lock"))
    }

    /// Remove the account with `address`, in one transaction: end its
    /// sign-in and sync state, deny its open actions, delete its items, and
    /// set the state [`STATE_REMOVING`]. The mail stays until
    /// [`Store::delete_removed`] deletes it. Refuses an account that is not
    /// there or is removed, and an account with mail in the scope of a job
    /// whose session runs. Gives the account ID.
    pub fn remove_account(&self, address: &str, now: i64) -> Result<i64> {
        // The removal deletes the items of the account, so it runs with
        // secure_delete, as each batch of the delete: SQLite writes zeros
        // over the deleted rows, and no free page keeps their text.
        let mut conn = lock(&self.writer);
        conn.pragma_update(None, "secure_delete", true)?;
        let out = Self::removal(&mut conn, address, now);
        conn.pragma_update(None, "secure_delete", false)?;
        out
    }

    fn removal(conn: &mut Connection, address: &str, now: i64) -> Result<i64> {
        let tx = conn.transaction()?;
        let id = (|| -> Result<i64> {
            let tx = &tx;
            let row: Option<(i64, Option<String>)> = tx
                .query_row(
                    "SELECT id, state FROM account WHERE address = ?1",
                    [address],
                    |r| Ok((r.get(0)?, r.get(1)?)),
                )
                .optional()?;
            let id = match row {
                None => return Err(Error::Account("no account has this address")),
                Some((_, Some(s))) if s == STATE_REMOVING || s == STATE_REMOVED => {
                    return Err(Error::Account("the account is removed"))
                }
                Some((id, _)) => id,
            };
            let live: bool = tx.query_row(
                &format!(
                    "SELECT EXISTS (SELECT 1 FROM job_session WHERE reaped_at IS NULL
                       AND job_id IN ({JOBS}))"
                ),
                [id],
                |r| r.get(0),
            )?;
            if live {
                return Err(Error::Account(
                    "a job with mail of this account runs now, or its agent did not stop yet; stop the job, then try again",
                ));
            }
            // The tombstone keeps its ID, provider, address and state only:
            // each other column (the sign-in, the IMAP server and user, the
            // sync state, a column of a later migration) becomes NULL.
            let cols: Vec<String> = tx
                .prepare(
                    "SELECT name FROM pragma_table_info('account')
                     WHERE name NOT IN ('id', 'provider', 'address', 'state')",
                )?
                .query_map([], |r| r.get(0))?
                .collect::<rusqlite::Result<_>>()?;
            let clear: String = cols.iter().map(|c| format!(", \"{c}\" = NULL")).collect();
            tx.execute(
                &format!("UPDATE account SET state = ?2{clear} WHERE id = ?1"),
                params![id, STATE_REMOVING],
            )?;
            // The actions of the account that can still apply. An action in
            // review waits for the person (the trigger action_state), and
            // an action whose provider calls started ends as applied or
            // failed: both keep their state.
            tx.execute(
                "UPDATE changeset_action SET state = 'denied', decided_by = 'engine', rule = NULL,
                   policy = NULL, reason = 'the account was removed', decided_at = ?2
                 WHERE state IN ('proposed', 'approved', 'held')
                   AND NOT EXISTS (SELECT 1 FROM ledger l
                                   WHERE l.action_id = changeset_action.id AND l.state = 'writing')
                   AND (EXISTS (SELECT 1 FROM action_message am JOIN message m ON m.id = am.message_id
                                WHERE am.action_id = changeset_action.id AND m.account_id = ?1)
                     OR EXISTS (SELECT 1 FROM action_item ai JOIN item i ON i.id = ai.item_id
                                WHERE ai.action_id = changeset_action.id AND i.account_id = ?1)
                     OR EXISTS (SELECT 1 FROM action_draft ad JOIN draft d ON d.id = ad.draft_id
                                WHERE ad.action_id = changeset_action.id AND d.account_id = ?1))",
                params![id, now],
            )?;
            // Before the items go: after that, no other row links a job to
            // the account.
            tx.execute(
                &format!(
                    "INSERT OR IGNORE INTO removed_workspace(account_id, job_id)
                     SELECT ?1, job_id FROM ({JOBS})"
                ),
                [id],
            )?;
            // The brief has no item of the account from the commit on.
            delete_items(tx, id)?;
            Ok(id)
        })()?;
        tx.commit()?;
        Ok(id)
    }

    /// Delete the mail of each account in state [`STATE_REMOVING`], then set
    /// it to [`STATE_REMOVED`]. `workspace` builds again the workspace of a
    /// job whose scope had mail of the account: the build leaves out a
    /// removed account and deletes the files of the agent other than the
    /// notes. `senders` derives each engine sender kind again from the
    /// evidence that stays (`iris_senders::refresh`), in the transaction of
    /// the last batch. Each batch is one
    /// transaction, so a crash loses at most one batch, and the next call
    /// continues. An account whose sync lock is held waits for the next
    /// call. Gives `None` and changes nothing if a backup holds the lock.
    ///
    /// With `wait`, the call waits for each lock in place of a skip: the
    /// command and the API call of a removal return only after the delete
    /// (spec, "The call waits for the delete").
    pub fn delete_removed(
        &self,
        wait: bool,
        workspace: &mut dyn FnMut(i64) -> io::Result<()>,
        senders: &mut dyn FnMut(&rusqlite::Transaction) -> Result<()>,
    ) -> Result<Option<Deleted>> {
        let _backup = if wait {
            self.backup_lock()?
        } else {
            match self.try_backup_lock()? {
                Some(l) => l,
                None => return Ok(None),
            }
        };
        let accounts: Vec<i64> = self.read(|c| {
            let mut q = c.prepare("SELECT id FROM account WHERE state = ?1 ORDER BY id")?;
            let ids = q
                .query_map([STATE_REMOVING], |r| r.get(0))?
                .collect::<rusqlite::Result<_>>()?;
            Ok(ids)
        })?;
        let mut out = Deleted::default();
        for account in accounts {
            // No sync or apply of the account runs while the delete runs.
            // A sync of the account fails at its next insert (the triggers
            // of migration 46), so a wait ends soon.
            let path = self.sync_lock_path(account);
            let _sync = if wait {
                FileLock::take(&path)?
            } else {
                match FileLock::try_take(&path)? {
                    Some(l) => l,
                    None => continue,
                }
            };
            self.delete_account(account, wait, workspace, senders, &mut out)?;
        }
        Ok(Some(out))
    }

    fn delete_account(
        &self,
        account: i64,
        wait: bool,
        workspace: &mut dyn FnMut(i64) -> io::Result<()>,
        senders: &mut dyn FnMut(&rusqlite::Transaction) -> Result<()>,
        out: &mut Deleted,
    ) -> Result<()> {
        // A raw file older than this can only be of a sync that ended.
        let start = SystemTime::now();
        // The workspaces first. A build after the removal leaves the mail of
        // the account out (iris-workspace). A job with a live session (not
        // reaped) is skipped: the removal refused each live session, so this
        // one started after the removal, and its build already ran.
        let jobs: Vec<i64> = self.read(|c| {
            let mut q = c.prepare(
                "SELECT job_id FROM removed_workspace w WHERE account_id = ?1
                   AND NOT EXISTS (SELECT 1 FROM job_session s
                                   WHERE s.job_id = w.job_id AND s.reaped_at IS NULL)",
            )?;
            let ids = q.query_map([account], |r| r.get(0))?.collect::<rusqlite::Result<_>>()?;
            Ok(ids)
        })?;
        for job in jobs {
            workspace(job)?;
        }
        let root = self.dir().join("content");
        loop {
            let mut conn = lock(&self.writer);
            conn.pragma_update(None, "secure_delete", true)?;
            let tx = conn.transaction_with_behavior(TransactionBehavior::Immediate)?;
            // Items that cite mail of the account from another account.
            delete_items(&tx, account)?;
            let batch: Vec<(i64, Option<String>)> = tx
                .prepare_cached("SELECT id, content_sha256 FROM message WHERE account_id = ?1 LIMIT ?2")?
                .query_map(params![account, BATCH], |r| Ok((r.get(0)?, r.get(1)?)))?
                .collect::<rusqlite::Result<_>>()?;
            for (id, _) in &batch {
                // Each address that this message is evidence for: its
                // sender, and each evidence row (a recipient of sent mail).
                tx.execute(
                    "INSERT OR IGNORE INTO removed_sender(account_id, address)
                     SELECT ?1, from_addr FROM header WHERE message_id = ?2 AND from_addr IS NOT NULL
                     UNION SELECT ?1, address FROM sender_evidence WHERE message_id = ?2",
                    params![account, id],
                )?;
                for sql in [
                    "DELETE FROM header WHERE message_id = ?1",
                    // The delete trigger on body removes the search text too.
                    "DELETE FROM body WHERE message_id = ?1",
                    "DELETE FROM message_label WHERE message_id = ?1",
                    "DELETE FROM use_open WHERE message_id = ?1",
                    "DELETE FROM notice WHERE message_id = ?1",
                    // The cascades delete attention_due, link_check,
                    // sender_evidence and reply_draft.
                    "DELETE FROM message WHERE id = ?1",
                ] {
                    tx.execute(sql, [id])?;
                }
            }
            for sha in batch.iter().filter_map(|(_, s)| s.as_deref()) {
                if used(&tx, sha)? {
                    continue;
                }
                // As clean_gone: the file goes while this transaction holds
                // the write lock. After a crash before the commit, a row
                // with no file goes in the next batch.
                if let Some(p) = content::path(&root, sha) {
                    match fs::remove_file(&p) {
                        Ok(()) => out.files += 1,
                        Err(e) if e.kind() == io::ErrorKind::NotFound => {}
                        Err(e) => return Err(e.into()),
                    }
                }
            }
            out.messages += batch.len() as u64;
            let done = (batch.len() as i64) < BATCH;
            if done {
                end(&tx, account)?;
                senders(&tx)?;
                out.files += orphans(&tx, &root, start)?;
            }
            tx.commit()?;
            conn.pragma_update(None, "secure_delete", false)?;
            if !done {
                continue;
            }
            // The deleted pages leave the write-ahead log. A busy checkpoint
            // (a reader) keeps the state, so the next call tries again. A
            // waiting call tries for CHECKPOINT_WAIT.
            let deadline = Instant::now() + if wait { CHECKPOINT_WAIT } else { Duration::ZERO };
            let busy = loop {
                let busy: i64 =
                    conn.query_row("PRAGMA wal_checkpoint(TRUNCATE)", [], |r| r.get(0))?;
                if busy == 0 || Instant::now() >= deadline {
                    break busy;
                }
                std::thread::sleep(Duration::from_millis(100));
            };
            if busy == 0 {
                conn.execute(
                    "UPDATE account SET state = ?2 WHERE id = ?1",
                    params![account, STATE_REMOVED],
                )?;
                out.accounts += 1;
            }
            return Ok(());
        }
    }
}

/// The last step of the delete of `account`, in the transaction of its last
/// batch: threads, labels, sync rows and senders with no mail left.
fn end(c: &Connection, account: i64) -> Result<()> {
    // A thread that a draft refers to stays as a bare row: the draft is a
    // part of the record of actions.
    c.execute(
        "DELETE FROM talk_due WHERE thread_id IN (SELECT id FROM thread WHERE account_id = ?1)",
        [account],
    )?;
    c.execute(
        "DELETE FROM thread WHERE account_id = ?1
           AND NOT EXISTS (SELECT 1 FROM draft d WHERE d.thread_id = thread.id)",
        [account],
    )?;
    for sql in [
        "DELETE FROM label WHERE account_id = ?1",
        "DELETE FROM sync_log WHERE account_id = ?1",
        "DELETE FROM sync_seen WHERE account_id = ?1",
    ] {
        c.execute(sql, [account])?;
    }
    // An address that the mail of this account was evidence for, and that
    // no message that stays is evidence for (as a sender or in an evidence
    // row), goes, unless the person decided it: a decision of the person is
    // a rule of the person, not mail. The caller then derives each other
    // engine kind again from the evidence that stays.
    c.execute(
        "DELETE FROM sender WHERE decided_by IS NOT 'person'
           AND lower(address) IN (SELECT address FROM removed_sender WHERE account_id = ?1)
           AND NOT EXISTS (SELECT 1 FROM header h WHERE h.from_addr = lower(sender.address))
           AND NOT EXISTS (SELECT 1 FROM sender_evidence e WHERE e.address = lower(sender.address))",
        [account],
    )?;
    c.execute("DELETE FROM removed_sender WHERE account_id = ?1", [account])?;
    c.execute("DELETE FROM removed_workspace WHERE account_id = ?1", [account])?;
    c.execute("INSERT INTO message_fts(message_fts) VALUES ('optimize')", [])?;
    Ok(())
}

/// Delete each raw file under `root` that no message refers to and that
/// changed before `start`, in the write transaction `c`. A sync of the
/// removed account can write a file and then fail at the insert of its row.
/// A younger file can be of a sync of another account that runs now.
fn orphans(c: &Connection, root: &Path, start: SystemTime) -> Result<u64> {
    let mut n = 0;
    let dirs = match fs::read_dir(root) {
        Ok(d) => d,
        Err(e) if e.kind() == io::ErrorKind::NotFound => return Ok(0),
        Err(e) => return Err(e.into()),
    };
    for dir in dirs {
        let dir = dir?;
        if !dir.file_type()?.is_dir() {
            continue;
        }
        for f in fs::read_dir(dir.path())? {
            let f = f?;
            let name = f.file_name();
            let Some(sha) = name.to_str().and_then(|n| n.strip_suffix(".eml")) else {
                continue;
            };
            // Only the files of the store: the path of the name is this file.
            if content::path(root, sha).as_deref() != Some(f.path().as_path()) {
                continue;
            }
            let meta = f.metadata()?;
            if !meta.is_file() || meta.modified()? >= start || used(c, sha)? {
                continue;
            }
            match fs::remove_file(f.path()) {
                Ok(()) => n += 1,
                Err(e) if e.kind() == io::ErrorKind::NotFound => {}
                Err(e) => return Err(e.into()),
            }
        }
    }
    Ok(n)
}

/// Delete the items of `account` and each item that cites its mail, with
/// the rows that refer to them with no cascade.
fn delete_items(c: &Connection, account: i64) -> Result<()> {
    c.execute(
        &format!("DELETE FROM correction WHERE item_id IN (SELECT id FROM item WHERE {ITEMS})"),
        [account],
    )?;
    c.execute(
        &format!(
            "UPDATE item SET duplicate_of = NULL
             WHERE duplicate_of IN (SELECT id FROM item WHERE {ITEMS})"
        ),
        [account],
    )?;
    // The cascades delete item_source and matter_item.
    c.execute(&format!("DELETE FROM item WHERE {ITEMS}"), [account])?;
    Ok(())
}

fn used(c: &Connection, sha: &str) -> Result<bool> {
    Ok(c.query_row(
        "SELECT EXISTS(SELECT 1 FROM message WHERE content_sha256 = ?1)",
        [sha],
        |r| r.get(0),
    )?)
}
