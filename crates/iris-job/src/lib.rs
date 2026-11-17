//! The job (docs/specs/2026-11-16-job-store.md): a goal with limits for an
//! agent, its state and the events of each state change.
//!
//! The person makes a job ([`create`]). The agent moves it to `waiting`
//! ([`wait`]) or to `needs_you` ([`ask`]). The engine or the person wakes a
//! waiting job ([`wake`]), the person answers a question ([`answer`]), and
//! [`finish`] ends a job. Each move is one `job_event` row.
//!
//! The SQL triggers of migration 14 allow only the moves of the move table
//! of the spec, with its actors, and copy the state of the newest event to
//! the job row. So no caller can make a move that the spec does not allow,
//! and this crate does not check the moves again.

use std::fmt;
use std::io::Write;

use iris_store::{format_id, Kind, Store};
use rusqlite::{params, Connection, OptionalExtension, Row};
use serde_json::{Map, Value};

#[derive(Debug)]
pub enum Error {
    Store(iris_store::Error),
    /// No job has this ID.
    NotFound,
    /// The move table, a text limit or a value check refuses the change.
    Refused(String),
}

impl fmt::Display for Error {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Error::Store(e) => write!(f, "{e}"),
            Error::NotFound => write!(f, "no job has this ID"),
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
        // A trigger or a CHECK of migration 14 refuses the change.
        if let rusqlite::Error::SqliteFailure(f, msg) = &e {
            if f.extended_code == rusqlite::ffi::SQLITE_CONSTRAINT_TRIGGER
                || f.extended_code == rusqlite::ffi::SQLITE_CONSTRAINT_CHECK
            {
                return Error::Refused(msg.clone().unwrap_or_default());
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
    /// The state of a job.
    State { Working = "working", Waiting = "waiting", NeedsYou = "needs_you", Done = "done" }
);

names!(
    /// Who makes a move.
    By { Person = "person", Agent = "agent", Engine = "engine" }
);

names!(
    /// How a job ended.
    End { Complete = "complete", Cancel = "cancel" }
);

/// The wait of a job in `waiting`: an optional thread and a required end.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Wait {
    pub thread: Option<i64>,
    pub until_ms: i64,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Job {
    pub id: i64,
    pub goal: String,
    pub due_ms: Option<i64>,
    pub created_ms: i64,
    pub updated_ms: i64,
    pub state: State,
    pub wait: Option<Wait>,
    pub question: Option<String>,
    pub end: Option<End>,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Event {
    pub at_ms: i64,
    pub state: State,
    pub by: By,
    pub wait: Option<Wait>,
    pub question: Option<String>,
    pub answer: Option<String>,
    pub end: Option<End>,
    pub why: Option<String>,
}

/// `j-` and the row ID.
pub fn format_job_id(row: i64) -> String {
    format!("j-{row}")
}

/// Parse `j-42`. Gives `None` for any other text, also `j-042`: only the
/// text that [`format_job_id`] gives for a row parses
/// (`docs/spec/schema/job.schema.json`, `^j-[1-9][0-9]*$`).
pub fn parse_job_id(text: &str) -> Option<i64> {
    let n = text.strip_prefix("j-")?;
    if n.is_empty() || n.starts_with('0') || !n.bytes().all(|b| b.is_ascii_digit()) {
        return None;
    }
    n.parse().ok()
}

/// Run `f` in one write transaction. The transaction commits only when `f`
/// gives `Ok`.
pub fn transaction<T>(store: &Store, f: impl FnOnce(&Connection) -> Result<T>) -> Result<T> {
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
    match (r, out) {
        (_, Some(Err(e))) => Err(e),
        (Err(e), _) => Err(Error::Store(e)),
        (Ok(()), Some(Ok(v))) => Ok(v),
        (Ok(()), None) => unreachable!("f ran"),
    }
}

/// Make a job in `working`. The trigger `job_create` writes the create
/// event of the person. Gives the row ID.
pub fn create(store: &Store, goal: &str, due_ms: Option<i64>, now_ms: i64) -> Result<i64> {
    transaction(store, |c| create_in(c, goal, due_ms, now_ms))
}

pub fn create_in(c: &Connection, goal: &str, due_ms: Option<i64>, now_ms: i64) -> Result<i64> {
    c.execute(
        "INSERT INTO job (goal, due_at, created_at, state, updated_at)
         VALUES (?1, ?2, ?3, 'working', ?3)",
        params![goal, due_ms, now_ms],
    )?;
    Ok(c.last_insert_rowid())
}

/// The values of one move. Each `None` is NULL.
#[derive(Default)]
struct Move<'a> {
    wait: Option<Wait>,
    question: Option<&'a str>,
    answer: Option<&'a str>,
    end: Option<End>,
    why: Option<&'a str>,
}

fn add_event(c: &Connection, job: i64, state: State, by: By, m: Move, now_ms: i64) -> Result<()> {
    let found: Option<i64> = c
        .query_row("SELECT 1 FROM job WHERE id = ?1", [job], |r| r.get(0))
        .optional()?;
    if found.is_none() {
        return Err(Error::NotFound);
    }
    c.execute(
        "INSERT INTO job_event
           (job_id, at, state, by, wait_thread, wait_until, question, answer, end_kind, why)
         VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10)",
        params![
            job,
            now_ms,
            state.as_str(),
            by.as_str(),
            m.wait.and_then(|w| w.thread),
            m.wait.map(|w| w.until_ms),
            m.question,
            m.answer,
            m.end.map(End::as_str),
            m.why,
        ],
    )?;
    Ok(())
}

/// The agent waits for a reply on `wait.thread`, or for the date
/// `wait.until_ms`. The end of the wait must be after `now_ms`.
pub fn wait(store: &Store, job: i64, wait: Wait, why: Option<&str>, now_ms: i64) -> Result<()> {
    transaction(store, |c| wait_in(c, job, wait, why, now_ms))
}

pub fn wait_in(c: &Connection, job: i64, w: Wait, why: Option<&str>, now_ms: i64) -> Result<()> {
    let m = Move {
        wait: Some(w),
        why,
        ..Move::default()
    };
    add_event(c, job, State::Waiting, By::Agent, m, now_ms)
}

/// The agent asks the person one question.
pub fn ask(store: &Store, job: i64, question: &str, why: Option<&str>, now_ms: i64) -> Result<()> {
    transaction(store, |c| ask_in(c, job, question, why, now_ms))
}

pub fn ask_in(
    c: &Connection,
    job: i64,
    question: &str,
    why: Option<&str>,
    now_ms: i64,
) -> Result<()> {
    let m = Move {
        question: Some(question),
        why,
        ..Move::default()
    };
    add_event(c, job, State::NeedsYou, By::Agent, m, now_ms)
}

/// The engine (a reply or the end of the wait) or the person ends the wait.
pub fn wake(store: &Store, job: i64, by: By, why: Option<&str>, now_ms: i64) -> Result<()> {
    transaction(store, |c| wake_in(c, job, by, why, now_ms))
}

pub fn wake_in(c: &Connection, job: i64, by: By, why: Option<&str>, now_ms: i64) -> Result<()> {
    let m = Move {
        why,
        ..Move::default()
    };
    add_event(c, job, State::Working, by, m, now_ms)
}

/// The person answers the question.
pub fn answer(store: &Store, job: i64, text: &str, now_ms: i64) -> Result<()> {
    transaction(store, |c| answer_in(c, job, text, now_ms))
}

pub fn answer_in(c: &Connection, job: i64, text: &str, now_ms: i64) -> Result<()> {
    let m = Move {
        answer: Some(text),
        ..Move::default()
    };
    add_event(c, job, State::Working, By::Person, m, now_ms)
}

/// End a job. The agent completes only a job in `working`. The person
/// completes or cancels a job in each state other than `done`.
pub fn finish(
    store: &Store,
    job: i64,
    by: By,
    end: End,
    why: Option<&str>,
    now_ms: i64,
) -> Result<()> {
    transaction(store, |c| finish_in(c, job, by, end, why, now_ms))
}

pub fn finish_in(
    c: &Connection,
    job: i64,
    by: By,
    end: End,
    why: Option<&str>,
    now_ms: i64,
) -> Result<()> {
    let m = Move {
        end: Some(end),
        why,
        ..Move::default()
    };
    add_event(c, job, State::Done, by, m, now_ms)
}

const JOB_COLUMNS: &str =
    "id, goal, due_at, created_at, updated_at, state, wait_thread, wait_until, question, end_kind";

fn bad(column: usize, text: &str) -> rusqlite::Error {
    rusqlite::Error::FromSqlConversionFailure(
        column,
        rusqlite::types::Type::Text,
        format!("unknown value {text:?}").into(),
    )
}

fn named<T>(r: &Row, i: usize, parse: fn(&str) -> Option<T>) -> rusqlite::Result<T> {
    let text: String = r.get(i)?;
    parse(&text).ok_or_else(|| bad(i, &text))
}

fn named_opt<T>(r: &Row, i: usize, parse: fn(&str) -> Option<T>) -> rusqlite::Result<Option<T>> {
    let text: Option<String> = r.get(i)?;
    text.map(|t| parse(&t).ok_or_else(|| bad(i, &t)))
        .transpose()
}

fn wait_of(r: &Row, thread: usize, until: usize) -> rusqlite::Result<Option<Wait>> {
    let until_ms: Option<i64> = r.get(until)?;
    let thread: Option<i64> = r.get(thread)?;
    Ok(until_ms.map(|until_ms| Wait { thread, until_ms }))
}

fn job_of(r: &Row) -> rusqlite::Result<Job> {
    Ok(Job {
        id: r.get(0)?,
        goal: r.get(1)?,
        due_ms: r.get(2)?,
        created_ms: r.get(3)?,
        updated_ms: r.get(4)?,
        state: named(r, 5, State::parse)?,
        wait: wait_of(r, 6, 7)?,
        question: r.get(8)?,
        end: named_opt(r, 9, End::parse)?,
    })
}

fn event_of(r: &Row) -> rusqlite::Result<Event> {
    Ok(Event {
        at_ms: r.get(0)?,
        state: named(r, 1, State::parse)?,
        by: named(r, 2, By::parse)?,
        wait: wait_of(r, 3, 4)?,
        question: r.get(5)?,
        answer: r.get(6)?,
        end: named_opt(r, 7, End::parse)?,
        why: r.get(8)?,
    })
}

pub fn get(store: &Store, job: i64) -> Result<Job> {
    read(store, |c| get_in(c, job))
}

/// The jobs in `state`, newest first.
pub fn list(store: &Store, state: State) -> Result<Vec<Job>> {
    Ok(store.read(|c| {
        let mut q = c.prepare(&format!(
            "SELECT {JOB_COLUMNS} FROM job WHERE state = ?1 ORDER BY id DESC"
        ))?;
        let jobs = q
            .query_map([state.as_str()], job_of)?
            .collect::<rusqlite::Result<Vec<_>>>()?;
        Ok(jobs)
    })?)
}

/// One page of the jobs in `states`, newest first: at most `limit` jobs
/// whose row ID is below `before` (all jobs with `None`).
pub fn page(store: &Store, states: &[State], limit: u32, before: Option<i64>) -> Result<Vec<Job>> {
    let names: Vec<&str> = states.iter().map(|s| s.as_str()).collect();
    let names = serde_json::to_string(&names).map_err(std::io::Error::other)?;
    Ok(store.read(|c| {
        let mut q = c.prepare_cached(&format!(
            "SELECT {JOB_COLUMNS} FROM job
             WHERE state IN (SELECT value FROM json_each(?1)) AND id < ?2
             ORDER BY id DESC LIMIT ?3"
        ))?;
        let jobs = q
            .query_map(params![names, before.unwrap_or(i64::MAX), limit], job_of)?
            .collect::<rusqlite::Result<Vec<_>>>()?;
        Ok(jobs)
    })?)
}

/// Call `f` for each event of `job`, in order.
fn each_event(c: &Connection, job: i64, mut f: impl FnMut(Event) -> Result<()>) -> Result<()> {
    let mut q = c.prepare(
        "SELECT at, state, by, wait_thread, wait_until, question, answer, end_kind, why
         FROM job_event WHERE job_id = ?1 ORDER BY id",
    )?;
    let mut rows = q.query([job])?;
    while let Some(r) = rows.next()? {
        f(event_of(r)?)?;
    }
    Ok(())
}

/// Run `f` in one read transaction, so that the job and its events agree.
fn read<T>(store: &Store, f: impl FnOnce(&Connection) -> Result<T>) -> Result<T> {
    let mut out = None;
    store.read(|c| {
        let tx = c.unchecked_transaction()?;
        out = Some(f(&tx));
        Ok(())
        // The drop of `tx` ends the read.
    })?;
    out.expect("f ran")
}

/// The job in a read or write transaction of the caller.
pub fn get_in(c: &Connection, job: i64) -> Result<Job> {
    c.query_row(
        &format!("SELECT {JOB_COLUMNS} FROM job WHERE id = ?1"),
        [job],
        job_of,
    )
    .optional()?
    .ok_or(Error::NotFound)
}

/// The events of `job`, in order.
pub fn events(store: &Store, job: i64) -> Result<Vec<Event>> {
    read(store, |c| {
        get_in(c, job)?;
        let mut out = Vec::new();
        each_event(c, job, |e| {
            out.push(e);
            Ok(())
        })?;
        Ok(out)
    })
}

fn time(ms: i64) -> Value {
    iris_talk::rfc3339(ms).into()
}

fn wait_json(w: Wait) -> Value {
    let mut m = Map::new();
    if let Some(t) = w.thread {
        m.insert("thread".into(), format_id(Kind::Thread, t).into());
    }
    m.insert("until".into(), time(w.until_ms));
    Value::Object(m)
}

fn event_json(e: &Event) -> Value {
    let mut m = Map::new();
    m.insert("at".into(), time(e.at_ms));
    m.insert("state".into(), e.state.as_str().into());
    m.insert("by".into(), e.by.as_str().into());
    if let Some(w) = e.wait {
        m.insert("wait".into(), wait_json(w));
    }
    for (key, v) in [("question", &e.question), ("answer", &e.answer)] {
        if let Some(v) = v {
            m.insert(key.into(), v.as_str().into());
        }
    }
    if let Some(end) = e.end {
        m.insert("end".into(), end.as_str().into());
    }
    if let Some(why) = &e.why {
        m.insert("why".into(), why.as_str().into());
    }
    Value::Object(m)
}

/// Write the JSON document of `job` (`docs/spec/schema/job.schema.json`).
/// The events stream from the store, so no list of events is in memory.
/// On an error of the store, nothing is written to `out`.
pub fn write_document(store: &Store, job: i64, out: &mut impl Write) -> Result<()> {
    read(store, |c| write_document_in(c, job, out))
}

/// The fields of the job document without `events`: the summary of a job.
pub fn job_json(j: &Job) -> Value {
    let mut m = Map::new();
    m.insert("model".into(), iris_store::MODEL.into());
    m.insert("id".into(), format_job_id(j.id).into());
    m.insert("goal".into(), j.goal.as_str().into());
    if let Some(due) = j.due_ms {
        m.insert("due".into(), time(due));
    }
    m.insert("created".into(), time(j.created_ms));
    m.insert("updated".into(), time(j.updated_ms));
    m.insert("state".into(), j.state.as_str().into());
    if let Some(w) = j.wait {
        m.insert("wait".into(), wait_json(w));
    }
    if let Some(q) = &j.question {
        m.insert("question".into(), q.as_str().into());
    }
    if let Some(end) = j.end {
        m.insert("end".into(), end.as_str().into());
    }
    Value::Object(m)
}

/// [`write_document`] on `c`, for one read with other queries.
pub fn write_document_in(c: &Connection, job: i64, out: &mut impl Write) -> Result<()> {
    let j = get_in(c, job)?;
    // Read each event once before the first byte (issue #303): a row that
    // does not parse gives an error and no partial document. The same read
    // transaction gives the same rows to the second pass.
    each_event(c, job, |_| Ok(()))?;
    let head = serde_json::to_string(&job_json(&j)).map_err(std::io::Error::other)?;
    // The head without its closing brace, then the events.
    out.write_all(&head.as_bytes()[..head.len() - 1])?;
    out.write_all(b",\"events\":[")?;
    let mut first = true;
    each_event(c, job, |e| {
        if !first {
            out.write_all(b",")?;
        }
        first = false;
        serde_json::to_writer(&mut *out, &event_json(&e)).map_err(std::io::Error::other)?;
        Ok(())
    })?;
    out.write_all(b"]}")?;
    Ok(())
}

/// One thread or one item of the scope of a job, by row ID.
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord)]
pub enum Ref {
    Thread(i64),
    Item(i64),
}

/// Add `what` to the scope of `job` (migration 19). Only the person or the
/// engine adds to a scope: an agent cannot widen the scope of its own job.
/// A thread ref grants the messages that the thread has at the add. An item
/// ref grants the sources of the item at the add. An add of a ref that is
/// in the scope already adds the messages that it has now to its list.
pub fn add_scope(store: &Store, job: i64, what: Ref, by: By, now_ms: i64) -> Result<()> {
    transaction(store, |c| add_scope_in(c, job, what, by, now_ms))
}

pub fn add_scope_in(c: &Connection, job: i64, what: Ref, by: By, now_ms: i64) -> Result<()> {
    if by == By::Agent {
        return Err(Error::Refused(
            "an agent cannot change the scope of a job".into(),
        ));
    }
    if get_in(c, job)?.state == State::Done {
        return Err(Error::Refused("the job is done".into()));
    }
    let (thread, item) = match what {
        Ref::Thread(t) => (Some(t), None),
        Ref::Item(i) => (None, Some(i)),
    };
    // No ID of a future row: the person grants only a row that exists.
    let exists: bool = c.query_row(
        "SELECT EXISTS (SELECT 1 FROM thread WHERE id = ?1)
             OR EXISTS (SELECT 1 FROM item WHERE id = ?2)",
        params![thread, item],
        |r| r.get(0),
    )?;
    if !exists {
        return Err(Error::Refused(
            "the thread or the item does not exist".into(),
        ));
    }
    let have: Option<i64> = c
        .query_row(
            "SELECT id FROM job_scope WHERE job_id = ?1 AND revoked_at IS NULL
             AND (thread_id = ?2 OR item_id = ?3)",
            params![job, thread, item],
            |r| r.get(0),
        )
        .optional()?;
    let scope_id = match have {
        Some(id) => id,
        None => {
            c.execute(
                "INSERT INTO job_scope (job_id, thread_id, item_id, by, at) VALUES (?1, ?2, ?3, ?4, ?5)",
                params![job, thread, item, by.as_str(), now_ms],
            )?;
            c.last_insert_rowid()
        }
    };
    // The list of the ref now: the messages of the thread, or the sources
    // of the item. A message that sync or understanding adds later comes in
    // only with the next add by the person.
    c.execute(
        "INSERT OR IGNORE INTO job_scope_message (scope_id, message_id)
         SELECT ?1, id FROM message WHERE thread_id = ?2
         UNION SELECT ?1, message_id FROM item_source WHERE item_id = ?3",
        params![scope_id, thread, item],
    )?;
    Ok(())
}

/// True when the agent of `job` read mail that you did not send: a
/// message of a scope row (revoked or not) with neither the label `SENT`
/// nor `DRAFT` (spec R4-06, "The mark"). A message that is not in the store
/// has no label, so it marks the job.
pub fn reads_outside_in(c: &Connection, job: i64) -> rusqlite::Result<bool> {
    c.query_row(
        "SELECT EXISTS (SELECT 1 FROM job_scope s
           JOIN job_scope_message sm ON sm.scope_id = s.id
           WHERE s.job_id = ?1
             AND NOT EXISTS (SELECT 1 FROM message_label ml JOIN label l ON l.id = ml.label_id
                             WHERE ml.message_id = sm.message_id
                               AND l.provider_id IN ('SENT', 'DRAFT')))",
        [job],
        |r| r.get(0),
    )
}

/// Revoke `what` from the active scope of `job`. The row stays, with the
/// revoke, and the next build drops its mail. Only the person or the engine
/// revokes. A ref that is not in the active scope changes nothing.
pub fn revoke_scope(store: &Store, job: i64, what: Ref, by: By, now_ms: i64) -> Result<()> {
    transaction(store, |c| revoke_scope_in(c, job, what, by, now_ms))
}

pub fn revoke_scope_in(c: &Connection, job: i64, what: Ref, by: By, now_ms: i64) -> Result<()> {
    if by == By::Agent {
        return Err(Error::Refused(
            "an agent cannot change the scope of a job".into(),
        ));
    }
    get_in(c, job)?;
    let (thread, item) = match what {
        Ref::Thread(t) => (Some(t), None),
        Ref::Item(i) => (None, Some(i)),
    };
    c.execute(
        "UPDATE job_scope SET revoked_by = ?4, revoked_at = ?5
         WHERE job_id = ?1 AND revoked_at IS NULL AND (thread_id = ?2 OR item_id = ?3)",
        params![job, thread, item, by.as_str(), now_ms],
    )?;
    Ok(())
}

/// The count of the revoked scope rows of `job`. Revokes only grow, so a
/// revoke during a build makes this count differ from `Built::revoked`.
pub fn revoked_count(store: &Store, job: i64) -> Result<i64> {
    read(store, |c| revoked_count_in(c, job))
}

pub fn revoked_count_in(c: &Connection, job: i64) -> Result<i64> {
    get_in(c, job)?;
    Ok(c.query_row(
        "SELECT count(revoked_at) FROM job_scope WHERE job_id = ?1",
        [job],
        |r| r.get(0),
    )?)
}

/// The active scope of `job`, in the order of the adds.
pub fn scope(store: &Store, job: i64) -> Result<Vec<Ref>> {
    read(store, |c| scope_in(c, job))
}

pub fn scope_in(c: &Connection, job: i64) -> Result<Vec<Ref>> {
    get_in(c, job)?;
    let mut q =
        c.prepare("SELECT thread_id, item_id FROM job_scope WHERE job_id = ?1 AND revoked_at IS NULL ORDER BY rowid")?;
    let refs = q
        .query_map([job], |r| {
            let thread: Option<i64> = r.get(0)?;
            Ok(match thread {
                Some(t) => Ref::Thread(t),
                None => Ref::Item(r.get(1)?),
            })
        })?
        .collect::<rusqlite::Result<Vec<_>>>()?;
    Ok(refs)
}

names!(
    /// Why a session ended (migration 21).
    EndReason {
        Exit = "exit",
        Stop = "stop",
        Revoke = "revoke",
        JobState = "job_state",
        TimeLimit = "time_limit",
        Engine = "engine",
        LaunchFailed = "launch_failed",
        HelperFailed = "helper_failed",
    }
);

/// One session of an agent on a job (docs/specs/2026-11-17-session-launcher.md).
/// Open while `ended_ms` is `None`, live while `reaped_ms` is `None`.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Session {
    pub id: i64,
    pub job_id: i64,
    pub agent: String,
    pub registered_ms: i64,
    pub helper_pid: Option<i64>,
    pub agent_pgid: Option<i64>,
    pub started_ms: Option<i64>,
    pub ended_ms: Option<i64>,
    pub end: Option<EndReason>,
    pub exit_code: Option<i64>,
    pub reaped_ms: Option<i64>,
}

/// `se-` and the row ID.
pub fn format_session_id(row: i64) -> String {
    format!("se-{row}")
}

/// Parse `se-42`. Only the text that [`format_session_id`] gives for a
/// positive row parses.
pub fn parse_session_id(text: &str) -> Option<i64> {
    let n = text.strip_prefix("se-")?;
    if n.is_empty() || n.starts_with('0') || !n.bytes().all(|b| b.is_ascii_digit()) {
        return None;
    }
    n.parse().ok()
}

const SESSION_COLUMNS: &str = "id, job_id, agent, registered_at, helper_pid, agent_pgid, \
     started_at, ended_at, end_reason, exit_code, reaped_at";

fn session_of(r: &Row) -> rusqlite::Result<Session> {
    Ok(Session {
        id: r.get(0)?,
        job_id: r.get(1)?,
        agent: r.get(2)?,
        registered_ms: r.get(3)?,
        helper_pid: r.get(4)?,
        agent_pgid: r.get(5)?,
        started_ms: r.get(6)?,
        ended_ms: r.get(7)?,
        end: named_opt(r, 8, EndReason::parse)?,
        exit_code: r.get(9)?,
        reaped_ms: r.get(10)?,
    })
}

/// Register a session of `agent` on `job`. The trigger refuses a job that
/// is not `working` and a job with a live session. Gives the row ID.
pub fn register(store: &Store, job: i64, agent: &str, now_ms: i64) -> Result<i64> {
    transaction(store, |c| register_in(c, job, agent, now_ms))
}

pub fn register_in(c: &Connection, job: i64, agent: &str, now_ms: i64) -> Result<i64> {
    get_in(c, job)?;
    c.execute(
        "INSERT INTO job_session (job_id, agent, registered_at) VALUES (?1, ?2, ?3)",
        params![job, agent, now_ms],
    )?;
    Ok(c.last_insert_rowid())
}

/// Run one move of `session`. Gives `false` if the `WHERE` of the move
/// found no row in the state that the move needs.
fn session_move(
    c: &Connection,
    session: i64,
    set: &str,
    when: &str,
    values: &[&dyn rusqlite::ToSql],
) -> Result<bool> {
    session_in(c, session)?;
    let n = c.execute(
        &format!("UPDATE job_session SET {set} WHERE id = ?1 AND {when}"),
        &[&[&session as &dyn rusqlite::ToSql], values].concat()[..],
    )?;
    Ok(n == 1)
}

fn must(done: bool, why: &str) -> Result<()> {
    if done {
        Ok(())
    } else {
        Err(Error::Refused(why.into()))
    }
}

/// The helper move: the process ID of the hold helper, once.
pub fn set_helper(store: &Store, session: i64, pid: i64) -> Result<()> {
    transaction(store, |c| set_helper_in(c, session, pid))
}

pub fn set_helper_in(c: &Connection, session: i64, pid: i64) -> Result<()> {
    let done = session_move(
        c,
        session,
        "helper_pid = ?2",
        "helper_pid IS NULL AND reaped_at IS NULL",
        &[&pid],
    )?;
    must(
        done,
        "the helper of a session is set once, while it is live",
    )
}

/// The spawn move: the process group ID of the agent, once, after the helper.
pub fn set_agent(store: &Store, session: i64, pgid: i64) -> Result<()> {
    transaction(store, |c| set_agent_in(c, session, pgid))
}

pub fn set_agent_in(c: &Connection, session: i64, pgid: i64) -> Result<()> {
    let done = session_move(
        c,
        session,
        "agent_pgid = ?2",
        "agent_pgid IS NULL AND helper_pid IS NOT NULL AND reaped_at IS NULL",
        &[&pgid],
    )?;
    must(
        done,
        "the agent group of a session is set once, after the helper, while it is live",
    )
}

/// The start move. Gives `false` if the session ended (a revoke, a job move
/// or a stop came first), and then changes nothing.
pub fn start(store: &Store, session: i64, now_ms: i64) -> Result<bool> {
    transaction(store, |c| start_in(c, session, now_ms))
}

pub fn start_in(c: &Connection, session: i64, now_ms: i64) -> Result<bool> {
    if session_in(c, session)?.started_ms.is_some() {
        return Err(Error::Refused("a session starts once".into()));
    }
    session_move(
        c,
        session,
        "started_at = ?2",
        "started_at IS NULL AND ended_at IS NULL",
        &[&now_ms],
    )
}

/// The end move. `exit_code` only with [`EndReason::Exit`]. Gives `false`
/// if the session ended already: the first end stays.
pub fn end(
    store: &Store,
    session: i64,
    why: EndReason,
    exit_code: Option<i64>,
    now_ms: i64,
) -> Result<bool> {
    transaction(store, |c| end_in(c, session, why, exit_code, now_ms))
}

pub fn end_in(
    c: &Connection,
    session: i64,
    why: EndReason,
    exit_code: Option<i64>,
    now_ms: i64,
) -> Result<bool> {
    session_move(
        c,
        session,
        "ended_at = ?2, end_reason = ?3, exit_code = ?4",
        "ended_at IS NULL",
        &[&now_ms, &why.as_str(), &exit_code],
    )
}

/// The reap move, once, after the end. Only the reap check of the engine
/// calls it.
pub fn reap(store: &Store, session: i64, now_ms: i64) -> Result<()> {
    transaction(store, |c| reap_in(c, session, now_ms))
}

pub fn reap_in(c: &Connection, session: i64, now_ms: i64) -> Result<()> {
    let done = session_move(
        c,
        session,
        "reaped_at = ?2",
        "reaped_at IS NULL AND ended_at IS NOT NULL",
        &[&now_ms],
    )?;
    must(done, "a session is reaped once, after its end")
}

/// One session by row ID.
pub fn session(store: &Store, id: i64) -> Result<Session> {
    read(store, |c| session_in(c, id))
}

pub fn session_in(c: &Connection, id: i64) -> Result<Session> {
    c.query_row(
        &format!("SELECT {SESSION_COLUMNS} FROM job_session WHERE id = ?1"),
        [id],
        session_of,
    )
    .optional()?
    .ok_or(Error::NotFound)
}

/// The open session of `job`, if one exists.
pub fn open_session(store: &Store, job: i64) -> Result<Option<Session>> {
    read(store, |c| open_session_in(c, job))
}

pub fn open_session_in(c: &Connection, job: i64) -> Result<Option<Session>> {
    get_in(c, job)?;
    Ok(c.query_row(
        &format!(
            "SELECT {SESSION_COLUMNS} FROM job_session WHERE job_id = ?1 AND ended_at IS NULL"
        ),
        [job],
        session_of,
    )
    .optional()?)
}

/// Each live session (not reaped), in the order of the register.
pub fn live_sessions(store: &Store) -> Result<Vec<Session>> {
    read(store, live_sessions_in)
}

pub fn live_sessions_in(c: &Connection) -> Result<Vec<Session>> {
    let mut q = c.prepare(&format!(
        "SELECT {SESSION_COLUMNS} FROM job_session WHERE reaped_at IS NULL ORDER BY id"
    ))?;
    let all = q
        .query_map([], session_of)?
        .collect::<rusqlite::Result<Vec<_>>>()?;
    Ok(all)
}
