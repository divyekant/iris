//! The Iris store (specification 1, section 5).
//!
//! One data folder holds all state: the SQLite database `iris.db`, the
//! encryption key `key`, and the raw messages in `content/`. This crate opens
//! the folder, applies the schema migrations, and gives the read and write
//! paths that the engine uses.

/// The SQL condition "message `m` can be hostile": it has a hostile mark
/// (specification 1, section 6.7), or it waits for its link check (a
/// `link_check` row, migration 20). Each gate that lets the model, a rule, an
/// agent, a notice or a watch use mail reads this condition, not the column
/// `hostile` alone. The test `each_gate_reads_may_be_hostile` finds other
/// forms.
#[macro_export]
macro_rules! may_be_hostile {
    () => {
        "((m.hostile IS NOT NULL AND m.hostile NOT IN ('', '[]'))
          OR EXISTS (SELECT 1 FROM link_check lc WHERE lc.message_id = m.id))"
    };
}

/// The SQL condition "message `m` is hostile mail": a hostile mark, the
/// label `SPAM`, or a `hostile` sender (specification 1, section 6.7).
/// Understanding deletes the unpinned items of such a message. Each read
/// that shows items to the person or to the model leaves out an item whose
/// origin is such a message, so a pinned item is hidden too (issue 112).
/// A message that only waits for its link check is not hostile mail here:
/// a gate of mail uses [`may_be_hostile!`].
#[macro_export]
macro_rules! hostile_mail {
    () => {
        "((m.hostile IS NOT NULL AND m.hostile NOT IN ('', '[]'))
          OR EXISTS (SELECT 1 FROM message_label hml JOIN label hl ON hl.id = hml.label_id
               WHERE hml.message_id = m.id AND hl.provider_id = 'SPAM')
          OR EXISTS (SELECT 1 FROM header hh JOIN sender hs
               ON hs.address = hh.from_addr COLLATE NOCASE
               WHERE hh.message_id = m.id AND hs.kind = 'hostile'))"
    };
}

/// The SQL condition "item `$i` comes from hostile mail": its origin
/// message is [`hostile_mail!`].
#[macro_export]
macro_rules! from_hostile_mail {
    ($i:literal) => {
        concat!(
            "EXISTS (SELECT 1 FROM item_source hos JOIN message m ON m.id = hos.message_id
               WHERE hos.item_id = ",
            $i,
            ".id AND hos.role = 'origin' AND ",
            $crate::hostile_mail!(),
            ")"
        )
    };
}

/// The SQL condition "message `$m` is not received mail": it has the label
/// `DRAFT`, `UNSENT` or `SENT_COPY` (docs/specs/2026-11-17-imap-provider.md,
/// section 4). The understanding, the watches, the people, the conversations,
/// the reply parent and the model context skip such a message. `UNSENT` and
/// `SENT_COPY` are no proof that you wrote the mail: a check of authorship
/// reads `SENT` and `DRAFT` only. The test `each_draft_read_is_classified`
/// finds other forms.
#[macro_export]
macro_rules! draft_or_copy {
    ($m:literal) => {
        concat!(
            "EXISTS (SELECT 1 FROM message_label ml JOIN label l ON l.id = ml.label_id
               WHERE ml.message_id = ",
            $m,
            ".id AND l.provider_id IN ('DRAFT', 'UNSENT', 'SENT_COPY'))"
        )
    };
}

/// SQL: the message `$m` is of an account in state `removing` or
/// `removed` (docs/specs/2026-11-24-remove-account.md). Its mail goes, so no
/// reader passes it on: understanding and the workspace of a job skip it.
#[macro_export]
macro_rules! removed_account {
    ($m:literal) => {
        concat!(
            "coalesce((SELECT a.state FROM account a WHERE a.id = ",
            $m,
            ".account_id), '') IN ('removing', 'removed')"
        )
    };
}

/// SQL: the message `m` is not mail that you got, for a watch (card R5-07):
/// hostile mail, mail that may be hostile, your own sent mail (`SENT`), or
/// a draft or a copy. The first arrival, the read gate of a stored match
/// and the content cleanup read this one rule, so a match that changes
/// later stops in each place (issue #931).
macro_rules! not_received {
    () => {
        concat!(
            "(",
            crate::hostile_mail!(),
            " OR ",
            crate::may_be_hostile!(),
            " OR EXISTS (SELECT 1 FROM message_label sml JOIN label sl ON sl.id = sml.label_id
               WHERE sml.message_id = m.id AND sl.provider_id = 'SENT') OR ",
            crate::draft_or_copy!("m"),
            ")"
        )
    };
}

/// The labels of [`draft_or_copy!`], for a check in Rust.
pub const DRAFT_OR_COPY: [&str; 3] = ["DRAFT", "UNSENT", "SENT_COPY"];

mod backup;
mod content;
mod file_lock;
mod id;
mod remove;
mod search;
mod secret;
mod tokens;
pub mod verify_log;
mod watch;

use std::borrow::Cow;
use std::fmt;
use std::fs;
use std::io;
use std::path::{Path, PathBuf};
use std::sync::{Mutex, MutexGuard};

use rusqlite::{params, Connection, OpenFlags, OptionalExtension, Transaction};

pub use backup::{
    BackupLock, Cleaned, PolicyExpect, PolicyWrite, ATTACH_BATCH, DRAFT_FILES_MAX, DRAFT_FILE_MAX,
    GONE_KEEP_MS,
};
pub use content::ContentWriter;
pub use file_lock::{FileLock, Probe};
pub use remove::{Deleted, STATE_REMOVED, STATE_REMOVING};

/// The result of a `sync_log` row whose process ended before the sync
/// ended (issue #710).
pub const SYNC_STOPPED: &str = "stopped: the process ended before the sync ended";
pub use id::{format_id, parse_id, Kind};
pub use search::{day_ms, Query};
pub use tokens::{index_tokens, Token};
pub use watch::{Watch, SINCE_MARGIN_MS, TEXT_MAX, WAIT_MAX};

/// The schema migrations, in order. Item `n` makes `PRAGMA user_version` `n + 1`.
/// Migrations go forward only. Never change a migration after it merged: add one.
const MIGRATIONS: &[&str] = &[
    include_str!("schema_1.sql"),
    include_str!("schema_2.sql"),
    include_str!("schema_3.sql"),
    include_str!("schema_4.sql"),
    include_str!("schema_5.sql"),
    include_str!("schema_6.sql"),
    include_str!("schema_7.sql"),
    include_str!("schema_8.sql"),
    include_str!("schema_9.sql"),
    include_str!("schema_10.sql"),
    include_str!("schema_11.sql"),
    include_str!("schema_12.sql"),
    include_str!("schema_13.sql"),
    include_str!("schema_14.sql"),
    include_str!("schema_15.sql"),
    include_str!("schema_16.sql"),
    include_str!("schema_17.sql"),
    include_str!("schema_18.sql"),
    include_str!("schema_19.sql"),
    include_str!("schema_20.sql"),
    include_str!("schema_21.sql"),
    include_str!("schema_22.sql"),
    include_str!("schema_23.sql"),
    include_str!("schema_24.sql"),
    include_str!("schema_25.sql"),
    include_str!("schema_26.sql"),
    include_str!("schema_27.sql"),
    include_str!("schema_28.sql"),
    include_str!("schema_29.sql"),
    include_str!("schema_30.sql"),
    include_str!("schema_31.sql"),
    include_str!("schema_32.sql"),
    include_str!("schema_33.sql"),
    include_str!("schema_34.sql"),
    include_str!("schema_35.sql"),
    include_str!("schema_36.sql"),
    include_str!("schema_37.sql"),
    include_str!("schema_38.sql"),
    include_str!("schema_39.sql"),
    include_str!("schema_40.sql"),
    include_str!("schema_41.sql"),
    include_str!("schema_42.sql"),
    include_str!("schema_43.sql"),
    include_str!("schema_44.sql"),
    include_str!("schema_45.sql"),
    include_str!("schema_46.sql"),
    include_str!("schema_47.sql"),
    include_str!("schema_48.sql"),
    include_str!("schema_49.sql"),
    include_str!("schema_50.sql"),
    include_str!("schema_51.sql"),
    include_str!("schema_52.sql"),
    include_str!("schema_53.sql"),
    include_str!("schema_54.sql"),
    include_str!("schema_55.sql"),
    include_str!("schema_56.sql"),
    include_str!("schema_57.sql"),
    include_str!("schema_58.sql"),
    include_str!("schema_59.sql"),
    include_str!("schema_60.sql"),
    include_str!("schema_61.sql"),
    include_str!("schema_62.sql"),
    include_str!("schema_63.sql"),
    include_str!("schema_64.sql"),
    include_str!("schema_65.sql"),
    include_str!("schema_66.sql"),
    include_str!("schema_67.sql"),
];

/// The SQLite settings of specification 1, section 5.2, for each connection,
/// after `journal_mode` (see [`settings`]).
const PRAGMAS: &str = "
PRAGMA synchronous = FULL;
PRAGMA foreign_keys = ON;
PRAGMA cache_size = -2048;
PRAGMA mmap_size = 0;
PRAGMA temp_store = FILE;
PRAGMA busy_timeout = 5000;
";

#[derive(Debug)]
pub enum Error {
    Db(rusqlite::Error),
    Io(io::Error),
    /// The database has a schema version that this program does not know:
    /// a newer version, or a negative one.
    UnknownSchema(i64),
    /// The `key` file is not 32 bytes, or a secret does not decrypt.
    Key(&'static str),
    /// The search text is not valid.
    Query(String),
    /// A message refers to a content file that is not in the content folder:
    /// the content cleanup deleted it, or the write of the file did not end.
    /// Fetch the message again.
    MissingContent(String),
    /// The text of a watch is not valid, or too many watches wait.
    Watch(&'static str),
    /// The address has an account of another provider or of another
    /// mailbox ([`Store::save_account_secret`]).
    Account(&'static str),
    /// The time zone name is not in the time zone database of the computer.
    TimeZone(String),
    /// Another message row of the account has the new provider ID
    /// ([`rebind`]).
    Taken(String),
}

impl fmt::Display for Error {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Error::Db(e) => write!(f, "database: {e}"),
            Error::Io(e) => write!(f, "file: {e}"),
            Error::UnknownSchema(v) => write!(
                f,
                "the database has schema version {v}, and this program knows only 0 to {}",
                MIGRATIONS.len()
            ),
            Error::Key(why) => write!(f, "secret: {why}"),
            Error::Query(why) => write!(f, "search: {why}"),
            Error::MissingContent(sha) => write!(f, "content file {sha} is missing"),
            Error::Watch(why) => write!(f, "watch: {why}"),
            Error::Account(why) => write!(f, "account: {why}"),
            Error::TimeZone(name) => write!(
                f,
                "time zone: {name} is not a time zone of this computer (use an IANA name, for example Europe/Berlin)"
            ),
            Error::Taken(id) => write!(f, "another message has the provider ID {id}"),
        }
    }
}

impl std::error::Error for Error {}

impl From<rusqlite::Error> for Error {
    fn from(e: rusqlite::Error) -> Self {
        Error::Db(e)
    }
}

impl From<io::Error> for Error {
    fn from(e: io::Error) -> Self {
        Error::Io(e)
    }
}

pub type Result<T> = std::result::Result<T, Error>;

/// The most characters of clean text for each message (specification 1,
/// section 7.2). The store cuts a longer text before FTS5 indexes it, because
/// FTS5 holds the terms of a document in memory while it indexes the
/// document. Same value as `iris_clean::TEXT_LIMIT`.
/// The `model` value of each open model document that Iris writes
/// (spec 4, rule 2.1).
pub const MODEL: &str = "iris/1.4";

/// The `$id` of the common definitions of the open model.
pub const COMMON_SCHEMA_ID: &str = "urn:iris:model:1:common";

pub const TEXT_LIMIT: usize = 32_000;
/// The most characters of the subject and of the sender in `body`.
pub const FIELD_LIMIT: usize = 1_000;
/// The mark at the end of a text that was cut. Same value as
/// `iris_clean::CUT_MARK`.
pub const CUT_MARK: &str = "\n\n[Iris cut the text here.]";

/// The default data folder (specification 1, section 5.1). `IRIS_DATA`
/// changes it. The `--data` setting of the program has priority over both.
pub fn default_data_dir() -> Option<PathBuf> {
    let var = |name| std::env::var_os(name).filter(|v| !v.is_empty());
    if let Some(dir) = var("IRIS_DATA") {
        return Some(PathBuf::from(dir));
    }
    let home = var("HOME").map(PathBuf::from);
    if cfg!(target_os = "macos") {
        return home.map(|h| h.join("Library/Application Support/iris"));
    }
    match var("XDG_DATA_HOME").map(PathBuf::from) {
        // The XDG rules say to ignore a relative path.
        Some(xdg) if xdg.is_absolute() => Some(xdg.join("iris")),
        _ => home.map(|h| h.join(".local/share/iris")),
    }
}

/// One message to write with [`Store::put_messages`].
#[derive(Clone, Debug, Default)]
pub struct NewMessage {
    pub provider_id: String,
    pub thread_provider_id: String,
    pub content_sha256: Option<String>,
    pub size: Option<i64>,
    pub internal_date: Option<i64>,
    pub fetched_at: Option<i64>,
    /// Provider IDs of the labels. A label that the store does not have is
    /// skipped (specification 1, section 4.3).
    pub labels: Vec<String>,
    pub header: Header,
    /// The clean text (specification 1, section 7.2), if it is known. The
    /// store applies the limits of [`Store::put_body`].
    pub body: Option<Body>,
}

#[derive(Clone, Debug, Default)]
pub struct Header {
    pub date: Option<i64>,
    pub from_addr: Option<String>,
    pub from_name: Option<String>,
    pub to_addrs: Option<String>,
    pub cc_addrs: Option<String>,
    pub subject: Option<String>,
    pub message_id_hdr: Option<String>,
    pub in_reply_to: Option<String>,
    pub list_id: Option<String>,
    pub auth: Option<String>,
}

impl Header {
    /// True if no field is known, as after a raw fetch that is not parsed.
    pub fn is_empty(&self) -> bool {
        let Header {
            date,
            from_addr,
            from_name,
            to_addrs,
            cc_addrs,
            subject,
            message_id_hdr,
            in_reply_to,
            list_id,
            auth,
        } = self;
        date.is_none()
            && [from_addr, from_name, to_addrs, cc_addrs, subject]
                .iter()
                .chain(&[message_id_hdr, in_reply_to, list_id, auth])
                .all(|f| f.is_none())
    }
}

#[derive(Clone, Debug, Default)]
pub struct Body {
    pub subject: String,
    pub sender: String,
    pub text: String,
}

/// The output of [`Store::read_to`]: a buffered spool file.
pub type Spool = io::BufWriter<fs::File>;

/// The most streamed answers ([`Store::read_to`]) of one store at the same
/// time. Each one holds a spool file open until its peer reads the end, so
/// many slow peers could use all file descriptors of the process (#713).
pub const MAX_SPOOLS: usize = 32;

/// One place of [`MAX_SPOOLS`], given back at the drop.
struct SpoolSlot<'a>(&'a std::sync::atomic::AtomicUsize);

impl Drop for SpoolSlot<'_> {
    fn drop(&mut self) {
        self.0.fetch_sub(1, std::sync::atomic::Ordering::SeqCst);
    }
}

/// An open data folder. One write connection and 2 read connections
/// (specification 1, section 5.2). All writes go through the write connection.
pub struct Store {
    dir: PathBuf,
    key: [u8; 32],
    writer: Mutex<Connection>,
    readers: [Mutex<Connection>; 2],
    /// The streamed answers that hold a spool file now.
    spools: std::sync::atomic::AtomicUsize,
}

impl Store {
    /// Open the data folder `dir`. Make the folder (mode 0700), the key, the
    /// content folder and the database if they do not exist, and apply the
    /// migrations that the database does not have.
    pub fn open(dir: &Path) -> Result<Store> {
        make_private_dir(dir)?;
        make_private_dir(&dir.join("content"))?;
        let db = dir.join("iris.db");
        // The key comes before the database at the first start. So a
        // database with no key lost its key: a new key cannot decrypt the
        // secrets that the database has.
        let key_path = dir.join("key");
        if db.exists() && !key_path.exists() {
            return Err(Error::Key(
                "the key file is missing, and iris.db exists. Restore the key from a backup",
            ));
        }
        let key = secret::load_or_make_key(&key_path)?;

        let mut writer = Connection::open(&db)?;
        settings(&writer)?;
        migrate(&mut writer, &db)?;

        let reader = || -> Result<Mutex<Connection>> {
            let c = Connection::open_with_flags(
                &db,
                OpenFlags::SQLITE_OPEN_READ_ONLY | OpenFlags::SQLITE_OPEN_NO_MUTEX,
            )?;
            settings(&c)?;
            Ok(Mutex::new(c))
        };
        Ok(Store {
            dir: dir.to_path_buf(),
            key,
            readers: [reader()?, reader()?],
            writer: Mutex::new(writer),
            spools: std::sync::atomic::AtomicUsize::new(0),
        })
    }

    pub fn dir(&self) -> &Path {
        &self.dir
    }

    /// Run `f` in one write transaction. If `f` returns an error, nothing of
    /// `f` is written. Put a batch of writes in one call: each commit waits
    /// for the disk (`synchronous = FULL`).
    pub fn write<T>(&self, f: impl FnOnce(&Transaction) -> Result<T>) -> Result<T> {
        let mut conn = lock(&self.writer);
        let tx = conn.transaction()?;
        let out = f(&tx)?;
        tx.commit()?;
        Ok(out)
    }

    /// Run `f` in a write transaction, then roll it back: nothing of `f` is
    /// written. For a read that needs the result of writes, for example an
    /// estimate.
    pub fn dry_write<T>(&self, f: impl FnOnce(&Transaction) -> Result<T>) -> Result<T> {
        let mut conn = lock(&self.writer);
        let tx = conn.transaction()?;
        f(&tx)
        // The drop of `tx` rolls it back.
    }

    /// Run `f` on a read connection. The connection sees one snapshot for
    /// the duration of each statement.
    pub fn read<T>(&self, f: impl FnOnce(&Connection) -> Result<T>) -> Result<T> {
        // The first free reader; wait only when both are busy.
        let conn = self.readers[1]
            .try_lock()
            .or_else(|_| self.readers[0].try_lock())
            .unwrap_or_else(|_| lock(&self.readers[0]));
        f(&conn)
    }

    /// Run `f` in one read transaction with a spool file as its output, then
    /// copy the spool to `out` after the read connection is free. Use it
    /// for each answer that goes to a socket or a channel: a slow or stopped
    /// reader of `out` then never holds a store connection, so the other
    /// threads of the engine never wait for a peer. The spool is a file in
    /// the data folder with no name, so memory stays flat. If `f` gives an
    /// error, `out` gets no byte. With [`MAX_SPOOLS`] answers open, it
    /// gives an error at once, with no read and no byte to `out`.
    pub fn read_to<T, E: From<Error>>(
        &self,
        out: &mut impl io::Write,
        f: impl FnOnce(&Connection, &mut Spool) -> std::result::Result<T, E>,
    ) -> std::result::Result<T, E> {
        let _slot = self.spool_slot()?;
        let mut spool = io::BufWriter::new(self.spool_file()?);
        let mut res = None;
        self.read(|c| {
            let tx = c.unchecked_transaction()?;
            res = Some(f(&tx, &mut spool));
            Ok(())
            // The drop of `tx` ends the read.
        })?;
        let value = res.expect("the read ran f")?;
        let mut file = spool.into_inner().map_err(|e| Error::Io(e.into_error()))?;
        io::Seek::rewind(&mut file).map_err(Error::Io)?;
        io::copy(&mut file, out).map_err(Error::Io)?;
        Ok(value)
    }

    /// Take one place of [`MAX_SPOOLS`], or give an error if none is free.
    fn spool_slot(&self) -> Result<SpoolSlot<'_>> {
        use std::sync::atomic::Ordering::SeqCst;
        // A refused request leaves the count as it is, so the count is
        // always the number of slots held (#823).
        self.spools
            .try_update(SeqCst, SeqCst, |n| (n < MAX_SPOOLS).then_some(n + 1))
            .map(|_| SpoolSlot(&self.spools))
            .map_err(|_| {
                Error::Io(io::Error::other(
                    "too many answers are open at the same time; try again",
                ))
            })
    }

    /// A new file in the data folder (mode 0700) with no name: the open file
    /// is the only reference, so the OS deletes it at the close, also after
    /// a crash.
    fn spool_file(&self) -> Result<fs::File> {
        use std::sync::atomic::{AtomicU64, Ordering};
        static NEXT: AtomicU64 = AtomicU64::new(0);
        let path = self.dir.join(format!(
            ".spool-{}-{}",
            std::process::id(),
            NEXT.fetch_add(1, Ordering::Relaxed)
        ));
        let file = fs::OpenOptions::new()
            .read(true)
            .write(true)
            .create_new(true)
            .open(&path)?;
        fs::remove_file(&path)?;
        Ok(file)
    }

    /// Set the time zone of the owner to the IANA name `name`, or with
    /// `None` to the time zone of the computer (decision D1).
    pub fn set_time_zone(&self, name: Option<&str>) -> Result<()> {
        if let Some(name) = name {
            jiff::tz::TimeZone::get(name).map_err(|_| Error::TimeZone(name.to_string()))?;
        }
        self.write(|tx| {
            tx.execute("DELETE FROM owner_setting", [])?;
            if let Some(name) = name {
                tx.execute(
                    "INSERT INTO owner_setting(id, time_zone) VALUES (1, ?1)",
                    [name],
                )?;
            }
            Ok(())
        })
    }

    /// Open each deferred commitment again on its `deferred_until` date, in
    /// the time zone of the owner ([`today`], specification 1, section 6.8). The document gets the new status
    /// and `updated` (RFC 3339 with milliseconds, as `iris_talk::rfc3339`).
    /// A gone item keeps its status. Gives the number of commitments that
    /// opened.
    pub fn reopen_deferred(&self, now: i64) -> Result<usize> {
        self.write(|tx| {
            Ok(tx.execute(
                "UPDATE item SET status = 'open', updated_at = ?1,
                   doc = json_remove(json_set(doc, '$.status', 'open', '$.updated',
                     strftime('%Y-%m-%dT%H:%M:%S', ?1 / 1000, 'unixepoch')
                       || printf('.%03dZ', ?1 % 1000)), '$.deferred_until')
                 WHERE form = 'commitment' AND status = 'deferred'
                   AND json_extract(doc, '$.deferred_until') <= ?2",
                rusqlite::params![now, today(tx, now)?],
            )?)
        })
    }

    /// Mark each active signal whose `expires` time is at or before `now`
    /// as `expired` ([`expire_signals`]).
    pub fn expire_signals(&self, now: i64) -> Result<usize> {
        self.write(|tx| expire_signals(tx, now))
    }

    /// Take the run lock of the `sync_log` row `row`, and create its file.
    /// Only the sync of the row calls it. The sync holds it from before its
    /// row is visible until after its result is written. `None`: the sync runs. A free
    /// lock on a row with no end: the process of the sync stopped
    /// (issue #710).
    pub fn sync_run_lock(&self, row: i64) -> Result<Option<FileLock>> {
        FileLock::try_take(&self.sync_run_path(row)).map_err(Error::Io)
    }

    /// Probe the run lock of the `sync_log` row `row` with no new file. A
    /// reader uses it (issue #710).
    pub fn sync_run_probe(&self, row: i64) -> Result<Probe> {
        FileLock::probe(&self.sync_run_path(row)).map_err(Error::Io)
    }

    /// The file of the run lock of the `sync_log` row `row`.
    pub fn sync_run_path(&self, row: i64) -> PathBuf {
        self.dir().join(format!("sync-run-{row}.lock"))
    }

    /// Add a mailbox, or give the ID of the mailbox with this address.
    pub fn add_account(&self, provider: &str, address: &str) -> Result<i64> {
        self.write(|tx| {
            Ok(tx.query_row(
                "INSERT INTO account(provider, address) VALUES (?1, ?2)
                 ON CONFLICT(address) DO UPDATE SET provider = provider
                 RETURNING id",
                params![provider, address],
                |r| r.get(0),
            )?)
        })
    }

    /// Save the sealed secret (`sealed`) of the account with `address`, and
    /// set its state to `state`, in one transaction. A new address gets a new
    /// account of `provider`. `imap` is the `(host, port, user)` of an
    /// `imap` account, and `None` for another provider. An address with an
    /// account of another provider, or of another IMAP mailbox, is refused
    /// before any change: an address never gets the mirror and the apply
    /// state of another mailbox (IMAP spec section 8). Gives the account ID.
    pub fn save_account_secret(
        &self,
        provider: &str,
        address: &str,
        imap: Option<(&str, u16, &str)>,
        sealed: &[u8],
        state: &str,
    ) -> Result<i64> {
        let (host, port, user) = match imap {
            Some((h, p, u)) => (Some(h), Some(p), Some(u)),
            None => (None, None, None),
        };
        self.write(|tx| {
            type Row = (i64, String, Option<String>, Option<u16>, Option<String>);
            let old: Option<(Row, Option<String>)> = tx
                .query_row(
                    "SELECT id, provider, imap_host, imap_port, imap_user, state FROM account
                     WHERE address = ?1",
                    [address],
                    |r| Ok(((r.get(0)?, r.get(1)?, r.get(2)?, r.get(3)?, r.get(4)?), r.get(5)?)),
                )
                .optional()?;
            // A removed account (a tombstone) keeps its ID and mailbox for
            // the record of actions, and never signs in again. The address
            // gets a new row (docs/specs/2026-11-24-remove-account.md).
            let old = match old {
                Some((_, Some(s))) if s == STATE_REMOVING => {
                    return Err(Error::Account(
                        "Iris still deletes the mail of this account; try again in a minute",
                    ))
                }
                Some(((id, ..), Some(s))) if s == STATE_REMOVED => {
                    tx.execute(
                        "UPDATE account SET address = 'removed:' || id || ':' || address WHERE id = ?1",
                        [id],
                    )?;
                    None
                }
                o => o.map(|(row, _)| row),
            };
            let id = match old {
                None => tx.query_row(
                    "INSERT INTO account(provider, address, imap_host, imap_port, imap_user)
                     VALUES (?1, ?2, ?3, ?4, ?5) RETURNING id",
                    params![provider, address, host, port, user],
                    |r| r.get(0),
                )?,
                Some((_, p, ..)) if p != provider => {
                    return Err(Error::Account(
                        "the address has an account of another provider; remove that account first",
                    ))
                }
                Some((_, _, h, p, u))
                    if h.as_deref() != host || p != port || u.as_deref() != user =>
                {
                    return Err(Error::Account(
                        "the address has an account of another IMAP server or user; remove that account first",
                    ))
                }
                Some((id, ..)) => id,
            };
            tx.execute(
                "UPDATE account SET token = ?2, state = ?3 WHERE id = ?1",
                params![id, sealed, state],
            )?;
            Ok(id)
        })
    }

    /// Add or rename labels of one account: `(provider_id, name, kind)`,
    /// with `kind` `system` or `user`.
    pub fn put_labels(&self, account: i64, labels: &[(&str, &str, &str)]) -> Result<()> {
        self.write(|tx| {
            let mut st = tx.prepare_cached(
                "INSERT INTO label(account_id, provider_id, name, kind) VALUES (?1, ?2, ?3, ?4)
                 ON CONFLICT(account_id, provider_id) DO UPDATE SET name = excluded.name, kind = excluded.kind",
            )?;
            for (pid, name, kind) in labels {
                st.execute(params![account, pid, name, kind])?;
            }
            Ok(())
        })
    }

    /// Write a batch of messages in one transaction, with their threads,
    /// labels, headers and clean text. A message that the store has gets the
    /// new values and keeps its ID. A message that was gone comes back, and
    /// its `understood_at` is cleared, so understanding runs on it again
    /// (specification 1, section 4.4). A message with a new `content_sha256`
    /// loses its old clean text and its `understood_at`, and so does a
    /// message that gets or loses the label `SENT` or `DRAFT`. A message with the
    /// same content and an empty header keeps its saved header. Gives the row
    /// ID of each message.
    ///
    /// Write each content file before the message that refers to it. If a
    /// content file is not there, nothing of the batch is written, and the
    /// result is [`Error::MissingContent`].
    pub fn put_messages(&self, account: i64, batch: &[NewMessage]) -> Result<Vec<i64>> {
        self.write(|tx| {
            let mut thread = tx.prepare_cached(
                "INSERT INTO thread(account_id, provider_id) VALUES (?1, ?2)
                 ON CONFLICT(account_id, provider_id) DO UPDATE SET provider_id = provider_id
                 RETURNING id",
            )?;
            // New content makes the old clean text wrong. The delete trigger
            // on body removes it from the full-text index too.
            let mut stale_body = tx.prepare_cached(
                "DELETE FROM body WHERE message_id = (SELECT id FROM message
                   WHERE account_id = ?1 AND provider_id = ?2 AND content_sha256 IS NOT ?3)",
            )?;
            let mut message = tx.prepare_cached(
                "INSERT INTO message(account_id, thread_id, provider_id, content_sha256, size, internal_date, fetched_at)
                 VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7)
                 ON CONFLICT(account_id, provider_id) DO UPDATE SET
                   thread_id = excluded.thread_id, content_sha256 = excluded.content_sha256,
                   size = excluded.size, internal_date = excluded.internal_date,
                   fetched_at = excluded.fetched_at,
                   understood_at = CASE WHEN gone_at IS NULL
                     AND content_sha256 IS excluded.content_sha256 THEN understood_at END,
                   gone_at = NULL
                 RETURNING id",
            )?;
            // Only the rows that differ change, so the triggers on
            // message_label (SENT and DRAFT clear `understood_at`) see only
            // real changes.
            let mut have_labels = tx.prepare_cached(
                "SELECT l.id, l.provider_id FROM message_label ml JOIN label l ON l.id = ml.label_id
                 WHERE ml.message_id = ?1",
            )?;
            let mut stale_label = tx
                .prepare_cached("DELETE FROM message_label WHERE message_id = ?1 AND label_id = ?2")?;
            let mut label = tx.prepare_cached(
                "INSERT OR IGNORE INTO message_label(message_id, label_id)
                 SELECT ?1, id FROM label WHERE account_id = ?2 AND provider_id = ?3",
            )?;
            let mut header = tx.prepare_cached(
                "INSERT INTO header(message_id, date, from_addr, from_name, to_addrs, cc_addrs, subject,
                   message_id_hdr, in_reply_to, list_id, auth)
                 VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11)
                 ON CONFLICT(message_id) DO UPDATE SET
                   date = excluded.date, from_addr = excluded.from_addr, from_name = excluded.from_name,
                   to_addrs = excluded.to_addrs, cc_addrs = excluded.cc_addrs, subject = excluded.subject,
                   message_id_hdr = excluded.message_id_hdr, in_reply_to = excluded.in_reply_to,
                   list_id = excluded.list_id, auth = excluded.auth",
            )?;
            let mut same_content = tx.prepare_cached(
                "SELECT 1 FROM message WHERE account_id = ?1 AND provider_id = ?2
                   AND content_sha256 IS ?3 AND gone_at IS NULL",
            )?;
            let mut ids = Vec::with_capacity(batch.len());
            for m in batch {
                let thread_id: i64 =
                    thread.query_row(params![account, m.thread_provider_id], |r| r.get(0))?;
                // A fetch of the same bytes (a repair) has no parsed header:
                // keep the saved one, as `understood_at` stays too.
                let keep_header = m.header.is_empty()
                    && same_content.exists(params![account, m.provider_id, m.content_sha256])?;
                stale_body.execute(params![account, m.provider_id, m.content_sha256])?;
                let id: i64 = message.query_row(
                    params![
                        account,
                        thread_id,
                        m.provider_id,
                        m.content_sha256,
                        m.size,
                        m.internal_date,
                        m.fetched_at
                    ],
                    |r| r.get(0),
                )?;
                // The check runs after the first write of the transaction,
                // so it holds the write lock. The content cleanup deletes a
                // file only while it holds the same lock, so a file that is
                // here now stays until the commit.
                if let Some(sha) = &m.content_sha256 {
                    if !self.content_path(sha).is_some_and(|p| p.exists()) {
                        return Err(Error::MissingContent(sha.clone()));
                    }
                }
                let have: Vec<(i64, String)> = have_labels
                    .query_map([id], |r| Ok((r.get(0)?, r.get(1)?)))?
                    .collect::<rusqlite::Result<_>>()?;
                for (label, provider_id) in have {
                    if !m.labels.contains(&provider_id) {
                        stale_label.execute(params![id, label])?;
                    }
                }
                // A label that the store does not have adds no row
                // (specification 1, section 4.3).
                for l in &m.labels {
                    label.execute(params![id, account, l])?;
                }
                let h = &m.header;
                if !keep_header { header.execute(params![
                    id,
                    h.date,
                    h.from_addr,
                    h.from_name,
                    h.to_addrs,
                    h.cc_addrs,
                    h.subject,
                    h.message_id_hdr,
                    h.in_reply_to,
                    h.list_id,
                    h.auth
                ])?; }
                if let Some(b) = &m.body {
                    put_body(tx, id, b)?;
                }
                ids.push(id);
            }
            Ok(ids)
        })
    }

    /// Write or replace the clean text of one message. The full-text index
    /// follows through the triggers on `body`. A text over [`TEXT_LIMIT`]
    /// characters is cut and gets [`CUT_MARK`]. A subject or sender over
    /// [`FIELD_LIMIT`] characters is cut.
    pub fn put_body(&self, message: i64, body: &Body) -> Result<()> {
        self.write(|tx| put_body(tx, message, body))
    }

    /// Search the messages that are not gone, newest first. Gives at most
    /// `limit` message row IDs. See [`Query`] for the syntax.
    pub fn search(&self, text: &str, limit: u32) -> Result<Vec<i64>> {
        let q = Query::parse(text)?;
        self.read(|c| q.run(c, limit))
    }

    /// A writer for one raw message in the content folder. See [`ContentWriter`].
    pub fn content_writer(&self) -> Result<ContentWriter> {
        ContentWriter::new(&self.dir.join("content"))
    }

    /// Write one raw message to the content folder and give its SHA-256.
    pub fn put_content(&self, raw: &[u8]) -> Result<String> {
        let mut w = self.content_writer()?;
        io::Write::write_all(&mut w, raw)?;
        w.finish()
    }

    /// The path of the content file with this SHA-256, or `None` if the
    /// value is not a SHA-256 in lowercase hexadecimal.
    pub fn content_path(&self, sha256: &str) -> Option<PathBuf> {
        content::path(&self.dir.join("content"), sha256)
    }

    /// Write one attachment file to the attachment folder and give its
    /// SHA-256 (send content spec `docs/specs/2026-11-24-send-content.md`,
    /// section 2). The content cleanup never reads this folder.
    /// [`Store::clean_attachments`] deletes an old file that nothing names.
    pub fn put_attachment(&self, bytes: &[u8]) -> Result<String> {
        let root = self.dir.join(ATTACHMENTS);
        make_private_dir(&root)?;
        let mut w = ContentWriter::with_ext(&root, "")?;
        io::Write::write_all(&mut w, bytes)?;
        // The rename runs under the write lock of this store and the SQLite
        // write lock (an IMMEDIATE transaction), which each Store of the
        // data folder shares, also in another process. clean_attachments
        // holds both locks in the same order, so the rename does not
        // overlap a cleanup run: the file is new, or made again.
        let mut conn = lock(&self.writer);
        let tx = conn.transaction_with_behavior(rusqlite::TransactionBehavior::Immediate)?;
        let sha = w.finish()?;
        tx.commit()?;
        Ok(sha)
    }

    /// The path of the attachment file with this SHA-256, or `None` if the
    /// value is not a SHA-256 in lowercase hexadecimal.
    pub fn attachment_path(&self, sha256: &str) -> Option<PathBuf> {
        content::file_path(&self.dir.join(ATTACHMENTS), sha256, "")
    }

    /// Encrypt a secret (specification 1, section 5.4) for a BLOB column.
    pub fn seal(&self, plain: &[u8]) -> Result<Vec<u8>> {
        secret::seal(&self.key, plain)
    }

    /// Decrypt a value that [`Store::seal`] made.
    pub fn unseal(&self, sealed: &[u8]) -> Result<Vec<u8>> {
        secret::unseal(&self.key, sealed)
    }
}

/// The attachment folder of the data folder.
pub(crate) const ATTACHMENTS: &str = "attachments";

/// `text` with at most `limit` characters before the cut mark. A text that
/// ends with the mark already and is in the limit stays as it is.
fn bound<'a>(text: &'a str, limit: usize, mark: &str) -> Cow<'a, str> {
    let core = text.strip_suffix(CUT_MARK).unwrap_or(text);
    match core.char_indices().nth(limit) {
        None => Cow::Borrowed(text),
        Some((end, _)) => Cow::Owned(format!("{}{mark}", &core[..end])),
    }
}

/// Save the clean text of a message in an open write transaction, so that
/// the caller can save it with other rows in one checked write.
pub fn put_body(tx: &Transaction, message: i64, b: &Body) -> Result<()> {
    let text = bound(&b.text, TEXT_LIMIT, CUT_MARK);
    let subject = bound(&b.subject, FIELD_LIMIT, "");
    let sender = bound(&b.sender, FIELD_LIMIT, "");
    tx.prepare_cached(
        "INSERT INTO body(message_id, subject, sender, text) VALUES (?1, ?2, ?3, ?4)
         ON CONFLICT(message_id) DO UPDATE SET
           subject = excluded.subject, sender = excluded.sender, text = excluded.text",
    )?
    .execute(params![message, subject, sender, text])?;
    Ok(())
}

/// Apply the settings of specification 1, section 5.2, to one connection.
///
/// The change to WAL needs an exclusive lock. When two connections open a
/// new database at the same time, SQLite gives `SQLITE_BUSY` at once and
/// does not call the busy handler, because a wait could deadlock. So this
/// tries again for up to 5 seconds, the same as `busy_timeout`.
pub(crate) fn settings(conn: &Connection) -> Result<()> {
    let end = std::time::Instant::now() + std::time::Duration::from_secs(5);
    loop {
        match conn.query_row("PRAGMA journal_mode = WAL", [], |_| Ok(())) {
            Err(rusqlite::Error::SqliteFailure(e, _))
                if e.code == rusqlite::ErrorCode::DatabaseBusy
                    && std::time::Instant::now() < end =>
            {
                std::thread::sleep(std::time::Duration::from_millis(10));
            }
            r => break r?,
        }
    }
    conn.execute_batch(PRAGMAS)?;
    // `iris_same_name(a, b)`: the two names are equal with Unicode case
    // folding, as the label rules of submit and apply (`unicase`). SQLite
    // `COLLATE NOCASE` folds only ASCII (issue #965).
    conn.create_scalar_function(
        "iris_same_name",
        2,
        rusqlite::functions::FunctionFlags::SQLITE_UTF8
            | rusqlite::functions::FunctionFlags::SQLITE_DETERMINISTIC,
        |ctx| {
            let (a, b) = (ctx.get::<Option<String>>(0)?, ctx.get::<Option<String>>(1)?);
            Ok(match (a, b) {
                (Some(a), Some(b)) => unicase::UniCase::new(a) == unicase::UniCase::new(b),
                _ => false,
            })
        },
    )?;
    // `iris_sha256(text)`: the SHA-256 of the UTF-8 text, as lower-case hex,
    // or NULL for NULL. The basis of a thread summary reads it (issue 478).
    conn.create_scalar_function(
        "iris_sha256",
        1,
        rusqlite::functions::FunctionFlags::SQLITE_UTF8
            | rusqlite::functions::FunctionFlags::SQLITE_DETERMINISTIC,
        |ctx| {
            use sha2::Digest;
            Ok(ctx.get::<Option<String>>(0)?.map(|t| {
                sha2::Sha256::digest(t.as_bytes())
                    .iter()
                    .map(|b| format!("{b:02x}"))
                    .collect::<String>()
            }))
        },
    )?;
    Ok(())
}

fn lock(m: &Mutex<Connection>) -> MutexGuard<'_, Connection> {
    // A panic in another thread does not make the connection unsafe: SQLite
    // rolls back an open transaction when the guard of rusqlite drops.
    m.lock().unwrap_or_else(|e| e.into_inner())
}

/// Make the folder `dest` of a copy of mail (a backup or an export) with
/// mode 0700. `dest` must not exist, or must be an empty folder, so that a
/// copy never mixes with other files.
pub fn new_private_dir(dest: &Path) -> Result<()> {
    if dest.exists() && fs::read_dir(dest)?.next().is_some() {
        return Err(Error::Io(io::Error::new(
            io::ErrorKind::AlreadyExists,
            "the folder is not empty",
        )));
    }
    Ok(make_private_dir(dest)?)
}

/// Make a folder with mode 0700, or set mode 0700 on a folder that exists.
/// An existing folder with a wider mode can let other users read the mail.
fn make_private_dir(dir: &Path) -> io::Result<()> {
    let mut b = fs::DirBuilder::new();
    b.recursive(true);
    #[cfg(unix)]
    std::os::unix::fs::DirBuilderExt::mode(&mut b, 0o700);
    b.create(dir)?;
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        fs::set_permissions(dir, fs::Permissions::from_mode(0o700))?;
    }
    Ok(())
}

/// Write the entries of a folder to disk, so that a new or renamed file in
/// the folder is still there after a power loss. Windows has no such step.
fn sync_dir(dir: &Path) -> io::Result<()> {
    #[cfg(unix)]
    fs::File::open(dir)?.sync_all()?;
    let _ = dir;
    Ok(())
}

fn migrate(conn: &mut Connection, db: &Path) -> Result<()> {
    let have = schema_version(conn)?;
    let know = MIGRATIONS.len() as i64;
    // SQLite accepts a negative user_version. A negative value is not a
    // version of this program, and as usize it skips every migration.
    if !(0..=know).contains(&have) {
        return Err(Error::UnknownSchema(have));
    }
    if have == know {
        return Ok(());
    }
    // A version 0 database with tables is not new: it gets a backup too.
    let has_data = have > 0
        || conn.query_row("SELECT count(*) FROM sqlite_master", [], |r| {
            r.get::<_, i64>(0)
        })? > 0;
    if has_data {
        // Specification 1, section 5.5: a backup copy before a migration.
        // The name is new for each start, so two starts do not write one file.
        let mut rand = [0u8; 4];
        getrandom::fill(&mut rand).map_err(|e| io::Error::other(e.to_string()))?;
        let ms = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map_or(0, |d| d.as_millis());
        let copy = db.with_file_name(format!("iris.db.v{have}.{ms}-{}.bak", content::hex(&rand)));
        let copy = copy
            .to_str()
            .ok_or_else(|| io::Error::other("the data folder path is not UTF-8"))?;
        conn.execute("VACUUM INTO ?1", [copy])?;
    }
    for (i, sql) in MIGRATIONS.iter().enumerate().skip(have as usize) {
        // IMMEDIATE takes the write lock first. Then the version check and
        // the migration are one step, also when two starts run at the same time.
        let tx = conn.transaction_with_behavior(rusqlite::TransactionBehavior::Immediate)?;
        if schema_version(&tx)? != i as i64 {
            continue;
        }
        tx.execute_batch(sql)?;
        tx.pragma_update(None, "user_version", i as i64 + 1)?;
        tx.commit()?;
    }
    Ok(())
}

/// The time zone of the owner (decision D1): the zone of `owner_setting`,
/// else the time zone of the computer. A saved name that the computer no
/// longer knows gives the time zone of the computer too.
pub fn time_zone(c: &Connection) -> rusqlite::Result<jiff::tz::TimeZone> {
    Ok(time_zone_and_set(c)?.0)
}

/// [`time_zone`], and true when it is the zone that the owner set, from one
/// read of `owner_setting`, so the two values always agree (issue 891).
pub fn time_zone_and_set(c: &Connection) -> rusqlite::Result<(jiff::tz::TimeZone, bool)> {
    Ok(match owner_time_zone(c)? {
        Some(zone) => (zone, true),
        None => (jiff::tz::TimeZone::system(), false),
    })
}

/// The time zone that the owner set (`owner_setting`), or `None` when the
/// owner set none, or set a name that the computer no longer knows. Then
/// [`time_zone`] gives the time zone of the computer.
pub fn owner_time_zone(c: &Connection) -> rusqlite::Result<Option<jiff::tz::TimeZone>> {
    let name: Option<String> = c
        .query_row("SELECT time_zone FROM owner_setting", [], |r| r.get(0))
        .optional()?;
    Ok(name.and_then(|n| jiff::tz::TimeZone::get(&n).ok()))
}

/// The day of the instant `now_ms` (milliseconds since the epoch) in the
/// time zone of the owner, as `YYYY-MM-DD`.
pub fn today(c: &Connection, now_ms: i64) -> rusqlite::Result<String> {
    Ok(day_in(&time_zone(c)?, now_ms))
}

/// The day of the instant `now_ms` in the time zone `zone`, as `YYYY-MM-DD`.
/// An instant out of range gives the day of the epoch.
pub fn day_in(zone: &jiff::tz::TimeZone, now_ms: i64) -> String {
    let at = jiff::Timestamp::from_millisecond(now_ms).unwrap_or(jiff::Timestamp::UNIX_EPOCH);
    zone.to_datetime(at).date().to_string()
}

/// Mark each active signal whose `expires` time is at or before `now_ms` as
/// `expired`, in the status column and the document (specification 1,
/// section 6.8). The status rule of `iris_understand` sets it only when it
/// writes the item, so the store would keep `active` after the time (issue
/// #927). A signal with no `expires` stays active. Gives the number of
/// signals that expired.
pub fn expire_signals(c: &Connection, now_ms: i64) -> Result<usize> {
    Ok(c.execute(
        "UPDATE item SET status = 'expired', updated_at = ?1,
           doc = json_set(doc, '$.status', 'expired', '$.updated',
             strftime('%Y-%m-%dT%H:%M:%S', ?1 / 1000, 'unixepoch')
               || printf('.%03dZ', ?1 % 1000))
         WHERE form = 'signal' AND status = 'active'
           AND unixepoch(json_extract(doc, '$.expires'), 'subsec') * 1000 <= ?1",
        [now_ms],
    )?)
}

/// The schema version of an open database.
pub fn schema_version(conn: &Connection) -> Result<i64> {
    Ok(conn.query_row("PRAGMA user_version", [], |r| r.get(0))?)
}

/// The time of the stop (docs/specs/2026-11-17-stop.md), in milliseconds
/// since 1970, or `None` while Iris runs. Read it in the transaction that
/// the stop guards.
pub fn stopped(conn: &Connection) -> Result<Option<i64>> {
    Ok(
        conn.query_row("SELECT stopped_at FROM stop_switch WHERE id = 1", [], |r| {
            r.get(0)
        })?,
    )
}

/// Give the message row `message` the provider ID `new`, after a move
/// that gave the message a new location ID (IMAP moves spec, section 3).
/// The row, its raw file, its forms and its ledger rows stay. A thread
/// whose provider ID is the old message ID (the fallback thread ID of a
/// message with no thread headers) gets `new` too. Refuses with
/// [`Error::Taken`] if another row of the account has `new`. Run it in the
/// transaction that saves the move.
pub fn rebind(tx: &Connection, message: i64, new: &str) -> Result<()> {
    let (account, old, thread): (i64, String, i64) = tx.query_row(
        "SELECT account_id, provider_id, thread_id FROM message WHERE id = ?1",
        [message],
        |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?)),
    )?;
    let taken: bool = tx.query_row(
        "SELECT EXISTS (SELECT 1 FROM message
                        WHERE account_id = ?1 AND provider_id = ?2 AND id != ?3)",
        params![account, new, message],
        |r| r.get(0),
    )?;
    if taken {
        return Err(Error::Taken(new.to_string()));
    }
    tx.execute(
        "UPDATE message SET provider_id = ?2 WHERE id = ?1",
        params![message, new],
    )?;
    tx.execute(
        "UPDATE thread SET provider_id = ?3 WHERE id = ?1 AND provider_id = ?2",
        params![thread, old, new],
    )?;
    Ok(())
}

#[cfg(test)]
mod spool_tests {
    use super::*;
    use std::sync::atomic::{AtomicBool, Ordering::SeqCst};

    /// Invariant (#823): the spool count is never more than the slots
    /// held, also while refused requests run at the same time.
    #[test]
    fn a_refused_request_does_not_change_the_spool_count() {
        let dir = std::env::temp_dir().join(format!("iris-spool-{}", std::process::id()));
        let _ = fs::remove_dir_all(&dir);
        let s = Store::open(&dir).unwrap();
        let held: Vec<_> = (0..MAX_SPOOLS).map(|_| s.spool_slot().unwrap()).collect();
        let stop = AtomicBool::new(false);
        std::thread::scope(|t| {
            for _ in 0..4 {
                t.spawn(|| {
                    let mut accepted = 0;
                    while !stop.load(SeqCst) {
                        accepted += usize::from(s.spool_slot().is_ok());
                    }
                    assert_eq!(accepted, 0);
                });
            }
            let most = (0..200_000).map(|_| s.spools.load(SeqCst)).max();
            stop.store(true, SeqCst);
            assert_eq!(most, Some(MAX_SPOOLS));
        });
        drop(held);
        assert_eq!(s.spools.load(SeqCst), 0);
        assert!(s.spool_slot().is_ok());
        drop(s);
        let _ = fs::remove_dir_all(&dir);
    }
}
