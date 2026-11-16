//! The jobs of the agents (card R4-09): a page of jobs, and one job with
//! its events, its changesets and the mail of its waits.

use std::io::Write;

use iris_job::State as JobState;
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
    let mut jobs = iris_job::page(store, &states, limit + 1, before).map_err(job_error)?;
    let next = if jobs.len() > limit as usize {
        jobs.truncate(limit as usize);
        jobs.last().map(|j| iris_job::format_job_id(j.id))
    } else {
        None
    };
    Ok(JobPage {
        jobs: jobs.iter().map(iris_job::job_json).collect(),
        next,
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

/// `{"job", "changesets", "threads"}` of one job:
///
/// - `job`: the job document (`docs/spec/schema/job.schema.json`).
/// - `changesets`: each submitted changeset of the job (`author_job`), the
///   first submit first: `{"id", "at", "state", "why", "actions"}`. `at`
///   is the first submit, and `actions` counts the actions of the newest
///   revision in each state.
/// - `threads`: for each thread of a wait of the job, the summary of its
///   newest message that is not gone, or `{"id", "gone": true}`.
///
/// The answer streams. All parts come from one read of the store. If the
/// job document fails, `out` gets no byte (issue #303).
pub fn write_job(store: &Store, row: i64, out: &mut impl Write) -> Result<()> {
    let mut res = Ok(());
    store.read(|c| {
        let tx = c.unchecked_transaction()?;
        res = write_job_in(&tx, row, out);
        Ok(())
        // The drop of `tx` ends the read.
    })?;
    res
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
    out.write_all(b"}").map_err(io)
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
