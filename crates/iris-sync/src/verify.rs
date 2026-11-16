//! Verify (specification 1, section 4.8): compare the mirror with Gmail,
//! and check the local content. The compare goes page by page against
//! SQLite, with the listed IDs in `sync_seen` on disk. Memory holds only the
//! changes of the verify time and the differences.

use std::collections::{BTreeSet, HashSet};
use std::fmt;
use std::io::Read;

use iris_gmail::{Gmail, HistoryType, Label};
use iris_store::Store;
use rusqlite::params;
use sha2::{Digest, Sha256};

use super::*;

/// One difference between the mirror and Gmail, or of the local content.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Difference {
    /// Gmail has the message, the mirror does not (or has it as gone).
    MissingMessage(String),
    /// The mirror has the message (not gone), Gmail does not.
    ExtraMessage(String),
    /// The message is in another thread in the mirror.
    Thread(String),
    /// The label set of the message is not the same.
    Labels(String),
    /// Gmail has the label, the mirror does not.
    MissingLabel(String),
    /// The mirror has the label, Gmail does not.
    ExtraLabel(String),
    /// The label has another name in the mirror.
    LabelName(String),
    /// The content file of the message is missing or has other bytes. Verify
    /// fetched the message again (section 4.8). This is a local defect.
    Content(String),
    /// The content file of the message is missing or has other bytes, and
    /// the new fetch failed. The file stays as it is.
    Unrepaired(String),
}

impl Difference {
    /// True for a difference of step 7, which starts a full resync.
    pub fn is_sync(&self) -> bool {
        !matches!(self, Difference::Content(_) | Difference::Unrepaired(_))
    }
}

impl fmt::Display for Difference {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Difference::MissingMessage(id) => write!(f, "message {id}: not in the mirror"),
            Difference::ExtraMessage(id) => write!(f, "message {id}: not at Gmail"),
            Difference::Thread(id) => write!(f, "message {id}: other thread"),
            Difference::Labels(id) => write!(f, "message {id}: other labels"),
            Difference::MissingLabel(id) => write!(f, "label {id}: not in the mirror"),
            Difference::ExtraLabel(id) => write!(f, "label {id}: not at Gmail"),
            Difference::LabelName(id) => write!(f, "label {id}: other name"),
            Difference::Content(id) => write!(f, "message {id}: bad content file, fetched again"),
            Difference::Unrepaired(id) => {
                write!(f, "message {id}: bad content file, the new fetch failed")
            }
        }
    }
}

/// The result of [`verify`].
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct Report {
    /// `V`: the history ID of the mirror at the start of the compare.
    pub history_id: u64,
    /// The first [`LISTED`] differences. `total` counts all of them.
    pub differences: Vec<Difference>,
    /// The count of all differences.
    pub total: u64,
    /// True if a difference of step 7 was found: the next [`sync`] runs a
    /// full resync.
    pub resync: bool,
    /// The messages that changed during verify. The compare leaves them out.
    pub skipped: u64,
}

/// The prefix of a `sync_seen` row that marks a listed message in another
/// thread than in the mirror.
pub const THREAD_MARK: &str = "thread:";

/// The most differences that a [`Report`] lists, so that memory stays flat
/// for a mirror with many differences (specification 1, section 9).
pub const LISTED: usize = 1_000;

impl Report {
    fn push(&mut self, d: Difference) {
        self.total += 1;
        self.resync |= d.is_sync();
        if self.differences.len() < LISTED {
            self.differences.push(d);
        }
    }
}

/// Verify `account` (specification 1, section 4.8). It holds the sync lock
/// from its incremental sync to its end, so the mirror does not change. If
/// the process stops, the operating system frees the lock.
///
/// A difference of step 7 starts a full resync: the next [`sync`] runs it.
/// Each difference of the content is fetched again in this call. If a
/// repair fetch gets an error that stops each fetch (for example
/// [`iris_gmail::Error::Unauthorized`]), the log and the resync are saved and
/// the result is that error: get a new token and call again. If the
/// history of `V` is too old at the end, the result is
/// [`iris_gmail::Error::HistoryExpired`]. Verify adds one row to `sync_log`.
/// After an error, that row holds the differences found before the error,
/// and a difference of step 7 among them still starts the resync.
pub fn verify(store: &Store, gmail: &Gmail, account: i64, token: &str) -> Result<Report> {
    let _lock = lock(store, account)?;
    // Step 1.
    let v = sync_locked(store, gmail, account, token)?.history_id;
    let started = now_ms();
    let mut report = Report {
        history_id: v,
        ..Report::default()
    };
    // After an error, the differences found before it are still true: save
    // them and their resync too.
    let result = compare(store, gmail, account, token, v, &mut report);
    let mut text = format!("{} differences, {} skipped", report.total, report.skipped);
    if let Err(e) = &result {
        text.push_str(&format!(", then error: {e}"));
    }
    for d in report.differences.iter().take(100) {
        text.push_str(&format!("\n{d}"));
    }
    let resync = report.resync;
    // One transaction: a logged sync difference always has its resync.
    let saved = store.write(|tx| {
        tx.execute(
            "INSERT INTO sync_log(account_id, kind, started_at, ended_at, result)
             VALUES (?1, 'verify', ?2, ?3, ?4)",
            params![account, started, now_ms(), text],
        )?;
        if resync {
            tx.execute(
                "UPDATE account SET sync_kind = ?2, sync_start = NULL, sync_step = NULL WHERE id = ?1",
                params![account, KIND_RESYNC],
            )?;
        }
        Ok(())
    });
    // The log and the resync are saved. The caller must see an error, for
    // example HTTP 401 of a repair fetch, to get a new token.
    result?;
    saved?;
    Ok(report)
}

/// Add each difference to `report` as it is found. An error ends the
/// compare and leaves the differences found before it in `report`.
fn compare(
    store: &Store,
    gmail: &Gmail,
    account: i64,
    token: &str,
    v: u64,
    report: &mut Report,
) -> Result<()> {
    // Steps 2 to 5.
    let before = gmail.labels(token)?;
    store.write(|tx| {
        tx.execute("DELETE FROM sync_seen WHERE account_id = ?1", [account])?;
        Ok(())
    })?;
    list_pages(store, gmail, account, token, &[], Mark::Seen)?;
    let ids: Vec<String> = before.iter().map(|l| l.id.clone()).collect();
    label_sets(store, gmail, account, token, &ids)?;
    let after = gmail.labels(token)?;

    // Step 6.
    let mut changed: HashSet<String> = HashSet::new();
    let mut page_token: Option<String> = None;
    loop {
        let page = gmail.history(token, v, &HistoryType::ALL, page_token.as_deref())?;
        for record in &page.history {
            for change in &record.changes {
                let m = match change {
                    Change::Added(m) | Change::Deleted(m) => m,
                    Change::LabelsAdded(m, _) | Change::LabelsRemoved(m, _) => m,
                };
                changed.insert(m.id.clone());
            }
        }
        match page.next_page_token {
            Some(t) => page_token = Some(t),
            None => break,
        }
    }
    let same = |l: &Label| before.iter().any(|b| b.id == l.id && b.name == l.name);
    let label_changed: HashSet<String> = before
        .iter()
        .chain(&after)
        .filter(|l| !(same(l) && after.iter().any(|a| a.id == l.id && a.name == l.name)))
        .map(|l| l.id.clone())
        .collect();

    // Step 7.
    report.skipped = changed.len() as u64;
    let mirror_labels: Vec<(String, String)> = store.read(|c| {
        let mut st = c.prepare("SELECT provider_id, name FROM label WHERE account_id = ?1")?;
        let rows = st.query_map([account], |r| Ok((r.get(0)?, r.get(1)?)))?;
        Ok(rows.collect::<rusqlite::Result<_>>()?)
    })?;
    for l in after.iter().filter(|l| !label_changed.contains(&l.id)) {
        match mirror_labels.iter().find(|(id, _)| *id == l.id) {
            None => report.push(Difference::MissingLabel(l.id.clone())),
            Some((_, name)) if *name != l.name => report.push(Difference::LabelName(l.id.clone())),
            Some(_) => {}
        }
    }
    for (id, _) in &mirror_labels {
        if !label_changed.contains(id) && !after.iter().any(|l| l.id == *id) {
            report.push(Difference::ExtraLabel(id.clone()));
        }
    }
    let keep = |id: &String| !changed.contains(id);
    each_page(
        store,
        // The literal is THREAD_MARK.
        "SELECT substr(message_id, 8), '', '' FROM sync_seen
         WHERE account_id = ?1 AND message_id > 'thread:' || ?2 AND message_id LIKE 'thread:%'
         ORDER BY message_id LIMIT ?3",
        account,
        |id, _, _| {
            if keep(&id) {
                report.push(Difference::Thread(id));
            }
            Ok(())
        },
    )?;
    // Gmail has the message: a mirror row that is not gone, or no row.
    each_page(
        store,
        "SELECT s.message_id, '', '' FROM sync_seen s
         WHERE s.account_id = ?1 AND s.message_id > ?2 AND s.message_id NOT LIKE 'thread:%'
           AND NOT EXISTS (SELECT 1 FROM message m
           WHERE m.account_id = s.account_id AND m.provider_id = s.message_id AND m.gone_at IS NULL)
         ORDER BY s.message_id LIMIT ?3",
        account,
        |id, _, _| {
            if keep(&id) {
                report.push(Difference::MissingMessage(id));
            }
            Ok(())
        },
    )?;
    each_page(
        store,
        "SELECT m.provider_id, '', '' FROM message m
         WHERE m.account_id = ?1 AND m.provider_id > ?2 AND m.gone_at IS NULL AND NOT EXISTS
           (SELECT 1 FROM sync_seen s WHERE s.account_id = m.account_id AND s.message_id = m.provider_id)
         ORDER BY m.provider_id LIMIT ?3",
        account,
        |id, _, _| {
            if !keep(&id) {
                return Ok(());
            }
            // A list can miss a message when another message leaves an
            // earlier page between two page calls. Gmail has the last word.
            // The raw answer also gives the thread and the labels to compare.
            // The bytes go to a sink: this path is rare, and memory stays flat.
            match gmail.raw(token, &id, &mut std::io::sink()) {
                Ok(m) => {
                    let (thread, fetched, labels) = mirror_message(store, account, &id)?;
                    if thread != m.thread_id {
                        report.push(Difference::Thread(id));
                    } else if fetched {
                        let gmail_labels: BTreeSet<&str> = m
                            .label_ids
                            .iter()
                            .map(String::as_str)
                            .filter(|l| !label_changed.contains(*l))
                            .collect();
                        let mirror: BTreeSet<&str> = labels
                            .iter()
                            .map(String::as_str)
                            .filter(|l| !label_changed.contains(*l))
                            .collect();
                        if gmail_labels != mirror {
                            report.push(Difference::Labels(id));
                        }
                    }
                }
                Err(iris_gmail::Error::NotFound) => report.push(Difference::ExtraMessage(id)),
                Err(e) => return Err(e.into()),
            }
            Ok(())
        },
    )?;
    each_page(
        store,
        "SELECT m.provider_id,
           (SELECT coalesce(group_concat(value, ' '), '') FROM json_each(s.labels)),
           (SELECT coalesce(group_concat(l.provider_id, ' '), '') FROM message_label ml
              JOIN label l ON l.id = ml.label_id WHERE ml.message_id = m.id)
         FROM message m JOIN sync_seen s
           ON s.account_id = m.account_id AND s.message_id = m.provider_id
         WHERE m.account_id = ?1 AND m.provider_id > ?2 AND m.gone_at IS NULL
           -- A message that is not fetched has no labels yet: step 8 fetches it.
           AND m.fetched_at IS NOT NULL
         ORDER BY m.provider_id LIMIT ?3",
        account,
        |id, gmail_labels, mirror_labels| {
            let set = |text: &str| -> BTreeSet<String> {
                text.split(' ')
                    .filter(|l| !l.is_empty() && !label_changed.contains(*l))
                    .map(str::to_string)
                    .collect()
            };
            let mirror = set(&mirror_labels);
            if !keep(&id) || set(&gmail_labels) == mirror {
                return Ok(());
            }
            // A label list can miss a message when a delete moves its
            // pages. The label set of the message itself has the last word.
            match gmail.minimal(token, &id) {
                Ok(m) if set(&m.label_ids.join(" ")) != mirror => {
                    report.push(Difference::Labels(id))
                }
                // Gmail deleted it during verify: the next sync marks it gone.
                Ok(_) | Err(iris_gmail::Error::NotFound) => {}
                Err(e) => return Err(e.into()),
            }
            Ok(())
        },
    )?;

    // Step 8: the bytes of each message that is not gone, page by page. A
    // message that changed during verify is left out: Gmail can have
    // deleted it. After an error that stops each fetch (network, sign-in),
    // no more fetches start, and each bad file stays a difference.
    let mut stop: Option<Error> = None;
    each_page(
        store,
        "SELECT m.provider_id, coalesce(m.content_sha256, ''), t.provider_id
         FROM message m JOIN thread t ON t.id = m.thread_id
         WHERE m.account_id = ?1 AND m.provider_id > ?2 AND m.gone_at IS NULL
         ORDER BY m.provider_id LIMIT ?3",
        account,
        |id, sha, thread| {
            if !keep(&id) || content_ok(store, &sha) {
                return Ok(());
            }
            if stop.is_none() {
                let r = repair(store, gmail, token, account, &id, &thread).and_then(|done| {
                    if done || sha.is_empty() {
                        return Ok(());
                    }
                    // Other messages with the same bytes share the file: a
                    // fetch of one of them repairs it too.
                    for (other, t) in sharing(store, account, &sha, &id)? {
                        if keep(&other) && repair(store, gmail, token, account, &other, &t)? {
                            break;
                        }
                    }
                    Ok(())
                });
                if let Err(e) = r {
                    stop = Some(e);
                }
            }
            let fixed =
                !sha.is_empty() && content_ok(store, &sha) || fetched_now(store, account, &id)?;
            report.push(if fixed {
                Difference::Content(id)
            } else {
                Difference::Unrepaired(id)
            });
            Ok(())
        },
    )?;
    if let Some(e) = &stop {
        log(
            store,
            account,
            "fetch",
            now_ms(),
            &format!("verify repair stopped: {e}"),
        )?;
    }
    store.write(|tx| {
        tx.execute("DELETE FROM sync_seen WHERE account_id = ?1", [account])?;
        Ok(())
    })?;
    match stop {
        Some(e) => Err(e),
        None => Ok(()),
    }
}

/// The thread, the fetched state and the labels of a message in the mirror.
fn mirror_message(store: &Store, account: i64, id: &str) -> Result<(String, bool, Vec<String>)> {
    Ok(store.read(|c| {
        let (row, thread, fetched): (i64, String, bool) = c.query_row(
            "SELECT m.id, t.provider_id, m.fetched_at IS NOT NULL FROM message m
             JOIN thread t ON t.id = m.thread_id WHERE m.account_id = ?1 AND m.provider_id = ?2",
            params![account, id],
            |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?)),
        )?;
        let mut st = c.prepare_cached(
            "SELECT l.provider_id FROM message_label ml JOIN label l ON l.id = ml.label_id
             WHERE ml.message_id = ?1",
        )?;
        let labels = st.query_map([row], |r| r.get(0))?;
        Ok((thread, fetched, labels.collect::<rusqlite::Result<_>>()?))
    })?)
}

/// Fetch one message again to repair its content file. Gives true if the
/// new file is saved.
fn repair(
    store: &Store,
    gmail: &Gmail,
    token: &str,
    account: i64,
    id: &str,
    thread: &str,
) -> Result<bool> {
    let mut counts = Done::default();
    match fetch_one(store, gmail, token, id, thread)? {
        Fetched::Message(m) => save(store, gmail, token, account, vec![*m], &mut counts)?,
        // Gmail deleted it during verify: the next sync marks it gone.
        Fetched::Gone(_) => {}
        Fetched::Failed(id, e) => record_failure(store, account, &id, &e)?,
    }
    Ok(counts.fetched == 1)
}

/// Up to 8 other messages (not gone) with the content file `sha`.
fn sharing(store: &Store, account: i64, sha: &str, id: &str) -> Result<Vec<(String, String)>> {
    Ok(store.read(|c| {
        let mut st = c.prepare_cached(
            "SELECT m.provider_id, t.provider_id FROM message m JOIN thread t ON t.id = m.thread_id
             WHERE m.account_id = ?1 AND m.content_sha256 = ?2 AND m.provider_id <> ?3
               AND m.gone_at IS NULL LIMIT 8",
        )?;
        let rows = st.query_map(params![account, sha, id], |r| Ok((r.get(0)?, r.get(1)?)))?;
        Ok(rows.collect::<rusqlite::Result<_>>()?)
    })?)
}

/// True if the message now has a content file with its hash.
fn fetched_now(store: &Store, account: i64, id: &str) -> Result<bool> {
    let sha: Option<String> = store.read(|c| {
        Ok(c.query_row(
            "SELECT content_sha256 FROM message WHERE account_id = ?1 AND provider_id = ?2",
            params![account, id],
            |r| r.get(0),
        )?)
    })?;
    Ok(sha.is_some_and(|s| content_ok(store, &s)))
}

/// Run `sql` page by page. The query gives (id, a, b) in the order of id,
/// and takes the account as `?1`, the last id as `?2` and the page size as
/// `?3`. `f` runs after the read of each page ends, so it can call Gmail and
/// write to the store.
fn each_page(
    store: &Store,
    sql: &str,
    account: i64,
    mut f: impl FnMut(String, String, String) -> Result<()>,
) -> Result<()> {
    let mut after = String::new();
    loop {
        let rows: Vec<(String, String, String)> = store.read(|c| {
            let mut st = c.prepare_cached(sql)?;
            let rows = st.query_map(params![account, after, CHUNK as i64 * 8], |r| {
                Ok((r.get(0)?, r.get(1)?, r.get(2)?))
            })?;
            Ok(rows.collect::<rusqlite::Result<_>>()?)
        })?;
        let Some(last) = rows.last() else {
            return Ok(());
        };
        after = last.0.clone();
        for (id, a, b) in rows {
            f(id, a, b)?;
        }
    }
}

/// True if the content file of `sha` is there and has the SHA-256 hash `sha`.
fn content_ok(store: &Store, sha: &str) -> bool {
    let Some(path) = store.content_path(sha) else {
        return false;
    };
    let Ok(mut file) = std::fs::File::open(path) else {
        return false;
    };
    let mut hash = Sha256::new();
    let mut buf = vec![0u8; 64 * 1024];
    loop {
        match file.read(&mut buf) {
            Ok(0) => break,
            Ok(n) => hash.update(&buf[..n]),
            Err(_) => return false,
        }
    }
    let hex: String = hash.finalize().iter().map(|b| format!("{b:02x}")).collect();
    hex == sha
}
