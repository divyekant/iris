//! The apply of one approved changeset action to Gmail (specification 2,
//! section 7.2), for the verbs `archive`, `inbox`, `read`, `unread` and
//! `trash`. The apply changes the mirror after each provider call.
//!
//! The steps of [`apply`]:
//!
//! 1. Wait while the active policy has the mode `observe`. Send an action
//!    that a rule approved to review (no ledger yet for the bulk count). Deny an action
//!    whose hash does not agree with its rows.
//! 2. Read the labels of each message at Gmail (`format=minimal`), 1,000
//!    messages at a time. Skip a gone message and a message that already
//!    has the target state.
//! 3. Change the other messages: one `batchModify` call for each page, or
//!    one `trash` call for each message. Write the new labels to the mirror
//!    after each call.
//! 4. Set the state `applied`, or `failed` after a clear refusal.
//!
//! Before step 2, the apply marks the action `applying` (migration 8). A
//! withdraw then keeps it approved, and an apply again ends it, also in
//! the mode `observe`.
//!
//! This release has no ledger (R2-03). Each verb here is idempotent, and
//! step 2 reads the labels again at each apply. So after HTTP 401 or an
//! unknown result, the action stays `approved`, and the next apply makes
//! only the calls that Gmail did not make. See the failure list in
//! specification 2, section 7.2.

use iris_gmail::{Gmail, BATCH_LIMIT};
use iris_policy::Mode;
use iris_store::{format_id, Kind, Store};
use rusqlite::{params, Connection, OptionalExtension};
use sha2::{Digest, Sha256};

#[derive(Debug)]
pub enum Error {
    Store(iris_store::Error),
    /// A Gmail call failed, and the action stays `approved`: HTTP 401 (get
    /// a new access token, then apply again), an unknown result of a write,
    /// or a read that failed. An apply again is safe.
    Gmail(iris_gmail::Error),
    /// The action is not `approved`: it does not exist, or another write
    /// changed its state before its first provider call (for example a
    /// withdraw).
    NotApproved(i64),
    /// The verb has no Gmail apply yet: `label` and `unlabel` (issue #164)
    /// and the state verbs.
    Verb(String),
}

impl std::fmt::Display for Error {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Error::Store(e) => write!(f, "store: {e}"),
            Error::Gmail(e) => write!(f, "Gmail: {e}"),
            Error::NotApproved(a) => write!(f, "action {a} is not approved"),
            Error::Verb(v) => write!(f, "the verb {v} has no Gmail apply"),
        }
    }
}

impl std::error::Error for Error {}

impl From<iris_store::Error> for Error {
    fn from(e: iris_store::Error) -> Error {
        Error::Store(e)
    }
}

impl From<rusqlite::Error> for Error {
    fn from(e: rusqlite::Error) -> Error {
        Error::Store(e.into())
    }
}

pub type Result<T> = std::result::Result<T, Error>;

/// The messages of an apply.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct Counts {
    /// Messages that a provider call changed.
    pub changed: u64,
    /// Messages that already had the target state.
    pub unchanged: u64,
    /// Message row IDs that are gone: not in the mirror, or HTTP 404.
    pub gone: Vec<i64>,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Outcome {
    /// The state is `applied`.
    Applied(Counts),
    /// The state is `failed`: Gmail refused a call. The changed messages
    /// keep the change.
    Failed { counts: Counts, error: String },
    /// The state is `denied` by `engine`, with this reason.
    Denied(String),
    /// The state is `in_review` by `engine`, with this reason: a rule
    /// approved the action.
    Review(String),
    /// The mode is `observe`. The action stays `approved` and waits.
    Waiting,
}

/// The label change of a verb.
struct Change {
    add: &'static [&'static str],
    remove: &'static [&'static str],
    /// `messages.trash` for each message, not `batchModify`.
    trash: bool,
}

impl Change {
    fn of(verb: &str) -> Option<Change> {
        let (add, remove, trash): (&[&str], &[&str], bool) = match verb {
            "archive" => (&[], &["INBOX"], false),
            "inbox" => (&["INBOX"], &[], false),
            "read" => (&[], &["UNREAD"], false),
            "unread" => (&["UNREAD"], &[], false),
            "trash" => (&["TRASH"], &[], true),
            _ => return None,
        };
        Some(Change { add, remove, trash })
    }

    /// True if a message with `labels` does not have the target state.
    fn needed(&self, labels: &[String]) -> bool {
        let has = |l: &str| labels.iter().any(|x| x == l);
        self.add.iter().any(|l| !has(l)) || self.remove.iter().any(|l| has(l))
    }
}

/// The approval hash of an action, from its rows (specification 2, section
/// 5.1): the verb, the targets text and the args text, each with its length
/// in bytes first (8 bytes, big-endian), then each resolved message ID in
/// the order of the row IDs, with a comma after it.
pub fn hash(c: &Connection, action: i64) -> rusqlite::Result<String> {
    let (verb, targets, args): (String, String, String) = c.query_row(
        "SELECT verb, targets, args FROM changeset_action WHERE id = ?1",
        [action],
        |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?)),
    )?;
    let mut h = Sha256::new();
    for text in [&verb, &targets, &args] {
        h.update((text.len() as u64).to_be_bytes());
        h.update(text.as_bytes());
    }
    let mut st =
        c.prepare("SELECT message_id FROM action_message WHERE action_id = ?1 ORDER BY 1")?;
    let mut rows = st.query([action])?;
    while let Some(r) = rows.next()? {
        h.update(format_id(Kind::Message, r.get(0)?).as_bytes());
        h.update(b",");
    }
    Ok(h.finalize().iter().map(|b| format!("{b:02x}")).collect())
}

/// Apply the approved action `action` to the mailbox `account` at Gmail,
/// with the access token `token`. Run one apply at a time, on the one
/// engine thread that writes.
pub fn apply(
    store: &Store,
    gmail: &Gmail,
    token: &str,
    account: i64,
    action: i64,
) -> Result<Outcome> {
    let head: Option<(String, Option<String>, String, bool)> = store.read(|c| {
        Ok(c.query_row(
            "SELECT verb, hash, decided_by, applying FROM changeset_action
             WHERE id = ?1 AND state = 'approved'",
            [action],
            |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?, r.get(3)?)),
        )
        .optional()?)
    })?;
    let (verb, approved_hash, decided_by, applying) = head.ok_or(Error::NotApproved(action))?;
    let change = Change::of(&verb).ok_or(Error::Verb(verb))?;
    // The calls of an earlier apply started: the checks passed then, and
    // the action ends as applied or failed (migration 8), in each mode.
    if !applying {
        if let Some(o) = check(store, account, action, &decided_by, approved_hash)? {
            return Ok(o);
        }
    }
    let marked = store.write(|tx| {
        Ok(tx.execute(
            "UPDATE changeset_action SET applying = 1 WHERE id = ?1 AND state = 'approved'
               AND (applying = 1 OR (SELECT state FROM changeset c WHERE c.id = changeset_id)
                                    <> 'withdrawn')",
            [action],
        )?)
    })?;
    if marked == 0 {
        return Err(Error::NotApproved(action));
    }

    let mut counts = Counts::default();
    let mut after = 0;
    loop {
        // A gone message has no provider ID here.
        let page: Vec<(i64, Option<String>)> = store.read(|c| {
            let mut st = c.prepare_cached(
                "SELECT am.message_id, m.provider_id FROM action_message am
                 LEFT JOIN message m ON m.id = am.message_id AND m.gone_at IS NULL
                 WHERE am.action_id = ?1 AND am.message_id > ?2
                 ORDER BY am.message_id LIMIT ?3",
            )?;
            let rows = st.query_map(params![action, after, BATCH_LIMIT as i64], |r| {
                Ok((r.get(0)?, r.get(1)?))
            })?;
            Ok(rows.collect::<rusqlite::Result<_>>()?)
        })?;
        let Some(&(last, _)) = page.last() else {
            break;
        };
        after = last;
        // The messages to change, with their labels at Gmail. The mirror
        // gets the labels of each other message as Gmail gave them.
        let mut todo: Vec<(i64, String, Vec<String>)> = Vec::new();
        let mut same: Vec<(i64, Vec<String>)> = Vec::new();
        for (id, provider_id) in page {
            let Some(provider_id) = provider_id else {
                counts.gone.push(id);
                continue;
            };
            match gmail.minimal(token, &provider_id) {
                Ok(m) if change.needed(&m.label_ids) => todo.push((id, provider_id, m.label_ids)),
                Ok(m) => same.push((id, m.label_ids)),
                Err(iris_gmail::Error::NotFound) => counts.gone.push(id),
                Err(e) => return Err(Error::Gmail(e)),
            }
        }
        counts.unchanged += same.len() as u64;
        mirror(store, account, &same);
        if let Err(e) = change_page(store, gmail, token, account, &change, &todo, &mut counts) {
            if e.write_result_unknown() || matches!(e, iris_gmail::Error::Unauthorized) {
                return Err(Error::Gmail(e));
            }
            finish(store, action, "failed")?;
            return Ok(Outcome::Failed {
                counts,
                error: e.to_string(),
            });
        }
    }
    finish(store, action, "applied")?;
    Ok(Outcome::Applied(counts))
}

/// The checks before the first provider call of an action. Gives the
/// outcome if the action does not apply now.
fn check(
    store: &Store,
    account: i64,
    action: i64,
    decided_by: &str,
    approved_hash: Option<String>,
) -> Result<Option<Outcome>> {
    let active = store.read(iris_policy::active)?;
    if active.policy.mode == Mode::Observe {
        return Ok(Some(Outcome::Waiting));
    }
    // Section 5.4 evaluates a rule approval again before the apply, with
    // the bulk count of the last hour from the ledger. With no ledger, the
    // count is not known, so only a person approves an apply.
    // shortcut: each rule approval goes to review; evaluate it again with
    // the ledger (issue #168).
    if decided_by == "rule" {
        let reason = "A person must approve the action: Iris cannot check the rule again \
                      before the ledger exists.";
        decide(store, action, "in_review", reason)?;
        return Ok(Some(Outcome::Review(reason.into())));
    }
    let other_account: bool = store.read(|c| {
        Ok(c.query_row(
            "SELECT EXISTS (SELECT 1 FROM action_message am JOIN message m ON m.id = am.message_id
               WHERE am.action_id = ?1 AND m.account_id <> ?2)",
            params![action, account],
            |r| r.get(0),
        )?)
    })?;
    let reason = if store.read(|c| Ok(hash(c, action)?))? != approved_hash.unwrap_or_default() {
        Some("The action changed after its approval.")
    } else if other_account {
        Some("The action has messages of another account.")
    } else {
        None
    };
    if let Some(reason) = reason {
        decide(store, action, "denied", reason)?;
        return Ok(Some(Outcome::Denied(reason.into())));
    }

    Ok(None)
}

/// The provider calls of one page, each with its mirror write.
fn change_page(
    store: &Store,
    gmail: &Gmail,
    token: &str,
    account: i64,
    change: &Change,
    todo: &[(i64, String, Vec<String>)],
    counts: &mut Counts,
) -> std::result::Result<(), iris_gmail::Error> {
    if change.trash {
        for (id, provider_id, _) in todo {
            let m = gmail.trash(token, provider_id)?;
            mirror(store, account, &[(*id, m.label_ids)]);
            counts.changed += 1;
        }
        return Ok(());
    }
    if todo.is_empty() {
        return Ok(());
    }
    let ids: Vec<&str> = todo.iter().map(|(_, p, _)| p.as_str()).collect();
    gmail.batch_modify(token, &ids, change.add, change.remove)?;
    // The labels after the call: the labels of step 2 with the change.
    let after: Vec<(i64, Vec<String>)> = todo
        .iter()
        .map(|(id, _, labels)| {
            let mut l: Vec<String> = labels
                .iter()
                .filter(|l| !change.remove.contains(&l.as_str()))
                .cloned()
                .collect();
            l.extend(change.add.iter().map(|a| a.to_string()));
            (*id, l)
        })
        .collect();
    mirror(store, account, &after);
    counts.changed += todo.len() as u64;
    Ok(())
}

/// Write the labels of messages to the mirror, in one transaction. A label
/// that the store does not have adds no row (specification 1, section
/// 4.3). The provider made the change, so a store error here loses only
/// the mirror write: the next sync writes the labels from Gmail.
fn mirror(store: &Store, account: i64, messages: &[(i64, Vec<String>)]) {
    if messages.is_empty() {
        return;
    }
    let _ = store.write(|tx| {
        let mut clear = tx.prepare_cached("DELETE FROM message_label WHERE message_id = ?1")?;
        let mut add = tx.prepare_cached(
            "INSERT OR IGNORE INTO message_label(message_id, label_id)
             SELECT ?1, id FROM label WHERE account_id = ?2 AND provider_id = ?3",
        )?;
        for (id, labels) in messages {
            clear.execute([id])?;
            for l in labels {
                add.execute(params![id, account, l])?;
            }
        }
        Ok(())
    });
}

/// Move an approved action to `applied` or `failed`. The decision that
/// approved it stays (migration 8).
fn finish(store: &Store, action: i64, state: &str) -> Result<()> {
    let n = store.write(|tx| {
        Ok(tx.execute(
            "UPDATE changeset_action SET state = ?2 WHERE id = ?1 AND state = 'approved'",
            params![action, state],
        )?)
    })?;
    if n == 0 {
        return Err(Error::NotApproved(action));
    }
    Ok(())
}

/// A decision of the engine on an approved action: `denied` or `in_review`.
fn decide(store: &Store, action: i64, state: &str, reason: &str) -> Result<()> {
    let now = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map_or(0, |d| d.as_millis() as i64);
    let n = store.write(|tx| {
        Ok(tx.execute(
            "UPDATE changeset_action SET state = ?2, decided_by = 'engine', rule = NULL,
               policy = NULL, reason = ?3, decided_at = ?4
             WHERE id = ?1 AND state = 'approved'",
            params![action, state, reason, now],
        )?)
    })?;
    if n == 0 {
        return Err(Error::NotApproved(action));
    }
    Ok(())
}
