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

/// The count of changeset IDs that the list reads in one query.
const PAGE: i64 = 64;

/// `{"changesets": [...]}`: each open changeset, the newest first. Each
/// document streams to `out`, and the IDs come in pages of [`PAGE`], so
/// memory does not grow with the count of open changesets (section 9).
pub fn write_open_changesets(store: &Store, out: &mut impl Write) -> Result<()> {
    write_open_paged(store, out, PAGE)
}

fn write_open_paged(store: &Store, out: &mut impl Write, page: i64) -> Result<()> {
    out.write_all(b"{\"changesets\":[").map_err(io)?;
    let mut first = true;
    // Keyset on the row ID: an ID never changes, so a changeset that a new
    // revision moves in time is not listed twice or left out.
    let mut before = i64::MAX;
    loop {
        let ids: Vec<i64> = store.read(|c| {
            let mut st = c.prepare_cached(
                "SELECT id FROM changeset WHERE state = 'open' AND id < ?1
                 ORDER BY id DESC LIMIT ?2",
            )?;
            let ids = st
                .query_map([before, page], |r| r.get(0))?
                .collect::<rusqlite::Result<_>>()?;
            Ok(ids)
        })?;
        let Some(&last) = ids.last() else { break };
        for id in ids {
            if !first {
                out.write_all(b",").map_err(io)?;
            }
            first = false;
            // A changeset row is never deleted, so each listed ID has its document.
            iris_changeset::write_document(store, id, out).map_err(changeset_error)?;
        }
        before = last;
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

#[cfg(test)]
mod tests {
    use super::*;

    /// The list reads the IDs in pages, and each page continues after the
    /// last ID of the page before: no changeset twice, none left out.
    #[test]
    fn the_list_pages_through_each_open_changeset() {
        let dir = std::env::temp_dir().join(format!("iris-api-pages-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        let store = Store::open(&dir).unwrap();
        let account = store.add_account("gmail", "owner@example.com").unwrap();
        let message = iris_store::NewMessage {
            provider_id: "g1".into(),
            thread_provider_id: "gt1".into(),
            ..Default::default()
        };
        store.put_messages(account, &[message]).unwrap();
        let author = iris_changeset::Author {
            kind: iris_changeset::AuthorKind::Agent,
            name: Some("Claude Code".into()),
            session: None,
            job: None,
        };
        let none = serde_json::Map::new();
        for i in 1..=7 {
            let cs = iris_changeset::create(&store, &author, i).unwrap();
            let verb = iris_changeset::Verb::Archive;
            iris_changeset::add_action(&store, cs, &author, verb, &["m-1"], &none, None).unwrap();
            // Changeset 4 stays a draft, so it is not open.
            if i != 4 {
                iris_changeset::submit(&store, cs, &author, "Tidy up.", i).unwrap();
            }
        }
        for page in [1, 2, 3, 6, 7, 64] {
            let mut out = Vec::new();
            write_open_paged(&store, &mut out, page).unwrap();
            let v: Value = serde_json::from_slice(&out).unwrap();
            let ids: Vec<&str> = v["changesets"]
                .as_array()
                .unwrap()
                .iter()
                .map(|c| c["id"].as_str().unwrap())
                .collect();
            assert_eq!(
                ids,
                ["cs-7", "cs-6", "cs-5", "cs-3", "cs-2", "cs-1"],
                "page {page}"
            );
        }
        let _ = std::fs::remove_dir_all(&dir);
    }
}
