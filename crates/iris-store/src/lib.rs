//! The Iris store (specification 1, section 5).
//!
//! One data folder holds all state: the SQLite database `iris.db`, the
//! encryption key `key`, and the raw messages in `content/`. This crate opens
//! the folder, applies the schema migrations, and gives the read and write
//! paths that the engine uses.

mod content;
mod id;
mod search;
mod secret;

use std::fmt;
use std::fs;
use std::io;
use std::path::{Path, PathBuf};
use std::sync::{Mutex, MutexGuard};

use rusqlite::{params, Connection, OpenFlags, Transaction};

pub use content::ContentWriter;
pub use id::{format_id, parse_id, Kind};
pub use search::Query;

/// The schema migrations, in order. Item `n` makes `PRAGMA user_version` `n + 1`.
/// Migrations go forward only. Never change a migration after it merged: add one.
const MIGRATIONS: &[&str] = &[include_str!("schema_1.sql")];

/// The SQLite settings of specification 1, section 5.2, for each connection.
const PRAGMAS: &str = "
PRAGMA journal_mode = WAL;
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
    /// The database has a schema version that this program does not know.
    NewerSchema(i64),
    /// The `key` file is not 32 bytes, or a secret does not decrypt.
    Key(&'static str),
    /// The search text is not valid.
    Query(String),
    /// A message names a label that the store does not have.
    UnknownLabel(String),
}

impl fmt::Display for Error {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Error::Db(e) => write!(f, "database: {e}"),
            Error::Io(e) => write!(f, "file: {e}"),
            Error::NewerSchema(v) => write!(
                f,
                "the database has schema version {v}, and this program knows only {}",
                MIGRATIONS.len()
            ),
            Error::Key(why) => write!(f, "secret: {why}"),
            Error::Query(why) => write!(f, "search: {why}"),
            Error::UnknownLabel(id) => write!(f, "unknown label {id}"),
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
    /// Provider IDs of the labels. Each label must be in the store.
    pub labels: Vec<String>,
    pub header: Header,
    /// The clean text (specification 1, section 7.2), if it is known.
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

#[derive(Clone, Debug, Default)]
pub struct Body {
    pub subject: String,
    pub sender: String,
    pub text: String,
}

/// An open data folder. One write connection and 2 read connections
/// (specification 1, section 5.2). All writes go through the write connection.
pub struct Store {
    dir: PathBuf,
    key: [u8; 32],
    writer: Mutex<Connection>,
    readers: [Mutex<Connection>; 2],
}

impl Store {
    /// Open the data folder `dir`. Make the folder (mode 0700), the key, the
    /// content folder and the database if they do not exist, and apply the
    /// migrations that the database does not have.
    pub fn open(dir: &Path) -> Result<Store> {
        make_private_dir(dir)?;
        make_private_dir(&dir.join("content"))?;
        let key = secret::load_or_make_key(&dir.join("key"))?;

        let db = dir.join("iris.db");
        let mut writer = Connection::open(&db)?;
        writer.execute_batch(PRAGMAS)?;
        migrate(&mut writer, &db)?;

        let reader = || -> Result<Mutex<Connection>> {
            let c = Connection::open_with_flags(
                &db,
                OpenFlags::SQLITE_OPEN_READ_ONLY | OpenFlags::SQLITE_OPEN_NO_MUTEX,
            )?;
            c.execute_batch(PRAGMAS)?;
            Ok(Mutex::new(c))
        };
        Ok(Store {
            dir: dir.to_path_buf(),
            key,
            readers: [reader()?, reader()?],
            writer: Mutex::new(writer),
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

    /// Run `f` on a read connection. The connection sees one snapshot for
    /// the duration of each statement.
    pub fn read<T>(&self, f: impl FnOnce(&Connection) -> Result<T>) -> Result<T> {
        let conn = self.readers[1]
            .try_lock()
            .ok()
            .unwrap_or_else(|| lock(&self.readers[0]));
        f(&conn)
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
    /// new values and keeps its ID. Gives the row ID of each message.
    pub fn put_messages(&self, account: i64, batch: &[NewMessage]) -> Result<Vec<i64>> {
        self.write(|tx| {
            let mut thread = tx.prepare_cached(
                "INSERT INTO thread(account_id, provider_id) VALUES (?1, ?2)
                 ON CONFLICT(account_id, provider_id) DO UPDATE SET provider_id = provider_id
                 RETURNING id",
            )?;
            let mut message = tx.prepare_cached(
                "INSERT INTO message(account_id, thread_id, provider_id, content_sha256, size, internal_date, fetched_at)
                 VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7)
                 ON CONFLICT(account_id, provider_id) DO UPDATE SET
                   thread_id = excluded.thread_id, content_sha256 = excluded.content_sha256,
                   size = excluded.size, internal_date = excluded.internal_date,
                   fetched_at = excluded.fetched_at
                 RETURNING id",
            )?;
            let mut clear_labels = tx.prepare_cached("DELETE FROM message_label WHERE message_id = ?1")?;
            let mut label = tx.prepare_cached(
                "INSERT OR IGNORE INTO message_label(message_id, label_id)
                 SELECT ?1, id FROM label WHERE account_id = ?2 AND provider_id = ?3",
            )?;
            let mut label_known =
                tx.prepare_cached("SELECT 1 FROM label WHERE account_id = ?1 AND provider_id = ?2")?;
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
            let mut ids = Vec::with_capacity(batch.len());
            for m in batch {
                let thread_id: i64 =
                    thread.query_row(params![account, m.thread_provider_id], |r| r.get(0))?;
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
                clear_labels.execute([id])?;
                for l in &m.labels {
                    if label.execute(params![id, account, l])? == 0
                        && !label_known.exists(params![account, l])?
                    {
                        return Err(Error::UnknownLabel(l.clone()));
                    }
                }
                let h = &m.header;
                header.execute(params![
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
                ])?;
                if let Some(b) = &m.body {
                    put_body(tx, id, b)?;
                }
                ids.push(id);
            }
            Ok(ids)
        })
    }

    /// Write or replace the clean text of one message. The full-text index
    /// follows through the triggers on `body`.
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

    /// Encrypt a secret (specification 1, section 5.4) for a BLOB column.
    pub fn seal(&self, plain: &[u8]) -> Result<Vec<u8>> {
        secret::seal(&self.key, plain)
    }

    /// Decrypt a value that [`Store::seal`] made.
    pub fn unseal(&self, sealed: &[u8]) -> Result<Vec<u8>> {
        secret::unseal(&self.key, sealed)
    }
}

fn put_body(tx: &Transaction, message: i64, b: &Body) -> Result<()> {
    tx.prepare_cached(
        "INSERT INTO body(message_id, subject, sender, text) VALUES (?1, ?2, ?3, ?4)
         ON CONFLICT(message_id) DO UPDATE SET
           subject = excluded.subject, sender = excluded.sender, text = excluded.text",
    )?
    .execute(params![message, b.subject, b.sender, b.text])?;
    Ok(())
}

fn lock(m: &Mutex<Connection>) -> MutexGuard<'_, Connection> {
    // A panic in another thread does not make the connection unsafe: SQLite
    // rolls back an open transaction when the guard of rusqlite drops.
    m.lock().unwrap_or_else(|e| e.into_inner())
}

/// Make a folder with mode 0700. A folder that exists keeps its mode.
fn make_private_dir(dir: &Path) -> io::Result<()> {
    let mut b = fs::DirBuilder::new();
    b.recursive(true);
    #[cfg(unix)]
    std::os::unix::fs::DirBuilderExt::mode(&mut b, 0o700);
    b.create(dir)
}

fn migrate(conn: &mut Connection, db: &Path) -> Result<()> {
    let have = schema_version(conn)?;
    let know = MIGRATIONS.len() as i64;
    if have > know {
        return Err(Error::NewerSchema(have));
    }
    if have == know {
        return Ok(());
    }
    if have > 0 {
        // Specification 1, section 5.5: a backup copy before a migration.
        let copy = db.with_file_name(format!("iris.db.v{have}.bak"));
        if copy.exists() {
            fs::remove_file(&copy)?;
        }
        let copy = copy
            .to_str()
            .ok_or_else(|| io::Error::other("the data folder path is not UTF-8"))?;
        conn.execute("VACUUM INTO ?1", [copy])?;
    }
    for (i, sql) in MIGRATIONS.iter().enumerate().skip(have as usize) {
        let tx = conn.transaction()?;
        tx.execute_batch(sql)?;
        tx.pragma_update(None, "user_version", i as i64 + 1)?;
        tx.commit()?;
    }
    Ok(())
}

/// The schema version of an open database.
pub fn schema_version(conn: &Connection) -> Result<i64> {
    Ok(conn.query_row("PRAGMA user_version", [], |r| r.get(0))?)
}
