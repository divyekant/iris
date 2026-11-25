//! The jobs of the agents (card R4-09): a page of jobs, and one job with
//! its events, its changesets and the mail of its waits.

use std::io::Write;

use std::collections::BTreeSet;

use iris_job::{By, Ref, State as JobState};
use iris_store::{format_id, Kind, Store};
use rusqlite::{Connection, OptionalExtension};
use serde::Serialize;
use serde_json::{json, Map, Value};

use crate::read::{check_limit, message_row, rfc3339};
use crate::{Error, Result};

/// One page of jobs, newest first.
#[derive(Debug, Serialize)]
pub struct JobPage {
    /// The job documents without `events`.
    pub jobs: Vec<Value>,
    /// The `after` value of the next page, or `None` on the last page.
    pub next: Option<String>,
    /// The newest session of each `working` job of the page, by job ID
    /// (issue 791), so that the list can say when no agent works on a job.
    /// A working job with no session has `null`. The job document itself
    /// stays as the schema gives it.
    pub sessions: Map<String, Value>,
}

fn job_error(e: iris_job::Error) -> Error {
    match e {
        iris_job::Error::Store(e) => Error::Store(e),
        other => Error::Store(iris_store::Error::Query(other.to_string())),
    }
}

/// The jobs of `state`, newest first. `state` is `open` (each state but
/// `done`) or one state of the job. `after` is the `next` value of the
/// page before.
pub fn jobs(store: &Store, state: &str, limit: u32, after: Option<&str>) -> Result<JobPage> {
    check_limit(limit)?;
    let states =
        match state {
            "open" => vec![JobState::Working, JobState::Waiting, JobState::NeedsYou],
            s => vec![JobState::parse(s)
                .ok_or_else(|| Error::Query(format!("not a job state: {s:?}")))?],
        };
    let before = match after {
        None => None,
        Some(a) => Some(
            iris_job::parse_job_id(a)
                .ok_or_else(|| Error::Query(format!("not a job ID: {a:?}")))?,
        ),
    };
    // One read transaction: the page and the sessions come from the same
    // snapshot, so a working job never shows a session of a later state.
    store.read(|c| {
        let tx = c.unchecked_transaction()?;
        Ok(page_and_sessions(&tx, &states, limit, before))
    })?
}

/// The page of [`jobs`] and the newest session of each working job, in the
/// transaction `c` of the caller.
fn page_and_sessions(
    c: &Connection,
    states: &[JobState],
    limit: u32,
    before: Option<i64>,
) -> Result<JobPage> {
    let mut jobs = iris_job::page_in(c, states, limit + 1, before).map_err(job_error)?;
    let next = if jobs.len() > limit as usize {
        jobs.truncate(limit as usize);
        jobs.last().map(|j| iris_job::format_job_id(j.id))
    } else {
        None
    };
    let mut sessions = Map::new();
    for j in jobs.iter().filter(|j| j.state == JobState::Working) {
        sessions.insert(iris_job::format_job_id(j.id), newest_session(c, j.id)?);
    }
    Ok(JobPage {
        jobs: jobs.iter().map(iris_job::job_json).collect(),
        next,
        sessions,
    })
}

/// The answer of the person to the question of the job `row` (card
/// R5-05). The job moves to `working`, and the engine starts a new session
/// with the answer in `JOB.md`. A job that does not ask the question now
/// gives `Conflict`: for example a second answer, a job that the person
/// ended, or a newer question when `asked` names an older one. With no
/// `asked` (the command `iris job answer`), the answer applies to the
/// question that the job asks now.
pub fn answer_job(
    store: &Store,
    row: i64,
    text: &str,
    asked: Option<iris_job::Asked>,
    now_ms: i64,
) -> Result<()> {
    iris_job::check_text(text, 10_000).map_err(Error::Query)?;
    iris_job::answer(store, row, text, asked, now_ms).map_err(|e| match e {
        iris_job::Error::Refused(_) => {
            Error::Conflict("The job does not ask this question now.".into())
        }
        e => job_error(e),
    })
}

/// The person ends the job `row` with `end` (`complete` or `cancel`). The
/// move ends a live session of the job with `job_state`, and the engine
/// builds the workspace of the done job, so its mail files go. A job that
/// is done already gives `Conflict`.
pub fn finish_job(store: &Store, row: i64, end: iris_job::End, now_ms: i64) -> Result<()> {
    iris_job::finish(store, row, By::Person, end, None, now_ms).map_err(|e| match e {
        iris_job::Error::Refused(_) => Error::Conflict("The job is done already.".into()),
        e => job_error(e),
    })
}

/// The person ends the wait of the job `row` (issue #959), if the job still
/// waits and is the job that the person saw (`updated`, as the job document
/// gives it). Else `Conflict`, and nothing changes. A wake by the person
/// starts no session (docs/specs/2026-11-17-wait.md): the job page then
/// shows the command that starts an agent.
pub fn wake_job(store: &Store, row: i64, updated: &str, now_ms: i64) -> Result<()> {
    iris_job::transaction(store, |c| {
        let j = iris_job::get_in(c, row)?;
        if j.state != JobState::Waiting || iris_job::job_json(&j)["updated"] != updated {
            return Err(iris_job::Error::Refused("the job does not wait now".into()));
        }
        iris_job::wake_in(c, row, By::Person, None, now_ms)
    })
    .map_err(|e| match e {
        iris_job::Error::Refused(_) => {
            Error::Conflict("The job does not wait now, or its wait changed.".into())
        }
        e => job_error(e),
    })
}

/// The row of the job `id` (`j-42`), or `None` if no job has it.
pub fn job_row(store: &Store, id: &str) -> Result<Option<i64>> {
    let Some(row) = iris_job::parse_job_id(id) else {
        return Ok(None);
    };
    Ok(store.read(|c| {
        Ok(
            c.query_row("SELECT id FROM job WHERE id = ?1", [row], |r| r.get(0))
                .optional()?,
        )
    })?)
}

fn io(e: std::io::Error) -> Error {
    Error::Store(e.into())
}

/// `{"job", "changesets", "threads", "session"}` of one job:
///
/// - `job`: the job document (`docs/spec/schema/job.schema.json`).
/// - `changesets`: each submitted changeset of the job (`author_job`), the
///   first submit first: `{"id", "at", "state", "why", "actions"}`. `at`
///   is the first submit, and `actions` counts the actions of the newest
///   revision in each state.
/// - `threads`: for each thread of a wait of the job, the summary of its
///   newest message that is not gone, or `{"id", "gone": true}`.
/// - `session`: the newest session of the job, or null (see
///   [`newest_session`]).
///
/// The answer streams. All parts come from one read of the store. If the
/// job document fails, `out` gets no byte (issue #303).
pub fn write_job(store: &Store, row: i64, out: &mut impl Write) -> Result<()> {
    // The spool keeps a slow reader of `out` from holding the store.
    store.read_to(out, |c, w| write_job_in(c, row, w))
}

fn write_job_in(c: &Connection, row: i64, out: &mut impl Write) -> Result<()> {
    let mut head = Prefixed {
        prefix: Some(b"{\"job\":"),
        out: &mut *out,
    };
    iris_job::write_document_in(c, row, &mut head).map_err(job_error)?;
    out.write_all(b",\"changesets\":[").map_err(io)?;
    let mut q = c
        .prepare_cached(
            "SELECT c.id, r.first, c.state, c.why,
           (SELECT json_group_object(state, n) FROM
             (SELECT coalesce(a.state, 'draft') AS state, count(*) AS n
              FROM changeset_action a
              WHERE a.changeset_id = c.id AND a.revision = c.revision
              GROUP BY 1))
         FROM changeset c
         JOIN (SELECT changeset_id, min(submitted_at) AS first
               FROM changeset_revision GROUP BY changeset_id) r ON r.changeset_id = c.id
         WHERE c.author_job = ?1
         ORDER BY r.first, c.id",
        )
        .map_err(sql)?;
    let mut rows = q.query([iris_job::format_job_id(row)]).map_err(sql)?;
    let mut first = true;
    while let Some(r) = rows.next().map_err(sql)? {
        let one = summary(r).map_err(sql)?;
        if !first {
            out.write_all(b",").map_err(io)?;
        }
        first = false;
        serde_json::to_writer(&mut *out, &one).map_err(|e| io(std::io::Error::other(e)))?;
    }
    out.write_all(b"],\"threads\":").map_err(io)?;
    let threads = wait_threads(c, row).map_err(sql)?;
    serde_json::to_writer(&mut *out, &threads).map_err(|e| io(std::io::Error::other(e)))?;
    out.write_all(b",\"scope\":").map_err(io)?;
    let scope = scope_list(c, row).map_err(sql)?;
    serde_json::to_writer(&mut *out, &scope).map_err(|e| io(std::io::Error::other(e)))?;
    out.write_all(b",\"session\":").map_err(io)?;
    let session = newest_session(c, row)?;
    serde_json::to_writer(&mut *out, &session).map_err(|e| io(std::io::Error::other(e)))?;
    out.write_all(b"}").map_err(io)
}

/// The newest session of `job` (issue #742), or null:
/// `{"id", "agent", "state", "end", "exit", "ended"}`. `state` is
/// `starting`, `running`, `ending` (ended, its processes not reaped yet)
/// or `ended`, as `iris job session` says.
fn newest_session(c: &Connection, job: i64) -> Result<Value> {
    let id: Option<i64> = c
        .query_row(
            "SELECT id FROM job_session WHERE job_id = ?1 ORDER BY id DESC LIMIT 1",
            [job],
            |r| r.get(0),
        )
        .optional()
        .map_err(sql)?;
    let Some(id) = id else {
        return Ok(Value::Null);
    };
    let s = iris_job::session_in(c, id).map_err(job_error)?;
    let state = match (s.started_ms, s.end, s.reaped_ms) {
        (None, None, _) => "starting",
        (Some(_), None, _) => "running",
        (_, Some(_), None) => "ending",
        (_, Some(_), Some(_)) => "ended",
    };
    Ok(json!({
        "id": iris_job::format_session_id(s.id),
        "agent": s.agent,
        "state": state,
        "end": s.end.map(|e| e.as_str()),
        "exit": s.exit_code,
        "ended": s.ended_ms.map(rfc3339),
    }))
}

fn sql(e: rusqlite::Error) -> Error {
    Error::Store(e.into())
}

/// One changeset of a job: `{"id", "at", "state", "why", "actions"}`.
fn summary(r: &rusqlite::Row) -> rusqlite::Result<Value> {
    let actions: Option<String> = r.get(4)?;
    let actions: Value = actions
        .and_then(|a| serde_json::from_str(&a).ok())
        .unwrap_or_else(|| json!({}));
    Ok(json!({
        "id": iris_changeset::format_changeset_id(r.get(0)?),
        "at": rfc3339(r.get(1)?),
        "state": r.get::<_, String>(2)?,
        "why": r.get::<_, Option<String>>(3)?,
        "actions": actions,
    }))
}

fn wait_threads(c: &Connection, job: i64) -> rusqlite::Result<Value> {
    let mut q = c.prepare_cached(
        "SELECT DISTINCT e.wait_thread,
           (SELECT m.id FROM message m WHERE m.thread_id = e.wait_thread AND m.gone_at IS NULL
            ORDER BY m.internal_date DESC, m.id DESC LIMIT 1)
         FROM job_event e WHERE e.job_id = ?1 AND e.wait_thread IS NOT NULL",
    )?;
    let rows = q
        .query_map([job], |r| {
            Ok((r.get::<_, i64>(0)?, r.get::<_, Option<i64>>(1)?))
        })?
        .collect::<rusqlite::Result<Vec<_>>>()?;
    let mut out = Map::new();
    for (thread, newest) in rows {
        let id = format_id(Kind::Thread, thread);
        // The gone state of the same read as the summary.
        let summary = match newest {
            Some(m) => message_row(c, m)?.filter(|(_, more)| !more.gone),
            None => None,
        };
        let v = match summary {
            Some((s, _)) => {
                let mut v = serde_json::to_value(s).unwrap_or(Value::Null);
                v["gone"] = false.into();
                v
            }
            None => json!({ "id": id, "gone": true }),
        };
        out.insert(id, v);
    }
    Ok(Value::Object(out))
}

/// A writer that writes `prefix` before the first byte, so a writer that
/// fails before its first byte leaves `out` empty.
struct Prefixed<'a, W: Write> {
    prefix: Option<&'static [u8]>,
    out: &'a mut W,
}

impl<W: Write> Write for Prefixed<'_, W> {
    fn write(&mut self, data: &[u8]) -> std::io::Result<usize> {
        if let Some(p) = self.prefix.take() {
            self.out.write_all(p)?;
        }
        self.out.write(data)
    }

    fn flush(&mut self) -> std::io::Result<()> {
        self.out.flush()
    }
}

/// The longest goal, in characters (`docs/spec/schema/job.schema.json`).
const GOAL_MAX: usize = 2000;
/// The last millisecond of the year 9999: the highest time of the job table.
const TIME_MAX: i64 = 253_402_300_799_999;

/// The result of [`start_job`].
#[derive(Debug)]
pub enum Started {
    /// The document of the new job.
    Created(Value),
    /// No commitment or conversation has this ID.
    NotFound,
    /// An open job has the item in its scope: its ID (`j-4`).
    Open(String),
}

/// The form and the row of a start item: a commitment or a conversation.
fn start_item(item: &str) -> Result<(&'static str, i64)> {
    match iris_store::parse_id(item) {
        Some((Kind::Commitment, row)) => Ok(("commitment", row)),
        Some((Kind::Conversation, row)) => Ok(("conversation", row)),
        _ => Err(Error::Query(format!(
            "not the ID of a commitment or a conversation: {item:?}"
        ))),
    }
}

/// The checks of an item before a start (section 2, steps 2 and 3).
enum Check {
    Ok,
    NotFound,
    Refused(&'static str),
}

fn check_item(c: &Connection, form: &str, row: i64) -> rusqlite::Result<Check> {
    let status: Option<String> = c
        .query_row(
            "SELECT status FROM item WHERE id = ?1 AND form = ?2",
            rusqlite::params![row, form],
            |r| r.get(0),
        )
        .optional()?;
    // The statuses of the item page: a commitment that is open, a
    // conversation that is active or quiet.
    match (form, status.as_deref()) {
        (_, None) => return Ok(Check::NotFound),
        ("commitment", Some("open")) | ("conversation", Some("active" | "quiet")) => {}
        _ => return Ok(Check::Refused("The item is not open any more.")),
    }
    let mut q = c.prepare_cached("SELECT message_id FROM item_source WHERE item_id = ?1")?;
    let sources = q
        .query_map([row], |r| r.get::<_, i64>(0))?
        .collect::<rusqlite::Result<Vec<_>>>()?;
    if sources.is_empty() {
        return Ok(Check::Refused("The item has no mail."));
    }
    // The rule of the workspace build: else the job starts with an item
    // that its workspace drops.
    for m in sources {
        if !iris_workspace::readable(c, m)? {
            return Ok(Check::Refused(
                "An agent cannot read the mail of this item.",
            ));
        }
    }
    Ok(Check::Ok)
}

/// Each message that a start from item `row` grants: the source messages
/// and each message of their threads, as the scope adds list them. In the
/// order of the thread, then the date that the summary shows (the header
/// date, else the date of the provider).
fn grant_list(c: &Connection, row: i64) -> rusqlite::Result<Vec<i64>> {
    let mut q = c.prepare_cached(
        "SELECT m.id FROM message m
         WHERE m.id IN (SELECT message_id FROM item_source WHERE item_id = ?1)
            OR m.thread_id IN (SELECT x.thread_id FROM item_source s
                               JOIN message x ON x.id = s.message_id WHERE s.item_id = ?1)
         ORDER BY m.thread_id,
                  coalesce((SELECT h.date FROM header h WHERE h.message_id = m.id),
                           m.internal_date, 0),
                  m.id",
    )?;
    let ids = q.query_map([row], |r| r.get(0))?.collect();
    ids
}

/// The mail that a start from `item` grants now (`GET /api/items/{id}/job`):
/// `{"messages": [...]}`, the summary of each message with `readable`.
/// Gives `None` when no commitment or conversation has the ID.
pub fn start_preview(store: &Store, item: &str) -> Result<Option<Value>> {
    let (form, row) = start_item(item)?;
    store.read(|c| {
        let tx = c.unchecked_transaction()?;
        let out = match check_item(&tx, form, row)? {
            Check::NotFound => Ok(None),
            Check::Refused(why) => Err(Error::Conflict(why.into())),
            Check::Ok => {
                let mut messages = Vec::new();
                for m in grant_list(&tx, row)? {
                    let Some((summary, _)) = message_row(&tx, m)? else {
                        continue;
                    };
                    let mut v = serde_json::to_value(summary).map_err(std::io::Error::other)?;
                    v["readable"] = iris_workspace::readable(&tx, m)?.into();
                    messages.push(v);
                }
                Ok(Some(json!({ "messages": messages })))
            }
        };
        Ok(out)
    })?
}

/// Start a job from one item, for the person ("Handle this",
/// `docs/specs/2026-11-17-start-job.md`, section 2). `messages` are the
/// IDs of the preview that the person saw. The scope is the item and the
/// thread of each source message. All steps are one write transaction, so
/// two starts for the same item make one job.
pub fn start_job(
    store: &Store,
    item: &str,
    goal: &str,
    due: Option<&str>,
    messages: &[String],
    now_ms: i64,
) -> Result<Started> {
    let (form, row) = start_item(item)?;
    let (goal, due_ms) = goal_and_due(goal, due)?;
    let seen = message_ids(messages)?;
    enum Out {
        Done(Started),
        Refused(&'static str),
    }
    let out = iris_job::transaction(store, |c| {
        // First: a retry after a lost answer finds the job, also when the
        // item closed after that start.
        let open: Option<i64> = c
            .query_row(
                "SELECT j.id FROM job_scope s JOIN job j ON j.id = s.job_id
                 JOIN item i ON i.id = s.item_id
                 WHERE s.item_id = ?1 AND i.form = ?2
                   AND s.revoked_at IS NULL AND j.state != 'done'
                 ORDER BY j.id LIMIT 1",
                rusqlite::params![row, form],
                |r| r.get(0),
            )
            .optional()?;
        if let Some(j) = open {
            return Ok(Out::Done(Started::Open(iris_job::format_job_id(j))));
        }
        match check_item(c, form, row)? {
            Check::Ok => {}
            Check::NotFound => return Ok(Out::Done(Started::NotFound)),
            Check::Refused(why) => return Ok(Out::Refused(why)),
        }
        if grant_list(c, row)?.into_iter().collect::<BTreeSet<_>>() != seen {
            return Ok(Out::Refused("The mail changed. Look at it again."));
        }
        let job = iris_job::create_in(c, &goal, due_ms, now_ms)?;
        iris_job::add_scope_in(c, job, Ref::Item(row), By::Person, now_ms)?;
        let mut q = c.prepare_cached(
            "SELECT DISTINCT m.thread_id FROM item_source s
             JOIN message m ON m.id = s.message_id WHERE s.item_id = ?1 ORDER BY 1",
        )?;
        let threads = q
            .query_map([row], |r| r.get::<_, i64>(0))?
            .collect::<rusqlite::Result<Vec<_>>>()?;
        for t in threads {
            iris_job::add_scope_in(c, job, Ref::Thread(t), By::Person, now_ms)?;
        }
        // The full document, with the create event (the same as GET).
        let mut doc = Vec::new();
        iris_job::write_document_in(c, job, &mut doc)?;
        let doc = serde_json::from_slice(&doc).map_err(|e| {
            iris_job::Error::Store(iris_store::Error::Query(format!("job document: {e}")))
        })?;
        Ok(Out::Done(Started::Created(doc)))
    })
    .map_err(|e| match e {
        iris_job::Error::Refused(why) => Error::Conflict(why),
        e => job_error(e),
    })?;
    match out {
        Out::Done(s) => Ok(s),
        Out::Refused(why) => Err(Error::Conflict(why.into())),
    }
}

/// The goal and the due date of a start or an edit, with the checks of the
/// job table, so that each value that the table refuses is a 400. The goal
/// loses the white space at its ends.
fn goal_and_due(goal: &str, due: Option<&str>) -> Result<(String, Option<i64>)> {
    let goal = goal.trim();
    if goal.is_empty() || goal.chars().count() > GOAL_MAX || goal.contains('\0') {
        return Err(Error::Query(format!(
            "the goal must have 1 to {GOAL_MAX} characters, and no NUL"
        )));
    }
    let due_ms = match due {
        None => None,
        Some(d) => Some(
            iris_understand::time::parse_rfc3339(d)
                .filter(|ms| (0..=TIME_MAX).contains(ms))
                .ok_or_else(|| {
                    Error::Query(format!(
                        "due is not an RFC 3339 time from 1970 to 9999: {d:?}"
                    ))
                })?,
        ),
    };
    Ok((goal.to_string(), due_ms))
}

/// The message IDs of a preview that the person saw, as a set.
fn message_ids(messages: &[String]) -> Result<BTreeSet<i64>> {
    let mut seen = BTreeSet::new();
    for m in messages {
        match iris_store::parse_id(m) {
            Some((Kind::Message, id)) => seen.insert(id),
            _ => return Err(Error::Query(format!("not a message ID: {m:?}"))),
        };
    }
    Ok(seen)
}

/// The most characters of the request ID of a typed start.
const REQUEST_MAX: usize = 64;

/// The document of `job`, as GET gives it.
fn document_in(c: &Connection, job: i64) -> iris_job::Result<Value> {
    let mut doc = Vec::new();
    iris_job::write_document_in(c, job, &mut doc)?;
    serde_json::from_slice(&doc)
        .map_err(|e| iris_job::Error::Store(iris_store::Error::Query(format!("job document: {e}"))))
}

/// A refusal of the job store is a 409; each other error is the error of
/// the store.
fn refused(e: iris_job::Error) -> Error {
    match e {
        iris_job::Error::Refused(why) => Error::Conflict(why),
        e => job_error(e),
    }
}

/// Start a job from a typed goal, with no mail (decision D25,
/// `docs/specs/2026-11-24-job-goal-and-scope.md`). `request` is the ID that
/// the start form made: a retry with the same ID gives [`Started::Open`]
/// with the job of the first start, and makes no job.
pub fn start_typed_job(
    store: &Store,
    goal: &str,
    due: Option<&str>,
    request: &str,
    now_ms: i64,
) -> Result<Started> {
    let (goal, due_ms) = goal_and_due(goal, due)?;
    let request = request.trim();
    if request.is_empty() || request.chars().count() > REQUEST_MAX || request.contains('\0') {
        return Err(Error::Query(format!(
            "the request must have 1 to {REQUEST_MAX} characters, and no NUL"
        )));
    }
    iris_job::transaction(store, |c| {
        let (job, made) = iris_job::create_for_request_in(c, &goal, due_ms, request, now_ms)?;
        Ok(if made {
            Started::Created(document_in(c, job)?)
        } else {
            Started::Open(iris_job::format_job_id(job))
        })
    })
    .map_err(refused)
}

/// The person changes the goal and the due date of an open job (decision
/// D25). `None` when no job has the ID. A done job, or an edit that
/// changes nothing, is a 409. The answer is the job document.
pub fn edit_job(
    store: &Store,
    id: &str,
    goal: &str,
    due: Option<&str>,
    now_ms: i64,
) -> Result<Option<Value>> {
    let (goal, due_ms) = goal_and_due(goal, due)?;
    let Some(row) = iris_job::parse_job_id(id) else {
        return Ok(None);
    };
    iris_job::transaction(store, |c| {
        match iris_job::edit_in(c, row, &goal, due_ms, now_ms) {
            Err(iris_job::Error::NotFound) => return Ok(None),
            r => r?,
        }
        Ok(Some(document_in(c, row)?))
    })
    .map_err(refused)
}

/// The thread row of `thread` (`t-<n>`), if it exists.
fn thread_row(c: &Connection, thread: &str) -> Result<Option<i64>> {
    let Some((Kind::Thread, row)) = iris_store::parse_id(thread) else {
        return Err(Error::Query(format!("not a thread ID: {thread:?}")));
    };
    c.query_row("SELECT id FROM thread WHERE id = ?1", [row], |r| r.get(0))
        .optional()
        .map_err(sql)
}

/// Each message of thread `row` that an add of the thread grants now, in
/// the order of the start preview.
fn thread_list(c: &Connection, row: i64) -> rusqlite::Result<Vec<i64>> {
    let mut q = c.prepare_cached(
        "SELECT m.id FROM message m WHERE m.thread_id = ?1
         ORDER BY coalesce((SELECT h.date FROM header h WHERE h.message_id = m.id),
                           m.internal_date, 0),
                  m.id",
    )?;
    let ids = q.query_map([row], |r| r.get(0))?.collect();
    ids
}

/// The state of `job`, or `None` when no job has the row.
fn job_state(c: &Connection, job: i64) -> rusqlite::Result<Option<String>> {
    c.query_row("SELECT state FROM job WHERE id = ?1", [job], |r| r.get(0))
        .optional()
}

/// The mail that an add of `thread` to the scope of job `id` grants now
/// (`GET /api/jobs/{id}/scope/{thread}`): `{"messages": [...]}`, the
/// summary of each message with `readable`. `None` when the job or the
/// thread does not exist. A done job is a 409.
pub fn scope_preview(store: &Store, id: &str, thread: &str) -> Result<Option<Value>> {
    let Some(job) = iris_job::parse_job_id(id) else {
        return Ok(None);
    };
    store.read(|c| {
        let tx = c.unchecked_transaction()?;
        let out = match job_state(&tx, job)?.as_deref() {
            None => Ok(None),
            Some("done") => Err(Error::Conflict("the job is done".into())),
            Some(_) => match thread_row(&tx, thread) {
                Err(e) => Err(e),
                Ok(None) => Ok(None),
                Ok(Some(t)) => {
                    let mut messages = Vec::new();
                    for m in thread_list(&tx, t)? {
                        let Some((summary, _)) = message_row(&tx, m)? else {
                            continue;
                        };
                        let mut v = serde_json::to_value(summary).map_err(std::io::Error::other)?;
                        v["readable"] = iris_workspace::readable(&tx, m)?.into();
                        messages.push(v);
                    }
                    Ok(Some(json!({ "messages": messages })))
                }
            },
        };
        Ok(out)
    })?
}

/// Add `thread` to the scope of job `id`, for the person (decision D25).
/// `messages` are the IDs of the preview that the person saw. One write
/// transaction compares them with the messages of the thread now: if they
/// differ, 409 and nothing changes. `false` when the job or the thread does
/// not exist.
pub fn add_thread(
    store: &Store,
    id: &str,
    thread: &str,
    messages: &[String],
    now_ms: i64,
) -> Result<bool> {
    let seen = message_ids(messages)?;
    let Some(job) = iris_job::parse_job_id(id) else {
        return Ok(false);
    };
    enum Out {
        Done(bool),
        Refused(Error),
    }
    let out = iris_job::transaction(store, |c| {
        if job_state(c, job)?.is_none() {
            return Ok(Out::Done(false));
        }
        let t = match thread_row(c, thread) {
            Ok(Some(t)) => t,
            Ok(None) => return Ok(Out::Done(false)),
            Err(e) => return Ok(Out::Refused(e)),
        };
        if thread_list(c, t)?.into_iter().collect::<BTreeSet<_>>() != seen {
            return Ok(Out::Refused(Error::Conflict(
                "The mail changed. Look at it again.".into(),
            )));
        }
        iris_job::add_scope_in(c, job, Ref::Thread(t), By::Person, now_ms)?;
        Ok(Out::Done(true))
    })
    .map_err(refused)?;
    match out {
        Out::Done(found) => Ok(found),
        Out::Refused(e) => Err(e),
    }
}

/// Take `what` (a thread `t-<n>` or an item ID) out of the scope of job
/// `id`, for the person (decision D25). The revoke ends the open session of
/// the job. `false` when the job does not exist or the ref is not active in
/// its scope.
pub fn remove_ref(store: &Store, id: &str, what: &str, now_ms: i64) -> Result<bool> {
    let (r, form) = match iris_store::parse_id(what) {
        Some((Kind::Thread, t)) => (Ref::Thread(t), None),
        Some((Kind::Commitment, i)) => (Ref::Item(i), Some("commitment")),
        Some((Kind::Conversation, i)) => (Ref::Item(i), Some("conversation")),
        Some((Kind::Record, i)) => (Ref::Item(i), Some("record")),
        Some((Kind::Feed, i)) => (Ref::Item(i), Some("feed")),
        Some((Kind::Signal, i)) => (Ref::Item(i), Some("signal")),
        _ => {
            return Err(Error::Query(format!(
                "not a thread or an item ID: {what:?}"
            )))
        }
    };
    let Some(job) = iris_job::parse_job_id(id) else {
        return Ok(false);
    };
    iris_job::transaction(store, |c| {
        match job_state(c, job)?.as_deref() {
            None => return Ok(false),
            Some("done") => return Err(iris_job::Error::Refused("the job is done".into())),
            Some(_) => {}
        }
        // An item ID names the row and its form: `c-3` is not record 3. The
        // form of the grant (`job_scope_form`) holds when the item row is
        // gone. A ref from before migration 50 whose item row is gone has
        // no known form: any item ID with its row number names it.
        let form_ok = match (r, form) {
            (Ref::Item(i), Some(f)) => match c
                .query_row(
                    "SELECT coalesce(i.form, sf.form) FROM job_scope s
                     LEFT JOIN item i ON i.id = s.item_id
                     LEFT JOIN job_scope_form sf ON sf.scope_id = s.id
                     WHERE s.job_id = ?1 AND s.item_id = ?2 AND s.revoked_at IS NULL",
                    rusqlite::params![job, i],
                    |r| r.get::<_, Option<String>>(0),
                )
                .optional()?
            {
                Some(Some(known)) => known == f,
                Some(None) => true,
                None => false,
            },
            _ => true,
        };
        let active = form_ok && iris_job::scope_in(c, job)?.contains(&r);
        if !active {
            return Ok(false);
        }
        iris_job::revoke_scope_in(c, job, r, By::Person, now_ms)?;
        Ok(true)
    })
    .map_err(refused)
}

/// The active refs of the scope of `job`, for the job page: the ID, the
/// kind, a label (the subject of the first message of a thread, the title
/// of an item) and the count of the granted messages.
fn scope_list(c: &Connection, job: i64) -> rusqlite::Result<Value> {
    let mut q = c.prepare_cached(
        "SELECT s.thread_id, coalesce(i.form, sf.form), s.item_id,
           CASE WHEN s.thread_id IS NOT NULL THEN
             (SELECT h.subject FROM message m LEFT JOIN header h ON h.message_id = m.id
              WHERE m.thread_id = s.thread_id
              ORDER BY coalesce(h.date, m.internal_date, 0), m.id LIMIT 1)
           ELSE json_extract(i.doc, '$.title') END,
           (SELECT count(*) FROM job_scope_message sm WHERE sm.scope_id = s.id)
         FROM job_scope s LEFT JOIN item i ON i.id = s.item_id
           LEFT JOIN job_scope_form sf ON sf.scope_id = s.id
         WHERE s.job_id = ?1 AND s.revoked_at IS NULL ORDER BY s.id",
    )?;
    let rows = q.query_map([job], |r| {
        let thread: Option<i64> = r.get(0)?;
        let form: Option<String> = r.get(1)?;
        let item: Option<i64> = r.get(2)?;
        let id = match (thread, form.as_deref(), item) {
            (Some(t), _, _) => Some(format_id(Kind::Thread, t)),
            (None, Some(f), Some(i)) => crate::read::item_id(i, f),
            // No known form (a ref from before migration 50 whose item row
            // is gone): the record form. The remove takes any item ID with
            // this row number.
            (None, None, Some(i)) => Some(format_id(Kind::Record, i)),
            _ => None,
        };
        Ok(json!({
            "id": id,
            "kind": if thread.is_some() { "thread" } else { "item" },
            "label": r.get::<_, Option<String>>(3)?,
            "messages": r.get::<_, i64>(4)?,
        }))
    })?;
    Ok(Value::Array(rows.collect::<rusqlite::Result<_>>()?))
}
