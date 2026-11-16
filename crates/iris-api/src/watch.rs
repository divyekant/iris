//! Watches (card R5-07): the routes `/api/watches` and the verb
//! `iris watch` call these functions.

use iris_store::{format_id, parse_id, Kind, Store};
use serde::Serialize;

use crate::read::{rfc3339, summaries};
use crate::{Error, MessageSummary, Result};

#[derive(Debug, Serialize, PartialEq)]
pub struct Watch {
    /// The `w-` ID.
    pub id: String,
    /// The search text (specification 1, section 5.6).
    pub text: String,
    pub created: String,
    /// `waits`, or `arrived` after the one notice.
    pub state: &'static str,
    /// The time when the engine found the message.
    pub arrived: Option<String>,
    /// The first new message that matched. `None` while the watch waits, and
    /// after the sync marked the message gone.
    pub message: Option<MessageSummary>,
}

/// Each watch, newest first.
pub fn watches(store: &Store) -> Result<Vec<Watch>> {
    let rows = store.watches()?;
    Ok(store.read(|c| {
        let mut out = Vec::with_capacity(rows.len());
        for w in rows {
            let message = match w.message {
                Some(m) => summaries(c, &[m])?.pop(),
                None => None,
            };
            out.push(Watch {
                id: format_id(Kind::Watch, w.id),
                text: w.text,
                created: rfc3339(w.created_at),
                state: if w.message.is_some() {
                    "arrived"
                } else {
                    "waits"
                },
                arrived: w.arrived_at.map(rfc3339),
                message,
            });
        }
        Ok(out)
    })?)
}

/// Add a watch. Gives its `w-` ID.
pub fn add_watch(store: &Store, text: &str, now_ms: i64) -> Result<String> {
    Ok(format_id(Kind::Watch, store.add_watch(text, now_ms)?))
}

/// Remove a watch by its `w-` ID. Gives false if there is no such watch.
pub fn remove_watch(store: &Store, id: &str) -> Result<bool> {
    match parse_id(id) {
        Some((Kind::Watch, row)) => Ok(store.remove_watch(row)?),
        _ => Err(Error::Query(format!("not a watch ID: {id:?}"))),
    }
}
