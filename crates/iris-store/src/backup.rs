//! The backup of a live data folder (specification 1, section 5.1), the
//! backup lock, and the content cleanup of gone messages (section 4.5).

use std::fs::{self, File};
use std::io::{self, Write};
use std::path::Path;

use rusqlite::{params, Connection, OpenFlags, TransactionBehavior};

use crate::{
    content, lock, make_private_dir, new_private_dir, settings, sync_dir, Error, FileLock, Result,
    Store,
};

/// How long the store keeps the content of a gone message (section 4.5).
pub const GONE_KEEP_MS: i64 = 30 * 24 * 60 * 60 * 1000;

/// The most messages in one transaction of the cleanup.
const CLEAN_BATCH: i64 = 500;

/// The backup lock. While a value of this type exists, no other holder of
/// the lock can run: in this process or in another one. Drop it to release
/// the lock.
pub struct BackupLock(#[allow(dead_code)] FileLock);

/// What `policy.toml` must hold before [`Store::write_policy`] replaces it.
#[derive(Clone, Copy, Debug)]
pub enum PolicyExpect<'a> {
    /// No file.
    Missing,
    /// This text.
    Text(&'a [u8]),
    /// Any file, or no file.
    Any,
}

/// The result of [`Store::write_policy`].
#[derive(Debug, PartialEq, Eq)]
pub struct PolicyWrite {
    /// The new text is at `policy.toml`. False when the file there was not
    /// the expected one, or a new file came during the write.
    pub done: bool,
    /// The copy of the file that was at `policy.toml`, when it did not go
    /// back. A done write that replaced a file always keeps one.
    pub saved: Option<std::path::PathBuf>,
}

/// What [`Store::clean_gone`] removed.
#[derive(Debug, Default, PartialEq, Eq)]
pub struct Cleaned {
    /// Messages that lost their content, header and clean text.
    pub messages: u64,
    /// Content files that the cleanup deleted.
    pub files: u64,
    /// Tombstones that the cleanup deleted, because no item cites them.
    pub tombstones: u64,
}

impl Store {
    /// Take the backup lock. Wait while another holder has it.
    pub fn backup_lock(&self) -> Result<BackupLock> {
        Ok(BackupLock(FileLock::take(&self.dir.join("backup.lock"))?))
    }

    /// Take the backup lock, or give `None` if another holder has it.
    pub fn try_backup_lock(&self) -> Result<Option<BackupLock>> {
        Ok(FileLock::try_take(&self.dir.join("backup.lock"))?.map(BackupLock))
    }

    /// Copy the data folder to `dest` while the engine runs (section 5.1).
    /// `dest` must not exist, or must be an empty folder. Gives the number
    /// of content files in the copy. A content file that the database
    /// refers to and that is not there is an error: the copy is not complete.
    pub fn backup(&self, dest: &Path) -> Result<u64> {
        let _lock = self.backup_lock()?;
        // The copy holds the key and the mail.
        new_private_dir(dest)?;
        let dest_content = dest.join("content");
        make_private_dir(&dest_content)?;

        // VACUUM INTO reads one snapshot in one read transaction. The online
        // backup API starts again after each write of another connection,
        // so it can fail to end while the engine writes.
        let db_copy = dest.join("iris.db");
        let db_copy_str = db_copy
            .to_str()
            .ok_or_else(|| io::Error::other("the backup folder path is not UTF-8"))?;
        {
            let src = Connection::open_with_flags(
                self.dir.join("iris.db"),
                OpenFlags::SQLITE_OPEN_READ_ONLY | OpenFlags::SQLITE_OPEN_NO_MUTEX,
            )?;
            settings(&src)?;
            src.execute("VACUUM INTO ?1", [db_copy_str])?;
        }
        sync_file(&db_copy)?;

        // The content files that the copy refers to. The cleanup cannot
        // delete one of them while this function holds the lock.
        let copy = Connection::open_with_flags(&db_copy, OpenFlags::SQLITE_OPEN_READ_ONLY)?;
        copy.execute_batch("PRAGMA cache_size = -2048; PRAGMA temp_store = FILE;")?;
        let src_content = self.dir.join("content");
        let mut st = copy.prepare(
            "SELECT DISTINCT content_sha256 FROM message WHERE content_sha256 IS NOT NULL",
        )?;
        let mut rows = st.query([])?;
        let mut files = 0;
        while let Some(r) = rows.next()? {
            let sha: String = r.get(0)?;
            let (Some(from), Some(to)) = (
                content::path(&src_content, &sha),
                content::path(&dest_content, &sha),
            ) else {
                return Err(Error::Io(io::Error::other(
                    "the database has a content_sha256 that is not a SHA-256",
                )));
            };
            let dir = to.parent().expect("the path has a folder");
            make_private_dir(dir)?;
            fs::copy(&from, &to)?;
            sync_file(&to)?;
            sync_dir(dir)?;
            files += 1;
        }
        drop(rows);
        drop(st);
        drop(copy);
        sync_dir(&dest_content)?;

        // Without the key, the copy cannot decrypt the secrets.
        copy_private(&self.dir.join("key"), &dest.join("key"))?;
        let policy = self.dir.join("policy.toml");
        if policy.exists() {
            copy_private(&policy, &dest.join("policy.toml"))?;
        }
        sync_dir(dest)?;
        Ok(files)
    }

    /// Puts `text` at `policy.toml` (section 5.1), or no file for `None`,
    /// if the file there is `expect`. The caller holds the backup lock, so no
    /// backup runs; it takes the lock before a database write transaction, in
    /// the order of [`Store::clean_gone`].
    ///
    /// A person can save the file at any time, also through an open file, so
    /// the write never deletes the file that was there: it goes back to
    /// `policy.toml`, or it stays as `policy.toml.saved-<seconds>` in the
    /// data folder ([`PolicyWrite::saved`]).
    pub fn write_policy(
        &self,
        _lock: &BackupLock,
        expect: PolicyExpect<'_>,
        text: Option<&[u8]>,
    ) -> Result<PolicyWrite> {
        let path = self.dir.join("policy.toml");
        let tmp = self.dir.join("policy.toml.tmp");
        if let Some(text) = text {
            // After a crash, `tmp` can be a second name of `policy.toml`:
            // remove the name, never write into that file.
            match fs::remove_file(&tmp) {
                Err(e) if e.kind() != io::ErrorKind::NotFound => return Err(e.into()),
                _ => {}
            }
            let mut f = private_file(&tmp, true)?;
            f.write_all(text)?;
            f.sync_all()?;
        }
        // The rename moves the file that is there now, so a save after it
        // makes a new file and cannot be lost.
        let aside = self.saved_name();
        let moved = match fs::rename(&path, &aside) {
            Ok(()) => true,
            Err(e) if e.kind() == io::ErrorKind::NotFound => false,
            Err(e) => return Err(e.into()),
        };
        let matches = match expect {
            PolicyExpect::Any => true,
            PolicyExpect::Missing => !moved,
            PolicyExpect::Text(t) => moved && fs::read(&aside)? == t,
        };
        let mut out = PolicyWrite {
            done: false,
            saved: None,
        };
        if matches {
            // A hard link never replaces a file: a save since the rename stays.
            out.done = match text {
                None => true,
                Some(_) => match fs::hard_link(&tmp, &path) {
                    Ok(()) => true,
                    Err(e) if e.kind() == io::ErrorKind::AlreadyExists => false,
                    Err(e) => return Err(e.into()),
                },
            };
            // Keep the old file under its new name, never delete it: an
            // editor can hold it open and save into it after the check.
            if moved {
                out.saved = Some(aside);
            }
        } else if moved {
            // Put the file back, unless a new file came since the rename.
            match fs::hard_link(&aside, &path) {
                Ok(()) => fs::remove_file(&aside)?,
                Err(e) if e.kind() == io::ErrorKind::AlreadyExists => out.saved = Some(aside),
                Err(e) => return Err(e.into()),
            }
        }
        if text.is_some() {
            fs::remove_file(&tmp)?;
        }
        sync_dir(&self.dir)?;
        Ok(out)
    }

    /// A name for a copy of `policy.toml` that no file has yet.
    fn saved_name(&self) -> std::path::PathBuf {
        let secs = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map_or(0, |d| d.as_secs());
        let mut n = 1;
        loop {
            let name = if n == 1 {
                format!("policy.toml.saved-{secs}")
            } else {
                format!("policy.toml.saved-{secs}-{n}")
            };
            let path = self.dir.join(name);
            if !path.exists() {
                return path;
            }
            n += 1;
        }
    }

    /// The content cleanup of section 4.5, step 3. For each message that is
    /// gone for [`GONE_KEEP_MS`] or more at the time `now` (milliseconds
    /// since the Unix epoch): remove the reference to its content file, and
    /// delete its header and clean text. Delete each content file that no
    /// message refers to after that, and each tombstone that no item cites.
    ///
    /// Gives `None` and changes nothing if a backup holds the lock. The
    /// cleanup holds the lock for its full step.
    pub fn clean_gone(&self, now: i64) -> Result<Option<Cleaned>> {
        let Some(_lock) = self.try_backup_lock()? else {
            return Ok(None);
        };
        let cutoff = now - GONE_KEEP_MS;
        let root = self.dir.join("content");
        let mut out = Cleaned::default();
        loop {
            let mut conn = lock(&self.writer);
            let tx = conn.transaction_with_behavior(TransactionBehavior::Immediate)?;
            let batch: Vec<(i64, String)> = tx
                .prepare_cached(
                    "SELECT id, content_sha256 FROM message
                     WHERE gone_at <= ?1 AND content_sha256 IS NOT NULL LIMIT ?2",
                )?
                .query_map(params![cutoff, CLEAN_BATCH], |r| Ok((r.get(0)?, r.get(1)?)))?
                .collect::<rusqlite::Result<_>>()?;
            settle_hostile(&tx, cutoff, now)?;
            for (id, _) in &batch {
                tx.execute("DELETE FROM header WHERE message_id = ?1", [id])?;
                // The delete trigger on body removes the search text too.
                tx.execute("DELETE FROM body WHERE message_id = ?1", [id])?;
                tx.execute(
                    "UPDATE message SET content_sha256 = NULL WHERE id = ?1",
                    [id],
                )?;
            }
            for (_, sha) in &batch {
                let used: bool = tx.query_row(
                    "SELECT EXISTS(SELECT 1 FROM message WHERE content_sha256 = ?1)",
                    [sha],
                    |r| r.get(0),
                )?;
                if used {
                    continue;
                }
                // The delete runs while this transaction holds the write
                // lock. So a put_messages that refers to this file waits,
                // and then finds the file missing (see put_messages). After
                // a crash before the commit, the rows still refer to the
                // file, and the next cleanup does the step again.
                if let Some(p) = content::path(&root, sha) {
                    match fs::remove_file(&p) {
                        Ok(()) => out.files += 1,
                        Err(e) if e.kind() == io::ErrorKind::NotFound => {}
                        Err(e) => return Err(e.into()),
                    }
                }
            }
            // Cascades delete the labels of a tombstone.
            out.tombstones += tx.execute(
                "DELETE FROM message WHERE gone_at <= ?1 AND content_sha256 IS NULL
                 AND NOT EXISTS (SELECT 1 FROM item_source s WHERE s.message_id = message.id)",
                [cutoff],
            )? as u64;
            tx.commit()?;
            out.messages += batch.len() as u64;
            if (batch.len() as i64) < CLEAN_BATCH {
                return Ok(Some(out));
            }
        }
    }
}

/// Before the cleanup deletes the evidence of gone mail (the header that
/// names the sender, then the message row with its marks and labels), it
/// keeps the last verdict of each notice and each watch match on such mail
/// (decision D33). An open notice on hostile mail is marked done, so it
/// stays hidden. A watch whose match is hostile mail, a draft or a copy
/// waits again. A safe notice or match stays as it is.
fn settle_hostile(tx: &rusqlite::Transaction, cutoff: i64, now: i64) -> Result<()> {
    tx.execute(
        concat!(
            "UPDATE notice SET done_at = ?2 WHERE done_at IS NULL AND EXISTS (
               SELECT 1 FROM message m WHERE m.id = notice.message_id AND m.gone_at <= ?1
                 AND (",
            crate::hostile_mail!(),
            " OR ",
            crate::may_be_hostile!(),
            "))"
        ),
        params![cutoff, now],
    )?;
    tx.execute(
        concat!(
            "UPDATE watch SET message_id = NULL, arrived_at = NULL
             WHERE message_id IS NOT NULL AND EXISTS (
               SELECT 1 FROM message m WHERE m.id = watch.message_id AND m.gone_at <= ?1
                 AND (",
            crate::hostile_mail!(),
            " OR ",
            crate::may_be_hostile!(),
            " OR ",
            crate::draft_or_copy!("m"),
            "))"
        ),
        [cutoff],
    )?;
    Ok(())
}

/// A file with mode 0600 at `path`. With `new`, refuses a file that exists.
fn private_file(path: &Path, new: bool) -> io::Result<File> {
    let mut o = fs::OpenOptions::new();
    if new {
        o.write(true).create_new(true);
    } else {
        o.write(true).create(true).truncate(true);
    }
    #[cfg(unix)]
    std::os::unix::fs::OpenOptionsExt::mode(&mut o, 0o600);
    let f = o.open(path)?;
    // The mode of `open` applies only to a new file.
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        f.set_permissions(fs::Permissions::from_mode(0o600))?;
    }
    Ok(f)
}

/// Copy `from` to a new file `to` with mode 0600, and write it to disk.
fn copy_private(from: &Path, to: &Path) -> io::Result<()> {
    let mut src = File::open(from)?;
    let mut dst = private_file(to, false)?;
    io::copy(&mut src, &mut dst)?;
    dst.sync_all()
}

fn sync_file(path: &Path) -> io::Result<()> {
    File::open(path)?.sync_all()
}
