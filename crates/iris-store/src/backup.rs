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

/// The largest file of `drafts/` that a backup copies. A larger file is not
/// a draft of mail (a body has at most 256 KiB, `iris-compose`).
pub const DRAFT_FILE_MAX: u64 = 1024 * 1024;

/// The most files of one `drafts/` folder that a backup copies.
pub const DRAFT_FILES_MAX: usize = 256;

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

        // Each attachment file (send content spec, section 2): a draft
        // action can name a file that no message names. Nothing deletes
        // one, so the copy takes each file whose name is its SHA-256.
        copy_attachments(
            &self.dir.join(crate::ATTACHMENTS),
            &dest.join(crate::ATTACHMENTS),
        )?;

        // The drafts of each job workspace (issue #350): the build makes the
        // other files of a workspace again from the store.
        copy_drafts(&self.dir.join("workspaces"), &dest.join("workspaces"))?;

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

impl Store {
    /// Delete each file of the attachment folder that is [`GONE_KEEP_MS`] or
    /// more old at `now` and that no live reference names (attachment
    /// cleanup spec `docs/specs/2026-11-25-attachment-cleanup.md`). Gives
    /// the number of deleted files, or `None` when a backup holds the lock.
    pub fn clean_attachments(&self, now: i64) -> Result<Option<u64>> {
        let Some(_lock) = self.try_backup_lock()? else {
            return Ok(None);
        };
        let root = self.dir.join(crate::ATTACHMENTS);
        let mut conn = lock(&self.writer);
        // The write lock stays held from the read of the references to the
        // last delete. put_attachment renames under the same lock, so an
        // upload makes a file new before the check or after the delete.
        let tx = conn.transaction_with_behavior(TransactionBehavior::Immediate)?;
        let live: std::collections::HashSet<String> = tx
            .prepare(LIVE_ATTACHMENTS)?
            .query_map([], |r| r.get(0))?
            .collect::<rusqlite::Result<_>>()?;
        let cutoff = now - GONE_KEEP_MS;
        let mut deleted = 0;
        let dirs = match fs::read_dir(&root) {
            Ok(d) => d,
            Err(e) if e.kind() == io::ErrorKind::NotFound => return Ok(Some(0)),
            Err(e) => return Err(e.into()),
        };
        for dir in dirs {
            let dir = dir?;
            let Some(aa) = dir.file_name().to_str().map(str::to_owned) else {
                continue;
            };
            if aa.len() != 2 || !dir.file_type()?.is_dir() {
                continue;
            }
            for file in fs::read_dir(dir.path())? {
                let file = file?;
                let Some(sha) = file.file_name().to_str().map(str::to_owned) else {
                    continue;
                };
                // Only `<aa>/<sha256>`: a temporary file or any other name
                // stays.
                if content::file_path(&root, &sha, "").as_deref() != Some(&*file.path())
                    || !file.file_type()?.is_file()
                    || live.contains(&sha)
                {
                    continue;
                }
                let modified = file.metadata()?.modified()?;
                let ms = match modified.duration_since(std::time::UNIX_EPOCH) {
                    Ok(d) => i64::try_from(d.as_millis()).unwrap_or(i64::MAX),
                    Err(_) => i64::MIN,
                };
                if ms > cutoff {
                    continue;
                }
                match fs::remove_file(file.path()) {
                    Ok(()) => deleted += 1,
                    Err(e) if e.kind() == io::ErrorKind::NotFound => {}
                    Err(e) => return Err(e.into()),
                }
            }
        }
        tx.commit()?;
        Ok(Some(deleted))
    }
}

/// The SHA-256 of each attachment file that a live reference names (spec
/// section 1): an open action (a draft revision, `proposed`, `in_review`,
/// `approved` or `held`), a ledger claim in `writing`, the content before
/// a draft change whose undo is available (view `ledger_can_undo`), and a
/// draft row that is not sent and not foreign. Each such file can be read
/// again by an apply, a recovery or an undo. The match does not filter by
/// verb, so each place that holds the content keeps its files.
///
/// The JSON is input that Iris does not control (`iris draft` saves a
/// revision before its check). So each document is read only when it is
/// valid JSON with an array at the path, and each entry only when it is an
/// object with a text `sha256`. Any other value names no file and does not
/// stop the cleanup.
const LIVE_ATTACHMENTS: &str = "
WITH refs(doc, path) AS (
  SELECT a.args, '$.attachments' FROM changeset_action a
  WHERE a.state IS NULL OR a.state IN ('proposed', 'in_review', 'approved', 'held')
  UNION ALL
  SELECT l.post, '$.content.attachments' FROM ledger l WHERE l.state = 'writing'
  UNION ALL
  SELECT l.pre, '$.draft.content.attachments' FROM ledger l
  WHERE l.id IN (SELECT ledger_id FROM ledger_can_undo)
  UNION ALL
  SELECT d.content, '$.content.attachments' FROM draft d
  WHERE d.sent_at IS NULL AND d.foreign_at IS NULL
),
arrays(doc, path) AS (
  SELECT CASE WHEN json_valid(doc) THEN
           CASE WHEN json_type(doc, path) = 'array' THEN doc END END, path
  FROM refs
),
hashes(sha) AS (
  SELECT CASE WHEN j.type = 'object' THEN
           CASE WHEN json_type(j.value, '$.sha256') = 'text'
             THEN json_extract(j.value, '$.sha256') END END
  FROM arrays r, json_each(r.doc, r.path) j
)
SELECT DISTINCT sha FROM hashes WHERE sha IS NOT NULL";

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
                 AND ",
            not_received!(),
            ")"
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
/// Copy each file `<aa>/<sha256>` of the attachment folder `src` whose
/// bytes have that SHA-256. A file with another name (a temporary file of
/// a writer) is not copied. A file whose bytes do not agree with its name
/// is an error: the copy would hold a damaged file.
fn copy_attachments(src: &Path, dest: &Path) -> Result<()> {
    use sha2::Digest as _;
    let dirs = match fs::read_dir(src) {
        Ok(d) => d,
        Err(e) if e.kind() == io::ErrorKind::NotFound => return Ok(()),
        Err(e) => return Err(e.into()),
    };
    make_private_dir(dest)?;
    for d in dirs {
        let d = d?;
        if !d.file_type()?.is_dir() {
            continue;
        }
        for f in fs::read_dir(d.path())? {
            let f = f?;
            let name = f.file_name();
            let Some(sha) = name.to_str() else { continue };
            let Some(to) = content::file_path(dest, sha, "") else {
                continue;
            };
            let bytes = fs::read(f.path())?;
            if content::hex(&sha2::Sha256::digest(&bytes)) != sha {
                return Err(Error::Io(io::Error::other(format!(
                    "the attachment file {sha} does not have its SHA-256"
                ))));
            }
            let dir = to.parent().expect("the path has a folder");
            make_private_dir(dir)?;
            fs::write(&to, &bytes)?;
            sync_file(&to)?;
            sync_dir(dir)?;
        }
    }
    sync_dir(dest)?;
    Ok(())
}

/// Copy `<src>/<job>/drafts/<file>` of each job workspace to the same path
/// under `dest`, with mode 0600 and folders of mode 0700.
///
/// A program in a session writes `drafts/` and can change it during the
/// copy. So the copy opens each folder relative to the handle of its parent,
/// with no link at any level (`O_NOFOLLOW`), and opens each file relative to
/// the handle of its `drafts/` folder. A rename of a folder after the open
/// does not move the copy: it reads only the folder that it opened. The copy
/// takes only regular files (no FIFO, no device, no subfolder), at most
/// [`DRAFT_FILE_MAX`] bytes each, and the first [`DRAFT_FILES_MAX`] names (by
/// byte order). The scan keeps at most [`DRAFT_FILES_MAX`] names in memory,
/// so a folder with many entries does not grow the memory of the engine. A
/// file that goes away, or that is not a regular file when the copy opens
/// it, is not copied. A file that changes during the copy gives the bytes of
/// one read, at most [`DRAFT_FILE_MAX`].
#[cfg(unix)]
fn copy_drafts(src: &Path, dest: &Path) -> Result<()> {
    let Some(root) = dir_at(None, src.as_os_str())? else {
        return Ok(());
    };
    let mut any = false;
    // The engine makes each job folder; a session cannot add one here.
    for job in smallest_names(&root, usize::MAX, true)? {
        let Some(job_dir) = dir_at(Some(&root), &job)? else {
            continue;
        };
        let Some(drafts) = dir_at(Some(&job_dir), std::ffi::OsStr::new("drafts"))? else {
            continue;
        };
        let to_job = dest.join(&job);
        if copy_from(&drafts, &to_job.join("drafts"))? {
            // The new `drafts` entry is in the job folder, and the new job
            // entry is in `dest`: sync both before the backup says done.
            sync_dir(&to_job)?;
            any = true;
        }
    }
    if any {
        sync_dir(dest)?;
    }
    Ok(())
}

#[cfg(not(unix))]
fn copy_drafts(_src: &Path, _dest: &Path) -> Result<()> {
    Ok(())
}

/// Copy the drafts of one opened `drafts/` folder to `to_dir`. True if the
/// copy made `to_dir`.
#[cfg(unix)]
fn copy_from(drafts: &std::os::fd::OwnedFd, to_dir: &Path) -> Result<bool> {
    let mut made = false;
    for name in smallest_names(drafts, DRAFT_FILES_MAX, false)? {
        let Some(bytes) = read_draft(drafts, &name)? else {
            continue;
        };
        make_private_dir(to_dir)?;
        made = true;
        let to = to_dir.join(&name);
        private_file(&to, true)?.write_all(&bytes)?;
        sync_file(&to)?;
    }
    if made {
        sync_dir(to_dir)?;
    }
    Ok(made)
}

/// Open the folder `name` relative to `parent` (or the path `name` with no
/// parent), with no link. `None` if it is gone, is not a folder, or is a
/// link.
#[cfg(unix)]
fn dir_at(
    parent: Option<&std::os::fd::OwnedFd>,
    name: &std::ffi::OsStr,
) -> io::Result<Option<std::os::fd::OwnedFd>> {
    open_at(parent, name, libc::O_DIRECTORY)
}

#[cfg(unix)]
fn open_at(
    parent: Option<&std::os::fd::OwnedFd>,
    name: &std::ffi::OsStr,
    flags: libc::c_int,
) -> io::Result<Option<std::os::fd::OwnedFd>> {
    use std::os::fd::{AsRawFd, FromRawFd};
    use std::os::unix::ffi::OsStrExt;
    let c = std::ffi::CString::new(name.as_bytes())
        .map_err(|_| io::Error::from(io::ErrorKind::InvalidInput))?;
    let at = parent.map_or(libc::AT_FDCWD, |p| p.as_raw_fd());
    let flags = flags | libc::O_RDONLY | libc::O_NOFOLLOW | libc::O_NONBLOCK | libc::O_CLOEXEC;
    // SAFETY: `c` is a valid C string and `at` is an open folder or AT_FDCWD.
    let fd = unsafe { libc::openat(at, c.as_ptr(), flags) };
    if fd < 0 {
        let e = io::Error::last_os_error();
        return match e.raw_os_error() {
            Some(libc::ENOENT | libc::ENOTDIR | libc::ELOOP) => Ok(None),
            _ => Err(e),
        };
    }
    // SAFETY: `fd` is a new descriptor that nothing else owns.
    Ok(Some(unsafe { std::os::fd::OwnedFd::from_raw_fd(fd) }))
}

/// The `max` smallest names (by byte order) of the entries of the folder
/// `dir` that can be folders (`dirs`) or regular files, sorted. The scan
/// keeps at most `max` names in memory. The caller checks the type again
/// at the open, because an entry can change after the scan.
#[cfg(unix)]
fn smallest_names(
    dir: &std::os::fd::OwnedFd,
    max: usize,
    dirs: bool,
) -> io::Result<Vec<std::ffi::OsString>> {
    use std::os::fd::IntoRawFd;
    use std::os::unix::ffi::OsStrExt;
    // `fdopendir` takes the descriptor, so give it a copy.
    let copy = dir.try_clone()?;
    // SAFETY: `copy` is an open folder; `closedir` below closes it.
    let d = unsafe { libc::fdopendir(copy.into_raw_fd()) };
    if d.is_null() {
        return Err(io::Error::last_os_error());
    }
    struct Close(*mut libc::DIR);
    impl Drop for Close {
        fn drop(&mut self) {
            // SAFETY: the pointer came from `fdopendir` and is closed once.
            unsafe { libc::closedir(self.0) };
        }
    }
    let d = Close(d);
    // The copy shares the read offset of `dir`: start at the first entry.
    // SAFETY: `d.0` is an open DIR.
    unsafe { libc::rewinddir(d.0) };
    let mut heap = std::collections::BinaryHeap::new();
    loop {
        // `readdir` gives NULL at the end and on an error: only errno tells
        // the two apart, so a failed read does not look like the end.
        set_errno(0);
        // SAFETY: `d.0` is an open DIR.
        let e = unsafe { libc::readdir(d.0) };
        if e.is_null() {
            let err = io::Error::last_os_error();
            if err.raw_os_error().is_some_and(|n| n != 0) {
                return Err(err);
            }
            break;
        }
        // SAFETY: `e` points to a valid entry until the next `readdir`.
        let (t, name) = unsafe {
            (
                (*e).d_type,
                std::ffi::CStr::from_ptr((*e).d_name.as_ptr())
                    .to_bytes()
                    .to_vec(),
            )
        };
        if name == b"." || name == b".." {
            continue;
        }
        let want = if dirs { libc::DT_DIR } else { libc::DT_REG };
        if t != want && t != libc::DT_UNKNOWN {
            continue;
        }
        heap.push(name);
        if heap.len() > max {
            heap.pop();
        }
    }
    Ok(heap
        .into_sorted_vec()
        .into_iter()
        .map(|n| std::ffi::OsStr::from_bytes(&n).to_owned())
        .collect())
}

#[cfg(unix)]
fn set_errno(v: libc::c_int) {
    // SAFETY: the errno of this thread is always valid to write.
    unsafe {
        #[cfg(any(target_os = "linux", target_os = "android"))]
        {
            *libc::__errno_location() = v;
        }
        #[cfg(not(any(target_os = "linux", target_os = "android")))]
        {
            *libc::__error() = v;
        }
    }
}

/// The bytes of the file `name` of the opened folder `dir`, or `None` if it
/// is gone, is not a regular file, or has more than [`DRAFT_FILE_MAX`] bytes.
#[cfg(unix)]
fn read_draft(dir: &std::os::fd::OwnedFd, name: &std::ffi::OsStr) -> Result<Option<Vec<u8>>> {
    use std::io::Read as _;
    // The type check below refuses a folder, a FIFO and a device.
    let Some(fd) = open_at(Some(dir), name, 0)? else {
        return Ok(None);
    };
    let file = File::from(fd);
    let m = file.metadata()?;
    if !m.is_file() || m.len() > DRAFT_FILE_MAX {
        return Ok(None);
    }
    let mut bytes = Vec::new();
    file.take(DRAFT_FILE_MAX + 1).read_to_end(&mut bytes)?;
    if bytes.len() as u64 > DRAFT_FILE_MAX {
        return Ok(None);
    }
    Ok(Some(bytes))
}

fn copy_private(from: &Path, to: &Path) -> io::Result<()> {
    let mut src = File::open(from)?;
    let mut dst = private_file(to, false)?;
    io::copy(&mut src, &mut dst)?;
    dst.sync_all()
}

fn sync_file(path: &Path) -> io::Result<()> {
    File::open(path)?.sync_all()
}

#[cfg(all(test, unix))]
mod tests {
    use super::*;
    use std::ffi::OsStr;

    fn temp(name: &str) -> std::path::PathBuf {
        let d = std::env::temp_dir().join(format!("iris-drafts-{name}-{}", std::process::id()));
        let _ = fs::remove_dir_all(&d);
        fs::create_dir_all(&d).unwrap();
        d
    }

    /// A rename of `drafts/` after the open, and a link to the data folder
    /// in its place, do not move the copy: it reads the folder that it
    /// opened, never the target of the link.
    #[test]
    fn the_copy_reads_the_opened_folder_after_a_swap() {
        let d = temp("swap");
        let drafts = d.join("job/drafts");
        fs::create_dir_all(&drafts).unwrap();
        fs::write(drafts.join("key"), b"draft").unwrap();
        let data = d.join("data");
        fs::create_dir(&data).unwrap();
        fs::write(data.join("key"), b"secret").unwrap();

        let job = dir_at(None, d.join("job").as_os_str()).unwrap().unwrap();
        let pinned = dir_at(Some(&job), OsStr::new("drafts")).unwrap().unwrap();
        fs::rename(&drafts, d.join("job/old")).unwrap();
        std::os::unix::fs::symlink(&data, &drafts).unwrap();

        let to = d.join("out/drafts");
        assert!(copy_from(&pinned, &to).unwrap());
        assert_eq!(fs::read(to.join("key")).unwrap(), b"draft");
        // A new open of the path refuses the link.
        assert!(dir_at(Some(&job), OsStr::new("drafts")).unwrap().is_none());
        let _ = fs::remove_dir_all(&d);
    }

    /// The scan keeps the smallest names only, and never more than `max`.
    #[test]
    fn the_scan_keeps_only_the_smallest_names() {
        let d = temp("scan");
        for i in (0..600).rev() {
            fs::write(d.join(format!("{i:04}")), b"x").unwrap();
        }
        fs::create_dir(d.join("0000a")).unwrap();
        let dir = dir_at(None, d.as_os_str()).unwrap().unwrap();
        let names = smallest_names(&dir, 3, false).unwrap();
        // The folder `0000a` is not a file, so it takes no place.
        assert_eq!(names, ["0000", "0001", "0002"]);
        assert_eq!(
            smallest_names(&dir, DRAFT_FILES_MAX, false).unwrap().len(),
            DRAFT_FILES_MAX
        );
        let _ = fs::remove_dir_all(&d);
    }
}
