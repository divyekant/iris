//! The wake rule of a waiting job (R5-04, docs/specs/2026-11-17-wait.md).
//! It lives here, next to the grant, because a reply joins the grant: the
//! rule that counts a reply is the rule of the workspace build.

use iris_job::{By, State};
use iris_store::Store;
use rusqlite::{params, Connection};

use crate::{readable, Result};

/// The replies on the wait thread of the waiting `job`: each message of an
/// active thread ref of the job on that thread that is not in the list of
/// the ref yet, not sent by you (no label `SENT` or `DRAFT`), and that the
/// workspace can read ([`readable`]). Gives the scope row and the messages.
/// No wait thread, a revoked ref or a job that does not wait gives none.
pub fn replies_in(c: &Connection, job: i64) -> Result<Option<(i64, Vec<i64>)>> {
    let j = iris_job::get_in(c, job)?;
    let Some(thread) = j
        .wait
        .and_then(|w| w.thread)
        .filter(|_| j.state == State::Waiting)
    else {
        return Ok(None);
    };
    let mut st = c.prepare_cached(
        "SELECT s.id, m.id FROM job_scope s JOIN message m ON m.thread_id = s.thread_id
         WHERE s.job_id = ?1 AND s.thread_id = ?2 AND s.revoked_at IS NULL
           AND NOT EXISTS (SELECT 1 FROM job_scope_message sm
                           WHERE sm.scope_id = s.id AND sm.message_id = m.id)
           AND NOT EXISTS (SELECT 1 FROM message_label ml JOIN label l ON l.id = ml.label_id
                           WHERE ml.message_id = m.id AND l.provider_id IN ('SENT', 'DRAFT'))
         ORDER BY m.id",
    )?;
    let rows: Vec<(i64, i64)> = st
        .query_map(params![job, thread], |r| Ok((r.get(0)?, r.get(1)?)))?
        .collect::<rusqlite::Result<_>>()?;
    let mut scope = None;
    let mut replies = Vec::new();
    for (s, m) in rows {
        if readable(c, m)? {
            scope = Some(s);
            replies.push(m);
        }
    }
    Ok(scope.map(|s| (s, replies)))
}

/// The reason of the wake of `job` at `now_ms`, if one is due: `reply` if
/// the wait thread has a reply, else `time` if the wait ended.
fn due_in(c: &Connection, job: i64, now_ms: i64) -> Result<Option<&'static str>> {
    if replies_in(c, job)?.is_some() {
        return Ok(Some("reply"));
    }
    let j = iris_job::get_in(c, job)?;
    let ended = j.state == State::Waiting && j.wait.is_some_and(|w| w.until_ms <= now_ms);
    Ok(ended.then_some("time"))
}

/// Wakes each waiting job whose wake is due at `now_ms`, by the engine,
/// with the reason in `why`. A reply wake first adds the replies, and only
/// the replies, to the list of the thread ref, so the next session reads
/// them. Each wake is one write transaction that reads the rule again, so a
/// move by the person between the read and the write wins. Gives the woken
/// jobs.
pub fn wake_due(store: &Store, now_ms: i64) -> Result<Vec<i64>> {
    let mut woken = Vec::new();
    for j in iris_job::list(store, State::Waiting)? {
        // A read first: the write lock only when a wake is due.
        let due = store.read(|c| {
            due_in(c, j.id, now_ms).map_err(|e| iris_store::Error::Query(e.to_string()))
        })?;
        if due.is_none() {
            continue;
        }
        let done = iris_job::transaction(store, |c| {
            let Some(why) = due_in(c, j.id, now_ms)? else {
                return Ok(false);
            };
            if let Some((scope, replies)) = replies_in(c, j.id)? {
                for m in replies {
                    c.execute(
                        "INSERT INTO job_scope_message (scope_id, message_id) VALUES (?1, ?2)",
                        params![scope, m],
                    )?;
                }
            }
            iris_job::wake_in(c, j.id, By::Engine, Some(why), now_ms)?;
            Ok(true)
        })?;
        if done {
            woken.push(j.id);
        }
    }
    Ok(woken)
}
