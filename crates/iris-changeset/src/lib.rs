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

use std::collections::HashSet;
use std::fmt;
use std::io::Write;

use iris_store::{format_id, parse_id, Kind, Store};
use rusqlite::{params, Connection, OptionalExtension};
use serde_json::{Map, Value};
use sha2::{Digest, Sha256};
use unicase::UniCase;

mod flags;
mod outbox;
mod written;
pub use flags::{
    action_content, attachment_spans, date_spans, draft_flags, draft_marks, has_attachment,
    has_date, has_money, money_spans, names, new_flags, words, Field, Mark, DRAFT_FLAGS,
};
pub use outbox::{cancel, cancel_draft, hold, send_of_draft, Cancel, CANCELLED};
pub use written::{
    last_mark, send_written, write_mail, written_since, Mail, Press, Refusal, Written, MARK_MS,
    NEW_WARNINGS, WRITTEN,
};

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
    /// An action of a job session is outside the grant of its job
    /// (docs/specs/2026-11-17-job-verbs.md). Nothing changed.
    Denied(String),
}

impl fmt::Display for Error {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Error::Store(e) => write!(f, "{e}"),
            Error::NotFound => write!(f, "no changeset or action has this ID"),
            Error::Refused(why) => write!(f, "refused: {why}"),
            Error::Denied(why) => write!(f, "denied: {why}"),
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
    /// The verbs of release 2 (section 4), and `draft` and `send` of
    /// release 3 (`docs/specs/2026-11-16-send.md`). Only an undo makes
    /// `untrash` and `restore` (section 7.4).
    Verb {
        Archive = "archive", Inbox = "inbox", Label = "label", Unlabel = "unlabel",
        Read = "read", Unread = "unread", Trash = "trash", Untrash = "untrash", Close = "close",
        Defer = "defer", Decline = "decline", Correct = "correct", Draft = "draft", Send = "send",
        Restore = "restore",
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

/// Run `f` in one write transaction, for example a create, its actions and
/// the submit with the `_in` functions. A refusal after a write in `f` also
/// rolls back, because the transaction commits only when `f` gives `Ok`.
pub fn transaction<T>(store: &Store, f: impl FnOnce(&Connection) -> Result<T>) -> Result<T> {
    write(store, f)
}

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
    write(store, |c| create_in(c, author, now_ms))
}

/// [`create`] in a write transaction of the caller, with the same checks.
/// Use [`transaction`] to run more than one step in one transaction.
pub fn create_in(c: &Connection, author: &Author, now_ms: i64) -> Result<i64> {
    if let Some(job) = &author.job {
        job_author(c, author, job)?;
        no_other_draft(c, job)?;
    }
    c.execute(
        "INSERT INTO changeset(author_kind, author_name, author_session, author_job, created_at)
             VALUES (?1, ?2, ?3, ?4, ?5)",
        params![
            author.kind.as_str(),
            author.name,
            author.session,
            author.job,
            now_ms
        ],
    )?;
    Ok(c.last_insert_rowid())
}

/// Add an action to the draft revision of a changeset. `targets` are short
/// IDs of one group: threads, messages, or items. Gives the action number.
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
    write(store, |c| {
        add_action_in(c, changeset, author, verb, targets, args, why)
    })
}

/// [`add_action`] in a write transaction of the caller, with the same checks.
/// Use [`transaction`] to run more than one step in one transaction.
pub fn add_action_in(
    c: &Connection,
    changeset: i64,
    author: &Author,
    verb: Verb,
    targets: &[&str],
    args: &Map<String, Value>,
    why: Option<&str>,
) -> Result<i64> {
    let targets = targets_json(targets)?;
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
}

/// The targets as JSON: `{"threads": [...]}`, `{"messages": [...]}`,
/// `{"items": [...]}`, `{"drafts": [...]}` or `{"accounts": [...]}`.
fn targets_json(targets: &[&str]) -> Result<String> {
    let mut group = None;
    let mut ids = Vec::with_capacity(targets.len());
    // One group for all targets, so the row ID names one target: the items
    // of all forms share one table (a `correct` can name `c-1` and `r-2`).
    let mut seen = HashSet::with_capacity(targets.len());
    for t in targets {
        let (kind, row) = parse_id(t).ok_or_else(|| refused(format!("{t:?} is not an ID")))?;
        let g = match kind {
            Kind::Thread => "threads",
            Kind::Message => "messages",
            Kind::Sender => return Err(refused(format!("{t} is a sender, not a target"))),
            Kind::Draft => "drafts",
            Kind::Account => "accounts",
            _ => "items",
        };
        if group.is_some_and(|k| k != g) {
            return Err(refused("the targets of one action are in one group".into()));
        }
        group = Some(g);
        if !seen.insert(row) {
            return Err(refused(format!("{t} is a target two times")));
        }
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
    write(store, |c| revise_in(c, changeset, author))
}

/// [`revise`] in a write transaction of the caller, with the same checks.
/// Use [`transaction`] to run more than one step in one transaction.
pub fn revise_in(c: &Connection, changeset: i64, author: &Author) -> Result<i64> {
    check_author(c, changeset, author)?;
    if let Some(job) = &author.job {
        no_other_draft(c, job)?;
    }
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
}

/// The same `kind`, `name` and `session`, or the same `kind` and `job`
/// for a job author (section 5.5).
/// Withdraw a changeset (section 5.5). `author` is the author of the
/// changeset (`iris withdraw`), or `None` for the person in the web client.
/// Each open action and each draft action becomes `cancelled` in the same
/// write (trigger `changeset_withdraw`), except an action whose provider
/// calls started: it ends as `applied` or `failed`. Applied actions stay.
pub fn withdraw(store: &Store, changeset: i64, author: Option<&Author>) -> Result<()> {
    write(store, |c| withdraw_in(c, changeset, author))
}

/// [`withdraw`] in a write transaction of the caller, with the same checks.
pub fn withdraw_in(c: &Connection, changeset: i64, author: Option<&Author>) -> Result<()> {
    match author {
        Some(author) => check_author(c, changeset, author)?,
        None => {
            c.query_row("SELECT 1 FROM changeset WHERE id = ?1", [changeset], |_| {
                Ok(())
            })
            .optional()?
            .ok_or(Error::NotFound)?;
        }
    }
    let changed = c.execute(
        "UPDATE changeset SET state = 'withdrawn' WHERE id = ?1 AND state <> 'withdrawn'",
        [changeset],
    )?;
    if changed == 0 {
        return Err(refused("the changeset is withdrawn".into()));
    }
    Ok(())
}

/// Expire each open action 14 days or more after its submit (section 5.5):
/// `proposed`, `in_review` and `approved`, by `expiry`. An action whose
/// provider calls started (a ledger row in `writing`) continues to its end.
/// An undo (migration 23) does not expire: the person approved it, and step
/// 2 of a `trash` undo must apply after a long stop (section 7.4). `held`
/// (release 3) does not expire. Gives the count of expired actions. The
/// engine runs it on each tick.
pub fn expire(store: &Store, now_ms: i64) -> Result<usize> {
    write(store, |c| expire_in(c, now_ms))
}

/// [`expire`] in a write transaction of the caller.
pub fn expire_in(c: &Connection, now_ms: i64) -> Result<usize> {
    Ok(c.execute(
        "UPDATE changeset_action SET state = 'expired', decided_by = 'expiry', rule = NULL,
           policy = NULL, reason = 'no decision or apply in 14 days', decided_at = ?1
         WHERE state IN ('proposed', 'in_review', 'approved', 'held')
           AND NOT EXISTS (SELECT 1 FROM ledger l
                           WHERE l.action_id = changeset_action.id AND l.state = 'writing')
           AND NOT EXISTS (SELECT 1 FROM ledger_undo u WHERE u.action_id = changeset_action.id)
           AND submitted_at <= ?1 - ?2",
        params![now_ms, EXPIRY_MS],
    )?)
}

/// Refuse each change by a job author that is not an agent session of a
/// job that is canonical, exists and is not `done` (issue #247): only an
/// agent session has a job, and a done job changes nothing.
fn job_author(c: &Connection, author: &Author, job: &str) -> Result<()> {
    if author.kind != AuthorKind::Agent || author.session.is_none() {
        return Err(refused("only an agent session has a job".into()));
    }
    let open: bool = match iris_job::parse_job_id(job) {
        Some(id) => c
            .query_row("SELECT state <> 'done' FROM job WHERE id = ?1", [id], |r| {
                r.get(0)
            })
            .optional()?
            .unwrap_or(false),
        None => false,
    };
    if !open {
        return Err(refused(format!("{job} is not a job that is open")));
    }
    Ok(())
}

/// The draft of a job: its changeset whose newest revision is not
/// submitted, and that is not withdrawn. A job has at most one
/// (docs/specs/2026-11-17-job-verbs.md, section 2).
pub fn job_draft(c: &Connection, job: &str) -> Result<Option<i64>> {
    Ok(c.query_row(
        "SELECT id FROM changeset
             WHERE author_job = ?1 AND submitted_at IS NULL AND state <> 'withdrawn'
             ORDER BY id DESC LIMIT 1",
        [job],
        |r| r.get(0),
    )
    .optional()?)
}

/// Refuse a second draft of a job.
fn no_other_draft(c: &Connection, job: &str) -> Result<()> {
    match job_draft(c, job)? {
        Some(cs) => Err(refused(format!(
            "{job} has the draft {}: submit or withdraw it first",
            format_changeset_id(cs)
        ))),
        None => Ok(()),
    }
}

fn check_author(c: &Connection, changeset: i64, author: &Author) -> Result<()> {
    let same: Option<bool> = c
        .query_row(
            // A changeset of a job belongs to the job: a later session of
            // the job continues it (docs/specs/2026-11-17-job-verbs.md).
            "SELECT author_kind = ?2 AND author_job IS ?5
                    AND (?5 IS NOT NULL OR (author_name IS ?3 AND author_session IS ?4))
             FROM changeset WHERE id = ?1",
            params![
                changeset,
                author.kind.as_str(),
                author.name,
                author.session,
                author.job
            ],
            |r| r.get(0),
        )
        .optional()?;
    match same {
        None => Err(Error::NotFound),
        Some(false) => Err(refused("only the author can do this".into())),
        Some(true) => match &author.job {
            Some(job) => job_author(c, author, job),
            None => Ok(()),
        },
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
    write(store, |c| submit_in(c, changeset, author, why, now_ms))
}

/// [`submit`] in a write transaction of the caller, with the same checks.
/// Use [`transaction`] to run more than one step in one transaction.
pub fn submit_in(
    c: &Connection,
    changeset: i64,
    author: &Author,
    why: &str,
    now_ms: i64,
) -> Result<()> {
    if why.trim().is_empty() {
        return Err(refused("a changeset needs a reason".into()));
    }
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
    // A job session: the grant of the job now. Each action must be in it,
    // else the whole submit fails and the caller rolls back (job verbs
    // spec). The check reads the rows of the targets, before any other
    // check can clear them, so no other denial hides a target outside the
    // grant.
    let job = changeset_job(c, changeset)?;
    let grant = job.map(|j| grant_of(c, j)).transpose()?;
    let mut last = 0;
    while let Some((id, n, verb, targets, args)) = next_action(c, changeset, revision, last)? {
        last = n;
        let verb = Verb::parse(&verb).expect("the CHECK of the table");
        // Only the targets give the rows: drop rows of another writer.
        clear(c, id)?;
        let resolved = resolve(c, id, verb, &targets)?;
        if let (Some(job), Some(grant)) = (job, &grant) {
            if let Some(why) = outside_grant(c, id, job, grant, Phase::Submit)? {
                return Err(Error::Denied(why));
            }
        }
        let mut denial = match supported(c, id, verb, &args)? {
            Err(why) => Some(why),
            Ok(()) => resolved,
        };
        if denial.is_none() && verb == Verb::Correct {
            denial = check_correct(c, id, &args, now_ms)?;
        }
        if denial.is_none() && WRITE_VERBS.contains(&verb.as_str()) {
            denial = check_write(c, id, verb, &args)?;
        }
        if let Some(why) = denial {
            clear(c, id)?;
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
               AND a.verb NOT IN ('close', 'defer', 'decline', 'correct', 'draft', 'send')",
        [changeset],
        |r| r.get(0),
    )?;
    let mut last = 0;
    while let Some((id, n, verb, ..)) = next_action(c, changeset, revision, last)? {
        last = n;
        let (count, hash) = hash(c, id)?;
        // The flags of a mailbox verb: a state verb changes no message, and
        // has no target thread. A write verb (draft, send) changes no
        // message either, but its thread can be a first contact.
        // A denied action has no message and no draft row (`clear`), so
        // `first_contact` finds no thread for it. A send whose thread has
        // only gone messages has no message row but keeps its draft thread.
        let mailbox = !STATE_VERBS.contains(&verb.as_str()) && verb != "restore";
        // The flag `bulk` does not apply to an undo (section 7.4).
        let undo: bool = c.query_row(
            "SELECT EXISTS (SELECT 1 FROM ledger_undo WHERE action_id = ?1)",
            [id],
            |r| r.get(0),
        )?;
        let mut flags = Vec::new();
        if mailbox
            && count > 0
            && !WRITE_VERBS.contains(&verb.as_str())
            && !undo
            && (count > BULK || total > BULK)
        {
            flags.push("bulk");
        }
        if mailbox && first_contact(c, id)? {
            flags.push("first_contact");
        }
        // The draft flags (spec 2, section 5.3) come from the content.
        if WRITE_VERBS.contains(&verb.as_str()) {
            if let Some((account, content)) = action_content(c, id)? {
                flags.extend(draft_flags(c, &account, &content)?);
                if marked_in(c, changeset)? && !outside_marks(c, id, &content)?.is_empty() {
                    flags.push("outside_thread");
                }
            }
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
}

/// The job of the author of a changeset, or `None` for an author with no
/// job. A job text that is not canonical is job 0, which has no grant.
fn changeset_job(c: &Connection, changeset: i64) -> Result<Option<i64>> {
    let job: Option<Option<String>> = c
        .query_row(
            "SELECT author_job FROM changeset WHERE id = ?1",
            [changeset],
            |r| r.get(0),
        )
        .optional()?;
    Ok(job
        .flatten()
        .map(|j| iris_job::parse_job_id(&j).unwrap_or(0)))
}

/// The grant of `job`, or an empty grant for a job that does not exist.
fn grant_of(c: &Connection, job: i64) -> Result<iris_workspace::Grant> {
    match iris_workspace::grant_in(c, job) {
        Ok(g) => Ok(g),
        Err(iris_job::Error::NotFound) => Ok(iris_workspace::Grant::default()),
        Err(iris_job::Error::Store(e)) => Err(Error::Store(e)),
        Err(e) => Err(Error::Store(iris_store::Error::Query(e.to_string()))),
    }
}

/// Why an action of a job session is outside the grant of its job now, or
/// `None` when the action is in the grant, or its author has no job. The
/// apply checks each action with this function before each change
/// (docs/specs/2026-11-17-job-verbs.md, section 4).
pub fn grant_denial(c: &Connection, action: i64) -> Result<Option<String>> {
    let changeset: Option<i64> = c
        .query_row(
            "SELECT changeset_id FROM changeset_action WHERE id = ?1",
            [action],
            |r| r.get(0),
        )
        .optional()?;
    let Some(changeset) = changeset else {
        return Err(Error::NotFound);
    };
    let Some(job) = changeset_job(c, changeset)? else {
        return Ok(None);
    };
    let grant = grant_of(c, job)?;
    outside_grant(c, action, job, &grant, Phase::Apply)
}

/// When the grant check runs: the targets are only declared at submit,
/// and resolved to rows after it.
#[derive(Clone, Copy, PartialEq)]
enum Phase {
    Submit,
    Apply,
}

/// The checks of one action against the grant of its job: no draft or
/// send, each resolved message granted, each item an item of the workspace.
/// Invariant: an action of a session changes only granted rows, and a
/// target that gives no row never passes. At submit, each declared target
/// must also be in the grant, so a target that gives no row (each message
/// gone, no such row, an ID that does not parse) fails. After submit, the
/// rows are fixed: the check reads them, not the targets, because sync can
/// move a granted message to another thread. An action with no row fails.
fn outside_grant(
    c: &Connection,
    action: i64,
    job: i64,
    grant: &iris_workspace::Grant,
    phase: Phase,
) -> Result<Option<String>> {
    let j = iris_job::format_job_id(job);
    let (verb, targets): (String, String) = c.query_row(
        "SELECT verb, targets FROM changeset_action WHERE id = ?1",
        [action],
        |r| Ok((r.get(0)?, r.get(1)?)),
    )?;
    if WRITE_VERBS.contains(&verb.as_str()) {
        return Ok(Some(format!(
            "a session of {j} cannot {verb}: a job has no grant of an account or a draft"
        )));
    }
    let outside = || Ok(Some(format!("the targets are outside the grant of {j}")));
    if phase == Phase::Submit {
        // Fail closed: a target list that does not parse is outside the grant.
        let Ok(groups) = serde_json::from_str::<Map<String, Value>>(&targets) else {
            return outside();
        };
        for ids in groups.values() {
            let Value::Array(ids) = ids else {
                return outside();
            };
            for id in ids {
                let Some((kind, row)) = id.as_str().and_then(parse_id) else {
                    return outside();
                };
                let granted = match kind {
                    Kind::Thread => grant.has_thread(row),
                    Kind::Message => grant.messages.contains(&row),
                    Kind::Commitment
                    | Kind::Conversation
                    | Kind::Record
                    | Kind::Feed
                    | Kind::Signal => grant.items.contains(&row),
                    _ => false,
                };
                if !granted {
                    let id = format_id(kind, row);
                    return Ok(Some(format!("{id} is outside the grant of {j}")));
                }
            }
        }
    } else {
        let rows: bool = c.query_row(
            "SELECT EXISTS (SELECT 1 FROM action_message WHERE action_id = ?1)
                 OR EXISTS (SELECT 1 FROM action_item WHERE action_id = ?1)",
            [action],
            |r| r.get(0),
        )?;
        if !rows {
            return outside();
        }
    }
    let mut q = c.prepare_cached(
        "SELECT message_id FROM action_message WHERE action_id = ?1 ORDER BY message_id",
    )?;
    for m in q.query_map([action], |r| r.get::<_, i64>(0))? {
        let m = m?;
        if !grant.messages.contains(&m) {
            let id = format_id(Kind::Message, m);
            return Ok(Some(format!("{id} is outside the grant of {j}")));
        }
    }
    let mut q = c.prepare_cached(
        "SELECT a.item_id, i.doc ->> '$.id' FROM action_item a
         LEFT JOIN item i ON i.id = a.item_id WHERE a.action_id = ?1 ORDER BY a.item_id",
    )?;
    for row in q.query_map([action], |r| {
        Ok((r.get::<_, i64>(0)?, r.get::<_, Option<String>>(1)?))
    })? {
        let (item, id) = row?;
        if !grant.items.contains(&item) {
            let id = id.unwrap_or_else(|| format!("item {item}"));
            return Ok(Some(format!("{id} is not an item of the workspace of {j}")));
        }
    }
    Ok(None)
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

/// True when the author of `changeset` read mail that you did not send
/// (spec R4-06, "The mark"). Only the person and an agent whose job scope
/// is your own mail are not marked; each other author is.
pub fn marked_in(c: &Connection, changeset: i64) -> Result<bool> {
    let (kind, job): (String, Option<String>) = c.query_row(
        "SELECT author_kind, author_job FROM changeset WHERE id = ?1",
        [changeset],
        |r| Ok((r.get(0)?, r.get(1)?)),
    )?;
    Ok(
        match (
            kind.as_str(),
            job.as_deref().and_then(iris_job::parse_job_id),
        ) {
            ("person", _) => false,
            ("agent", Some(job)) => iris_job::reads_outside_in(c, job)?,
            _ => true,
        },
    )
}

/// The marks of the flag `outside_thread` (spec R4-06): each address of
/// `to` and `cc` of `content` that is not in From, To or Cc of a live
/// message of the threads of `action`, and is not one of your addresses. A
/// new mail has no thread. Submit gives the flag to a marked author when
/// this list is not empty.
pub fn outside_marks(
    c: &Connection,
    action: i64,
    content: &iris_compose::Content,
) -> Result<Vec<Mark>> {
    // Memory holds the recipients that no row names yet (at most the
    // recipients of the draft) and one header row, never a list of the
    // addresses of the thread or of one header.
    let mut open: HashSet<String> = content
        .to
        .iter()
        .chain(&content.cc)
        .map(|a| a.trim().to_ascii_lowercase())
        .collect();
    iris_talk::each_your_address(c, |a| {
        open.remove(&a);
        !open.is_empty()
    })?;
    if !open.is_empty() {
        let mut st = c.prepare(&format!(
            "SELECT h.from_addr, h.to_addrs, h.cc_addrs
             FROM changeset_action a, message t JOIN header h ON h.message_id = t.id
             WHERE a.id = ?1 AND t.gone_at IS NULL AND t.thread_id IN ({ACTION_THREADS})"
        ))?;
        let mut rows = st.query([action])?;
        while let (false, Some(r)) = (open.is_empty(), rows.next()?) {
            for i in 0..3 {
                if let Some(v) = r.get::<_, Option<String>>(i)? {
                    iris_senders::head::each_address(&v, |a, _| {
                        open.remove(&a);
                    });
                }
            }
        }
    }
    let to = content
        .to
        .iter()
        .enumerate()
        .map(|(i, a)| (Field::To(i), a));
    let cc = content
        .cc
        .iter()
        .enumerate()
        .map(|(i, a)| (Field::Cc(i), a));
    Ok(to
        .chain(cc)
        .filter(|(_, a)| open.contains(&a.trim().to_ascii_lowercase()))
        .map(|(field, a)| Mark {
            flag: "outside_thread",
            field,
            range: 0..a.len(),
            source: None,
        })
        .collect())
}

/// The flag `first_contact` (section 5.3): a thread of a resolved message has
/// mail from a sender of the kind `stranger`.
fn first_contact(c: &Connection, action: i64) -> Result<bool> {
    Ok(c.query_row(
        &format!(
            "SELECT EXISTS (
               SELECT 1 FROM message t
               JOIN header h ON h.message_id = t.id
               JOIN sender p ON p.address = h.from_addr
               WHERE t.thread_id IN ({ACTION_THREADS}) AND p.kind = 'stranger')
             FROM changeset_action a WHERE a.id = ?1"
        ),
        [action],
        |r| r.get(0),
    )?)
}

/// The threads of the action `a`: the threads of its messages and the
/// thread of its draft. The thread of a draft stays when each message of
/// the thread is gone, so the hostile check and the flags read the same
/// threads.
const ACTION_THREADS: &str = "SELECT t.thread_id FROM action_message am
       JOIN message t ON t.id = am.message_id WHERE am.action_id = a.id
     UNION
     SELECT d.thread_id FROM action_draft ad
       JOIN draft d ON d.id = ad.draft_id WHERE ad.action_id = a.id";

/// The Gmail system labels (Gmail API, labels guide). The store also has the
/// system labels of each account in `label`.
const SYSTEM_LABELS: [&str; 9] = [
    "INBOX",
    "SPAM",
    "TRASH",
    "UNREAD",
    "STARRED",
    "IMPORTANT",
    "SENT",
    "DRAFT",
    "CHAT",
];

/// The argument checks of each verb (section 4), for the action row
/// `action`. Gives the reason of the denial for an action that is not valid.
fn supported(
    c: &Connection,
    action: i64,
    verb: Verb,
    args: &str,
) -> Result<std::result::Result<(), String>> {
    // add_action writes an object. Another writer of the table may not.
    let Ok(args) = serde_json::from_str::<Map<String, Value>>(args) else {
        return Ok(Err("the arguments are not a JSON object".into()));
    };
    Ok(match verb {
        Verb::Archive | Verb::Inbox | Verb::Read | Verb::Unread | Verb::Trash => {
            if args.is_empty() {
                Ok(())
            } else {
                Err(format!("{} takes no arguments", verb.as_str()))
            }
        }
        Verb::Label | Verb::Unlabel => label_name(c, &args)?,
        Verb::Close | Verb::Decline => match (args.get("reason"), args.len()) {
            (Some(Value::String(r)), 1) if !r.trim().is_empty() => Ok(()),
            _ => Err(format!(
                "{} needs one reason: {{\"reason\": \"...\"}}",
                verb.as_str()
            )),
        },
        Verb::Defer => match (args.get("date"), args.len()) {
            (Some(Value::String(d)), 1) if is_date(d) => Ok(()),
            _ => Err("defer needs one date: {\"date\": \"YYYY-MM-DD\"}".into()),
        },
        Verb::Correct => match (args.get("field"), args.get("value"), args.len()) {
            (Some(Value::String(f)), Some(_), 2) if !f.is_empty() => Ok(()),
            _ => Err("correct needs {\"field\": \"...\", \"value\": ...}".into()),
        },
        // Only the undo of a trash makes untrash (section 7.4): its action
        // is step 1 of the undo of a `trash` entry (migration 23).
        Verb::Untrash => {
            let undo: bool = c.query_row(
                "SELECT EXISTS (SELECT 1 FROM ledger_undo u JOIN ledger l ON l.id = u.ledger_id
                                JOIN changeset_action o ON o.id = l.action_id
                                WHERE u.action_id = ?1 AND u.step = 1 AND o.verb = 'trash')",
                [action],
                |r| r.get(0),
            )?;
            match (undo, args.is_empty()) {
                (true, true) => Ok(()),
                (true, false) => Err("untrash takes no arguments".into()),
                (false, _) => Err("only the undo of a trash makes untrash".into()),
            }
        }
        // Only the undo of a state entry makes restore (section 7.4).
        Verb::Restore => {
            let undo: bool = c.query_row(
                "SELECT EXISTS (SELECT 1 FROM ledger_undo u JOIN ledger l ON l.id = u.ledger_id
                                JOIN changeset_action o ON o.id = l.action_id
                                WHERE u.action_id = ?1 AND u.step = 1
                                  AND o.verb IN ('close', 'decline', 'defer', 'correct'))",
                [action],
                |r| r.get(0),
            )?;
            match (undo, args.is_empty()) {
                (true, true) => Ok(()),
                (true, false) => Err("restore takes no arguments".into()),
                (false, _) => Err("only the undo of a state entry makes restore".into()),
            }
        }
        // The content checks need the target: see check_draft.
        Verb::Draft => Ok(()),
        Verb::Send => {
            if args.is_empty() {
                Ok(())
            } else {
                Err("send takes no arguments".into())
            }
        }
    })
}

/// `label` and `unlabel` take one user label name (section 4). A name that
/// is the same as the name or the ID of a system label, with no regard to
/// case, is refused: a system label changes only through its own verb.
fn label_name(
    c: &Connection,
    args: &Map<String, Value>,
) -> Result<std::result::Result<(), String>> {
    let name = match (args.get("label"), args.len()) {
        (Some(Value::String(n)), 1) if !n.trim().is_empty() => n,
        _ => {
            return Ok(Err(
                "the argument is one label name: {\"label\": \"...\"}".into()
            ))
        }
    };
    // Unicode case folding on both sides (`unicase`): SQLite `upper` and
    // `lower` change only ASCII letters, and a lower case does not equate
    // each case pair (for example `Σ` and `ς`). The store has a few system
    // labels for each account. The whole name is compared: " INBOX" is a
    // user label, not `INBOX` (the schema permits spaces at the ends).
    let folded = UniCase::new(name.as_str());
    let mut system = SYSTEM_LABELS.iter().any(|l| UniCase::new(*l) == folded)
        || UniCase::new(name.get(..9).unwrap_or("")) == UniCase::new("CATEGORY_");
    let mut st = c.prepare("SELECT name, provider_id FROM label WHERE kind = 'system'")?;
    let mut rows = st.query([])?;
    while let (false, Some(r)) = (system, rows.next()?) {
        let (n, id): (String, String) = (r.get(0)?, r.get(1)?);
        system = UniCase::new(n.as_str()) == folded || UniCase::new(id.as_str()) == folded;
    }
    Ok(if system {
        Err(format!("{name} is a system label: use its own verb"))
    } else {
        Ok(())
    })
}

/// The verbs that change Iris state, not a mailbox (section 4). Their
/// targets are items.
const STATE_VERBS: [&str; 4] = ["close", "defer", "decline", "correct"];

/// The verbs that write mail (release 3). A `draft` takes one account, one
/// thread or one draft; a `send` takes one draft.
const WRITE_VERBS: [&str; 2] = ["draft", "send"];

/// Delete the rows that submit derives from the targets of an action.
fn clear(c: &Connection, action: i64) -> Result<()> {
    c.execute("DELETE FROM action_message WHERE action_id = ?1", [action])?;
    c.execute("DELETE FROM action_item WHERE action_id = ?1", [action])?;
    c.execute("DELETE FROM action_draft WHERE action_id = ?1", [action])?;
    Ok(())
}

/// Resolve the targets of an action (section 4). A mailbox verb takes
/// threads or messages and gives `action_message` rows: a thread gives each
/// of its messages that is not gone and has no `SPAM` or `TRASH`. A state
/// verb takes items (`close`, `defer` and `decline` take commitments only)
/// and gives `action_item` rows, which pin the items while the action is
/// open, and the origin messages of the items as `action_message` rows, for
/// the rules (section 6.2). Gives the reason of the denial, or `None`.
fn resolve(c: &Connection, action: i64, verb: Verb, targets: &str) -> Result<Option<String>> {
    // add_action writes one group of IDs. Another writer of the table may not.
    let bad = || Ok(Some("the targets are not valid".to_string()));
    let Ok(targets) = serde_json::from_str::<Map<String, Value>>(targets) else {
        return bad();
    };
    let (Some((group, Value::Array(ids))), 1) = (targets.iter().next(), targets.len()) else {
        return bad();
    };
    let state = STATE_VERBS.contains(&verb.as_str());
    // The undo of a ledger entry (section 7.4) names the entry. Its
    // messages come from the ledger in SQL, never as a list in memory.
    if group == "ledger" && !state {
        return resolve_undo(c, action, ids);
    }
    let linked: bool = c.query_row(
        "SELECT EXISTS (SELECT 1 FROM ledger_undo WHERE action_id = ?1)",
        [action],
        |r| r.get(0),
    )?;
    if linked {
        return Ok(Some("an undo targets its ledger entry".into()));
    }
    let write = WRITE_VERBS.contains(&verb.as_str());
    match (verb, group.as_str()) {
        (Verb::Draft, "accounts" | "threads") | (Verb::Send, "drafts") => {}
        // A change of a draft needs its undo first (spec 2, section 7.5).
        (Verb::Draft, _) => return Ok(Some("draft takes one account or one thread".into())),
        (Verb::Send, _) => return Ok(Some("send takes one draft".into())),
        _ if state && group == "items" => {}
        _ if !state && (group == "threads" || group == "messages") => {}
        _ if state => return Ok(Some(format!("{} takes items", verb.as_str()))),
        _ => return Ok(Some("a mailbox verb takes threads or messages".into())),
    }
    // Each verb needs one or more targets. A thread can still give no message.
    if ids.is_empty() {
        return Ok(Some("the action has no target".into()));
    }
    if write && ids.len() != 1 {
        return Ok(Some(format!("{} takes one target", verb.as_str())));
    }
    let mut seen = HashSet::with_capacity(ids.len());
    for id in ids {
        let Some((kind, row)) = id.as_str().and_then(parse_id) else {
            return bad();
        };
        // The short form, once: the document shows the targets as stored.
        if id.as_str() != Some(format_id(kind, row).as_str()) || !seen.insert(id.as_str()) {
            return bad();
        }
        let found: bool = match (kind, group.as_str()) {
            (Kind::Thread, "threads") => {
                if verb == Verb::Draft {
                    // A reply resolves to one message, its parent (send
                    // spec, section 3), with the rule of `reply_parent`.
                    if let Some(m) = reply_parent(c, row)? {
                        c.execute(
                            "INSERT OR IGNORE INTO action_message(action_id, message_id)
                             VALUES (?1, ?2)",
                            params![action, m],
                        )?;
                    }
                } else {
                    c.execute(
                        "INSERT OR IGNORE INTO action_message(action_id, message_id)
                         SELECT ?1, m.id FROM message m WHERE m.thread_id = ?2 AND m.gone_at IS NULL
                           AND NOT EXISTS (SELECT 1 FROM message_label ml
                             JOIN label l ON l.id = ml.label_id
                             WHERE ml.message_id = m.id AND l.provider_id IN ('SPAM', 'TRASH'))",
                        params![action, row],
                    )?;
                }
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
            (Kind::Account, "accounts") => c.query_row(
                "SELECT EXISTS (SELECT 1 FROM account WHERE id = ?1)",
                [row],
                |r| r.get(0),
            )?,
            (Kind::Draft, "drafts") => {
                let found = c.execute(
                    "INSERT INTO action_draft(action_id, draft_id)
                     SELECT ?1, id FROM draft WHERE id = ?2",
                    params![action, row],
                )? == 1;
                // The thread of a reply, for the hostile check and the flags.
                c.execute(
                    "INSERT OR IGNORE INTO action_message(action_id, message_id)
                     SELECT ?1, m.id FROM draft d JOIN message m ON m.thread_id = d.thread_id
                     WHERE d.id = ?2 AND m.gone_at IS NULL",
                    params![action, row],
                )?;
                found
            }
            (Kind::Thread | Kind::Message | Kind::Sender | Kind::Draft | Kind::Account, _) => {
                return bad()
            }
            (_, "items") => {
                if verb != Verb::Correct && kind != Kind::Commitment {
                    return Ok(Some(format!("{} takes only commitments", verb.as_str())));
                }
                // The document holds its ID, so the prefix must fit the item.
                let found = c.execute(
                    "INSERT OR IGNORE INTO action_item(action_id, item_id)
                     SELECT ?1, id FROM item WHERE id = ?2 AND json_extract(doc, '$.id') = ?3",
                    params![action, row, format_id(kind, row)],
                )? == 1;
                if found {
                    c.execute(
                        "INSERT OR IGNORE INTO action_message(action_id, message_id)
                         SELECT ?1, message_id FROM item_source
                         WHERE item_id = ?2 AND role = 'origin'",
                        params![action, row],
                    )?;
                }
                found
            }
            _ => return bad(),
        };
        if !found {
            let id = format_id(kind, row);
            return Ok(Some(format!("{id} is not in the mirror")));
        }
    }
    // A thread with each message gone or in SPAM or TRASH. An item can have
    // no origin message.
    let none: bool = c.query_row(
        "SELECT NOT EXISTS (SELECT 1 FROM action_message WHERE action_id = ?1)",
        [action],
        |r| r.get(0),
    )?;
    if none && !state && !write {
        return Ok(Some("the targets have no message to change".into()));
    }
    // One action changes one mailbox: the apply and the undo use one account.
    let accounts: i64 = c.query_row(
        "SELECT count(DISTINCT account) FROM (
           SELECT m.account_id AS account FROM action_message am
           JOIN message m ON m.id = am.message_id WHERE am.action_id = ?1
           UNION ALL
           SELECT i.account_id FROM action_item ai
           JOIN item i ON i.id = ai.item_id WHERE ai.action_id = ?1
           UNION ALL
           SELECT d.account_id FROM action_draft ad
           JOIN draft d ON d.id = ad.draft_id WHERE ad.action_id = ?1)",
        [action],
        |r| r.get(0),
    )?;
    if accounts > 1 {
        return Ok(Some("the targets are in two or more accounts".into()));
    }
    Ok(None)
}

/// The messages of an undo: the messages with `done` of the ledger entry
/// that the action undoes, that are not gone (migration 23). Step 2 of the
/// undo of a `trash` takes only those with no `INBOX` in `pre`.
fn resolve_undo(c: &Connection, action: i64, ids: &[Value]) -> Result<Option<String>> {
    let ledger = match ids {
        [Value::String(id)] => id
            .strip_prefix("l-")
            .and_then(|n| n.parse::<i64>().ok())
            .filter(|n| *n > 0 && format!("l-{n}") == *id),
        _ => None,
    };
    let Some(ledger) = ledger else {
        return Ok(Some("the targets are not valid".into()));
    };
    let linked: bool = c.query_row(
        "SELECT EXISTS (SELECT 1 FROM ledger_undo WHERE action_id = ?1 AND ledger_id = ?2)",
        params![action, ledger],
        |r| r.get(0),
    )?;
    if !linked {
        return Ok(Some(
            "only an undo of the entry targets a ledger entry".into(),
        ));
    }
    // The undo of a state entry: the items of the entry, which pins them
    // while the undo is open. The apply sets back only the items that
    // still have the `post` values of the entry.
    let state: bool = c.query_row(
        "SELECT EXISTS (SELECT 1 FROM ledger l JOIN changeset_action o ON o.id = l.action_id
                        WHERE l.id = ?1 AND o.verb IN ('close', 'decline', 'defer', 'correct'))",
        [ledger],
        |r| r.get(0),
    )?;
    if state {
        let n = c.execute(
            "INSERT INTO action_item(action_id, item_id)
             SELECT ?1, ai.item_id FROM ledger l
             JOIN action_item ai ON ai.action_id = l.action_id
             JOIN item i ON i.id = ai.item_id
             WHERE l.id = ?2",
            params![action, ledger],
        )?;
        return Ok((n == 0).then(|| "the entry has no item to change".into()));
    }
    let n = c.execute(
        "INSERT INTO action_message(action_id, message_id)
         SELECT ?1, lm.message_id FROM ledger_undo u
         JOIN ledger_message lm ON lm.ledger_id = u.ledger_id
         JOIN message m ON m.id = lm.message_id AND m.gone_at IS NULL
         WHERE u.action_id = ?1 AND lm.done = 1
           AND (u.step = 1
                OR NOT EXISTS (SELECT 1 FROM json_each(lm.pre) WHERE value = 'INBOX'))",
        [action],
    )?;
    Ok((n == 0).then(|| "the targets have no message to change".into()))
}

/// The checks of a write verb (`docs/specs/2026-11-16-send.md`, sections 4
/// and 5), after resolve. Gives the reason of the denial, or `None`.
fn check_write(c: &Connection, action: i64, verb: Verb, args: &str) -> Result<Option<String>> {
    if hostile(c, action)? {
        return Ok(Some(format!(
            "the thread has hostile mail: only a person can {} in it",
            verb.as_str()
        )));
    }
    if verb == Verb::Send {
        let draft: i64 = c.query_row(
            "SELECT draft_id FROM action_draft WHERE action_id = ?1",
            [action],
            |r| r.get(0),
        )?;
        if let Some(why) = draft_busy(c, action, draft)? {
            return Ok(Some(why));
        }
        // The flags come from the stored bytes (docs/specs/2026-11-17-send-checks.md).
        if action_content(c, action)?.is_none() {
            return Ok(Some(format!(
                "{} has no content that Iris can check",
                format_id(Kind::Draft, draft)
            )));
        }
        return Ok(None);
    }
    let Ok(content) = serde_json::from_str::<iris_compose::Content>(args) else {
        return Ok(Some(
            "draft takes {\"to\": [...], \"cc\": [...], \"subject\": \"...\", \"body\": \"...\"}"
                .into(),
        ));
    };
    let reply: bool = c.query_row(
        "SELECT json_type(targets, '$.threads') IS NOT NULL FROM changeset_action WHERE id = ?1",
        [action],
        |r| r.get(0),
    )?;
    let content = if reply {
        // The field itself, also an empty one: the schema of a checked reply
        // has no subject.
        let has_subject = serde_json::from_str::<Map<String, Value>>(args)
            .is_ok_and(|m| m.contains_key("subject"));
        if has_subject {
            return Ok(Some("a reply takes no subject: Iris sets it".into()));
        }
        // The apply reads the Message-ID of the parent at Gmail: the mirror
        // can lack it before the understanding of the message.
        match parent(c, action)? {
            None => return Ok(Some("the thread has no message to reply to".into())),
            Some(p) => iris_compose::Content {
                subject: iris_compose::reply_subject(&p.subject),
                ..content
            },
        }
    } else {
        content
    };
    Ok(content.check().err())
}

/// Spec 2, section 6.5: an author other than the person writes no mail
/// into a thread with hostile mail. True if the action is such a write:
/// each message of the threads of its resolved messages and of its draft
/// counts, also a gone message, not only the resolved ones. A gone message
/// with no header (after the content cleanup) counts as hostile, because
/// its sender is no longer known. The apply checks it again.
pub fn hostile(c: &Connection, action: i64) -> Result<bool> {
    Ok(c.query_row(
        &format!(
            "SELECT cs.author_kind <> 'person' AND EXISTS (
           SELECT 1 FROM message m
           LEFT JOIN header h ON h.message_id = m.id
           LEFT JOIN sender p ON p.address = h.from_addr
           WHERE m.thread_id IN ({ACTION_THREADS})
             AND ({may_be_hostile} OR p.kind = 'hostile'
                  -- A gone message after the cleanup has no header, so its
                  -- sender is not known: it counts as hostile evidence.
                  OR (m.gone_at IS NOT NULL AND h.message_id IS NULL)))
         FROM changeset_action a JOIN changeset cs ON cs.id = a.changeset_id WHERE a.id = ?1",
            may_be_hostile = iris_store::may_be_hostile!(),
        ),
        [action],
        |r| r.get(0),
    )?)
}

/// A draft that has a send which can go out, or that went out, takes no
/// other send: a `ledger_send` row, a sent draft, a draft
/// in a form that Iris does not write (`foreign_at`), an
/// `approved` or `held` send, an open send of another changeset, or an
/// earlier send of the same revision. A `proposed` or `in_review` send of
/// an older revision of the same changeset is replaced by this submit.
fn draft_busy(c: &Connection, action: i64, draft: i64) -> Result<Option<String>> {
    let busy: bool = c.query_row(
        "SELECT EXISTS (SELECT 1 FROM ledger_send WHERE draft_id = ?2)
             OR EXISTS (SELECT 1 FROM draft WHERE id = ?2
                        AND (sent_at IS NOT NULL OR foreign_at IS NOT NULL))
             OR EXISTS (
               SELECT 1 FROM action_draft o JOIN changeset_action b ON b.id = o.action_id
               JOIN changeset_action a ON a.id = ?1
               WHERE o.draft_id = ?2 AND b.id <> a.id AND b.verb = 'send'
                 AND (b.state IN ('approved', 'held')
                      OR (b.state IN ('proposed', 'in_review') AND b.changeset_id <> a.changeset_id)
                      OR (b.state IS NULL AND b.changeset_id = a.changeset_id
                          AND b.revision = a.revision)))",
        params![action, draft],
        |r| r.get(0),
    )?;
    Ok(busy.then(|| {
        format!(
            "{} was sent, has a send that can go out, or has a form that Iris does not write",
            format_id(Kind::Draft, draft)
        )
    }))
}

/// The message that a reply in the thread `thread` (a row ID) answers
/// (send spec, section 3): the newest message of the thread, by internal
/// date, that is not gone, has no `DRAFT` label and is not in `SPAM` or
/// `TRASH`. Submit resolves a `draft` action of a thread with this rule,
/// and the write view shows the same message (write spec, section 5).
pub fn reply_parent(c: &Connection, thread: i64) -> rusqlite::Result<Option<i64>> {
    c.query_row(
        "SELECT m.id FROM message m WHERE m.thread_id = ?1 AND m.gone_at IS NULL
           AND NOT EXISTS (SELECT 1 FROM message_label ml
             JOIN label l ON l.id = ml.label_id
             WHERE ml.message_id = m.id AND l.provider_id IN ('SPAM', 'TRASH', 'DRAFT'))
         ORDER BY m.internal_date DESC, m.id DESC LIMIT 1",
        [thread],
        |r| r.get(0),
    )
    .optional()
}

/// The parent of a reply (spec section 3): the one message that submit
/// resolved (`action_message`, in the action hash). A message that comes
/// into the thread after the submit is never the parent.
pub struct Parent {
    pub message: i64,
    /// The message is gone now: the apply denies the reply.
    pub gone: bool,
    pub message_id: Option<String>,
    pub in_reply_to: Option<String>,
    pub subject: String,
}

/// The parent of the reply `action`, or `None` if submit resolved no
/// message.
pub fn parent(c: &Connection, action: i64) -> Result<Option<Parent>> {
    Ok(c.query_row(
        "SELECT m.id, m.gone_at IS NOT NULL, nullif(trim(h.message_id_hdr), ''), h.in_reply_to,
                coalesce(h.subject, '')
             FROM action_message am JOIN message m ON m.id = am.message_id
             LEFT JOIN header h ON h.message_id = m.id
             WHERE am.action_id = ?1",
        [action],
        |r| {
            Ok(Parent {
                message: r.get(0)?,
                gone: r.get(1)?,
                message_id: r.get(2)?,
                in_reply_to: r.get(3)?,
                subject: r.get(4)?,
            })
        },
    )
    .optional()?)
}

/// `correct` (section 4): the rules of a correction (`iris_understand::correct`)
/// hold for each item. Gives the reason of the denial, or `None`.
fn check_correct(c: &Connection, action: i64, args: &str, now_ms: i64) -> Result<Option<String>> {
    let args: Map<String, Value> = serde_json::from_str(args).expect("supported read it");
    let (field, value) = (args["field"].as_str().expect("supported"), &args["value"]);
    let mut st = c.prepare(
        "SELECT i.doc ->> '$.id' FROM action_item a JOIN item i ON i.id = a.item_id
         WHERE a.action_id = ?1 ORDER BY i.id",
    )?;
    let mut rows = st.query([action])?;
    while let Some(r) = rows.next()? {
        let id: String = r.get(0)?;
        if let Err(why) = iris_understand::correct::check(c, &id, field, value.clone(), now_ms)? {
            return Ok(Some(format!("{id}: {why}")));
        }
    }
    Ok(None)
}

/// `YYYY-MM-DD`, a date of the Gregorian calendar.
fn is_date(text: &str) -> bool {
    let b = text.as_bytes();
    let digits = |r: std::ops::Range<usize>| b[r].iter().all(u8::is_ascii_digit);
    if b.len() != 10
        || b[4] != b'-'
        || b[7] != b'-'
        || !digits(0..4)
        || !digits(5..7)
        || !digits(8..10)
    {
        return false;
    }
    let n = |r: std::ops::Range<usize>| text[r].parse::<u32>().expect("digits");
    let (y, m, d) = (n(0..4), n(5..7), n(8..10));
    let leap = y % 4 == 0 && (y % 100 != 0 || y % 400 == 0);
    let days = match m {
        1 | 3 | 5 | 7 | 8 | 10 | 12 => 31,
        4 | 6 | 9 | 11 => 30,
        2 if leap => 29,
        2 => 28,
        _ => return false,
    };
    (1..=days).contains(&d)
}

/// The count of resolved messages, and the SHA-256 (hex) of the verb, the
/// targets, the arguments and the resolved message IDs that a review shows
/// (section 5.5).
///
/// `action` is the row ID of the action. The apply (R2-02) checks the
/// stored hash again with this function, in the transaction of the apply.
pub fn hash(c: &Connection, action: i64) -> Result<(i64, String)> {
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
    // A draft target (release 3): the approval binds to the content of the
    // draft (docs/specs/2026-11-16-send.md, section 5).
    let draft: Option<String> = c
        .query_row(
            "SELECT d.hash FROM action_draft a JOIN draft d ON d.id = a.draft_id
             WHERE a.action_id = ?1",
            [action],
            |r| r.get(0),
        )
        .optional()?;
    if let Some(d) = draft {
        h.update(b"draft:");
        h.update(d.as_bytes());
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
    write(store, |c| decide_in(c, action, to, by, reason, now_ms))
}

/// [`decide`] in a write transaction of the caller, with the same checks.
/// Use [`transaction`] to run more than one step in one transaction.
pub fn decide_in(
    c: &Connection,
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
    write(store, |c| approve_in(c, action, hash, now_ms))
}

/// [`approve`] in a write transaction of the caller, with the same checks.
/// Use [`transaction`] to run more than one step in one transaction.
pub fn approve_in(c: &Connection, action: ActionId, hash: &str, now_ms: i64) -> Result<()> {
    let ActionId {
        changeset,
        revision,
        n,
    } = action;
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
                reason, decided_at, hash, policy,
                (SELECT l.id FROM ledger l WHERE l.action_id = changeset_action.id)
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
        let ledger: Option<i64> = r.get(15)?;
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
            r#"]}},"args":{},"why":{},"flags":{},"hash":{},"decision":{}{}}}"#,
            r.get::<_, String>(5)?,
            r.get::<_, Option<String>>(6)?
                .map_or(Value::Null, Value::String),
            r.get::<_, String>(7)?,
            hash.map_or(Value::Null, Value::String),
            decision,
            // The ledger entry of the action (section 7.1).
            ledger.map_or(String::new(), |l| format!(r#","ledger":"l-{l}""#)),
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

    /// Property test of `is_date` against a second calendar: each year of
    /// 0000 to 9999, each month 00 to 13 and each day 00 to 32. A day is
    /// valid when it is before the first day of the next month
    /// (`days_from_civil` of iris-understand).
    #[test]
    fn is_date_agrees_with_the_calendar() {
        use iris_understand::time::days_from_civil as day;
        for y in 0..=9999i64 {
            for m in 0..=13i64 {
                let len = if (1..=12).contains(&m) {
                    day(y + m / 12, m % 12 + 1, 1) - day(y, m, 1)
                } else {
                    0
                };
                for d in 0..=32i64 {
                    let text = format!("{y:04}-{m:02}-{d:02}");
                    assert_eq!(is_date(&text), (1..=len).contains(&d), "{text}");
                }
            }
        }
    }

    /// Fuzz test of `is_date` with random strings near the date form: it
    /// never panics, and it accepts only the ten-byte form that the
    /// calendar test checks.
    #[test]
    fn is_date_takes_no_other_text() {
        let parts = [
            "0", "1", "2", "9", "-", "/", " ", "+", "x", "\u{663}", "２", "é", "29", "02", "2024-",
        ];
        let mut x: u64 = 0x9e37_79b9_7f4a_7c15;
        let mut next = move |n: u64| {
            x ^= x << 13;
            x ^= x >> 7;
            x ^= x << 17;
            x % n
        };
        for _ in 0..200_000 {
            let text: String = (0..next(12))
                .map(|_| parts[next(parts.len() as u64) as usize])
                .collect();
            if is_date(&text) {
                let b = text.as_bytes();
                assert!(b.len() == 10 && b[4] == b'-' && b[7] == b'-', "{text:?}");
                assert!(
                    text.chars().all(|c| c == '-' || c.is_ascii_digit()),
                    "{text:?}"
                );
            }
        }
    }
}
