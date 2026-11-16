//! The apply of one approved changeset action to Gmail, with its ledger
//! entry (specification 2, sections 7.1 to 7.3), for the verbs `archive`,
//! `inbox`, `read`, `unread`, `trash`, `label` and `unlabel`.
//!
//! The steps of [`apply`]:
//!
//! 1. Expire an action 14 days after its submit. Wait while the active
//!    policy has the mode `observe`. Send an action that a rule approved
//!    to review (issue #168). Deny an action whose hash does not agree with
//!    its rows. Before these checks, run the recovery of each ledger row
//!    of the account in `writing`. For `label` and `unlabel`, find the
//!    user label of the name at Gmail, and make it for `label` if Gmail
//!    does not have it (no ledger entry: the label stays after an undo).
//! 2. Read the labels of each message at Gmail (`format=minimal`). Skip a
//!    gone message and a message that already has the target state.
//! 3. In one transaction, claim the action: the ledger row in `writing`,
//!    with one row for each other message, its `pre`, `post` and call.
//! 4. For each call: save its number, call Gmail (one `batchModify` call
//!    for each 1,000 messages, or one `trash` call for each message), save
//!    `done` for its messages and write the change to the mirror.
//! 5. Set the state `applied`, or `failed`. The trigger `ledger_state`
//!    moves the action to the same state.
//!
//! No provider call is made two times. After an unknown result, and for a
//! row in `writing` of an apply that stopped, the recovery of section 7.3
//! reads the labels at Gmail. See the failure list in specification 2,
//! section 7.2.

use iris_gmail::{Gmail, BATCH_LIMIT};
use iris_policy::Mode;
use iris_store::Store;
use rusqlite::{params, Connection, OptionalExtension};
use unicase::UniCase;

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
    /// Another apply, or a sync of the account, runs now, in this process
    /// or in another. The action
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
    /// Message row IDs whose call started and whose change the recovery
    /// cannot confirm (section 7.3). Iris does not call Gmail again for
    /// them, and the person decides.
    pub unknown: Vec<i64>,
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

/// The label change of an action, with Gmail label IDs.
struct Change {
    add: Vec<String>,
    remove: Vec<String>,
    /// `messages.trash` for each message, not `batchModify`.
    trash: bool,
}

impl Change {
    /// The change of a verb with no label argument.
    fn of(verb: &str) -> Option<Change> {
        let (add, remove, trash): (&[&str], &[&str], bool) = match verb {
            "archive" => (&[], &["INBOX"], false),
            "inbox" => (&["INBOX"], &[], false),
            "read" => (&[], &["UNREAD"], false),
            "unread" => (&["UNREAD"], &[], false),
            "trash" => (&["TRASH"], &[], true),
            _ => return None,
        };
        let own = |v: &[&str]| v.iter().map(|l| l.to_string()).collect();
        Some(Change {
            add: own(add),
            remove: own(remove),
            trash,
        })
    }

    /// The change that a ledger row recorded in `ops` (see [`Change::ops`]).
    /// The recovery uses it: the label IDs of `label` and `unlabel` are
    /// known only at the claim.
    fn of_ledger(c: &Connection, ledger: i64) -> rusqlite::Result<Change> {
        let trash: bool = c.query_row(
            "SELECT json_extract(ops, '$.call') = 'trash' FROM ledger WHERE id = ?1",
            [ledger],
            |r| r.get(0),
        )?;
        let list = |key: &str| -> rusqlite::Result<Vec<String>> {
            let mut st = c.prepare(&format!(
                "SELECT value FROM ledger, json_each(ledger.ops, '$.{key}')
                 WHERE ledger.id = ?1 ORDER BY key"
            ))?;
            let rows = st.query_map([ledger], |r| r.get(0))?;
            rows.collect()
        };
        Ok(Change {
            add: list("add")?,
            remove: list("remove")?,
            trash,
        })
    }

    /// The labels that the action changes, that a message with `labels`
    /// has before the action, as a JSON array. For `trash`, also `INBOX`
    /// (section 7.1).
    fn pre(&self, labels: &[String]) -> String {
        let inbox = ["INBOX".to_string()];
        let watched: Vec<&String> = self
            .add
            .iter()
            .chain(&self.remove)
            .chain(if self.trash { &inbox[..] } else { &[] })
            .filter(|w| labels.contains(w))
            .collect();
        labels_json(watched)
    }

    /// The `ops` of the ledger row: the call, the labels to add and remove,
    /// and the number of calls.
    fn ops(&self, calls: i64) -> String {
        format!(
            r#"{{"call":"{}","add":{},"remove":{},"calls":{calls}}}"#,
            if self.trash { "trash" } else { "batchModify" },
            labels_json(&self.add),
            labels_json(&self.remove),
        )
    }

    /// True if a message with `labels` does not have the target state.
    fn needed(&self, labels: &[String]) -> bool {
        self.add.iter().any(|l| !labels.contains(l))
            || self.remove.iter().any(|l| labels.contains(l))
    }
}

/// The user label of the name `name` in `labels` (the labels of Gmail).
/// Gmail refuses two labels whose names differ only in case, so the match
/// has no regard to case, with the Unicode case folding of submit
/// (`unicase`, as `iris-changeset`). `Err` gives the reason of a denial:
/// the name is the name or the ID of a system label.
fn find_label<'l>(
    labels: &'l [iris_gmail::Label],
    name: &str,
) -> std::result::Result<Option<&'l iris_gmail::Label>, String> {
    let name = UniCase::new(name);
    let mut found = None;
    for l in labels {
        let same_name = UniCase::new(l.name.as_str()) == name;
        if (same_name || UniCase::new(l.id.as_str()) == name) && l.kind == "system" {
            return Err(format!(
                "{} is a system label at Gmail: use its own verb.",
                l.name
            ));
        }
        if same_name {
            found = Some(l);
        }
    }
    Ok(found)
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
    /// Set by the apply when Gmail refused the new token too, or there is
    /// no new token. The caller then sets the account to "needs sign-in"
    /// (section 7.2), as the engine does after a sync.
    pub refused: bool,
}

/// Apply the approved action `action` to the mailbox `account` at Gmail,
/// with its ledger entry (section 7.2). One apply runs at a time: a second
/// apply gets [`Error::Busy`].
///
/// If the action has a ledger row in `writing`, an earlier apply stopped:
/// the apply runs the recovery of section 7.3 for the row, also in the
/// mode `observe`. Else, before its own claim, the apply runs the recovery
/// of each row of the account in `writing`, so no two rows in `writing`
/// share a message.
///
/// No provider call is made two times. Before the claim, an error leaves
/// the action as it was, and the next apply does each check again. After
/// the claim, an unknown result starts the recovery at once. If a read of
/// the recovery fails, the apply gives the error, the row stays in
/// `writing`, and the next apply of an action of the account continues the
/// recovery.
pub fn apply(
    store: &Store,
    gmail: &Gmail,
    token: &mut Token<'_>,
    account: i64,
    action: i64,
) -> Result<Outcome> {
    // One apply at a time. While the lock lives, a row in `writing` is the
    // row of an apply that stopped, not of one that runs.
    let _lock = lock(store)?;
    // No sync of the account runs while the apply reads labels and writes
    // them to the mirror, so no label that a sync saved is lost.
    let _sync = sync_lock(store, account)?;
    #[allow(clippy::type_complexity)]
    let head: Option<(String, Option<String>, String, Option<i64>, Option<i64>)> =
        store.read(|c| {
            Ok(c.query_row(
                "SELECT a.verb, a.hash, a.decided_by, l.id, l.account_id
                 FROM changeset_action a
                 LEFT JOIN ledger l ON l.action_id = a.id AND l.state = 'writing'
                 WHERE a.id = ?1 AND a.state = 'approved'",
                [action],
                |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?, r.get(3)?, r.get(4)?)),
            )
            .optional()?)
        })?;
    let (verb, approved_hash, decided_by, writing, writing_account) =
        head.ok_or(Error::NotApproved(action))?;
    // The label name of `label` and `unlabel`; submit checked the argument.
    let label: Option<String> = match verb.as_str() {
        "label" | "unlabel" => store.read(|c| {
            Ok(c.query_row(
                "SELECT json_extract(args, '$.label') FROM changeset_action WHERE id = ?1",
                [action],
                |r| r.get(0),
            )?)
        })?,
        _ => None,
    };
    let mut change = match (verb.as_str(), &label) {
        ("label" | "unlabel", Some(_)) => Change {
            add: Vec::new(),
            remove: Vec::new(),
            trash: false,
        },
        _ => Change::of(&verb).ok_or(Error::Verb(verb.clone()))?,
    };
    if let Some(ledger) = writing {
        // The Gmail IDs of the row are of its own mailbox.
        if writing_account != Some(account) {
            return Err(Error::OtherMailbox);
        }
        let mut run = Run::new(store, gmail, token, account)?;
        run.mailbox()?;
        return run.recover(ledger);
    }
    // The rows in `writing` of the account end before any outcome of this
    // action, also when this action waits or is refused (section 7.3).
    let mut run = None;
    if writing_rows(store, account)? {
        let mut r = Run::new(store, gmail, token, account)?;
        r.mailbox()?;
        r.recover_account()?;
        run = Some(r);
    }
    if let Some(o) = check(store, account, action, &decided_by, approved_hash)? {
        return Ok(o);
    }
    let mut run = match run {
        Some(r) => r,
        None => {
            let mut r = Run::new(store, gmail, token, account)?;
            r.mailbox()?;
            r
        }
    };
    // Step 1: the user label of `label` and `unlabel`. A label that Gmail
    // does not have is made now, with no ledger entry: it stays after an
    // undo (section 7.2).
    if let Some(name) = &label {
        let labels = run.call(|g, t| g.labels(t)).map_err(Error::Gmail)?;
        let id = match find_label(&labels, name) {
            Err(reason) => {
                decide(store, action, "denied", &reason)?;
                return Ok(Outcome::Denied(reason));
            }
            Ok(Some(l)) => {
                // The mirror needs the label row to show the change.
                store.put_labels(account, &[(&l.id, &l.name, "user")])?;
                Some(l.id.clone())
            }
            Ok(None) if verb == "label" => match run.create_label(name)? {
                Ok(id) => Some(id),
                // The create was a write call: the action ends, with its
                // ledger entry and no message (section 7.2, step 1).
                Err(error) => return run.fail_unclaimed(action, &change, error),
            },
            Ok(None) => None,
        };
        match (verb.as_str(), id) {
            ("label", Some(id)) => change.add.push(id),
            (_, Some(id)) => change.remove.push(id),
            // No message has a label that Gmail does not have.
            (_, None) => {}
        }
    }

    // Step 2: the labels at Gmail. The mirror gets the labels of each
    // message as Gmail gave them. The messages to change wait in a table
    // of the write connection, page by page (section 9), until the claim.
    store.write(|tx| {
        tx.execute_batch(
            "CREATE TEMP TABLE IF NOT EXISTS apply_todo(
               message_id INTEGER PRIMARY KEY, pre TEXT NOT NULL);
             DELETE FROM temp.apply_todo;",
        )?;
        Ok(())
    })?;
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
        let mut seen: Vec<(i64, Vec<String>)> = Vec::new();
        let mut todo: Vec<(i64, String)> = Vec::new();
        for (id, provider_id) in page {
            let Some(provider_id) = provider_id else {
                run.counts.gone.push(id);
                continue;
            };
            match run.call(|g, t| g.minimal(t, &provider_id)) {
                Ok(m) => {
                    if change.needed(&m.label_ids) {
                        todo.push((id, change.pre(&m.label_ids)));
                    } else {
                        run.counts.unchanged += 1;
                    }
                    seen.push((id, m.label_ids));
                }
                Err(iris_gmail::Error::NotFound) => run.counts.gone.push(id),
                Err(e) => {
                    mirror(store, account, &seen);
                    return Err(Error::Gmail(e));
                }
            }
        }
        mirror(store, account, &seen);
        store.write(|tx| {
            let mut add =
                tx.prepare_cached("INSERT INTO temp.apply_todo(message_id, pre) VALUES (?1, ?2)")?;
            for (id, pre) in &todo {
                add.execute(params![id, pre])?;
            }
            Ok(())
        })?;
    }
    // Step 3: the claim, with the ledger row in `writing`.
    match run.claim(action, &change)? {
        Ok(ledger) => run.calls(ledger, &change),
        Err(o) => Ok(o),
    }
}

/// The recovery of section 7.3 for each ledger row in `writing` of the
/// account. The engine runs it at start. It holds the same locks as
/// [`apply`].
pub fn recover(store: &Store, gmail: &Gmail, token: &mut Token<'_>, account: i64) -> Result<()> {
    let _lock = lock(store)?;
    let _sync = sync_lock(store, account)?;
    let mut run = Run::new(store, gmail, token, account)?;
    run.mailbox()?;
    run.recover_account()
}

fn sync_lock(store: &Store, account: i64) -> Result<iris_sync::SyncLock> {
    match iris_sync::lock(store, account) {
        Ok(l) => Ok(l),
        Err(iris_sync::Error::Store(e)) => Err(Error::Store(e)),
        // The lock gives only a store error or OtherSync.
        Err(_) => Err(Error::Busy),
    }
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
    /// The messages of step 2 that have no ledger message row.
    counts: Counts,
}

/// What the recovery learned of one message (section 7.3).
enum Found {
    Done,
    Listed(&'static str),
    Pre(String),
}

impl<'a, 'b> Run<'a, 'b> {
    fn new(
        store: &'a Store,
        gmail: &'a Gmail,
        token: &'a mut Token<'b>,
        account: i64,
    ) -> Result<Run<'a, 'b>> {
        let address: String = store.read(|c| {
            Ok(c.query_row(
                "SELECT address FROM account WHERE id = ?1",
                [account],
                |r| r.get(0),
            )?)
        })?;
        Ok(Run {
            store,
            gmail,
            token,
            renewed: false,
            address,
            account,
            counts: Counts::default(),
        })
    }

    /// Gmail message IDs belong to one mailbox: the token must be of the
    /// mailbox of the account.
    fn mailbox(&mut self) -> Result<()> {
        let address = self.address.clone();
        match self.call(|g, t| owns(g, t, &address)) {
            Ok(true) => Ok(()),
            Ok(false) => Err(Error::OtherMailbox),
            Err(e) => Err(Error::Gmail(e)),
        }
    }

    /// One Gmail call. After HTTP 401, Gmail did not run the call: get a
    /// new token one time for the apply, and call one time again.
    fn call<T>(
        &mut self,
        f: impl Fn(&Gmail, &str) -> iris_gmail::Result<T>,
    ) -> iris_gmail::Result<T> {
        let mut r = f(self.gmail, &self.token.token);
        if matches!(r, Err(iris_gmail::Error::Unauthorized)) && !self.renewed {
            self.renewed = true;
            r = match (self.token.renew)() {
                // A token of another mailbox is no token for this one.
                Some(t) => match owns(self.gmail, &t, &self.address) {
                    Ok(true) => {
                        self.token.token = t;
                        f(self.gmail, &self.token.token)
                    }
                    Ok(false) => Err(iris_gmail::Error::Unauthorized),
                    Err(e) => Err(e),
                },
                None => Err(iris_gmail::Error::Unauthorized),
            };
        }
        if matches!(r, Err(iris_gmail::Error::Unauthorized)) {
            self.token.refused = true;
        }
        r
    }

    /// Step 3: in one transaction, the ledger row in `writing` and one
    /// message row for each message to change, with its call. The same
    /// transaction checks the expiry and the mode again, because the reads
    /// of step 2 take time. Gives the ledger row, or the outcome of the
    /// expiry or the mode.
    fn claim(&mut self, action: i64, change: &Change) -> Result<std::result::Result<i64, Outcome>> {
        let account = self.account;
        let claimed = self.store.write(|tx| {
            if let Some(o) = gate(tx, action)? {
                return Ok(Err(o));
            }
            let approved: bool = tx.query_row(
                "SELECT EXISTS (SELECT 1 FROM changeset_action WHERE id = ?1 AND state = 'approved')",
                [action],
                |r| r.get(0),
            )?;
            if !approved {
                return Ok(Ok(None));
            }
            let n: i64 = tx.query_row("SELECT count(*) FROM temp.apply_todo", [], |r| r.get(0))?;
            let calls = if change.trash {
                n
            } else {
                (n + BATCH_LIMIT as i64 - 1) / BATCH_LIMIT as i64
            };
            tx.execute(
                "INSERT INTO ledger(action_id, account_id, ops, state, undo, at)
                 VALUES (?1, ?2, ?3, 'writing', 'inverse', ?4)",
                params![action, account, change.ops(calls), now()],
            )?;
            let ledger = tx.last_insert_rowid();
            // One trash call for each message, one batchModify call for
            // each 1,000 messages, in the order of the message IDs.
            tx.execute(
                "INSERT INTO ledger_message(ledger_id, message_id, call, pre, post)
                 SELECT ?1, message_id, CASE WHEN ?2 THEN n ELSE (n - 1) / ?3 + 1 END, pre, ?4
                 FROM (SELECT message_id, pre, row_number() OVER (ORDER BY message_id) AS n
                       FROM temp.apply_todo)",
                params![ledger, change.trash, BATCH_LIMIT as i64, labels_json(&change.add)],
            )?;
            tx.execute("DELETE FROM temp.apply_todo", [])?;
            Ok(Ok(Some(ledger)))
        })?;
        match claimed {
            Ok(Some(ledger)) => Ok(Ok(ledger)),
            Ok(None) => Err(Error::NotApproved(action)),
            Err(o) => Ok(Err(o)),
        }
    }

    /// Step 4: each call that did not start, in order. Each call first
    /// saves its number, then calls, then saves `done` for its messages.
    fn calls(&mut self, ledger: i64, change: &Change) -> Result<Outcome> {
        let (from, last): (i64, i64) = self.store.read(|c| {
            Ok(c.query_row(
                "SELECT call, coalesce((SELECT max(call) FROM ledger_message
                                        WHERE ledger_id = l.id), 0)
                 FROM ledger l WHERE id = ?1",
                [ledger],
                |r| Ok((r.get(0)?, r.get(1)?)),
            )?)
        })?;
        for k in from + 1..=last {
            let messages: Vec<(i64, Option<String>)> = self.store.read(|c| {
                let mut st = c.prepare_cached(
                    "SELECT lm.message_id, m.provider_id FROM ledger_message lm
                     LEFT JOIN message m ON m.id = lm.message_id AND m.gone_at IS NULL
                     WHERE lm.ledger_id = ?1 AND lm.call = ?2 AND lm.done = 0
                       AND lm.listed IS NULL
                     ORDER BY 1",
                )?;
                let rows = st.query_map(params![ledger, k], |r| Ok((r.get(0)?, r.get(1)?)))?;
                Ok(rows.collect::<rusqlite::Result<_>>()?)
            })?;
            let live: Vec<(i64, String)> = messages
                .iter()
                .filter_map(|(id, p)| p.clone().map(|p| (*id, p)))
                .collect();
            self.store.write(|tx| {
                tx.execute(
                    "UPDATE ledger SET call = ?2 WHERE id = ?1",
                    params![ledger, k],
                )?;
                // Gone in the mirror after the claim: no call for it.
                for (id, _) in messages.iter().filter(|(_, p)| p.is_none()) {
                    set_listed(tx, ledger, *id, "gone")?;
                }
                Ok(())
            })?;
            if live.is_empty() {
                continue;
            }
            let result = if change.trash {
                let (id, p) = &live[0];
                self.call(|g, t| g.trash(t, p))
                    .map(|m| vec![(*id, m.label_ids)])
            } else {
                let ids: Vec<&str> = live.iter().map(|(_, p)| p.as_str()).collect();
                let add: Vec<&str> = change.add.iter().map(String::as_str).collect();
                let remove: Vec<&str> = change.remove.iter().map(String::as_str).collect();
                self.call(|g, t| g.batch_modify(t, &ids, &add, &remove))
                    .map(|()| Vec::new())
            };
            match result {
                Ok(labels) => {
                    self.store.write(|tx| {
                        Ok(tx.execute(
                            "UPDATE ledger_message SET done = 1
                             WHERE ledger_id = ?1 AND call = ?2 AND listed IS NULL",
                            params![ledger, k],
                        )?)
                    })?;
                    if change.trash {
                        mirror(self.store, self.account, &labels);
                    } else {
                        let ids: Vec<i64> = live.iter().map(|(id, _)| *id).collect();
                        mirror_change(self.store, self.account, &ids, change);
                    }
                }
                // Gmail can have applied the call: read, never call again.
                Err(e) if e.write_result_unknown() => return self.recover(ledger),
                // A clear refusal, or a second 401: no later call.
                Err(e) => return self.end(ledger, Some(e.to_string())),
            }
        }
        self.end(ledger, None)
    }

    /// Step 5: `applied` when no message is `unknown`, else `failed`. The
    /// trigger ledger_state moves the action to the same state.
    fn end(&mut self, ledger: i64, error: Option<String>) -> Result<Outcome> {
        let (call, unknown): (i64, bool) = self.store.read(|c| {
            Ok(c.query_row(
                "SELECT call, EXISTS (SELECT 1 FROM ledger_message
                                      WHERE ledger_id = l.id AND listed = 'unknown')
                 FROM ledger l WHERE id = ?1",
                [ledger],
                |r| Ok((r.get(0)?, r.get(1)?)),
            )?)
        })?;
        // The history ID is a note for the person: a failed read leaves it out.
        let history = if call > 0 {
            self.call(|g, t| g.profile(t))
                .ok()
                .map(|p| p.history_id.to_string())
        } else {
            None
        };
        let error = error.or_else(|| {
            unknown.then(|| {
                "Iris cannot confirm the change of one or more messages, and does not call \
                 Gmail again for them."
                    .to_string()
            })
        });
        let state = if error.is_some() { "failed" } else { "applied" };
        self.store.write(|tx| {
            Ok(tx.execute(
                "UPDATE ledger SET state = ?2, error = ?3, history_id = ?4
                 WHERE id = ?1 AND state = 'writing'",
                params![ledger, state, error, history],
            )?)
        })?;
        let counts = self.counts(ledger)?;
        Ok(match error {
            None => Outcome::Applied(counts),
            Some(error) => Outcome::Failed { counts, error },
        })
    }

    /// The counts of step 2 with the messages of the ledger row. Only the
    /// listed messages come back as IDs.
    fn counts(&self, ledger: i64) -> Result<Counts> {
        let mut counts = self.counts.clone();
        let (changed, skipped): (i64, i64) = self.store.read(|c| {
            Ok(c.query_row(
                "SELECT count(*) FILTER (WHERE done = 1),
                        count(*) FILTER (WHERE listed = 'skipped')
                 FROM ledger_message WHERE ledger_id = ?1",
                [ledger],
                |r| Ok((r.get(0)?, r.get(1)?)),
            )?)
        })?;
        counts.changed += changed as u64;
        counts.unchanged += skipped as u64;
        let listed: Vec<(i64, String)> = self.store.read(|c| {
            let mut st = c.prepare(
                "SELECT message_id, listed FROM ledger_message
                 WHERE ledger_id = ?1 AND listed IN ('gone', 'unknown') ORDER BY 1",
            )?;
            let rows = st.query_map([ledger], |r| Ok((r.get(0)?, r.get(1)?)))?;
            Ok(rows.collect::<rusqlite::Result<_>>()?)
        })?;
        for (id, why) in listed {
            if why == "gone" {
                counts.gone.push(id);
            } else {
                counts.unknown.push(id);
            }
        }
        counts.gone.sort_unstable();
        Ok(counts)
    }

    /// The recovery of section 7.3 for one row in `writing`. Each message
    /// whose call started is read: `done` if it has the `post` state, else
    /// `unknown`, and no call is made again for it. Each message whose call
    /// never started is read again for its `pre`. Then the calls that never
    /// started continue.
    fn recover(&mut self, ledger: i64) -> Result<Outcome> {
        let (call, change) = self.store.read(|c| {
            let call: i64 =
                c.query_row("SELECT call FROM ledger WHERE id = ?1", [ledger], |r| {
                    r.get(0)
                })?;
            Ok((call, Change::of_ledger(c, ledger)?))
        })?;
        let mut after = 0;
        loop {
            let page: Vec<(i64, Option<String>, i64)> = self.store.read(|c| {
                let mut st = c.prepare_cached(
                    "SELECT lm.message_id, m.provider_id, lm.call FROM ledger_message lm
                     LEFT JOIN message m ON m.id = lm.message_id AND m.gone_at IS NULL
                     WHERE lm.ledger_id = ?1 AND lm.done = 0 AND lm.listed IS NULL
                       AND lm.message_id > ?2
                     ORDER BY 1 LIMIT ?3",
                )?;
                let rows = st.query_map(params![ledger, after, BATCH_LIMIT as i64], |r| {
                    Ok((r.get(0)?, r.get(1)?, r.get(2)?))
                })?;
                Ok(rows.collect::<rusqlite::Result<_>>()?)
            })?;
            let Some(&(last, _, _)) = page.last() else {
                break;
            };
            after = last;
            let mut found: Vec<(i64, Found)> = Vec::new();
            let mut seen: Vec<(i64, Vec<String>)> = Vec::new();
            let mut failure = None;
            for (id, provider_id, message_call) in page {
                let started = message_call <= call;
                let Some(provider_id) = provider_id else {
                    found.push((id, Found::Listed("gone")));
                    continue;
                };
                match self.call(|g, t| g.minimal(t, &provider_id)) {
                    Ok(m) => {
                        let post = !change.needed(&m.label_ids);
                        found.push((
                            id,
                            match (started, post) {
                                (true, true) => Found::Done,
                                (true, false) => Found::Listed("unknown"),
                                (false, true) => Found::Listed("skipped"),
                                (false, false) => Found::Pre(change.pre(&m.label_ids)),
                            },
                        ));
                        seen.push((id, m.label_ids));
                    }
                    Err(iris_gmail::Error::NotFound) => found.push((id, Found::Listed("gone"))),
                    Err(e) => {
                        failure = Some(e);
                        break;
                    }
                }
            }
            // Keep what the reads confirmed, also before a read that failed.
            self.store.write(|tx| {
                for (id, f) in &found {
                    match f {
                        Found::Done => {
                            tx.execute(
                                "UPDATE ledger_message SET done = 1
                                 WHERE ledger_id = ?1 AND message_id = ?2",
                                params![ledger, id],
                            )?;
                        }
                        Found::Listed(why) => set_listed(tx, ledger, *id, why)?,
                        Found::Pre(pre) => {
                            tx.execute(
                                "UPDATE ledger_message SET pre = ?3
                                 WHERE ledger_id = ?1 AND message_id = ?2",
                                params![ledger, id, pre],
                            )?;
                        }
                    }
                }
                Ok(())
            })?;
            mirror(self.store, self.account, &seen);
            if let Some(e) = failure {
                return Err(Error::Gmail(e));
            }
        }
        self.calls(ledger, &change)
    }

    /// The recovery of each row of the account in `writing`.
    fn recover_account(&mut self) -> Result<()> {
        let account = self.account;
        let rows: Vec<i64> = self.store.read(|c| {
            let mut st = c.prepare(
                "SELECT id FROM ledger WHERE state = 'writing' AND account_id = ?1 ORDER BY id",
            )?;
            let rows = st.query_map([account], |r| r.get(0))?;
            Ok(rows.collect::<rusqlite::Result<_>>()?)
        })?;
        for ledger in rows {
            self.recover(ledger)?;
        }
        Ok(())
    }
}

impl Run<'_, '_> {
    /// Make the user label `name` at Gmail, and save it in the store.
    /// Gives its ID. After each error (for example HTTP 409: a label with
    /// the name exists now, or an unknown result), the apply reads the
    /// labels: the user label of the name is the label to use. Else the
    /// action stays as it was, and no message changed (section 7.2, step 1).
    fn create_label(&mut self, name: &str) -> Result<std::result::Result<String, String>> {
        let label = match self.call(|g, t| g.create_label(t, name)) {
            Ok(l) => l,
            Err(e) => {
                let labels = self.call(|g, t| g.labels(t)).map_err(Error::Gmail)?;
                match find_label(&labels, name) {
                    Ok(Some(l)) => l.clone(),
                    _ => {
                        return Ok(Err(format!(
                            "Gmail did not make the label, and has no user label of the name: {e}"
                        )))
                    }
                }
            }
        };
        // A store error loses only the mirror rows: the next sync writes them.
        let _ = self
            .store
            .put_labels(self.account, &[(&label.id, &label.name, "user")]);
        Ok(Ok(label.id))
    }

    /// End an action as `failed` with a ledger entry of no message, after
    /// a write call before its claim (the label create).
    fn fail_unclaimed(&mut self, action: i64, change: &Change, error: String) -> Result<Outcome> {
        let account = self.account;
        let ok = self.store.write(|tx| {
            let n = tx.execute(
                "INSERT INTO ledger(action_id, account_id, ops, state, undo, at)
                 SELECT ?1, ?2, ?3, 'writing', 'inverse', ?4
                 WHERE EXISTS (SELECT 1 FROM changeset_action WHERE id = ?1 AND state = 'approved')",
                params![action, account, change.ops(0), now()],
            )?;
            if n == 1 {
                tx.execute(
                    "UPDATE ledger SET state = 'failed', error = ?2 WHERE id = ?1",
                    params![tx.last_insert_rowid(), error],
                )?;
            }
            Ok(n == 1)
        })?;
        if !ok {
            return Err(Error::NotApproved(action));
        }
        Ok(Outcome::Failed {
            counts: self.counts.clone(),
            error,
        })
    }
}

fn set_listed(tx: &Connection, ledger: i64, message: i64, why: &str) -> iris_store::Result<()> {
    tx.execute(
        "UPDATE ledger_message SET listed = ?3 WHERE ledger_id = ?1 AND message_id = ?2",
        params![ledger, message, why],
    )?;
    Ok(())
}

/// Milliseconds since the Unix epoch.
fn now() -> i64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map_or(0, |d| d.as_millis() as i64)
}

/// A JSON array of label IDs, each as a JSON string.
fn labels_json<S: AsRef<str>>(labels: impl IntoIterator<Item = S>) -> String {
    let quoted: Vec<String> = labels
        .into_iter()
        .map(|l| json_string(l.as_ref()))
        .collect();
    format!("[{}]", quoted.join(","))
}

/// A JSON string: Gmail gives the user label IDs, so the text is escaped.
fn json_string(text: &str) -> String {
    let mut out = String::with_capacity(text.len() + 2);
    out.push('"');
    for ch in text.chars() {
        match ch {
            '"' => out.push_str("\\\""),
            '\\' => out.push_str("\\\\"),
            c if (c as u32) < 0x20 => out.push_str(&format!("\\u{:04x}", c as u32)),
            c => out.push(c),
        }
    }
    out.push('"');
    out
}

/// True if the account has a ledger row in `writing`.
fn writing_rows(store: &Store, account: i64) -> Result<bool> {
    Ok(store.read(|c| {
        Ok(c.query_row(
            "SELECT EXISTS (SELECT 1 FROM ledger WHERE state = 'writing' AND account_id = ?1)",
            [account],
            |r| r.get(0),
        )?)
    })?)
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
         WHERE id = ?1 AND state = 'approved'
           AND NOT EXISTS (SELECT 1 FROM ledger WHERE action_id = ?1 AND state = 'writing')
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
    // The hash of submit (iris-changeset), from the rows of now.
    let now_hash = store.read(|c| {
        iris_changeset::hash(c, action).map_err(|e| match e {
            iris_changeset::Error::Store(e) => e,
            other => iris_store::Error::Query(other.to_string()),
        })
    })?;
    let reason = if now_hash.1 != approved_hash.unwrap_or_default() {
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

/// Write the change of a `batchModify` call to the mirror: the labels of
/// step 2 (or of the recovery) are in the mirror, so only the labels of the
/// change differ. As [`mirror`], a store error loses only the mirror write.
fn mirror_change(store: &Store, account: i64, messages: &[i64], change: &Change) {
    let _ = store.write(|tx| {
        let mut remove = tx.prepare_cached(
            "DELETE FROM message_label WHERE message_id = ?1 AND label_id =
               (SELECT id FROM label WHERE account_id = ?2 AND provider_id = ?3)",
        )?;
        let mut add = tx.prepare_cached(
            "INSERT OR IGNORE INTO message_label(message_id, label_id)
             SELECT ?1, id FROM label WHERE account_id = ?2 AND provider_id = ?3",
        )?;
        for id in messages {
            for l in &change.remove {
                remove.execute(params![id, account, l])?;
            }
            for l in &change.add {
                add.execute(params![id, account, l])?;
            }
        }
        Ok(())
    });
}

/// A decision of the engine on an approved action: `denied` or `in_review`.
fn decide(store: &Store, action: i64, state: &str, reason: &str) -> Result<()> {
    let now = now();
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
