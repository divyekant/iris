//! Notices by rule (card R5-08): the routes `/api/notices`.

use iris_store::{format_id, parse_id, Kind, Store};
use serde::Serialize;

use crate::read::{rfc3339, summaries};
use crate::{Error, MessageSummary, Result};

#[derive(Debug, Serialize, PartialEq)]
pub struct Notice {
    /// The `n-` ID.
    pub id: String,
    /// The rule and its name at the time of the notice.
    pub rule: u32,
    pub rule_name: String,
    /// The sender and the subject, one line each.
    pub title: String,
    pub text: String,
    pub created: String,
    /// The mail. `None` after the sync marked it gone.
    pub message: Option<MessageSummary>,
}

/// The notices that are not done, newest first, at most 50.
pub fn notices(store: &Store) -> Result<Vec<Notice>> {
    Ok(store.read(|c| shape(c, iris_policy::notice::list(c)?))?)
}

/// One page of the notices for the Mac app (`docs/specs/2026-11-17-mac-app.md`):
/// the notices that are not done with a row ID larger than `after`, oldest
/// first, at most `limit`. Also the count of notices that are not done, and
/// the largest row ID of all notices. One snapshot.
pub fn notices_after(
    store: &Store,
    after: i64,
    limit: i64,
) -> Result<(Vec<(i64, Notice)>, i64, i64)> {
    Ok(store.read(|c| {
        let page = iris_policy::notice::after(c, after, limit)?;
        let rows: Vec<i64> = page.iter().map(|n| n.id).collect();
        let (open, max) = iris_policy::notice::counts(c)?;
        Ok((rows.into_iter().zip(shape(c, page)?).collect(), open, max))
    })?)
}

fn shape(
    c: &rusqlite::Connection,
    list: Vec<iris_policy::notice::Notice>,
) -> iris_store::Result<Vec<Notice>> {
    let mut out = Vec::new();
    for n in list {
        out.push(Notice {
            id: format_id(Kind::Notice, n.id),
            rule: n.rule,
            rule_name: n.rule_name,
            title: n.title,
            text: n.text,
            created: rfc3339(n.created_at),
            message: summaries(c, &[n.message_id])?.pop(),
        });
    }
    Ok(out)
}

/// Marks a notice done by its `n-` ID. Gives false if there is no such notice.
pub fn notice_done(store: &Store, id: &str, now_ms: i64) -> Result<bool> {
    match parse_id(id) {
        Some((Kind::Notice, row)) => Ok(iris_policy::notice::done(store, row, now_ms)?),
        _ => Err(Error::Query(format!("not a notice ID: {id:?}"))),
    }
}
