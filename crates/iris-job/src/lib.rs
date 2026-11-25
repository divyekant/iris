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

/// Make a job from a typed goal with the request ID `request` of the
/// start form (decision D25). A request ID that has a job gives that job
/// and makes no job, so a retry after a lost answer makes one job. Gives
/// the job and `true` when this call made it.
pub fn create_for_request_in(
    c: &Connection,
    goal: &str,
    due_ms: Option<i64>,
    request: &str,
    now_ms: i64,
) -> Result<(i64, bool)> {
    let have: Option<i64> = c
        .query_row(
            "SELECT job_id FROM job_request WHERE request = ?1",
            [request],
            |r| r.get(0),
        )
        .optional()?;
    if let Some(job) = have {
        return Ok((job, false));
    }
    let job = create_in(c, goal, due_ms, now_ms)?;
    c.execute(
        "INSERT INTO job_request (request, job_id) VALUES (?1, ?2)",
        params![request, job],
    )?;
    Ok((job, true))
}

/// The person changes the goal and the due date of an open job (decision
/// D25). Migration 50 keeps the values before and after in `job_edit`, and
/// refuses an edit of a done job or an edit that changes nothing. An edit
/// is not a move: the state, `updated` and the sessions do not change.
pub fn edit(store: &Store, job: i64, goal: &str, due_ms: Option<i64>, now_ms: i64) -> Result<()> {
    transaction(store, |c| edit_in(c, job, goal, due_ms, now_ms))
}

pub fn edit_in(
    c: &Connection,
    job: i64,
    goal: &str,
    due_ms: Option<i64>,
    now_ms: i64,
) -> Result<()> {
    let j = get_in(c, job)?;
    // Not before the newest edit: a clock that goes back must not fail it.
    let last: Option<i64> = c.query_row(
        "SELECT max(at) FROM job_edit WHERE job_id = ?1",
        [job],
        |r| r.get(0),
    )?;
    let at = last.map_or(now_ms, |l| now_ms.max(l));
    c.execute(
        "INSERT INTO job_edit (job_id, at, goal_before, due_before, goal, due_at, after_event)
         VALUES (?1, ?2, ?3, ?4, ?5, ?6, (SELECT max(id) FROM job_event WHERE job_id = ?1))",
        params![job, at, j.goal, j.due_ms, goal, due_ms],
    )?;
    Ok(())
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

/// A move out of `needs_you` takes the time of the question if the clock is
/// earlier: the time of a question can be 1 ms ahead of the clock
/// ([`ask_in`]), and the answer or the end must not fail on the time.
fn add_event(c: &Connection, job: i64, state: State, by: By, m: Move, now_ms: i64) -> Result<()> {
    let found: Option<(String, i64)> = c
        .query_row(
            "SELECT state, updated_at FROM job WHERE id = ?1",
            [job],
            |r| Ok((r.get(0)?, r.get(1)?)),
        )
        .optional()?;
    let Some((current, updated)) = found else {
        return Err(Error::NotFound);
    };
    let now_ms = if current == State::NeedsYou.as_str() {
        now_ms.max(updated)
    } else {
        now_ms
    };
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

/// `iris wait` of the session `session` of `job` (docs/specs/2026-11-17-wait.md):
/// one transaction checks that the session is open and, for a reply wait,
/// that the thread is an active thread ref of the scope, then writes the
/// wait. The move ends the session with `job_state`, so the start path of
/// the wake finds it. A session that a revoke or a stop ended, or a thread
/// outside the scope, is refused, and nothing changes.
pub fn session_wait(
    store: &Store,
    session: i64,
    job: i64,
    w: Wait,
    why: Option<&str>,
    now_ms: i64,
) -> Result<()> {
    transaction(store, |c| {
        open_in(c, session, job)?;
        if let Some(t) = w.thread {
            let scoped: bool = c.query_row(
                "SELECT EXISTS (SELECT 1 FROM job_scope
                   WHERE job_id = ?1 AND thread_id = ?2 AND revoked_at IS NULL)",
                params![job, t],
                |r| r.get(0),
            )?;
            if !scoped {
                return Err(Error::Refused(format!(
                    "t-{t} is not a thread of the scope of the job"
                )));
            }
        }
        wait_in(c, job, w, why, now_ms)
    })
}

/// The check of the text of a question (`max` 2000) or an answer (`max`
/// 10000) as the agent or the person gave it: not only white space, at most
/// `max` characters, and no NUL. The callers store the text as given, so
/// the check and the store see the same text.
pub fn check_text(text: &str, max: usize) -> std::result::Result<(), String> {
    if text.trim().is_empty() || text.chars().count() > max || text.contains('\0') {
        return Err(format!(
            "the text must have 1 to {max} characters, not only white space, and no NUL character"
        ));
    }
    Ok(())
}

/// Refuses unless `session` is a session of `job` that did not end.
fn open_in(c: &Connection, session: i64, job: i64) -> Result<()> {
    let open: bool = c.query_row(
        "SELECT EXISTS (SELECT 1 FROM job_session
           WHERE id = ?1 AND job_id = ?2 AND ended_at IS NULL)",
        params![session, job],
        |r| r.get(0),
    )?;
    if open {
        Ok(())
    } else {
        Err(Error::Refused(format!(
            "{} is not open",
            format_session_id(session)
        )))
    }
}

/// `ask` of a session (card R5-05): in one write transaction, a check that
/// `session` of `job` is still open, then the move to `needs_you`. The
/// trigger of migration 21 ends the session with `job_state` in the same
/// transaction. A session that a revoke or a stop ended is refused, and
/// nothing changes.
pub fn session_ask(
    store: &Store,
    session: i64,
    job: i64,
    question: &str,
    now_ms: i64,
) -> Result<()> {
    transaction(store, |c| {
        open_in(c, session, job)?;
        ask_in(c, job, question, None, now_ms)
    })
}

/// `done` of a session: in one write transaction, a check that `session`
/// of `job` is still open, then the move of the job to `done` with the end
/// `complete` by the agent. The trigger of migration 21 ends the session
/// with `job_state` in the same transaction. A session that a revoke or a
/// stop ended is refused, and nothing changes.
pub fn session_done(
    store: &Store,
    session: i64,
    job: i64,
    why: Option<&str>,
    now_ms: i64,
) -> Result<()> {
    transaction(store, |c| {
        open_in(c, session, job)?;
        finish_in(c, job, By::Agent, End::Complete, why, now_ms)
    })
}

/// The agent asks the person one question. The time of the question is
/// `now_ms`, or 1 ms after the newest event of the job if that is later.
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
    // A question is later than each earlier event of its job, so the
    // `updated` time of a job in `needs_you` names one question: an answer
    // binds to it ([`Asked`]), even when the clock gives the same
    // millisecond twice.
    let at = now_ms.max(get_in(c, job)?.updated_ms.saturating_add(1));
    let m = Move {
        question: Some(question),
        why,
        ..Move::default()
    };
    add_event(c, job, State::NeedsYou, By::Agent, m, at)
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

/// The question of the move on the due date (decision D24). Fixed engine
/// text: no mail text and no goal goes into it.
pub const DUE_QUESTION: &str = "The due date passed. Answer to go on, or end the job.";

/// A job that [`move_due`] moved to `needs_you`.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Due {
    pub id: i64,
    pub goal: String,
}

/// The rule of docs/specs/2026-11-24-job-due-date.md, as SQL on the job
/// row `j`: an open job that does not wait for the person, at or after its
/// due date (`?1` is now), with no move to `needs_you` and no move by the
/// person at or after the due date. So the move happens once for each due
/// date. The first event (the start, by the person) is not a move: a job
/// that starts after its due date also moves.
/// Rule 3 counts the events at or after the due date, and after the newest
/// edit of the due date: an edit to a past date (decision D25) is a new due
/// date, and an event before the edit does not count. The edit keeps the ID
/// of the newest event (`after_event`), so the order holds also when the
/// clock goes back.
const DUE_SQL: &str = "j.state IN ('working', 'waiting') AND j.due_at <= ?1
     AND NOT EXISTS (SELECT 1 FROM job_event e WHERE e.job_id = j.id
                     AND e.id > max((SELECT min(f.id) FROM job_event f WHERE f.job_id = j.id),
                                    coalesce((SELECT max(d.after_event) FROM job_edit d
                                              WHERE d.job_id = j.id AND d.due_at IS NOT d.due_before), 0))
                     AND e.at >= j.due_at AND (e.state = 'needs_you' OR e.by = 'person'))";

/// The result of [`move_due`]: the moved jobs, and each job whose move
/// failed, with its error.
#[derive(Debug, Default)]
pub struct DueTurn {
    pub moved: Vec<Due>,
    pub failed: Vec<(i64, Error)>,
}

/// Moves each job that is due at `now_ms` to `needs_you` ([`due_jobs`],
/// then [`move_due_one`] for each). The failure of one job does not stop
/// the others, and a move that committed stays in `moved`.
pub fn move_due(store: &Store, now_ms: i64) -> Result<DueTurn> {
    let mut turn = DueTurn::default();
    for id in due_jobs(store, now_ms)? {
        match move_due_one(store, id, now_ms) {
            Ok(m) => turn.moved.extend(m),
            Err(e) => turn.failed.push((id, e)),
        }
    }
    Ok(turn)
}

/// The IDs of the jobs that are due at `now_ms`, in a read (no write lock).
pub fn due_jobs(store: &Store, now_ms: i64) -> Result<Vec<i64>> {
    Ok(store.read(|c| {
        let mut st = c.prepare_cached(&format!(
            "SELECT j.id FROM job j WHERE {DUE_SQL} ORDER BY j.id"
        ))?;
        let rows = st.query_map([now_ms], |r| r.get(0))?;
        Ok(rows.collect::<rusqlite::Result<_>>()?)
    })?)
}

/// Moves `job` to `needs_you`, by the engine, with [`DUE_QUESTION`] and
/// the reason `due`, if it is due at `now_ms`. One write transaction reads
/// the rule again, so a move of the person or of the agent after
/// [`due_jobs`] wins, and this gives `None`.
pub fn move_due_one(store: &Store, job: i64, now_ms: i64) -> Result<Option<Due>> {
    transaction(store, |c| {
        // A goal that is not UTF-8 fails here, before the move: each reader
        // of the job refuses it, so the job must not ask a question.
        let still: Option<String> = c
            .query_row(
                &format!("SELECT j.goal FROM job j WHERE j.id = ?2 AND {DUE_SQL}"),
                params![now_ms, job],
                |r| r.get(0),
            )
            .optional()?;
        let Some(goal) = still else {
            return Ok(None);
        };
        let m = Move {
            question: Some(DUE_QUESTION),
            why: Some("due"),
            ..Move::default()
        };
        // Not before the newest event: a clock that goes back must not fail
        // the move (as [`finish_in`]).
        let updated: i64 = c.query_row("SELECT updated_at FROM job WHERE id = ?1", [job], |r| {
            r.get(0)
        })?;
        let at = now_ms.max(updated);
        add_event(c, job, State::NeedsYou, By::Engine, m, at)?;
        Ok(Some(Due { id: job, goal }))
    })
}

/// The jobs that wait for a new session after a wake by the engine or an
/// answer of the person (card R5-05): the job is `working`, its newest
/// event is a wake by the engine or has an answer, and the last session of
/// the job ended with `job_state` (the move to `waiting` or `needs_you`). A
/// wake by the person has no answer and starts no session. A
/// session that starts after the wake becomes the last session, so the job
/// leaves the list. The order is the order of the rows, not of the clock.
/// Gives each job, its last session and the agent of that session.
pub fn woken(store: &Store) -> Result<Vec<(i64, i64, String)>> {
    Ok(store.read(|c| {
        let mut st = c.prepare_cached(
            "SELECT j.id, s.id, s.agent FROM job j
             JOIN job_session s
               ON s.id = (SELECT max(id) FROM job_session WHERE job_id = j.id)
             WHERE j.state = 'working' AND s.end_reason = 'job_state'
               AND (SELECT by = 'engine' OR answer IS NOT NULL FROM job_event
                    WHERE job_id = j.id ORDER BY id DESC LIMIT 1)",
        )?;
        let rows = st.query_map([], |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?)))?;
        Ok(rows.collect::<rusqlite::Result<_>>()?)
    })?)
}

/// The question that the person saw: its text and the `updated` of the job
/// document (RFC 3339 with milliseconds) at that time.
#[derive(Clone, Copy, Debug)]
pub struct Asked<'a> {
    pub question: &'a str,
    pub updated: &'a str,
}

/// The person answers the question. With `asked`, the answer applies only
/// if the job still asks that question: an answer to an older question
/// gets [`Error::Refused`], in the same transaction as the move.
pub fn answer(
    store: &Store,
    job: i64,
    text: &str,
    asked: Option<Asked>,
    now_ms: i64,
) -> Result<()> {
    transaction(store, |c| answer_in(c, job, text, asked, now_ms))
}

pub fn answer_in(
    c: &Connection,
    job: i64,
    text: &str,
    asked: Option<Asked>,
    now_ms: i64,
) -> Result<()> {
    if let Some(a) = asked {
        let j = get_in(c, job)?;
        if j.question.as_deref() != Some(a.question)
            || iris_talk::rfc3339(j.updated_ms) != a.updated
        {
            return Err(Error::Refused("the job asks another question now".into()));
        }
    }
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
    // The end is never before the newest event: an ask can set the time of
    // the job 1 ms after the clock, an answer keeps it, and a clock can go
    // back. The move to `done` must not fail for that (#356).
    let at = now_ms.max(get_in(c, job)?.updated_ms);
    add_event(c, job, State::Done, by, m, at)
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
    read(store, |c| page_in(c, states, limit, before))
}

/// [`page`] in a read or write transaction of the caller, so that the
/// caller can read more rows from the same snapshot.
pub fn page_in(
    c: &Connection,
    states: &[State],
    limit: u32,
    before: Option<i64>,
) -> Result<Vec<Job>> {
    let names: Vec<&str> = states.iter().map(|s| s.as_str()).collect();
    let names = serde_json::to_string(&names).map_err(std::io::Error::other)?;
    let mut q = c.prepare_cached(&format!(
        "SELECT {JOB_COLUMNS} FROM job
         WHERE state IN (SELECT value FROM json_each(?1)) AND id < ?2
         ORDER BY id DESC LIMIT ?3"
    ))?;
    let jobs = q
        .query_map(params![names, before.unwrap_or(i64::MAX), limit], job_of)?
        .collect::<rusqlite::Result<Vec<_>>>()?;
    Ok(jobs)
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
    // The spool keeps a slow reader of `out` from holding the store.
    store.read_to(out, |c, w| write_document_in(c, job, w))
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

/// The most bytes of the copy of `notes.md` of a job
/// (docs/specs/2026-11-17-job-memory.md). The CHECK of `job_notes` has the
/// same value.
pub const NOTES_MAX: usize = 65_536;

/// The copy of the notes of `job` in the store, if one was saved.
pub fn notes(store: &Store, job: i64) -> Result<Option<Vec<u8>>> {
    let mut out = None;
    store.read(|c| {
        out = c
            .query_row("SELECT text FROM job_notes WHERE job_id = ?1", [job], |r| {
                r.get(0)
            })
            .optional()?;
        Ok(())
    })?;
    Ok(out)
}

/// Replace the copy of the notes of `job` with `text`, as it is. A text of
/// more than [`NOTES_MAX`] bytes is refused, and the old copy stays. Gives
/// false for a done job: it has no copy (migration 35).
pub fn save_notes(store: &Store, job: i64, text: &[u8], now_ms: i64) -> Result<bool> {
    transaction(store, |c| save_notes_in(c, job, text, now_ms))
}

pub fn save_notes_in(c: &Connection, job: i64, text: &[u8], now_ms: i64) -> Result<bool> {
    if text.len() > NOTES_MAX {
        return Err(Error::Refused(format!(
            "the notes have {} bytes, more than {NOTES_MAX}",
            text.len()
        )));
    }
    get_in(c, job)?;
    let n = c.execute(
        "INSERT INTO job_notes (job_id, text, saved_ms) VALUES (?1, ?2, ?3)
         ON CONFLICT(job_id) DO UPDATE SET text = excluded.text, saved_ms = excluded.saved_ms",
        params![job, text, now_ms],
    )?;
    Ok(n == 1)
}

/// Reap `session` and, if `notes` is given, save it as the copy of the
/// notes of its job, in one write transaction: no backup and no crash sees
/// the reap without the save (docs/specs/2026-11-17-job-memory.md). Gives
/// the result of the save (false: no notes, or a done job).
pub fn reap_with_notes(
    store: &Store,
    session: i64,
    notes: Option<&[u8]>,
    now_ms: i64,
) -> Result<bool> {
    transaction(store, |c| {
        reap_in(c, session, now_ms)?;
        match notes {
            Some(text) => save_notes_in(c, session_in(c, session)?.job_id, text, now_ms),
            None => Ok(false),
        }
    })
}
