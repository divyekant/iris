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
    pub differences: Vec<Difference>,
    /// The messages that changed during verify. The compare leaves them out.
    pub skipped: u64,
}

/// Verify `account` (specification 1, section 4.8). It holds the sync lock
/// from its incremental sync to its end, so the mirror does not change. If
/// the process stops, the operating system frees the lock.
///
/// A difference of step 7 starts a full resync: the next [`sync`] runs it.
/// Each difference of the content is fetched again in this call. If the
/// history of `V` is too old at the end, the result is
/// [`iris_gmail::Error::HistoryExpired`]. Verify adds one row to `sync_log`.
pub fn verify(store: &Store, gmail: &Gmail, account: i64, token: &str) -> Result<Report> {
    let _lock = lock(store, account)?;
    // Step 1.
    let v = sync_locked(store, gmail, account, token)?.history_id;
    let started = now_ms();
    let result = compare(store, gmail, account, token, v);
    let text = match &result {
        Ok(r) => {
            let mut t = format!("{} differences, {} skipped", r.differences.len(), r.skipped);
            for d in r.differences.iter().take(100) {
                t.push_str(&format!("\n{d}"));
            }
            t
        }
        Err(e) => format!("error: {e}"),
    };
    let logged = log(store, account, "verify", started, &text);
    let report = result?;
    logged?;
    if report.differences.iter().any(Difference::is_sync) {
        store.write(|tx| {
            tx.execute(
                "UPDATE account SET sync_kind = ?2, sync_start = NULL, sync_step = NULL WHERE id = ?1",
                params![account, KIND_RESYNC],
            )?;
            Ok(())
        })?;
    }
    Ok(report)
}

fn compare(store: &Store, gmail: &Gmail, account: i64, token: &str, v: u64) -> Result<Report> {
    // Steps 2 to 5.
    let before = gmail.labels(token)?;
    store.write(|tx| {
        tx.execute("DELETE FROM sync_seen WHERE account_id = ?1", [account])?;
        Ok(())
    })?;
    let other_thread = list_pages(store, gmail, account, token, &[], Mark::Seen)?;
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
    let mut report = Report {
        history_id: v,
        skipped: changed.len() as u64,
        ..Report::default()
    };
    let d = &mut report.differences;
    let mirror_labels: Vec<(String, String)> = store.read(|c| {
        let mut st = c.prepare("SELECT provider_id, name FROM label WHERE account_id = ?1")?;
        let rows = st.query_map([account], |r| Ok((r.get(0)?, r.get(1)?)))?;
        Ok(rows.collect::<rusqlite::Result<_>>()?)
    })?;
    for l in after.iter().filter(|l| !label_changed.contains(&l.id)) {
        match mirror_labels.iter().find(|(id, _)| *id == l.id) {
            None => d.push(Difference::MissingLabel(l.id.clone())),
            Some((_, name)) if *name != l.name => d.push(Difference::LabelName(l.id.clone())),
            Some(_) => {}
        }
    }
    for (id, _) in &mirror_labels {
        if !label_changed.contains(id) && !after.iter().any(|l| l.id == *id) {
            d.push(Difference::ExtraLabel(id.clone()));
        }
    }
    d.extend(
        other_thread
            .into_iter()
            .filter(|id| !changed.contains(id))
            .map(Difference::Thread),
    );
    let keep = |id: &String| !changed.contains(id);
    // Gmail has the message: a mirror row that is not gone, or no row.
    each_page(
        store,
        "SELECT s.message_id, '', '' FROM sync_seen s
         WHERE s.account_id = ?1 AND s.message_id > ?2 AND NOT EXISTS (SELECT 1 FROM message m
           WHERE m.account_id = s.account_id AND m.provider_id = s.message_id AND m.gone_at IS NULL)
         ORDER BY s.message_id LIMIT ?3",
        account,
        |id, _, _| {
            if keep(&id) {
                d.push(Difference::MissingMessage(id));
            }
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
            if keep(&id) {
                d.push(Difference::ExtraMessage(id));
            }
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
            if keep(&id) && set(&gmail_labels) != set(&mirror_labels) {
                d.push(Difference::Labels(id));
            }
        },
    )?;

    // Step 8: the bytes of each message that is not gone.
    let mut bad: Vec<(String, String)> = Vec::new();
    each_page(
        store,
        "SELECT m.provider_id, coalesce(m.content_sha256, ''), t.provider_id
         FROM message m JOIN thread t ON t.id = m.thread_id
         WHERE m.account_id = ?1 AND m.provider_id > ?2 AND m.gone_at IS NULL
         ORDER BY m.provider_id LIMIT ?3",
        account,
        |id, sha, thread| {
            if !content_ok(store, &sha) {
                bad.push((id, thread));
            }
        },
    )?;
    for (id, thread) in bad {
        // A local defect: fetch the message again to repair it.
        let mut counts = Done::default();
        match fetch_one(store, gmail, token, &id, &thread)? {
            Fetched::Message(m) => save(store, gmail, token, account, vec![*m], &mut counts)?,
            // Gmail deleted it during verify: the next sync marks it gone.
            Fetched::Gone(_) => {}
            Fetched::Failed(id, e) => record_failure(store, account, &id, &e)?,
        }
        report.differences.push(if counts.fetched == 1 {
            Difference::Content(id)
        } else {
            Difference::Unrepaired(id)
        });
    }
    store.write(|tx| {
        tx.execute("DELETE FROM sync_seen WHERE account_id = ?1", [account])?;
        Ok(())
    })?;
    Ok(report)
}

/// Run `sql` page by page. The query gives (id, a, b) in the order of id,
/// and takes the account as `?1`, the last id as `?2` and the page size as
/// `?3`.
fn each_page(
    store: &Store,
    sql: &str,
    account: i64,
    mut f: impl FnMut(String, String, String),
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
            f(id, a, b);
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
