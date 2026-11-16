//! The first sync of the Iris mirror (specification 1, section 4.2): a full
//! copy of one Gmail mailbox into the store.
//!
//! [`first_sync`] saves its state in the `account` row after each step. If
//! it stops (crash, network loss, restart), call it again: it continues from
//! the saved step and fetches only the messages that are not fetched.
//!
//! Step 5 of section 4.2 (one incremental sync from `H0`) is tracker item
//! R1-06. This crate ends the first sync at [`STEP_INCREMENTAL`], with `H0`
//! in `account.sync_start`. The incremental sync writes `account.history_id`
//! and `mirror_ready_at` in the transaction of its last step.

use std::fmt;
use std::sync::atomic::{AtomicBool, AtomicUsize, Ordering};
use std::sync::Mutex;
use std::time::{SystemTime, UNIX_EPOCH};

use iris_gmail::{Gmail, MAX_GETS};
use iris_store::{NewMessage, Store};
use rusqlite::{params, OptionalExtension};

/// `account.sync_kind` of a first sync.
pub const KIND_FIRST: &str = "first";
/// `account.sync_step`: list the labels and the messages (steps 1 to 3).
pub const STEP_LIST: i64 = 1;
/// `account.sync_step`: fetch each message that is not fetched (step 4).
pub const STEP_FETCH: i64 = 2;
/// `account.sync_step`: each message is fetched or gone. The incremental
/// sync from `account.sync_start` (step 5, tracker item R1-06) is next.
pub const STEP_INCREMENTAL: i64 = 3;

/// The messages that the fetch step reads from the store at one time. The
/// fetches of one chunk are written in one transaction.
const CHUNK: usize = 64;

#[derive(Debug)]
pub enum Error {
    Gmail(iris_gmail::Error),
    Store(iris_store::Error),
    /// The account row does not exist.
    NoAccount,
    /// The token gives a mailbox with another address than the account.
    WrongMailbox,
    /// Another sync of the account runs, or a full resync is not complete.
    /// Only one sync of an account runs at one time (specification 1,
    /// section 4.2).
    OtherSync,
    /// The saved sync state is not valid.
    BadState(&'static str),
}

impl fmt::Display for Error {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Error::Gmail(e) => write!(f, "Gmail: {e}"),
            Error::Store(e) => write!(f, "store: {e}"),
            Error::NoAccount => write!(f, "the account does not exist"),
            Error::WrongMailbox => write!(f, "the access token is for another mailbox"),
            Error::OtherSync => write!(f, "another sync of the account runs"),
            Error::BadState(why) => write!(f, "sync state: {why}"),
        }
    }
}

impl std::error::Error for Error {}

impl From<iris_gmail::Error> for Error {
    fn from(e: iris_gmail::Error) -> Self {
        Error::Gmail(e)
    }
}

impl From<iris_store::Error> for Error {
    fn from(e: iris_store::Error) -> Self {
        Error::Store(e)
    }
}

impl From<rusqlite::Error> for Error {
    fn from(e: rusqlite::Error) -> Self {
        Error::Store(e.into())
    }
}

pub type Result<T> = std::result::Result<T, Error>;

/// The result of a first sync.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Done {
    /// `H0`: the history ID before the list. The incremental sync starts here.
    pub history_start: u64,
    /// The messages that this call fetched.
    pub fetched: u64,
    /// The messages that Gmail deleted after the list (HTTP 404), in this call.
    pub gone: u64,
}

struct State {
    address: String,
    history_id: Option<String>,
    kind: Option<String>,
    start: Option<String>,
    step: Option<i64>,
}

/// The sync lock of one account: while a value lives, no other sync of the
/// account runs, in this process or in another (specification 1, section
/// 4.2). Each sync of the account takes it first, the incremental sync too.
/// The operating system frees the lock when the process ends.
pub struct SyncLock(#[allow(dead_code)] std::fs::File);

/// Take the sync lock of `account`, or give [`Error::OtherSync`] if another
/// sync holds it.
pub fn lock(store: &Store, account: i64) -> Result<SyncLock> {
    let path = store.dir().join(format!("sync-{account}.lock"));
    let file = std::fs::OpenOptions::new()
        .create(true)
        .truncate(false)
        .write(true)
        .open(path)
        .map_err(iris_store::Error::Io)?;
    // flock on Linux and macOS: two opens of the file conflict, also in one process.
    match file.try_lock() {
        Ok(()) => Ok(SyncLock(file)),
        Err(std::fs::TryLockError::WouldBlock) => Err(Error::OtherSync),
        Err(std::fs::TryLockError::Error(e)) => Err(iris_store::Error::Io(e).into()),
    }
}

/// Run or continue the first sync of `account`. `token` is the access
/// token. On [`iris_gmail::Error::Unauthorized`], get a new token and call
/// again: the sync continues where it stopped.
///
/// An account with a complete first sync gives `Done` with zero counts and
/// calls Gmail for nothing.
pub fn first_sync(store: &Store, gmail: &Gmail, account: i64, token: &str) -> Result<Done> {
    let _lock = lock(store, account)?;
    let s = state(store, account)?;
    match (s.kind.as_deref(), s.start.as_deref(), s.step) {
        (Some(KIND_FIRST), Some(_), Some(_)) => {}
        (Some(_), _, _) => return Err(Error::OtherSync),
        (None, _, _) if s.history_id.is_some() => {
            return Ok(Done {
                history_start: parse_history(s.history_id.as_deref().unwrap_or_default())?,
                fetched: 0,
                gone: 0,
            })
        }
        (None, _, _) => {}
    }
    // On each call, also a resume: a token of another mailbox gives 404 for
    // each fetch, and the sync would mark the messages of the account gone.
    let profile = gmail.profile(token)?;
    if !profile.email_address.eq_ignore_ascii_case(&s.address) {
        return Err(Error::WrongMailbox);
    }
    let start = match s.start.as_deref() {
        Some(h0) => parse_history(h0)?,
        None => {
            // Step 1: H0 before the list, so that no change after it is lost.
            set_step(store, account, Some(profile.history_id), STEP_LIST)?;
            profile.history_id
        }
    };
    let mut step = s.step.unwrap_or(STEP_LIST);

    // Step 2 on each call: a label made after the last call must be known
    // before a message that has it is saved.
    let labels = gmail.labels(token)?;
    let rows: Vec<(&str, &str, &str)> = labels
        .iter()
        .map(|l| {
            let kind = if l.kind == "system" { "system" } else { "user" };
            (l.id.as_str(), l.name.as_str(), kind)
        })
        .collect();
    store.put_labels(account, &rows)?;

    if step == STEP_LIST {
        list(store, gmail, account, token)?;
        set_step(store, account, None, STEP_FETCH)?;
        step = STEP_FETCH;
    }
    let mut done = Done {
        history_start: start,
        fetched: 0,
        gone: 0,
    };
    if step == STEP_FETCH {
        fetch(store, gmail, account, token, &mut done)?;
        set_step(store, account, None, STEP_INCREMENTAL)?;
    }
    Ok(done)
}

fn parse_history(text: &str) -> Result<u64> {
    text.parse()
        .map_err(|_| Error::BadState("the saved history ID is not a number"))
}

fn state(store: &Store, account: i64) -> Result<State> {
    store
        .read(|c| {
            Ok(c.query_row(
                "SELECT address, history_id, sync_kind, sync_start, sync_step
                 FROM account WHERE id = ?1",
                [account],
                |r| {
                    Ok(State {
                        address: r.get(0)?,
                        history_id: r.get(1)?,
                        kind: r.get(2)?,
                        start: r.get(3)?,
                        step: r.get(4)?,
                    })
                },
            )
            .optional()?)
        })?
        .ok_or(Error::NoAccount)
}

/// Save the step. With `start`, also begin the first sync with `H0`.
fn set_step(store: &Store, account: i64, start: Option<u64>, step: i64) -> Result<()> {
    store.write(|tx| {
        match start {
            Some(h0) => tx.execute(
                "UPDATE account SET sync_kind = ?2, sync_start = ?3, sync_step = ?4 WHERE id = ?1",
                params![account, KIND_FIRST, h0.to_string(), step],
            )?,
            None => tx.execute(
                "UPDATE account SET sync_step = ?2 WHERE id = ?1",
                params![account, step],
            )?,
        };
        Ok(())
    })?;
    Ok(())
}

/// Step 3: save each listed message as "known, not fetched", one page in
/// each transaction. A message that the store has stays as it is, so a list
/// again after a stop loses no fetch. The `AUTOINCREMENT` row IDs keep the
/// list order (newest first) for the fetch step.
fn list(store: &Store, gmail: &Gmail, account: i64, token: &str) -> Result<()> {
    let mut page_token: Option<String> = None;
    loop {
        let page = gmail.messages(token, &[], true, page_token.as_deref())?;
        store.write(|tx| {
            let mut thread = tx.prepare_cached(
                "INSERT INTO thread(account_id, provider_id) VALUES (?1, ?2)
                 ON CONFLICT(account_id, provider_id) DO UPDATE SET provider_id = provider_id
                 RETURNING id",
            )?;
            let mut message = tx.prepare_cached(
                "INSERT INTO message(account_id, thread_id, provider_id) VALUES (?1, ?2, ?3)
                 ON CONFLICT(account_id, provider_id) DO NOTHING",
            )?;
            for m in &page.messages {
                let t: i64 = thread.query_row(params![account, m.thread_id], |r| r.get(0))?;
                message.execute(params![account, t, m.id])?;
            }
            Ok(())
        })?;
        match page.next_page_token {
            Some(t) => page_token = Some(t),
            None => return Ok(()),
        }
    }
}

enum Fetched {
    Message(Box<NewMessage>),
    Gone(String),
}

/// Step 4: fetch by thread, newest thread first. The thread row IDs keep
/// the order in which the newest-first list named each thread first.
fn fetch(store: &Store, gmail: &Gmail, account: i64, token: &str, done: &mut Done) -> Result<()> {
    loop {
        let ids: Vec<(String, String)> = store.read(|c| {
            let mut st = c.prepare_cached(
                "SELECT m.provider_id, t.provider_id FROM message m JOIN thread t ON t.id = m.thread_id
                 WHERE m.account_id = ?1 AND m.fetched_at IS NULL AND m.gone_at IS NULL
                 ORDER BY m.thread_id, m.id LIMIT ?2",
            )?;
            let rows = st.query_map(params![account, CHUNK as i64], |r| Ok((r.get(0)?, r.get(1)?)))?;
            Ok(rows.collect::<rusqlite::Result<_>>()?)
        })?;
        if ids.is_empty() {
            return Ok(());
        }
        let (results, error) = fetch_chunk(store, gmail, token, &ids);
        // Save the fetches of the chunk also when one of them failed.
        let mut messages = Vec::new();
        let mut gone = Vec::new();
        for r in results {
            match r {
                Fetched::Message(m) => messages.push(*m),
                Fetched::Gone(id) => gone.push(id),
            }
        }
        store.put_messages(account, &messages)?;
        if !gone.is_empty() {
            let now = now_ms();
            store.write(|tx| {
                let mut st = tx.prepare_cached(
                    "UPDATE message SET gone_at = ?3 WHERE account_id = ?1 AND provider_id = ?2",
                )?;
                for id in &gone {
                    st.execute(params![account, id, now])?;
                }
                Ok(())
            })?;
        }
        done.fetched += messages.len() as u64;
        done.gone += gone.len() as u64;
        if let Some(e) = error {
            return Err(e);
        }
    }
}

/// Fetch the messages of one chunk, with at most [`MAX_GETS`] fetches at
/// the same time. After the first error, no new fetch starts. Gives the
/// fetches that ended, and the first error.
fn fetch_chunk(
    store: &Store,
    gmail: &Gmail,
    token: &str,
    ids: &[(String, String)],
) -> (Vec<Fetched>, Option<Error>) {
    let next = AtomicUsize::new(0);
    let stop = AtomicBool::new(false);
    let results = Mutex::new(Vec::with_capacity(ids.len()));
    let error = Mutex::new(None);
    std::thread::scope(|s| {
        for _ in 0..MAX_GETS.min(ids.len()) {
            s.spawn(|| {
                while !stop.load(Ordering::Relaxed) {
                    let i = next.fetch_add(1, Ordering::Relaxed);
                    let Some((id, thread)) = ids.get(i) else {
                        return;
                    };
                    match fetch_one(store, gmail, token, id, thread) {
                        Ok(f) => results.lock().unwrap_or_else(|e| e.into_inner()).push(f),
                        Err(e) => {
                            stop.store(true, Ordering::Relaxed);
                            error
                                .lock()
                                .unwrap_or_else(|e| e.into_inner())
                                .get_or_insert(e);
                        }
                    }
                }
            });
        }
    });
    (
        results.into_inner().unwrap_or_else(|e| e.into_inner()),
        error.into_inner().unwrap_or_else(|e| e.into_inner()),
    )
}

/// Stream one raw message to the content folder. The sync stores bytes and
/// never parses them (specification 1, section 4.7).
fn fetch_one(store: &Store, gmail: &Gmail, token: &str, id: &str, thread: &str) -> Result<Fetched> {
    // If the fetch fails, the writer drops and deletes its temporary file.
    let mut w = store.content_writer()?;
    let raw = match gmail.raw(token, id, &mut w) {
        Ok(raw) => raw,
        Err(iris_gmail::Error::NotFound) => return Ok(Fetched::Gone(id.to_string())),
        Err(e) => return Err(e.into()),
    };
    let sha = w.finish()?;
    Ok(Fetched::Message(Box::new(NewMessage {
        provider_id: id.to_string(),
        // Gmail does not move a message to another thread. Keep the listed
        // thread if the answer has none.
        thread_provider_id: if raw.thread_id.is_empty() {
            thread.to_string()
        } else {
            raw.thread_id
        },
        content_sha256: Some(sha),
        size: Some(raw.size as i64),
        internal_date: Some(raw.internal_date),
        fetched_at: Some(now_ms()),
        labels: raw.label_ids,
        ..NewMessage::default()
    })))
}

fn now_ms() -> i64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map_or(0, |d| d.as_millis() as i64)
}
