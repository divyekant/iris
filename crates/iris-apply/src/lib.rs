//! The apply of one approved changeset action to Gmail (specification 2,
//! section 7.2), for the verbs `archive`, `inbox`, `read`, `unread` and
//! `trash`. The apply changes the mirror after each provider call.
//!
//! The steps of [`apply`]:
//!
//! 1. Wait while the active policy has the mode `observe`. Send an action
//!    that a rule approved to review (no ledger yet for the bulk count).
//!    Deny an action whose hash does not agree with its rows.
//! 2. Read the labels of each message at Gmail (`format=minimal`), 1,000
//!    messages at a time. Skip a gone message and a message that already
//!    has the target state.
//! 3. Change the other messages: one `batchModify` call for each page, or
//!    one `trash` call for each message. Write the new labels to the mirror
//!    after each call.
//! 4. Set the state `applied`, or `failed` after a clear refusal.
//!
//! This release has no ledger (R2-03). Before the first write call, the
//! apply marks the action `applying` (migration 8). A withdraw keeps a
//! marked action approved. After the mark, no call is made again: an
//! unknown result, a refusal, a read that fails, or the mark of an earlier
//! apply that stopped gives `failed`. See the failure list in
//! specification 2, section 7.2.

use iris_gmail::{Gmail, BATCH_LIMIT};
use iris_policy::Mode;
use iris_store::{format_id, Kind, Store};
use rusqlite::{params, Connection, OptionalExtension};
use sha2::{Digest, Sha256};

#[derive(Debug)]
pub enum Error {
    Store(iris_store::Error),
    /// A Gmail call failed before the first write call, and the action
    /// stays as it was: HTTP 401 with no new token, or a read that failed.
    /// An apply again does each check again.
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

/// The access token of the mailbox. `renew` gives a new token after HTTP
/// 401, or `None` if there is none (the account needs a sign-in).
pub struct Token<'a> {
    pub token: String,
    pub renew: &'a mut dyn FnMut() -> Option<String>,
}

/// Apply the approved action `action` to the mailbox `account` at Gmail.
/// Run one apply at a time, on the one engine thread that writes.
///
/// No provider call of an action is made two times. Before the first write
/// call, the apply marks the action `applying` (migration 8). After that,
/// each end other than success is `failed`: an unknown result, a refusal,
/// a read that fails, and an apply that finds the mark of an earlier apply
/// that stopped. Before the first write call, an error leaves the action
/// as it was, and the next apply does each check again.
pub fn apply(
    store: &Store,
    gmail: &Gmail,
    token: &mut Token<'_>,
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
    if applying {
        // An earlier apply stopped after a write call started. Its result
        // is not known, so no call is made again (section 7.3).
        return failed(
            store,
            action,
            Counts::default(),
            "An earlier apply stopped after a call to Gmail started. Iris does not call \
             Gmail again for this action. The next sync shows the labels.",
        );
    }
    let change = Change::of(&verb).ok_or(Error::Verb(verb))?;
    if let Some(o) = check(store, account, action, &decided_by, approved_hash)? {
        return Ok(o);
    }

    let mut run = Run {
        store,
        gmail,
        token,
        renewed: false,
        account,
        action,
        marked: false,
        counts: Counts::default(),
    };
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
                run.counts.gone.push(id);
                continue;
            };
            match run.call(|g, t| g.minimal(t, &provider_id)) {
                Ok(m) if change.needed(&m.label_ids) => todo.push((id, provider_id, m.label_ids)),
                Ok(m) => same.push((id, m.label_ids)),
                Err(iris_gmail::Error::NotFound) => run.counts.gone.push(id),
                Err(e) => return run.stop(e),
            }
        }
        run.counts.unchanged += same.len() as u64;
        mirror(store, account, &same);
        if !todo.is_empty() {
            run.mark()?;
        }
        if let Err(e) = run.change(&change, &todo) {
            return run.stop(e);
        }
    }
    finish(store, action, "applied")?;
    Ok(Outcome::Applied(run.counts))
}

/// One apply in progress.
struct Run<'a, 'b> {
    store: &'a Store,
    gmail: &'a Gmail,
    token: &'a mut Token<'b>,
    renewed: bool,
    account: i64,
    action: i64,
    /// True after the mark `applying`: a write call can have started.
    marked: bool,
    counts: Counts,
}

impl Run<'_, '_> {
    /// One Gmail call. After HTTP 401, Gmail did not run the call: get a
    /// new token one time for the apply, and call one time again.
    fn call<T>(
        &mut self,
        f: impl Fn(&Gmail, &str) -> iris_gmail::Result<T>,
    ) -> iris_gmail::Result<T> {
        match f(self.gmail, &self.token.token) {
            Err(iris_gmail::Error::Unauthorized) if !self.renewed => {
                self.renewed = true;
                match (self.token.renew)() {
                    Some(t) => {
                        self.token.token = t;
                        f(self.gmail, &self.token.token)
                    }
                    None => Err(iris_gmail::Error::Unauthorized),
                }
            }
            other => other,
        }
    }

    /// Mark the action before its first write call.
    fn mark(&mut self) -> Result<()> {
        if self.marked {
            return Ok(());
        }
        let n = self.store.write(|tx| {
            Ok(tx.execute(
                "UPDATE changeset_action SET applying = 1 WHERE id = ?1 AND state = 'approved'
                   AND (SELECT state FROM changeset c WHERE c.id = changeset_id) <> 'withdrawn'",
                [self.action],
            )?)
        })?;
        if n == 0 {
            return Err(Error::NotApproved(self.action));
        }
        self.marked = true;
        Ok(())
    }

    /// The end after a Gmail error. Before the mark, the action stays as it
    /// was. After the mark, it is failed: no call is made again.
    fn stop(self, e: iris_gmail::Error) -> Result<Outcome> {
        if !self.marked {
            return Err(Error::Gmail(e));
        }
        let error = if e.write_result_unknown() {
            format!("The result of a call is not known, and Iris does not call again: {e}")
        } else {
            e.to_string()
        };
        failed(self.store, self.action, self.counts, &error)
    }

    /// The write calls of one page, each with its mirror write.
    fn change(
        &mut self,
        change: &Change,
        todo: &[(i64, String, Vec<String>)],
    ) -> std::result::Result<(), iris_gmail::Error> {
        if todo.is_empty() {
            return Ok(());
        }
        if change.trash {
            for (id, provider_id, _) in todo {
                let m = self.call(|g, t| g.trash(t, provider_id))?;
                mirror(self.store, self.account, &[(*id, m.label_ids)]);
                self.counts.changed += 1;
            }
            return Ok(());
        }
        let ids: Vec<&str> = todo.iter().map(|(_, p, _)| p.as_str()).collect();
        self.call(|g, t| g.batch_modify(t, &ids, change.add, change.remove))?;
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
        mirror(self.store, self.account, &after);
        self.counts.changed += todo.len() as u64;
        Ok(())
    }
}

fn failed(store: &Store, action: i64, counts: Counts, error: &str) -> Result<Outcome> {
    finish(store, action, "failed")?;
    Ok(Outcome::Failed {
        counts,
        error: error.to_string(),
    })
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
