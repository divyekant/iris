//! Watches (card R5-07, `docs/specs/2026-11-16-watch.md`): a search text
//! that gives one notice when a new message matches.

use rusqlite::{params, OptionalExtension};

use crate::{Error, Query, Result, Store};

/// The most characters of the text of a watch.
pub const TEXT_MAX: usize = 500;
/// The most watches that wait at one time. Each check of the engine runs
/// one search for each.
// shortcut: a fixed number, so the check stays small. Make it a setting if a
// person needs more.
pub const WAIT_MAX: i64 = 100;
/// A match must have a date at most this long before the watch. Mail that
/// Gmail got just before the watch, and that was not in the mirror yet, can
/// match. Old mail that a first sync or a full resync adds cannot.
pub const SINCE_MARGIN_MS: i64 = 3_600_000;

/// One watch, as `iris watch list` and the API show it.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Watch {
    pub id: i64,
    pub text: String,
    pub created_at: i64,
    /// The first new message that matched. `None` while the watch waits.
    pub message: Option<i64>,
    pub arrived_at: Option<i64>,
}

impl Store {
    /// Add a watch for `text` (the search syntax of specification 1,
    /// section 5.6). Only a message that the mirror gets after this call
    /// can match. Gives the row ID.
    pub fn add_watch(&self, text: &str, now_ms: i64) -> Result<i64> {
        let text = text.trim();
        if text.is_empty() {
            return Err(Error::Watch("the watch has no search text"));
        }
        if text.chars().count() > TEXT_MAX {
            return Err(Error::Watch("the search text has more than 500 characters"));
        }
        Query::parse(text)?;
        self.write(|tx| {
            let waiting: i64 = tx.query_row(
                "SELECT count(*) FROM watch WHERE message_id IS NULL",
                [],
                |r| r.get(0),
            )?;
            if waiting >= WAIT_MAX {
                return Err(Error::Watch("100 watches wait already: remove one first"));
            }
            tx.execute(
                "INSERT INTO watch(text, created_at, after_message)
                 VALUES (?1, ?2, (SELECT coalesce(max(id), 0) FROM message))",
                params![text, now_ms],
            )?;
            Ok(tx.last_insert_rowid())
        })
    }

    /// Each watch, newest first.
    pub fn watches(&self) -> Result<Vec<Watch>> {
        self.read(|c| {
            let mut st = c.prepare(
                "SELECT id, text, created_at, message_id, arrived_at FROM watch ORDER BY id DESC",
            )?;
            let rows = st.query_map([], |r| {
                Ok(Watch {
                    id: r.get(0)?,
                    text: r.get(1)?,
                    created_at: r.get(2)?,
                    message: r.get(3)?,
                    arrived_at: r.get(4)?,
                })
            })?;
            Ok(rows.collect::<rusqlite::Result<_>>()?)
        })
    }

    /// Remove a watch: one that waits, or one whose notice the person saw.
    /// Gives false if there is no such watch.
    pub fn remove_watch(&self, id: i64) -> Result<bool> {
        self.write(|tx| Ok(tx.execute("DELETE FROM watch WHERE id = ?1", [id])? > 0))
    }

    /// Find the first new match of each watch that waits, and keep it. Each
    /// watch matches one time only. Gives the pairs (watch, message) that
    /// matched in this call.
    pub fn check_watches(&self, now_ms: i64) -> Result<Vec<(i64, i64)>> {
        let waiting: Vec<(i64, String, i64, i64)> = self.read(|c| {
            let mut st = c.prepare(
                "SELECT id, text, created_at, after_message FROM watch
                 WHERE message_id IS NULL ORDER BY id",
            )?;
            let rows = st.query_map([], |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?, r.get(3)?)))?;
            Ok(rows.collect::<rusqlite::Result<_>>()?)
        })?;
        let mut found = Vec::new();
        for (id, text, created_at, after) in waiting {
            // The text was valid at the add. If a later version of the
            // syntax refuses it, the watch waits, and the list shows it.
            let Ok(q) = Query::parse(&text) else { continue };
            // The search and the write are one transaction, so a message
            // that goes gone between the two cannot match.
            let hit = self.write(|tx| {
                let Some(m) = q.first_arrival(tx, after, created_at - SINCE_MARGIN_MS)? else {
                    return Ok(None);
                };
                let set = tx.execute(
                    "UPDATE watch SET message_id = ?2, arrived_at = ?3
                     WHERE id = ?1 AND message_id IS NULL",
                    params![id, m, now_ms],
                )?;
                Ok((set > 0).then_some(m))
            })?;
            if let Some(m) = hit {
                found.push((id, m));
            }
        }
        Ok(found)
    }

    /// One watch by row ID.
    pub fn watch(&self, id: i64) -> Result<Option<Watch>> {
        self.read(|c| {
            Ok(c.query_row(
                "SELECT id, text, created_at, message_id, arrived_at FROM watch WHERE id = ?1",
                [id],
                |r| {
                    Ok(Watch {
                        id: r.get(0)?,
                        text: r.get(1)?,
                        created_at: r.get(2)?,
                        message: r.get(3)?,
                        arrived_at: r.get(4)?,
                    })
                },
            )
            .optional()?)
        })
    }
}
