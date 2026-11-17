//! Review comments (docs/specs/2026-11-17-review-comments.md, card R4-08):
//! the person comments on a revision of a changeset of a job, and the next
//! build of the job workspace gives the comment to the agent.

use rusqlite::{params, Connection, OptionalExtension};

use crate::{refused, Error, Result};

/// The most characters of one comment.
pub const TEXT_MAX: usize = 4000;

/// One comment of the person.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Comment {
    pub id: i64,
    pub changeset: i64,
    pub revision: i64,
    pub text: String,
    pub at_ms: i64,
}

/// The reason why `text` or `key` breaks the rules of the table, or `None`.
pub fn check_comment(text: &str, key: &str) -> Option<String> {
    if text.trim().is_empty() || text.chars().count() > TEXT_MAX || text.contains('\0') {
        return Some(format!(
            "the text must have 1 to {TEXT_MAX} characters, not only white space, and no NUL"
        ));
    }
    let key_ok = (16..=64).contains(&key.len())
        && key
            .bytes()
            .all(|b| b.is_ascii_alphanumeric() || b == b'_' || b == b'-');
    (!key_ok).then(|| "the key must have 16 to 64 characters of A-Z, a-z, 0-9, _ and -".into())
}

/// Write one comment on `revision` of `changeset` (spec, section 2), in a
/// write transaction of the caller. A second call with the same key and
/// the same body gives the first comment and writes nothing.
pub fn comment_in(
    c: &Connection,
    changeset: i64,
    revision: i64,
    text: &str,
    key: &str,
    now_ms: i64,
) -> Result<Comment> {
    let row: Option<(i64, Option<String>, String)> = c
        .query_row(
            "SELECT revision, author_job, state FROM changeset WHERE id = ?1",
            [changeset],
            |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?)),
        )
        .optional()?;
    let (newest, job, state) = row.ok_or(Error::NotFound)?;
    let earlier: Option<i64> = c
        .query_row(
            "SELECT id FROM changeset_comment WHERE changeset_id = ?1 AND key = ?2",
            params![changeset, key],
            |r| r.get(0),
        )
        .optional()?;
    if let Some(id) = earlier {
        let first = get(c, id)?;
        if first.revision != revision || first.text != text {
            return Err(refused("the key names another comment".into()));
        }
        return Ok(first);
    }
    let job = job.ok_or_else(|| refused("only a changeset of a job takes a comment".into()))?;
    // The review screen shows only open changesets: a draft has a revision
    // that the person cannot see yet.
    match state.as_str() {
        "open" => {}
        "draft" => return Err(refused("a new revision is a draft".into())),
        other => return Err(refused(format!("the changeset is {other}"))),
    }
    if revision != newest {
        return Err(refused("the changeset has a newer revision".into()));
    }
    let job = iris_job::parse_job_id(&job)
        .ok_or_else(|| refused("the changeset names no valid job".into()))?;
    let j = iris_job::get_in(c, job).map_err(|e| refused(e.to_string()))?;
    if j.state == iris_job::State::Done {
        return Err(refused("the job is done".into()));
    }
    if let Some(why) = check_comment(text, key) {
        return Err(refused(why));
    }
    c.execute(
        "INSERT INTO changeset_comment (changeset_id, revision, text, key, at_ms)
         VALUES (?1, ?2, ?3, ?4, ?5)",
        params![changeset, revision, text, key, now_ms],
    )?;
    let id = c.last_insert_rowid();
    // So a session can start for the comment (spec, section 2, step 9).
    if j.state == iris_job::State::Waiting {
        iris_job::wake_in(c, job, iris_job::By::Person, Some("comment"), now_ms)
            .map_err(|e| refused(e.to_string()))?;
    }
    get(c, id)
}

/// If `job` has a pending comment: a comment with a higher ID than the
/// delivery of each session of the job (spec, section 5).
pub fn comment_pending_in(c: &Connection, job: i64) -> Result<bool> {
    Ok(c.query_row(
        "SELECT EXISTS (SELECT 1 FROM changeset_comment k JOIN changeset s ON s.id = k.changeset_id
           WHERE s.author_job = ?1 AND k.id > coalesce((SELECT max(d.comment_id)
             FROM comment_delivery d JOIN job_session e ON e.id = d.session_id
             WHERE e.job_id = ?2), 0))",
        params![iris_job::format_job_id(job), job],
        |r| r.get(0),
    )?)
}

/// Each comment of `changeset`, oldest first.
pub fn comments_in(c: &Connection, changeset: i64) -> Result<Vec<Comment>> {
    let mut q = c.prepare_cached(
        "SELECT id, changeset_id, revision, text, at_ms FROM changeset_comment
         WHERE changeset_id = ?1 ORDER BY id",
    )?;
    let rows = q.query_map([changeset], comment)?;
    Ok(rows.collect::<rusqlite::Result<_>>()?)
}

fn get(c: &Connection, id: i64) -> Result<Comment> {
    Ok(c.query_row(
        "SELECT id, changeset_id, revision, text, at_ms FROM changeset_comment WHERE id = ?1",
        [id],
        comment,
    )?)
}

fn comment(r: &rusqlite::Row) -> rusqlite::Result<Comment> {
    Ok(Comment {
        id: r.get(0)?,
        changeset: r.get(1)?,
        revision: r.get(2)?,
        text: r.get(3)?,
        at_ms: r.get(4)?,
    })
}
