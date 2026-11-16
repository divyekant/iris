//! The apply of one approved changeset action to Gmail (specification 2,
//! section 7.2), for the verbs `archive`, `inbox`, `read`, `unread` and
//! `trash`. The apply changes the mirror after each provider call.
//!
//! The steps of [`apply`]:
//!
//! 1. Expire an action 14 days after its submit. Wait while the active
//!    policy has the mode `observe`. Send an action
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
//! apply marks the action `applying` (migration 9). A withdraw keeps a
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
    /// withdraw), or another apply of the action has the mark.
    NotApproved(i64),
    /// The token is of another mailbox than the account. The action stays
    /// as it was.
    OtherMailbox,
    /// Another apply runs now, in this process or in another. The action
    /// stays as it was. Apply again after the other apply.
    Busy,
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
            Error::Busy => write!(f, "another apply runs now"),
            Error::OtherMailbox => write!(f, "the token is of another mailbox"),
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
    /// 14 days passed after the submit: the state is `expired`.
    Expired,
}

/// An open action expires 14 days after its submit (section 5.5).
const EXPIRY_MS: i64 = 14 * 24 * 60 * 60 * 1000;

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

/// True if `token` is a token of the mailbox `address` (Gmail profile).
fn owns(gmail: &Gmail, token: &str, address: &str) -> iris_gmail::Result<bool> {
    Ok(gmail
        .profile(token)?
        .email_address
        .eq_ignore_ascii_case(address))
}

/// The file lock `apply.lock` in the data folder. The operating system
/// frees it when the process ends.
fn lock(store: &Store) -> Result<std::fs::File> {
    let file = std::fs::OpenOptions::new()
        .create(true)
        .truncate(false)
        .write(true)
        .open(store.dir().join(LOCK_FILE))
        .map_err(iris_store::Error::Io)?;
    // flock on Linux and macOS: two opens of the file conflict, also in one process.
    match file.try_lock() {
        Ok(()) => Ok(file),
        Err(std::fs::TryLockError::WouldBlock) => Err(Error::Busy),
        Err(std::fs::TryLockError::Error(e)) => Err(iris_store::Error::Io(e).into()),
    }
}

/// The name of the apply lock in the data folder.
pub const LOCK_FILE: &str = "apply.lock";

/// The access token of the mailbox. `renew` gives a new token after HTTP
/// 401, or `None` if there is none (the account needs a sign-in).
pub struct Token<'a> {
    pub token: String,
    pub renew: &'a mut dyn FnMut() -> Option<String>,
}

/// Apply the approved action `action` to the mailbox `account` at Gmail.
/// One apply runs at a time: a second apply gets [`Error::Busy`].
///
/// No provider call of an action is made two times. Before the first write
/// call, the apply marks the action `applying` (migration 9). After that,
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
    // One apply at a time. While the lock lives, a mark found below is the
    // mark of an apply that stopped, not of one that runs.
    let _lock = lock(store)?;
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

    // Gmail message IDs belong to one mailbox: the token must be of the
    // mailbox of the account.
    let address: String = store.read(|c| {
        Ok(c.query_row(
            "SELECT address FROM account WHERE id = ?1",
            [account],
            |r| r.get(0),
        )?)
    })?;
    let mut run = Run {
        store,
        gmail,
        token,
        renewed: false,
        address,
        account,
        action,
        marked: false,
        counts: Counts::default(),
    };
    let address = run.address.clone();
    match run.call(|g, t| owns(g, t, &address)) {
        Ok(true) => {}
        Ok(false) => return Err(Error::OtherMailbox),
        Err(e) => return Err(Error::Gmail(e)),
    }
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
            if let Some(o) = run.mark()? {
                return Ok(o);
            }
        }
        if let Err(e) = run.change(&change, &todo) {
            return run.stop(e);
        }
    }
    // An action with no write call has no mark: the expiry and the mode
    // apply until its end, in the transaction that closes it.
    if let Some(o) = finish(store, action, "applied", !run.marked)? {
        return Ok(o);
    }
    Ok(Outcome::Applied(run.counts))
}

/// One apply in progress.
struct Run<'a, 'b> {
    store: &'a Store,
    gmail: &'a Gmail,
    token: &'a mut Token<'b>,
    renewed: bool,
    /// The address of the account: each token must be of this mailbox.
    address: String,
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
                    // A token of another mailbox is no token for this one.
                    Some(t) if owns(self.gmail, &t, &self.address)? => {
                        self.token.token = t;
                        f(self.gmail, &self.token.token)
                    }
                    _ => Err(iris_gmail::Error::Unauthorized),
                }
            }
            other => other,
        }
    }

    /// Mark the action before its first write call. The mark is a claim:
    /// only one apply sets it, so a second apply of the same action at the
    /// same time makes no write call. The same transaction checks the
    /// expiry and the mode again, because the reads of step 2 take time.
    fn mark(&mut self) -> Result<Option<Outcome>> {
        if self.marked {
            return Ok(None);
        }
        let action = self.action;
        let (gated, n) = self.store.write(|tx| {
            if let Some(o) = gate(tx, action)? {
                return Ok((Some(o), 0));
            }
            let n = tx.execute(
                "UPDATE changeset_action SET applying = 1
                 WHERE id = ?1 AND state = 'approved' AND applying = 0
                   AND (SELECT state FROM changeset c WHERE c.id = changeset_id) <> 'withdrawn'",
                [action],
            )?;
            Ok((None, n))
        })?;
        if gated.is_some() {
            return Ok(gated);
        }
        if n == 0 {
            return Err(Error::NotApproved(action));
        }
        self.marked = true;
        Ok(None)
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
    // Only a marked action fails: no gate.
    finish(store, action, "failed", false)?;
    Ok(Outcome::Failed {
        counts,
        error: error.to_string(),
    })
}

/// The expiry and the mode, in the transaction of the caller: `expired`
/// 14 days after the submit (section 5.5), and a wait in the mode
/// `observe` (section 5.4). Gives the outcome if the action does not apply
/// now.
fn gate(tx: &Connection, action: i64) -> iris_store::Result<Option<Outcome>> {
    let expired = tx.execute(
        "UPDATE changeset_action SET state = 'expired', decided_by = 'expiry', rule = NULL,
           policy = NULL, reason = NULL,
           decided_at = CAST(unixepoch('subsec') * 1000 AS INTEGER)
         WHERE id = ?1 AND state = 'approved' AND applying = 0
           AND CAST(unixepoch('subsec') * 1000 AS INTEGER) - submitted_at >= ?2",
        params![action, EXPIRY_MS],
    )?;
    if expired > 0 {
        return Ok(Some(Outcome::Expired));
    }
    if iris_policy::active(tx)?.policy.mode == Mode::Observe {
        return Ok(Some(Outcome::Waiting));
    }
    Ok(None)
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
    if let Some(o) = store.write(|tx| gate(tx, action))? {
        return Ok(Some(o));
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
    // Only the rows that differ change, so the triggers on message_label
    // (the model wait, the attention) see only real changes.
    let _ = store.write(|tx| {
        let mut now = tx.prepare_cached(
            "SELECT l.id, l.provider_id FROM message_label ml JOIN label l ON l.id = ml.label_id
             WHERE ml.message_id = ?1",
        )?;
        let mut remove =
            tx.prepare_cached("DELETE FROM message_label WHERE message_id = ?1 AND label_id = ?2")?;
        let mut add = tx.prepare_cached(
            "INSERT OR IGNORE INTO message_label(message_id, label_id)
             SELECT ?1, id FROM label WHERE account_id = ?2 AND provider_id = ?3",
        )?;
        for (id, labels) in messages {
            let have: Vec<(i64, String)> = now
                .query_map([id], |r| Ok((r.get(0)?, r.get(1)?)))?
                .collect::<rusqlite::Result<_>>()?;
            for (label, provider_id) in &have {
                if !labels.contains(provider_id) {
                    remove.execute(params![id, label])?;
                }
            }
            for l in labels {
                if !have.iter().any(|(_, p)| p == l) {
                    add.execute(params![id, account, l])?;
                }
            }
        }
        Ok(())
    });
}

/// Move an approved action to `applied` or `failed`. The decision that
/// approved it stays (migration 9). With `gated`, the same transaction
/// checks the expiry and the mode first, and gives their outcome.
fn finish(store: &Store, action: i64, state: &str, gated: bool) -> Result<Option<Outcome>> {
    let (gate_outcome, n) = store.write(|tx| {
        if gated {
            if let Some(o) = gate(tx, action)? {
                return Ok((Some(o), 0));
            }
        }
        let n = tx.execute(
            "UPDATE changeset_action SET state = ?2 WHERE id = ?1 AND state = 'approved'",
            params![action, state],
        )?;
        Ok((None, n))
    })?;
    if gate_outcome.is_some() {
        return Ok(gate_outcome);
    }
    if n == 0 {
        return Err(Error::NotApproved(action));
    }
    Ok(None)
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
