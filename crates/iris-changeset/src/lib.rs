//! The changeset (specification 2, section 5): the data format, the store
//! rows and the states of each action.
//!
//! An author makes a draft changeset ([`create`]), adds actions
//! ([`add_action`]) and submits it ([`submit`]). Submit resolves each target
//! to message IDs from the mirror, adds the flags `bulk` and `first_contact`,
//! and gives each action the state `proposed`, or `denied` with a reason.
//! The policy and the review move the action on with [`decide`] and
//! [`approve`]. A person approves an action only with its current hash.
//!
//! The SQL trigger `action_state` (migration 7) allows only the moves of
//! section 5.2, so no caller can make a move that the specification does not
//! allow. This crate has no provider client: it changes no mailbox
//! (requirement C1).
//!
//! Memory: the message IDs of an action are `action_message` rows. Submit
//! resolves them with `INSERT ... SELECT`, and [`write_document`] streams
//! them, so no list of message IDs is in memory (section 9).

use std::fmt;
use std::io::Write;

use iris_store::{format_id, parse_id, Kind, Store};
use rusqlite::{params, Connection, OptionalExtension};
use serde_json::{Map, Value};
use sha2::{Digest, Sha256};

/// The flag `bulk`: more than this count of messages in one action, or in
/// one revision (section 5.3).
pub const BULK: i64 = 500;

#[derive(Debug)]
pub enum Error {
    Store(iris_store::Error),
    /// No changeset or no action has this ID.
    NotFound,
    /// The request does not agree with the state of the changeset.
    Refused(String),
}

impl fmt::Display for Error {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Error::Store(e) => write!(f, "{e}"),
            Error::NotFound => write!(f, "no changeset or action has this ID"),
            Error::Refused(why) => write!(f, "refused: {why}"),
        }
    }
}

impl std::error::Error for Error {}

impl From<iris_store::Error> for Error {
    fn from(e: iris_store::Error) -> Self {
        Error::Store(e)
    }
}

impl From<rusqlite::Error> for Error {
    fn from(e: rusqlite::Error) -> Self {
        // The triggers of migration 7 refuse a move that section 5.2 does
        // not allow.
        if let rusqlite::Error::SqliteFailure(f, Some(msg)) = &e {
            if f.extended_code == rusqlite::ffi::SQLITE_CONSTRAINT_TRIGGER {
                return Error::Refused(msg.clone());
            }
        }
        Error::Store(e.into())
    }
}

impl From<std::io::Error> for Error {
    fn from(e: std::io::Error) -> Self {
        Error::Store(e.into())
    }
}

pub type Result<T> = std::result::Result<T, Error>;

macro_rules! names {
    ($(#[$m:meta])* $ty:ident { $($v:ident = $s:literal),* $(,)? }) => {
        $(#[$m])*
        #[derive(Clone, Copy, Debug, PartialEq, Eq)]
        pub enum $ty { $($v),* }

        impl $ty {
            pub const ALL: &'static [$ty] = &[$($ty::$v),*];

            pub fn as_str(self) -> &'static str {
                match self { $($ty::$v => $s),* }
            }

            pub fn parse(text: &str) -> Option<$ty> {
                match text { $($s => Some($ty::$v),)* _ => None }
            }
        }
    };
}

names!(
    /// Who proposes a changeset (section 2).
    AuthorKind { Person = "person", Agent = "agent", Triage = "triage", Local = "local" }
);

names!(
    /// The verbs of release 2 (section 4).
    Verb {
        Archive = "archive", Inbox = "inbox", Label = "label", Unlabel = "unlabel",
        Read = "read", Unread = "unread", Trash = "trash", Untrash = "untrash", Close = "close",
        Defer = "defer", Decline = "decline", Correct = "correct",
    }
);

names!(
    /// The state of an action (section 5.2).
    State {
        Proposed = "proposed", InReview = "in_review", Approved = "approved",
        Held = "held", Applied = "applied", Failed = "failed", Denied = "denied",
        Expired = "expired", Cancelled = "cancelled", Replaced = "replaced",
        Undone = "undone",
    }
);

impl State {
    /// `proposed`, `in_review`, `approved` and `held` are open. Each other
    /// state is decided.
    pub fn is_open(self) -> bool {
        matches!(
            self,
            State::Proposed | State::InReview | State::Approved | State::Held
        )
    }
}

/// Who decided an action.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum By {
    /// An `allow` or `deny` rule of the active policy: its ID, and the
    /// confirmed policy version. A rule can change and keep its ID, so the two
    /// together name one rule text. The policy of the first start has no rule.
    Rule { id: i64, policy: i64 },
    /// The person, in the web client.
    Person,
    /// The policy with no rule: its default, or the mode.
    Policy,
    /// A hard limit (section 6.5).
    Limit,
    /// The engine: a target or an argument that is not valid, or a check
    /// before the apply.
    Engine,
    /// The author withdrew the changeset.
    Author,
    /// 14 days with no decision or no apply.
    Expiry,
}

impl By {
    /// The column values: `decided_by`, `rule` and `policy`.
    fn parts(self) -> (&'static str, Option<i64>, Option<i64>) {
        match self {
            By::Rule { id, policy } => ("rule", Some(id), Some(policy)),
            By::Person => ("person", None, None),
            By::Policy => ("policy", None, None),
            By::Limit => ("limit", None, None),
            By::Engine => ("engine", None, None),
            By::Author => ("author", None, None),
            By::Expiry => ("expiry", None, None),
        }
    }
}

/// One action: the changeset, the revision and the action number.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct ActionId {
    pub changeset: i64,
    pub revision: i64,
    pub n: i64,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Author {
    pub kind: AuthorKind,
    pub name: Option<String>,
    pub session: Option<String>,
    pub job: Option<String>,
}

/// `cs-` and the row ID.
pub fn format_changeset_id(row: i64) -> String {
    format!("cs-{row}")
}

/// Parse `cs-42`. Gives `None` for any other text.
pub fn parse_changeset_id(text: &str) -> Option<i64> {
    let n = text.strip_prefix("cs-")?;
    if n.is_empty() || !n.bytes().all(|b| b.is_ascii_digit()) {
        return None;
    }
    n.parse().ok().filter(|row| *row > 0)
}

/// Run `f` in a write transaction. A refusal after a write in `f` also rolls
/// back, because the transaction commits only when `f` gives `Ok`.
fn write<T>(store: &Store, f: impl FnOnce(&Connection) -> Result<T>) -> Result<T> {
    let mut out = None;
    let r = store.write(|tx| {
        match f(tx) {
            Ok(v) => out = Some(Ok(v)),
            Err(Error::Store(e)) => return Err(e),
            Err(e) => {
                out = Some(Err(e));
                // An Err rolls the transaction back. The caller gets `e`
                // from `out`, not this placeholder.
                return Err(iris_store::Error::Query(String::new()));
            }
        }
        Ok(())
    });
    finish(r, out)
}

/// The result of [`write`]: the error of `f`, else the error of the commit,
/// else the value of `f`.
fn finish<T>(r: iris_store::Result<()>, out: Option<Result<T>>) -> Result<T> {
    match (r, out) {
        (_, Some(Err(e))) => Err(e),
        (Err(e), _) => Err(Error::Store(e)),
        (Ok(()), Some(Ok(v))) => Ok(v),
        (Ok(()), None) => unreachable!("f ran"),
    }
}

/// Make a draft changeset. Gives its row ID.
pub fn create(store: &Store, author: &Author, now_ms: i64) -> Result<i64> {
    write(store, |c| {
        c.execute(
            "INSERT INTO changeset(author_kind, author_name, author_session, author_job, created_at)
             VALUES (?1, ?2, ?3, ?4, ?5)",
            params![author.kind.as_str(), author.name, author.session, author.job, now_ms],
        )?;
        Ok(c.last_insert_rowid())
    })
}

/// Add an action to the draft revision of a changeset. `targets` are short
/// IDs of one kind: threads, messages, or items. Gives the action number.
/// Submit checks the targets and the arguments.
pub fn add_action(
    store: &Store,
    changeset: i64,
    author: &Author,
    verb: Verb,
    targets: &[&str],
    args: &Map<String, Value>,
    why: Option<&str>,
) -> Result<i64> {
    let targets = targets_json(targets)?;
    write(store, |c| {
        check_author(c, changeset, author)?;
        let revision = draft_revision(c, changeset)?;
        let n: i64 = c.query_row(
            "SELECT coalesce(max(n), 0) + 1 FROM changeset_action
             WHERE changeset_id = ?1 AND revision = ?2",
            params![changeset, revision],
            |r| r.get(0),
        )?;
        c.execute(
            "INSERT INTO changeset_action(changeset_id, revision, n, verb, targets, args, why)
             VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7)",
            params![
                changeset,
                revision,
                n,
                verb.as_str(),
                targets,
                Value::Object(args.clone()).to_string(),
                why
            ],
        )?;
        Ok(n)
    })
}

/// The targets as JSON: `{"threads": [...]}`, `{"messages": [...]}` or
/// `{"items": [...]}`.
fn targets_json(targets: &[&str]) -> Result<String> {
    let mut group = None;
    let mut first = None;
    let mut ids = Vec::with_capacity(targets.len());
    for t in targets {
        let (kind, row) = parse_id(t).ok_or_else(|| refused(format!("{t:?} is not an ID")))?;
        let g = match kind {
            Kind::Thread => "threads",
            Kind::Message => "messages",
            Kind::Sender => return Err(refused(format!("{t} is a sender, not a target"))),
            _ => "items",
        };
        if first.is_some_and(|k| k != kind) {
            return Err(refused("the targets of one action are of one kind".into()));
        }
        first = Some(kind);
        group = Some(g);
        ids.push(Value::String(format_id(kind, row)));
    }
    let g = group.ok_or_else(|| refused("an action needs a target".into()))?;
    Ok(Value::Object(Map::from_iter([(g.to_string(), Value::Array(ids))])).to_string())
}

fn refused(why: String) -> Error {
    Error::Refused(why)
}

/// The revision that takes new actions: the newest, if it is not submitted.
fn draft_revision(c: &Connection, changeset: i64) -> Result<i64> {
    let row: Option<(i64, Option<i64>, String)> = c
        .query_row(
            "SELECT revision, submitted_at, state FROM changeset WHERE id = ?1",
            [changeset],
            |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?)),
        )
        .optional()?;
    match row {
        None => Err(Error::NotFound),
        Some((_, _, state)) if state == "withdrawn" => {
            Err(refused("the changeset is withdrawn".into()))
        }
        Some((_, Some(_), _)) => Err(refused(
            "the revision is submitted: make a new revision first".into(),
        )),
        Some((revision, None, _)) => Ok(revision),
    }
}

/// Start a new draft revision of a submitted changeset, after a review
/// comment. Only the author can do this. The actions of the old revision
/// that are still `proposed` or `in_review` become `replaced` when the new
/// revision is submitted.
pub fn revise(store: &Store, changeset: i64, author: &Author) -> Result<i64> {
    write(store, |c| {
        check_author(c, changeset, author)?;
        let (state, submitted): (String, Option<i64>) = c.query_row(
            "SELECT state, submitted_at FROM changeset WHERE id = ?1",
            [changeset],
            |r| Ok((r.get(0)?, r.get(1)?)),
        )?;
        if state == "withdrawn" {
            return Err(refused("the changeset is withdrawn".into()));
        }
        if submitted.is_none() {
            return Err(refused("the newest revision is not submitted".into()));
        }
        c.execute(
            "UPDATE changeset SET revision = revision + 1, submitted_at = NULL WHERE id = ?1",
            [changeset],
        )?;
        Ok(c.query_row(
            "SELECT revision FROM changeset WHERE id = ?1",
            [changeset],
            |r| r.get(0),
        )?)
    })
}

/// The same `kind`, `name` and `session` (section 5.5).
fn check_author(c: &Connection, changeset: i64, author: &Author) -> Result<()> {
    let same: Option<bool> = c
        .query_row(
            "SELECT author_kind = ?2 AND author_name IS ?3 AND author_session IS ?4
             FROM changeset WHERE id = ?1",
            params![changeset, author.kind.as_str(), author.name, author.session],
            |r| r.get(0),
        )
        .optional()?;
    match same {
        None => Err(Error::NotFound),
        Some(false) => Err(refused("only the author can do this".into())),
        Some(true) => Ok(()),
    }
}

/// Submit the draft revision with the reason of the author (section 5.4,
/// steps 2 to 4). Each action gets `proposed`, or `denied` with the reason.
/// The policy (step 5) decides the `proposed` actions next.
///
/// The store does the state moves of the submit (migration 7): the update
/// of the changeset proposes each draft action that has its hash, and
/// replaces the open actions of older revisions. This function resolves and
/// hashes each action first, and denies the actions that are not valid
/// after.
pub fn submit(
    store: &Store,
    changeset: i64,
    author: &Author,
    why: &str,
    now_ms: i64,
) -> Result<()> {
    if why.trim().is_empty() {
        return Err(refused("a changeset needs a reason".into()));
    }
    write(store, |c| {
        check_author(c, changeset, author)?;
        let revision = draft_revision(c, changeset)?;
        let any: bool = c.query_row(
            "SELECT EXISTS (SELECT 1 FROM changeset_action WHERE changeset_id = ?1 AND revision = ?2)",
            params![changeset, revision],
            |r| r.get(0),
        )?;
        if !any {
            return Err(refused("the revision has no action".into()));
        }
        // Pass 1: check and resolve each action, one at a time. An action
        // that is not valid keeps no message row, and its reason waits here.
        // shortcut: one reason string for each denied action is in memory
        // until the end of the submit; stream the reasons if an author can
        // send a very large number of actions.
        let mut denials = Vec::new();
        let mut last = 0;
        while let Some((id, n, verb, targets, args)) = next_action(c, changeset, revision, last)? {
            last = n;
            let verb = Verb::parse(&verb).expect("the CHECK of the table");
            // Only the targets give the messages: drop rows of another writer.
            c.execute("DELETE FROM action_message WHERE action_id = ?1", [id])?;
            let denial = match supported(verb, &args) {
                Err(why) => Some(why),
                Ok(()) => resolve(c, id, &targets)?,
            };
            if let Some(why) = denial {
                c.execute("DELETE FROM action_message WHERE action_id = ?1", [id])?;
                denials.push((id, why));
            }
        }
        // Pass 2: the hash and the flags of each action. The flags need the
        // count of the changeset (section 5.3): the valid actions of this
        // revision (a denied one has no message row), and the approved, held
        // and applied actions of older revisions. The submit replaces the
        // older proposed and in_review actions, so they do not count. Each
        // action counts its own
        // messages: ten actions on the same 100 messages count 1,000
        // (section 10).
        let total: i64 = c.query_row(
            "SELECT count(*) FROM action_message m
             JOIN changeset_action a ON a.id = m.action_id
             WHERE a.changeset_id = ?1
               AND (a.state IS NULL
                    OR a.state IN ('approved', 'held', 'applied'))
               AND a.verb NOT IN ('close', 'defer', 'decline', 'correct')",
            [changeset],
            |r| r.get(0),
        )?;
        let mut last = 0;
        while let Some((id, n, ..)) = next_action(c, changeset, revision, last)? {
            last = n;
            let (count, hash) = hash(c, id)?;
            let valid = count > 0;
            let mut flags = Vec::new();
            if valid && (count > BULK || total > BULK) {
                flags.push("bulk");
            }
            if valid && first_contact(c, id)? {
                flags.push("first_contact");
            }
            c.execute(
                "UPDATE changeset_action SET hash = ?2, flags = ?3 WHERE id = ?1",
                params![id, hash, serde_json::to_string(&flags).expect("strings")],
            )?;
        }
        // The submit: the store proposes each action of the revision.
        c.execute(
            "UPDATE changeset SET why = ?2, submitted_at = ?3 WHERE id = ?1",
            params![changeset, why, now_ms],
        )?;
        for (id, why) in denials {
            c.execute(
                "UPDATE changeset_action SET state = 'denied', decided_by = 'engine',
                   reason = ?2, decided_at = ?3
                 WHERE id = ?1",
                params![id, why, now_ms],
            )?;
        }
        Ok(())
    })
}

type ActionRow = (i64, i64, String, String, String);

/// The draft action after number `last` in a revision: ID, number, verb,
/// targets and arguments.
fn next_action(
    c: &Connection,
    changeset: i64,
    revision: i64,
    last: i64,
) -> Result<Option<ActionRow>> {
    Ok(c.prepare_cached(
        "SELECT id, n, verb, targets, args FROM changeset_action
         WHERE changeset_id = ?1 AND revision = ?2 AND n > ?3 AND state IS NULL
         ORDER BY n LIMIT 1",
    )?
    .query_row(params![changeset, revision, last], |r| {
        Ok((r.get(0)?, r.get(1)?, r.get(2)?, r.get(3)?, r.get(4)?))
    })
    .optional()?)
}

/// The flag `first_contact` (section 5.3): a thread of a resolved message has
/// mail from a sender of the kind `stranger`.
fn first_contact(c: &Connection, action: i64) -> Result<bool> {
    Ok(c.query_row(
        "SELECT EXISTS (
           SELECT 1 FROM action_message am
           JOIN message m ON m.id = am.message_id
           JOIN message t ON t.thread_id = m.thread_id
           JOIN header h ON h.message_id = t.id
           JOIN sender p ON p.address = h.from_addr
           WHERE am.action_id = ?1 AND p.kind = 'stranger')",
        [action],
        |r| r.get(0),
    )?)
}

/// Part 1 of R2-01 submits the mailbox verbs with no arguments. Gives the
/// reason of the denial for each other action.
// shortcut: label, unlabel and the state verbs are denied; part 2 of R2-01
// adds their argument checks and item targets.
fn supported(verb: Verb, args: &str) -> std::result::Result<(), String> {
    match verb {
        Verb::Archive | Verb::Inbox | Verb::Read | Verb::Unread | Verb::Trash => {
            // Another writer can store an empty object as other text.
            let empty =
                serde_json::from_str::<Map<String, Value>>(args).is_ok_and(|a| a.is_empty());
            if empty {
                Ok(())
            } else {
                Err(format!("{} takes no arguments", verb.as_str()))
            }
        }
        // The undo of a trash (R2-03, section 7.4) makes its own changeset.
        Verb::Untrash => Err("only the undo of a trash makes untrash".into()),
        _ => Err(format!("{} is not available yet", verb.as_str())),
    }
}

/// Resolve the targets of an action to `action_message` rows (section 4). A
/// thread gives each of its messages that is not gone and has no `SPAM` or
/// `TRASH`. Gives the reason of the denial, or `None`.
fn resolve(c: &Connection, action: i64, targets: &str) -> Result<Option<String>> {
    // add_action writes one group of IDs. Another writer of the table may not.
    let bad = || Ok(Some("the targets are not valid".to_string()));
    let Ok(targets) = serde_json::from_str::<Map<String, Value>>(targets) else {
        return bad();
    };
    let (Some((group, Value::Array(ids))), 1) = (targets.iter().next(), targets.len()) else {
        return bad();
    };
    if group != "threads" && group != "messages" {
        return Ok(Some("a mailbox verb takes threads or messages".into()));
    }
    for id in ids {
        let Some((kind, row)) = id.as_str().and_then(parse_id) else {
            return bad();
        };
        let found: bool = match (kind, group.as_str()) {
            (Kind::Thread, "threads") => {
                c.execute(
                    "INSERT OR IGNORE INTO action_message(action_id, message_id)
                     SELECT ?1, m.id FROM message m WHERE m.thread_id = ?2 AND m.gone_at IS NULL
                       AND NOT EXISTS (SELECT 1 FROM message_label ml
                         JOIN label l ON l.id = ml.label_id
                         WHERE ml.message_id = m.id AND l.provider_id IN ('SPAM', 'TRASH'))",
                    params![action, row],
                )?;
                c.query_row(
                    "SELECT EXISTS (SELECT 1 FROM thread WHERE id = ?1)",
                    [row],
                    |r| r.get(0),
                )?
            }
            (Kind::Message, "messages") => {
                c.execute(
                    "INSERT OR IGNORE INTO action_message(action_id, message_id)
                     SELECT ?1, id FROM message WHERE id = ?2 AND gone_at IS NULL",
                    params![action, row],
                )? == 1
                    || c.query_row(
                        "SELECT EXISTS (SELECT 1 FROM action_message
                         WHERE action_id = ?1 AND message_id = ?2)",
                        params![action, row],
                        |r| r.get(0),
                    )?
            }
            _ => return bad(),
        };
        if !found {
            let id = format_id(kind, row);
            return Ok(Some(format!("{id} is not in the mirror")));
        }
    }
    // No target, or a thread with each message gone or in SPAM or TRASH.
    let none: bool = c.query_row(
        "SELECT NOT EXISTS (SELECT 1 FROM action_message WHERE action_id = ?1)",
        [action],
        |r| r.get(0),
    )?;
    if none {
        return Ok(Some("the targets have no message to change".into()));
    }
    // One action changes one mailbox: the apply and the undo use one account.
    let accounts: i64 = c.query_row(
        "SELECT count(DISTINCT m.account_id) FROM action_message am
         JOIN message m ON m.id = am.message_id WHERE am.action_id = ?1",
        [action],
        |r| r.get(0),
    )?;
    if accounts > 1 {
        return Ok(Some("the targets are in two or more accounts".into()));
    }
    Ok(None)
}

/// The count of resolved messages, and the SHA-256 (hex) of the verb, the
/// targets, the arguments and the resolved message IDs that a review shows
/// (section 5.5).
fn hash(c: &Connection, action: i64) -> Result<(i64, String)> {
    let (verb, targets, args): (String, String, String) = c.query_row(
        "SELECT verb, targets, args FROM changeset_action WHERE id = ?1",
        [action],
        |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?)),
    )?;
    // Each text has its length first, so no two inputs give the same bytes.
    let mut h = Sha256::new();
    for text in [&verb, &targets, &args] {
        h.update((text.len() as u64).to_be_bytes());
        h.update(text.as_bytes());
    }
    let mut st =
        c.prepare("SELECT message_id FROM action_message WHERE action_id = ?1 ORDER BY 1")?;
    let mut rows = st.query([action])?;
    let mut count = 0;
    while let Some(r) = rows.next()? {
        let id: i64 = r.get(0)?;
        h.update(format_id(Kind::Message, id).as_bytes());
        h.update(b",");
        count += 1;
    }
    let hex: String = h.finalize().iter().map(|b| format!("{b:02x}")).collect();
    Ok((count, hex))
}

/// Move an action to `to`. The trigger `action_state` refuses a move that
/// section 5.2 does not allow. A person approves only with [`approve`].
pub fn decide(
    store: &Store,
    action: ActionId,
    to: State,
    by: By,
    reason: Option<&str>,
    now_ms: i64,
) -> Result<()> {
    if to == State::Approved && by == By::Person {
        return Err(refused(
            "a person approves with the hash that the review showed".into(),
        ));
    }
    // The policy and the review move an action. Each other state has its
    // own path, with the check of its event: submit (replaced), withdraw
    // (cancelled), expiry (expired), the apply (R2-02) and the ledger (R2-03)
    // in the transaction of the ledger entry, a send (held, release 3).
    if !matches!(to, State::InReview | State::Approved | State::Denied) {
        return Err(refused(format!("decide does not set {}", to.as_str())));
    }
    if to == State::Denied && reason.is_none_or(|r| r.trim().is_empty()) {
        return Err(refused("a denial needs a reason".into()));
    }
    // Who decides each state (section 5.2). A person approves with
    // `approve`, and does not send an action to review.
    let allowed = match to {
        State::Approved => matches!(by, By::Rule { .. }),
        State::InReview => matches!(by, By::Rule { .. } | By::Policy | By::Limit | By::Engine),
        _ => !matches!(by, By::Author | By::Expiry),
    };
    if !allowed {
        return Err(refused(format!(
            "{} does not set {}",
            by.parts().0,
            to.as_str()
        )));
    }
    write(store, |c| {
        let state: Option<Option<String>> = c
            .query_row(
                "SELECT state FROM changeset_action WHERE changeset_id = ?1 AND revision = ?2 AND n = ?3",
                params![action.changeset, action.revision, action.n],
                |r| r.get(0),
            )
            .optional()?;
        match state {
            None => Err(Error::NotFound),
            Some(None) => Err(refused("the action is a draft: submit it first".into())),
            Some(Some(_)) => set_state(c, action, to, by, reason, now_ms),
        }
    })
}

/// The time from the submit of an action to its expiry (section 5.5), the
/// same as in the trigger `action_state` of migration 7.
const EXPIRY_MS: i64 = 14 * 24 * 60 * 60 * 1000;

fn set_state(
    c: &Connection,
    a: ActionId,
    to: State,
    by: By,
    reason: Option<&str>,
    now_ms: i64,
) -> Result<()> {
    // An open action expires 14 days after its submit (section 5.5). Both
    // decide and approve come here, so no decision lands after the deadline.
    let submitted: Option<i64> = c
        .query_row(
            "SELECT submitted_at FROM changeset_action
             WHERE changeset_id = ?1 AND revision = ?2 AND n = ?3",
            params![a.changeset, a.revision, a.n],
            |r| r.get(0),
        )
        .optional()?
        .ok_or(Error::NotFound)?;
    if submitted.is_some_and(|t| now_ms >= t + EXPIRY_MS) {
        return Err(refused(
            "the action expired 14 days after its submit".into(),
        ));
    }
    let (by, rule, policy) = by.parts();
    let changed = c.execute(
        "UPDATE changeset_action SET state = ?4, decided_by = ?5, rule = ?6, reason = ?7,
           decided_at = ?8, policy = ?9
         WHERE changeset_id = ?1 AND revision = ?2 AND n = ?3",
        params![
            a.changeset,
            a.revision,
            a.n,
            to.as_str(),
            by,
            rule,
            reason,
            now_ms,
            policy
        ],
    )?;
    if changed == 0 {
        return Err(Error::NotFound);
    }
    Ok(())
}

/// The approval of a person (section 5.5). It names the revision, the
/// action number, and the hash that the review showed. The engine refuses
/// it for an old revision or another hash.
pub fn approve(store: &Store, action: ActionId, hash: &str, now_ms: i64) -> Result<()> {
    let ActionId {
        changeset,
        revision,
        n,
    } = action;
    write(store, |c| {
        let row: Option<(i64, Option<i64>, Option<String>)> = c
            .query_row(
                "SELECT c.revision, c.submitted_at, a.hash FROM changeset c
                 JOIN changeset_action a ON a.changeset_id = c.id
                 WHERE c.id = ?1 AND a.revision = ?2 AND a.n = ?3",
                params![changeset, revision, n],
                |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?)),
            )
            .optional()?;
        let (_, submitted, seen) = row.ok_or(Error::NotFound)?;
        // The submit of a new revision replaces each older action with no
        // decision, and the trigger refuses to approve a replaced action. An
        // older action that stays open was approved, and can come back to
        // review (section 5.4): the person approves it with its hash.
        if submitted.is_none() {
            return Err(refused("a new revision is a draft".into()));
        }
        if seen.as_deref() != Some(hash) {
            return Err(refused("the action changed after the review".into()));
        }
        set_state(c, action, State::Approved, By::Person, None, now_ms)
    })
}

/// Write a changeset as JSON (section 5.1). The message IDs stream from the
/// store, so the document is never in memory. Each action names its
/// revision, because an old revision can keep decided and open actions.
pub fn write_document(store: &Store, changeset: i64, out: &mut impl Write) -> Result<()> {
    store
        .read(|c| {
            let tx = c.unchecked_transaction()?;
            Ok(document(&tx, changeset, out))
        })
        .map_err(Error::Store)?
}

fn document(c: &Connection, changeset: i64, out: &mut impl Write) -> Result<()> {
    type Head = (
        i64,
        String,
        Option<String>,
        Option<String>,
        Option<String>,
        Option<String>,
        i64,
        Option<i64>,
        String,
    );
    let head: Option<Head> = c
        .query_row(
            "SELECT revision, author_kind, author_name, author_session, author_job, why,
                    created_at, submitted_at, state
             FROM changeset WHERE id = ?1",
            [changeset],
            |r| {
                Ok((
                    r.get(0)?,
                    r.get(1)?,
                    r.get(2)?,
                    r.get(3)?,
                    r.get(4)?,
                    r.get(5)?,
                    r.get(6)?,
                    r.get(7)?,
                    r.get(8)?,
                ))
            },
        )
        .optional()?;
    let (revision, kind, name, session, job, why, created, submitted, state) =
        head.ok_or(Error::NotFound)?;
    let mut author = Map::new();
    author.insert("kind".into(), kind.into());
    for (k, v) in [("name", name), ("session", session), ("job", job)] {
        if let Some(v) = v {
            author.insert(k.into(), v.into());
        }
    }
    let time = |ms: Option<i64>| ms.map_or(Value::Null, |ms| iris_talk::rfc3339(ms).into());
    write!(
        out,
        r#"{{"model":"iris/0","id":"{}","revision":{revision},"author":{},"why":{},"created":{},"submitted":{},"state":"{state}","actions":["#,
        format_changeset_id(changeset),
        Value::Object(author),
        why.map_or(Value::Null, Value::String),
        time(Some(created)),
        time(submitted),
    )?;
    let mut st = c.prepare(
        "SELECT id, revision, n, verb, targets, args, why, flags, state, decided_by, rule,
                reason, decided_at, hash, policy
         FROM changeset_action WHERE changeset_id = ?1 ORDER BY revision, n",
    )?;
    let mut rows = st.query([changeset])?;
    let mut first = true;
    while let Some(r) = rows.next()? {
        if !first {
            out.write_all(b",")?;
        }
        first = false;
        let id: i64 = r.get(0)?;
        let state: Option<String> = r.get(8)?;
        let by: Option<String> = r.get(9)?;
        let rule: Option<i64> = r.get(10)?;
        let reason: Option<String> = r.get(11)?;
        let at: Option<i64> = r.get(12)?;
        let hash: Option<String> = r.get(13)?;
        let policy: Option<i64> = r.get(14)?;
        write!(
            out,
            r#"{{"n":{},"revision":{},"verb":"{}","targets":{},"resolved":{{"messages":["#,
            r.get::<_, i64>(2)?,
            r.get::<_, i64>(1)?,
            r.get::<_, String>(3)?,
            r.get::<_, String>(4)?,
        )?;
        let mut ids = c.prepare_cached(
            "SELECT message_id FROM action_message WHERE action_id = ?1 ORDER BY 1",
        )?;
        let mut m = ids.query([id])?;
        let mut first_m = true;
        while let Some(m) = m.next()? {
            if !first_m {
                out.write_all(b",")?;
            }
            first_m = false;
            write!(out, "\"{}\"", format_id(Kind::Message, m.get(0)?))?;
        }
        let decision = state.map_or(Value::Null, |s| {
            let mut d = Map::new();
            d.insert("state".into(), s.into());
            let fields: [(&str, Option<Value>); 5] = [
                ("by", by.map(Into::into)),
                ("rule", rule.map(Into::into)),
                ("policy", policy.map(Into::into)),
                ("reason", reason.map(Into::into)),
                ("at", at.map(|ms| time(Some(ms)))),
            ];
            for (k, v) in fields {
                if let Some(v) = v {
                    d.insert(k.into(), v);
                }
            }
            Value::Object(d)
        });
        write!(
            out,
            r#"]}},"args":{},"why":{},"flags":{},"hash":{},"decision":{}}}"#,
            r.get::<_, String>(5)?,
            r.get::<_, Option<String>>(6)?
                .map_or(Value::Null, Value::String),
            r.get::<_, String>(7)?,
            hash.map_or(Value::Null, Value::String),
            decision,
        )?;
    }
    out.write_all(b"]}")?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A commit that fails after `f` gave `Ok` is an error, not a success.
    #[test]
    fn a_failed_commit_is_an_error() {
        let failed = || Err(iris_store::Error::Query("commit".into()));
        assert!(matches!(
            finish(failed(), Some(Ok(1))),
            Err(Error::Store(_))
        ));
        assert!(matches!(
            finish::<i64>(failed(), Some(Err(Error::NotFound))),
            Err(Error::NotFound)
        ));
        assert!(matches!(finish(Ok(()), Some(Ok(1))), Ok(1)));
    }
}
