//! The sync of the Iris mirror (specification 1, sections 4.2 to 4.7): the
//! first sync, the incremental sync and the full resync of one Gmail mailbox.
//!
//! [`sync`] is the entry point. It continues an unfinished first sync or
//! full resync from its saved step, then runs one incremental sync. If the
//! saved history ID is too old, it starts a full resync.
//!
//! Each step saves its state in the `account` row. If a sync stops (crash,
//! network loss, restart), call [`sync`] again: it continues from the saved
//! step. The sync writes `account.history_id` and `mirror_ready_at` only
//! after the last change of its incremental sync.
//!
//! A message that gives an error that is not retried (for example a bad
//! answer) does not stop the sync. The sync records the message as failed in
//! `sync_log`, keeps it as "known, not fetched", and continues. The verify
//! step finds such a message and fetches it again.

use std::fmt;
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::Mutex;
use std::time::{SystemTime, UNIX_EPOCH};

use iris_gmail::{Change, Gmail, HistoryType, MAX_GETS};
use iris_store::{NewMessage, Store};
use rusqlite::{params, OptionalExtension, Transaction};

/// `account.sync_kind` of a first sync.
pub const KIND_FIRST: &str = "first";
/// `account.sync_kind` of a full resync (specification 1, section 4.4).
pub const KIND_RESYNC: &str = "resync";
/// `account.sync_step` of a first sync: list the labels and the messages
/// (steps 1 to 3).
pub const STEP_LIST: i64 = 1;
/// `account.sync_step` of a first sync: fetch each message that is not
/// fetched (step 4).
pub const STEP_FETCH: i64 = 2;
/// `account.sync_step` of a first sync: each message is fetched, gone or
/// failed. The incremental sync from `account.sync_start` (step 5) is next.
pub const STEP_INCREMENTAL: i64 = 3;
/// `account.sync_step` of a full resync: list the messages (steps 2 and 3).
pub const RESYNC_LIST: i64 = 1;
/// `account.sync_step` of a full resync: bring back the listed messages
/// that are gone, and fetch the new messages (steps 4 and 6).
pub const RESYNC_FETCH: i64 = 2;
/// `account.sync_step` of a full resync: check each message of the mirror
/// that the list does not have (step 5).
pub const RESYNC_GONE: i64 = 3;
/// `account.sync_step` of a full resync: get and correct the label sets
/// (step 7).
pub const RESYNC_LABELS: i64 = 4;
/// `account.sync_step` of a full resync: the incremental sync from
/// `account.sync_start` (step 8) is next.
pub const RESYNC_INCREMENTAL: i64 = 5;

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
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct Done {
    /// `H0`: the history ID before the list. The incremental sync starts here.
    pub history_start: u64,
    /// The messages that this call fetched.
    pub fetched: u64,
    /// The messages that Gmail deleted after the list (HTTP 404), in this call.
    pub gone: u64,
    /// The messages that gave an error that is not retried, in this call.
    /// Each one is in `sync_log` with the kind `fetch`.
    pub failed: u64,
}

/// The result of [`sync`].
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct Synced {
    /// The history ID of the mirror after the sync.
    pub history_id: u64,
    /// The messages that this call fetched.
    pub fetched: u64,
    /// The messages that this call marked as gone.
    pub gone: u64,
    /// The messages that gave an error that is not retried, in this call.
    pub failed: u64,
    /// True if this call ran a full resync, or a part of one.
    pub resync: bool,
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
/// 4.2). Each sync of the account takes it first. The operating system
/// frees the lock when the process ends.
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

/// Sync `account` with Gmail: continue an unfinished first sync or full
/// resync, then run one incremental sync (specification 1, section 4.3).
/// A new account gets its first sync. `token` is the access token. On
/// [`iris_gmail::Error::Unauthorized`], get a new token and call again: the
/// sync continues where it stopped.
///
/// If Gmail does not have the history of the saved history ID any more,
/// the call starts a full resync (section 4.4) and runs it to the end.
/// Each call adds one row to `sync_log`.
pub fn sync(store: &Store, gmail: &Gmail, account: i64, token: &str) -> Result<Synced> {
    let _lock = lock(store, account)?;
    sync_locked(store, gmail, account, token)
}

/// [`sync`] for a caller that holds the sync lock.
fn sync_locked(store: &Store, gmail: &Gmail, account: i64, token: &str) -> Result<Synced> {
    let started = now_ms();
    let mut kind = "incremental";
    let result = run(store, gmail, account, token, &mut kind);
    let text = match &result {
        Ok(s) => format!(
            "ok: history {}, fetched {}, gone {}, failed {}",
            s.history_id, s.fetched, s.gone, s.failed
        ),
        Err(e) => format!("error: {e}"),
    };
    let logged = log(store, account, kind, started, &text);
    let synced = result?;
    logged?;
    Ok(synced)
}

fn run(
    store: &Store,
    gmail: &Gmail,
    account: i64,
    token: &str,
    kind: &mut &'static str,
) -> Result<Synced> {
    // On each call: history of another mailbox would change the mirror of this one.
    let profile = gmail.profile(token)?;
    let mut done = Done::default();
    let mut resync = false;
    loop {
        let s = state(store, account)?;
        if !profile.email_address.eq_ignore_ascii_case(&s.address) {
            return Err(Error::WrongMailbox);
        }
        let from = match (s.kind.as_deref(), s.history_id.as_deref()) {
            (None, Some(h)) => parse_history(h)?,
            (None, None) | (Some(KIND_FIRST), _) => {
                *kind = KIND_FIRST;
                first_steps(store, gmail, account, token, &s, &mut done)?
            }
            (Some(KIND_RESYNC), _) => {
                *kind = KIND_RESYNC;
                resync = true;
                resync_steps(store, gmail, account, token, &s, &mut done)?
            }
            (Some(_), _) => return Err(Error::BadState("unknown sync kind")),
        };
        match incremental(store, gmail, account, token, from, &mut done) {
            // H1 of a resync of this call is too old too: stop, do not loop.
            Err(Error::Gmail(iris_gmail::Error::HistoryExpired)) if !resync => {
                // Section 4.4: the next turn of the loop starts the full resync.
                store.write(|tx| {
                    tx.execute(
                        "UPDATE account SET sync_kind = ?2, sync_start = NULL, sync_step = NULL
                         WHERE id = ?1",
                        params![account, KIND_RESYNC],
                    )?;
                    Ok(())
                })?;
            }
            r => {
                return Ok(Synced {
                    history_id: r?,
                    fetched: done.fetched,
                    gone: done.gone,
                    failed: done.failed,
                    resync,
                })
            }
        }
    }
}

/// Run or continue only the first sync of `account` (specification 1,
/// section 4.2, steps 1 to 4). It ends at [`STEP_INCREMENTAL`], with `H0` in
/// `account.sync_start`. [`sync`] does the rest. `token` is the access
/// token.
///
/// An account at [`STEP_INCREMENTAL`] or with a complete sync gives `Done`
/// with zero counts and calls Gmail for nothing.
pub fn first_sync(store: &Store, gmail: &Gmail, account: i64, token: &str) -> Result<Done> {
    let _lock = lock(store, account)?;
    let s = state(store, account)?;
    match (s.kind.as_deref(), s.start.as_deref(), s.step) {
        (Some(KIND_FIRST), Some(h0), Some(STEP_INCREMENTAL)) => {
            return Ok(Done {
                history_start: parse_history(h0)?,
                ..Done::default()
            })
        }
        (Some(KIND_FIRST), Some(_), Some(_)) => {}
        (Some(_), _, _) => return Err(Error::OtherSync),
        (None, _, _) if s.history_id.is_some() => {
            return Ok(Done {
                history_start: parse_history(s.history_id.as_deref().unwrap_or_default())?,
                ..Done::default()
            })
        }
        (None, _, _) => {}
    }
    let mut done = Done::default();
    done.history_start = first_steps(store, gmail, account, token, &s, &mut done)?;
    Ok(done)
}

/// Steps 1 to 4 of the first sync, from the saved step. Gives `H0`.
fn first_steps(
    store: &Store,
    gmail: &Gmail,
    account: i64,
    token: &str,
    s: &State,
    done: &mut Done,
) -> Result<u64> {
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
            begin(store, account, KIND_FIRST, profile.history_id, STEP_LIST)?;
            profile.history_id
        }
    };
    let mut step = s.step.unwrap_or(STEP_LIST);

    // Step 2 on each call: a label made after the last call must be known
    // before a message that has it is saved.
    save_labels(store, gmail, account, token, false)?;

    if step == STEP_LIST {
        list(store, gmail, account, token)?;
        set_step(store, account, STEP_FETCH)?;
        step = STEP_FETCH;
    }
    if step == STEP_FETCH {
        fetch(store, gmail, account, token, done)?;
        set_step(store, account, STEP_INCREMENTAL)?;
    }
    Ok(start)
}

/// Steps 1 to 7 of the full resync (specification 1, section 4.4), from the
/// saved step. Gives `H1`.
fn resync_steps(
    store: &Store,
    gmail: &Gmail,
    account: i64,
    token: &str,
    s: &State,
    done: &mut Done,
) -> Result<u64> {
    let (start, mut step) = match s.start.as_deref() {
        Some(h1) => (parse_history(h1)?, s.step.unwrap_or(RESYNC_LIST)),
        None => {
            // Step 1: H1 before the list, as H0 of the first sync.
            let h1 = gmail.profile(token)?.history_id;
            begin(store, account, KIND_RESYNC, h1, RESYNC_LIST)?;
            (h1, RESYNC_LIST)
        }
    };
    // Step 2 on each call, as in the first sync.
    save_labels(store, gmail, account, token, true)?;

    if step == RESYNC_LIST {
        // Step 3. A list that stopped starts again, so its marks go first.
        store.write(|tx| {
            tx.execute("DELETE FROM sync_seen WHERE account_id = ?1", [account])?;
            Ok(())
        })?;
        list_pages(store, gmail, account, token, &[], Mark::KnownAndSeen)?;
        set_step(store, account, RESYNC_FETCH)?;
        step = RESYNC_FETCH;
    }
    if step == RESYNC_FETCH {
        come_back(store, account)?;
        // Step 4. A 404 marks the new row gone: the row stays as a tombstone.
        fetch(store, gmail, account, token, done)?;
        set_step(store, account, RESYNC_GONE)?;
        step = RESYNC_GONE;
    }
    if step == RESYNC_GONE {
        check_unlisted(store, gmail, account, token, done)?;
        set_step(store, account, RESYNC_LABELS)?;
        step = RESYNC_LABELS;
    }
    if step == RESYNC_LABELS {
        label_sets(store, gmail, account, token)?;
        store.write(|tx| {
            // Step 7: the label set of each listed message is the set of
            // the label lists that name it (section 4.10).
            let listed = "SELECT m.id FROM message m JOIN sync_seen s
                ON s.account_id = m.account_id AND s.message_id = m.provider_id
                WHERE m.account_id = ?1 AND m.gone_at IS NULL";
            tx.execute(
                &format!("DELETE FROM message_label WHERE message_id IN ({listed})"),
                [account],
            )?;
            tx.execute(
                "INSERT OR IGNORE INTO message_label(message_id, label_id)
                 SELECT m.id, l.id FROM sync_seen s
                 JOIN message m ON m.account_id = s.account_id AND m.provider_id = s.message_id
                 JOIN json_each(s.labels) j
                 JOIN label l ON l.account_id = s.account_id AND l.provider_id = j.value
                 WHERE s.account_id = ?1 AND m.gone_at IS NULL",
                [account],
            )?;
            tx.execute(
                "UPDATE account SET sync_step = ?2 WHERE id = ?1",
                params![account, RESYNC_INCREMENTAL],
            )?;
            Ok(())
        })?;
    }
    Ok(start)
}

/// Step 6 of the full resync: each listed message that is gone comes back,
/// and understanding runs on it again. If its content file is not there, the
/// fetch step fetches it again.
fn come_back(store: &Store, account: i64) -> Result<()> {
    loop {
        let rows: Vec<(i64, Option<String>)> = store.read(|c| {
            let mut st = c.prepare_cached(
                "SELECT m.id, m.content_sha256 FROM message m JOIN sync_seen s
                   ON s.account_id = m.account_id AND s.message_id = m.provider_id
                 WHERE m.account_id = ?1 AND m.gone_at IS NOT NULL LIMIT ?2",
            )?;
            let rows = st.query_map(params![account, CHUNK as i64], |r| {
                Ok((r.get(0)?, r.get(1)?))
            })?;
            Ok(rows.collect::<rusqlite::Result<_>>()?)
        })?;
        if rows.is_empty() {
            return Ok(());
        }
        store.write(|tx| {
            for (id, sha) in &rows {
                // Section 4.5: the items of the message get their saved status again.
                tx.execute(
                    "UPDATE item SET status = status_before_gone, status_before_gone = NULL
                     WHERE status = 'gone' AND status_before_gone IS NOT NULL
                       AND id IN (SELECT item_id FROM item_source WHERE message_id = ?1)",
                    [id],
                )?;
                let kept = sha
                    .as_deref()
                    .and_then(|s| store.content_path(s))
                    .is_some_and(|p| p.is_file());
                if kept {
                    tx.execute(
                        "UPDATE message SET gone_at = NULL, understood_at = NULL WHERE id = ?1",
                        [id],
                    )?;
                } else {
                    tx.execute(
                        "UPDATE message SET gone_at = NULL, understood_at = NULL,
                           fetched_at = NULL, content_sha256 = NULL WHERE id = ?1",
                        [id],
                    )?;
                }
            }
            Ok(())
        })?;
    }
}

/// Step 5 of the full resync: a message of the mirror that the list does
/// not have is gone only if Gmail gives HTTP 404 for it. Else its labels
/// come from the answer.
fn check_unlisted(
    store: &Store,
    gmail: &Gmail,
    account: i64,
    token: &str,
    done: &mut Done,
) -> Result<()> {
    let mut after = 0i64;
    loop {
        let rows: Vec<(i64, String)> = store.read(|c| {
            let mut st = c.prepare_cached(
                "SELECT m.id, m.provider_id FROM message m
                 WHERE m.account_id = ?1 AND m.gone_at IS NULL AND m.id > ?2
                   AND NOT EXISTS (SELECT 1 FROM sync_seen s
                     WHERE s.account_id = m.account_id AND s.message_id = m.provider_id)
                 ORDER BY m.id LIMIT ?3",
            )?;
            let rows = st.query_map(params![account, after, CHUNK as i64], |r| {
                Ok((r.get(0)?, r.get(1)?))
            })?;
            Ok(rows.collect::<rusqlite::Result<_>>()?)
        })?;
        let Some(last) = rows.last() else {
            return Ok(());
        };
        after = last.0;
        for (id, provider_id) in rows {
            match gmail.minimal(token, &provider_id) {
                Ok(m) => store.write(|tx| set_labels(tx, account, id, &m.label_ids, Op::Set))?,
                Err(iris_gmail::Error::NotFound) => {
                    done.gone += mark_gone(store, account, &[provider_id])?;
                }
                Err(e) if per_message(&e) => {
                    record_failure(store, account, &provider_id, &e.into())?;
                    done.failed += 1;
                }
                Err(e) => return Err(e.into()),
            }
        }
    }
}

/// Step 7 of the full resync, and step 4 of verify: mark in `sync_seen` the
/// labels of each listed message, from one list for each label (section
/// 4.10). A list that stopped starts again from no labels.
fn label_sets(store: &Store, gmail: &Gmail, account: i64, token: &str) -> Result<()> {
    store.write(|tx| {
        tx.execute(
            "UPDATE sync_seen SET labels = '[]' WHERE account_id = ?1",
            [account],
        )?;
        Ok(())
    })?;
    let labels: Vec<String> = store.read(|c| {
        let mut st =
            c.prepare("SELECT provider_id FROM label WHERE account_id = ?1 ORDER BY id")?;
        let rows = st.query_map([account], |r| r.get(0))?;
        Ok(rows.collect::<rusqlite::Result<_>>()?)
    })?;
    for label in &labels {
        list_pages(store, gmail, account, token, &[label], Mark::Label(label))?;
    }
    Ok(())
}

/// Save the step.
fn set_step(store: &Store, account: i64, step: i64) -> Result<()> {
    store.write(|tx| {
        tx.execute(
            "UPDATE account SET sync_step = ?2 WHERE id = ?1",
            params![account, step],
        )?;
        Ok(())
    })?;
    Ok(())
}

/// Begin a first sync or a full resync from the history ID `start`.
fn begin(store: &Store, account: i64, kind: &str, start: u64, step: i64) -> Result<()> {
    store.write(|tx| {
        tx.execute(
            "UPDATE account SET sync_kind = ?2, sync_start = ?3, sync_step = ?4 WHERE id = ?1",
            params![account, kind, start.to_string(), step],
        )?;
        Ok(())
    })?;
    Ok(())
}

/// Save each label of `labels.list` with its name. With `delete`, also
/// delete each label row, and its `message_label` rows, that the answer
/// does not have (specification 1, section 4.3).
fn save_labels(
    store: &Store,
    gmail: &Gmail,
    account: i64,
    token: &str,
    delete: bool,
) -> Result<()> {
    let labels = gmail.labels(token)?;
    store.write(|tx| {
        let mut put = tx.prepare_cached(
            "INSERT INTO label(account_id, provider_id, name, kind) VALUES (?1, ?2, ?3, ?4)
             ON CONFLICT(account_id, provider_id) DO UPDATE SET name = excluded.name, kind = excluded.kind",
        )?;
        for l in &labels {
            let kind = if l.kind == "system" { "system" } else { "user" };
            put.execute(params![account, l.id, l.name, kind])?;
        }
        if delete {
            let have: Vec<(i64, String)> = {
                let mut st = tx.prepare_cached("SELECT id, provider_id FROM label WHERE account_id = ?1")?;
                let rows = st.query_map([account], |r| Ok((r.get(0)?, r.get(1)?)))?;
                rows.collect::<rusqlite::Result<_>>()?
            };
            for (id, provider_id) in have {
                if !labels.iter().any(|l| l.id == provider_id) {
                    // ON DELETE CASCADE removes its message_label rows.
                    tx.execute("DELETE FROM label WHERE id = ?1", [id])?;
                }
            }
        }
        Ok(())
    })?;
    Ok(())
}

/// The incremental sync from the history ID `from` (specification 1,
/// section 4.3). Its last transaction writes the history ID and
/// `mirror_ready_at`, and clears the `sync_` columns. Gives the new history
/// ID.
///
/// A sync that stops before its end leaves the old history ID. The next
/// call applies the same records again from the old history ID, and gets
/// the same mirror: each record sets a state, and a later record of a
/// message sets its later state.
fn incremental(
    store: &Store,
    gmail: &Gmail,
    account: i64,
    token: &str,
    from: u64,
    done: &mut Done,
) -> Result<u64> {
    let mut page_token: Option<String> = None;
    loop {
        let page = gmail.history(token, from, &HistoryType::ALL, page_token.as_deref())?;
        // After history.list: Gmail makes no history record for a new name
        // or a delete of a label. A label that a record of this page names
        // was made before this call.
        save_labels(store, gmail, account, token, true)?;
        for record in &page.history {
            for change in &record.changes {
                apply(store, gmail, account, token, change, done)?;
            }
        }
        match page.next_page_token {
            Some(t) => page_token = Some(t),
            None => {
                let now = now_ms();
                store.write(|tx| {
                    tx.execute(
                        "UPDATE account SET history_id = ?2,
                           mirror_ready_at = coalesce(mirror_ready_at, ?3),
                           sync_kind = NULL, sync_start = NULL, sync_step = NULL
                         WHERE id = ?1",
                        params![account, page.history_id.to_string(), now],
                    )?;
                    tx.execute("DELETE FROM sync_seen WHERE account_id = ?1", [account])?;
                    Ok(())
                })?;
                return Ok(page.history_id);
            }
        }
    }
}

#[derive(Clone, Copy)]
enum Op {
    Set,
    Add,
    Remove,
}

/// Apply one change of a history record.
fn apply(
    store: &Store,
    gmail: &Gmail,
    account: i64,
    token: &str,
    change: &Change,
    done: &mut Done,
) -> Result<()> {
    let (m, labels, op) = match change {
        Change::Deleted(m) => {
            // A message that the mirror does not have changes nothing.
            done.gone += mark_gone(store, account, std::slice::from_ref(&m.id))?;
            return Ok(());
        }
        Change::Added(m) => (m, &m.label_ids, Op::Set),
        Change::LabelsAdded(m, l) => (m, l, Op::Add),
        Change::LabelsRemoved(m, l) => (m, l, Op::Remove),
    };
    let row: Option<i64> = store.read(|c| {
        Ok(c.query_row(
            "SELECT id FROM message WHERE account_id = ?1 AND provider_id = ?2
               AND fetched_at IS NOT NULL",
            params![account, m.id],
            |r| r.get(0),
        )
        .optional()?)
    })?;
    let Some(row) = row else {
        // The mirror does not have the message: fetch it, with the labels of now.
        return match fetch_one(store, gmail, token, &m.id, &m.thread_id)? {
            Fetched::Message(msg) => {
                save(store, gmail, token, account, vec![*msg])?;
                done.fetched += 1;
                Ok(())
            }
            // Gmail deleted the message after the record. A known row of
            // the first sync becomes gone.
            Fetched::Gone(id) => {
                done.gone += mark_gone(store, account, &[id])?;
                Ok(())
            }
            Fetched::Failed(id, e) => {
                store.write(|tx| insert_known(tx, account, &id, &m.thread_id))?;
                record_failure(store, account, &id, &e)?;
                done.failed += 1;
                Ok(())
            }
        };
    };
    Ok(store.write(|tx| set_labels(tx, account, row, labels, op))?)
}

/// Change the label set of the message row `message`. A label that the
/// store does not have changes nothing (specification 1, section 4.3).
fn set_labels(
    tx: &Transaction,
    account: i64,
    message: i64,
    labels: &[String],
    op: Op,
) -> iris_store::Result<()> {
    if let Op::Set = op {
        tx.execute("DELETE FROM message_label WHERE message_id = ?1", [message])?;
    }
    let sql = match op {
        Op::Set | Op::Add => {
            "INSERT OR IGNORE INTO message_label(message_id, label_id)
             SELECT ?1, id FROM label WHERE account_id = ?2 AND provider_id = ?3"
        }
        Op::Remove => {
            "DELETE FROM message_label WHERE message_id = ?1 AND label_id IN
               (SELECT id FROM label WHERE account_id = ?2 AND provider_id = ?3)"
        }
    };
    let mut st = tx.prepare_cached(sql)?;
    for l in labels {
        st.execute(params![message, account, l])?;
    }
    Ok(())
}

/// Mark the messages as gone (specification 1, section 4.5). Gives the
/// count of messages that were not gone.
fn mark_gone(store: &Store, account: i64, ids: &[String]) -> Result<u64> {
    if ids.is_empty() {
        return Ok(0);
    }
    let now = now_ms();
    Ok(store.write(|tx| {
        let mut st = tx.prepare_cached(
            "UPDATE message SET gone_at = ?3
             WHERE account_id = ?1 AND provider_id = ?2 AND gone_at IS NULL",
        )?;
        // Section 4.5: an item whose sources are all gone gets the status
        // `gone`, and keeps its status before it.
        let mut items = tx.prepare_cached(
            "UPDATE item SET status_before_gone = status, status = 'gone'
             WHERE status <> 'gone'
               AND id IN (SELECT s.item_id FROM item_source s JOIN message m ON m.id = s.message_id
                          WHERE m.account_id = ?1 AND m.provider_id = ?2)
               AND NOT EXISTS (SELECT 1 FROM item_source s JOIN message m ON m.id = s.message_id
                               WHERE s.item_id = item.id AND m.gone_at IS NULL)",
        )?;
        let mut n = 0;
        for id in ids {
            let changed = st.execute(params![account, id, now])? as u64;
            if changed > 0 {
                items.execute(params![account, id])?;
            }
            n += changed;
        }
        Ok(n)
    })?)
}

fn insert_known(tx: &Transaction, account: i64, id: &str, thread: &str) -> iris_store::Result<()> {
    let mut t = tx.prepare_cached(
        "INSERT INTO thread(account_id, provider_id) VALUES (?1, ?2)
         ON CONFLICT(account_id, provider_id) DO UPDATE SET provider_id = provider_id
         RETURNING id",
    )?;
    let mut m = tx.prepare_cached(
        "INSERT INTO message(account_id, thread_id, provider_id) VALUES (?1, ?2, ?3)
         ON CONFLICT(account_id, provider_id) DO NOTHING",
    )?;
    let thread: i64 = t.query_row(params![account, thread], |r| r.get(0))?;
    m.execute(params![account, thread, id])?;
    Ok(())
}

/// True for an error of one message that a new try of the same call does
/// not fix: a bad answer, or a refusal of the request (HTTP 400). Other
/// errors (network, sign-in, quota, the local disk) stop the sync.
fn per_message(e: &iris_gmail::Error) -> bool {
    match e {
        iris_gmail::Error::Decode(_) => true,
        iris_gmail::Error::Api { status, .. } => *status == 400,
        _ => false,
    }
}

fn record_failure(store: &Store, account: i64, id: &str, e: &Error) -> Result<()> {
    let now = now_ms();
    log(store, account, "fetch", now, &format!("failed {id}: {e}"))
}

/// Add one row to `sync_log`.
fn log(store: &Store, account: i64, kind: &str, started: i64, result: &str) -> Result<()> {
    store.write(|tx| {
        tx.execute(
            "INSERT INTO sync_log(account_id, kind, started_at, ended_at, result)
             VALUES (?1, ?2, ?3, ?4, ?5)",
            params![account, kind, started, now_ms(), result],
        )?;
        Ok(())
    })?;
    Ok(())
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

/// Step 3 of the first sync: save each listed message as "known, not
/// fetched", one page in each transaction. A message that the store has
/// stays as it is. The `AUTOINCREMENT` row IDs keep the list order (newest
/// first) for the fetch step.
///
/// A list that stopped starts again from the first page. In the list step no
/// message is fetched yet, so the rows of the stopped list go first: new
/// rows then follow the order of the new list, also for mail that came in
/// a new thread after the stop.
fn list(store: &Store, gmail: &Gmail, account: i64, token: &str) -> Result<()> {
    store.write(|tx| {
        tx.execute(
            "DELETE FROM message WHERE account_id = ?1 AND fetched_at IS NULL AND gone_at IS NULL",
            [account],
        )?;
        tx.execute(
            "DELETE FROM thread WHERE account_id = ?1
             AND NOT EXISTS (SELECT 1 FROM message m WHERE m.thread_id = thread.id)",
            [account],
        )?;
        Ok(())
    })?;
    list_pages(store, gmail, account, token, &[], Mark::Known)
}

/// What [`list_pages`] writes for each listed message.
#[derive(Clone, Copy)]
enum Mark<'a> {
    /// A "known, not fetched" row, if the store does not have the message.
    Known,
    /// A known row, and a `sync_seen` row.
    KnownAndSeen,
    /// The label in the `sync_seen` row of the message, if it has one.
    Label(&'a str),
}

/// List each message (with `includeSpamTrash`) that has each label of
/// `labels`, page by page, one transaction for each page.
fn list_pages(
    store: &Store,
    gmail: &Gmail,
    account: i64,
    token: &str,
    labels: &[&str],
    mark: Mark,
) -> Result<()> {
    let mut page_token: Option<String> = None;
    loop {
        let page = gmail.messages(token, labels, true, page_token.as_deref())?;
        store.write(|tx| {
            let mut seen = tx.prepare_cached(
                "INSERT INTO sync_seen(account_id, message_id, labels) VALUES (?1, ?2, '[]')
                 ON CONFLICT(account_id, message_id) DO NOTHING",
            )?;
            let mut label = tx.prepare_cached(
                "UPDATE sync_seen SET labels = json_insert(labels, '$[#]', ?3)
                 WHERE account_id = ?1 AND message_id = ?2",
            )?;
            for m in &page.messages {
                match mark {
                    Mark::Known => insert_known(tx, account, &m.id, &m.thread_id)?,
                    Mark::KnownAndSeen => {
                        insert_known(tx, account, &m.id, &m.thread_id)?;
                        seen.execute(params![account, m.id])?;
                    }
                    Mark::Label(l) => {
                        label.execute(params![account, m.id, l])?;
                    }
                }
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
    /// An error of this message that a new try does not fix.
    Failed(String, Error),
}

/// Step 4: fetch by thread, newest thread first. The thread row IDs keep
/// the order in which the newest-first list named each thread first. A
/// message that fails stays "known, not fetched": the loop goes on after it.
fn fetch(store: &Store, gmail: &Gmail, account: i64, token: &str, done: &mut Done) -> Result<()> {
    let mut after = (0i64, 0i64);
    loop {
        let ids: Vec<(i64, i64, String, String)> = store.read(|c| {
            let mut st = c.prepare_cached(
                "SELECT m.thread_id, m.id, m.provider_id, t.provider_id
                 FROM message m JOIN thread t ON t.id = m.thread_id
                 WHERE m.account_id = ?1 AND m.fetched_at IS NULL AND m.gone_at IS NULL
                   AND (m.thread_id, m.id) > (?3, ?4)
                 ORDER BY m.thread_id, m.id LIMIT ?2",
            )?;
            let rows = st.query_map(params![account, CHUNK as i64, after.0, after.1], |r| {
                Ok((r.get(0)?, r.get(1)?, r.get(2)?, r.get(3)?))
            })?;
            Ok(rows.collect::<rusqlite::Result<_>>()?)
        })?;
        let Some(last) = ids.last() else {
            return Ok(());
        };
        after = (last.0, last.1);
        let ids: Vec<(String, String)> = ids.into_iter().map(|(_, _, m, t)| (m, t)).collect();
        let (results, error) = fetch_chunk(store, gmail, token, &ids);
        // Save the fetches of the chunk also when one of them failed.
        let mut messages = Vec::new();
        let mut gone = Vec::new();
        let mut failed = Vec::new();
        for r in results {
            match r {
                Fetched::Message(m) => messages.push(*m),
                Fetched::Gone(id) => gone.push(id),
                Fetched::Failed(id, e) => failed.push((id, e)),
            }
        }
        done.fetched += messages.len() as u64;
        save(store, gmail, token, account, messages)?;
        done.gone += mark_gone(store, account, &gone)?;
        for (id, e) in &failed {
            record_failure(store, account, id, e)?;
        }
        done.failed += failed.len() as u64;
        if let Some(e) = error {
            return Err(e);
        }
    }
}

/// Fetch the messages of one chunk, with at most [`MAX_GETS`] fetches at
/// the same time. After the first error that stops the sync, no new fetch
/// starts. Gives the fetches that ended, and the first such error.
fn fetch_chunk(
    store: &Store,
    gmail: &Gmail,
    token: &str,
    ids: &[(String, String)],
) -> (Vec<Fetched>, Option<Error>) {
    let next = AtomicUsize::new(0);
    let stop = std::sync::atomic::AtomicBool::new(false);
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
        Err(e) if per_message(&e) => return Ok(Fetched::Failed(id.to_string(), e.into())),
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

/// Write fetched messages. If the content cleanup deleted a content file
/// after its fetch (the store gives `MissingContent`), fetch the messages of
/// that file again, and write again.
fn save(
    store: &Store,
    gmail: &Gmail,
    token: &str,
    account: i64,
    mut batch: Vec<NewMessage>,
) -> Result<()> {
    for _ in 0..3 {
        let sha = match store.put_messages(account, &batch) {
            Err(iris_store::Error::MissingContent(sha)) => sha,
            r => return r.map(|_| ()).map_err(Error::from),
        };
        let (stale, keep): (Vec<NewMessage>, Vec<NewMessage>) = batch
            .into_iter()
            .partition(|m| m.content_sha256.as_deref() == Some(sha.as_str()));
        batch = keep;
        for m in stale {
            match fetch_one(store, gmail, token, &m.provider_id, &m.thread_provider_id)? {
                Fetched::Message(new) => batch.push(*new),
                Fetched::Gone(id) => {
                    mark_gone(store, account, &[id])?;
                }
                Fetched::Failed(id, e) => record_failure(store, account, &id, &e)?,
            }
        }
    }
    Err(iris_store::Error::MissingContent("after 3 fetches".into()).into())
}

fn now_ms() -> i64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map_or(0, |d| d.as_millis() as i64)
}

#[cfg(test)]
mod tests {
    use super::*;
    use iris_fakegmail::FakeGmail;
    use iris_synth::Mailbox;

    #[test]
    fn a_missing_content_file_makes_a_new_fetch() {
        let fake = FakeGmail::start(Mailbox::new(1, 3)).unwrap();
        let gmail = Gmail::new(iris_gmail::Config {
            base_url: fake.base_url(),
            ..iris_gmail::Config::default()
        });
        let dir = std::env::temp_dir().join(format!("iris-sync-missing-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        let store = Store::open(&dir).unwrap();
        let account = store.add_account("gmail", iris_synth::OWNER).unwrap();
        // As if the content cleanup deleted the file after the fetch.
        let lost = NewMessage {
            provider_id: FakeGmail::id_of(0),
            thread_provider_id: "t".into(),
            content_sha256: Some("0".repeat(64)),
            fetched_at: Some(1),
            ..NewMessage::default()
        };
        save(&store, &gmail, "token", account, vec![lost]).unwrap();
        let sha: String = store
            .read(|c| Ok(c.query_row("SELECT content_sha256 FROM message", [], |r| r.get(0))?))
            .unwrap();
        assert!(store.content_path(&sha).unwrap().is_file());
        assert_eq!(
            fake.requests()
                .iter()
                .filter(|r| r.contains("format=raw"))
                .count(),
            1
        );
    }
}
