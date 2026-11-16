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

/// Parse `j-42`. Gives `None` for any other text.
pub fn parse_job_id(text: &str) -> Option<i64> {
    let n = text.strip_prefix("j-")?;
    if n.is_empty() || !n.bytes().all(|b| b.is_ascii_digit()) {
        return None;
    }
    n.parse().ok().filter(|row| *row > 0)
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
    read(store, |c| job_in(c, job))
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

fn job_in(c: &Connection, job: i64) -> Result<Job> {
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
        job_in(c, job)?;
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
    read(store, |c| write_in(c, job, out))
}

fn write_in(c: &Connection, job: i64, out: &mut impl Write) -> Result<()> {
    let j = job_in(c, job)?;
    // Read each event once before the first byte (issue #303): a row that
    // does not parse gives an error and no partial document. The same read
    // transaction gives the same rows to the second pass.
    each_event(c, job, |_| Ok(()))?;
    let mut m = Map::new();
    m.insert("model".into(), "iris/0".into());
    m.insert("id".into(), format_job_id(j.id).into());
    m.insert("goal".into(), j.goal.into());
    if let Some(due) = j.due_ms {
        m.insert("due".into(), time(due));
    }
    m.insert("created".into(), time(j.created_ms));
    m.insert("updated".into(), time(j.updated_ms));
    m.insert("state".into(), j.state.as_str().into());
    if let Some(w) = j.wait {
        m.insert("wait".into(), wait_json(w));
    }
    if let Some(q) = j.question {
        m.insert("question".into(), q.into());
    }
    if let Some(end) = j.end {
        m.insert("end".into(), end.as_str().into());
    }
    let head = serde_json::to_string(&Value::Object(m)).map_err(std::io::Error::other)?;
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
