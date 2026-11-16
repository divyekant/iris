//! The review of the web client (specification 2, section 5.5): the open
//! changesets as JSON documents of section 5.1, and the message summaries
//! of their actions. Read only. The approval comes with the decisions of
//! the changeset store (R2-01 part 3).

use std::io::Write;

use iris_store::{format_id, parse_id, Kind, Store};
use rusqlite::{Connection, OptionalExtension};
use serde::Serialize;
use serde_json::{json, Value};

use crate::read::message_row;
use crate::{Error, Result};

/// The most message IDs in one request of [`messages_by_id`].
pub const MAX_IDS: usize = 200;

/// The open changesets and their actions that wait for a person: `proposed`
/// or `in_review` (sections 5.2 and 5.4).
#[derive(Debug, Serialize, PartialEq, Default)]
pub struct Review {
    pub changesets: i64,
    pub actions: i64,
}

pub(crate) fn review_counts(c: &Connection) -> rusqlite::Result<Review> {
    c.query_row(
        "SELECT count(DISTINCT changeset_id), count(*) FROM changeset_action
         WHERE state IN ('proposed', 'in_review')",
        [],
        |r| {
            Ok(Review {
                changesets: r.get(0)?,
                actions: r.get(1)?,
            })
        },
    )
}

pub(crate) fn changeset_error(e: iris_changeset::Error) -> Error {
    match e {
        iris_changeset::Error::Store(e) => Error::Store(e),
        other => Error::Store(iris_store::Error::Query(other.to_string())),
    }
}

fn io(e: std::io::Error) -> Error {
    Error::Store(e.into())
}

/// `{"changesets": [...]}`: each open changeset, the newest submit first.
/// Each document streams to `out` (section 9).
pub fn write_open_changesets(store: &Store, out: &mut impl Write) -> Result<()> {
    let ids: Vec<i64> = store.read(|c| {
        let mut st = c.prepare(
            "SELECT id FROM changeset WHERE state = 'open' ORDER BY submitted_at DESC, id DESC",
        )?;
        let ids = st
            .query_map([], |r| r.get(0))?
            .collect::<rusqlite::Result<_>>()?;
        Ok(ids)
    })?;
    out.write_all(b"{\"changesets\":[").map_err(io)?;
    for (i, id) in ids.into_iter().enumerate() {
        if i > 0 {
            out.write_all(b",").map_err(io)?;
        }
        // A changeset row is never deleted, so each ID of the list has its document.
        iris_changeset::write_document(store, id, out).map_err(changeset_error)?;
    }
    out.write_all(b"]}").map_err(io)
}

/// The row ID of the changeset `id` (`cs-77`), or `None` when no changeset
/// has it.
pub fn changeset_row(store: &Store, id: &str) -> Result<Option<i64>> {
    let Some(row) = iris_changeset::parse_changeset_id(id) else {
        return Ok(None);
    };
    Ok(store.read(|c| {
        Ok(
            c.query_row("SELECT id FROM changeset WHERE id = ?1", [row], |r| {
                r.get(0)
            })
            .optional()?,
        )
    })?)
}

/// The summaries of the messages `ids` (`m-1,m-2`), in the order asked.
/// A message that is gone, or that is not in the mirror any more, is
/// `{"id", "gone": true}`, so a review shows each target that it names.
pub fn messages_by_id(store: &Store, ids: &str) -> Result<Vec<Value>> {
    if ids.split(',').count() > MAX_IDS {
        return Err(Error::Query(format!("at most {MAX_IDS} IDs")));
    }
    let rows = ids
        .split(',')
        .map(|id| match parse_id(id) {
            Some((Kind::Message, row)) => Ok(row),
            _ => Err(Error::Query(format!("not a message ID: {id:?}"))),
        })
        .collect::<Result<Vec<i64>>>()?;
    Ok(store.read(|c| {
        let mut out = Vec::with_capacity(rows.len());
        for &row in &rows {
            out.push(match message_row(c, row)? {
                Some((summary, more)) if !more.gone => {
                    let mut v = serde_json::to_value(summary).unwrap_or(Value::Null);
                    v["gone"] = false.into();
                    v
                }
                _ => json!({ "id": format_id(Kind::Message, row), "gone": true }),
            });
        }
        Ok(out)
    })?)
}
