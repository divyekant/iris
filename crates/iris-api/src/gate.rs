//! The stranger gate of the web client (specification 1, section 6.7): the
//! senders whose items wait at the gate, the senders that the person
//! declined, and the answers of the person.
//!
//! The gate is the `strangers` section of the brief, by sender. A `hide` or a
//! `bundle` rule that matches an item takes it out of the section, so a rule
//! decides before the gate asks the person.

use iris_senders::gate::{self, Answer};
use iris_store::{format_id, Kind, Store};
use rusqlite::{types::Value as SqlValue, Connection};
use serde::Serialize;
use serde_json::{json, Value};

use crate::read::{
    declined_sql, decode_cursor, doc_of, encode_cursor, head, row_of, section_sql, MAX_LIMIT, SHOWN,
};
use crate::{Error, Result};

/// The most senders of the gate in the brief. `/api/gate` gives all.
pub const GATE_IN_BRIEF: u32 = 20;

/// The newest items of each sender in a page of the gate.
pub const GATE_ITEMS: usize = 3;

/// One sender at the gate.
#[derive(Debug, Serialize, PartialEq)]
pub struct GateSender {
    /// The `p-` ID of the sender row.
    pub id: String,
    pub address: String,
    pub name: Option<String>,
    /// The count of items of the sender at the gate (or declined).
    pub count: i64,
    /// The newest [`GATE_ITEMS`] items, newest first.
    pub items: Vec<Value>,
}

/// One page of the gate.
#[derive(Debug, Serialize, PartialEq, Default)]
pub struct GatePage {
    /// The count of all senders of the list.
    pub count: i64,
    pub senders: Vec<GateSender>,
    /// The `after` value of the next page, or `None` on the last page.
    pub next: Option<String>,
}

/// The time of an item for the order of the gate: the newest message of a
/// conversation, else the origin message, else the time the item was made.
const AT: &str = "coalesce(
    CASE WHEN form = 'conversation'
         THEN CAST(unixepoch(json_extract(doc, '$.latest.at'), 'subsec') * 1000 AS INTEGER) END,
    (SELECT m.internal_date FROM item_source s JOIN message m ON m.id = s.message_id
      WHERE s.item_id = item.id AND s.role = 'origin'),
    created_at)";

/// The items and their senders of the list: the items of the `strangers`
/// section and their stranger parties, or (with `declined`) the items of
/// declined senders and these senders. Gives the common table expressions
/// `g(id, at)` and `party(item, sender)`.
fn parties(now: i64, declined: bool) -> String {
    let base = if declined {
        format!("{} AND {SHOWN}", head(&declined_sql()))
    } else {
        section_sql(now)[5].0.clone()
    };
    let state = if declined { "NOT NULL" } else { "NULL" };
    format!(
        "g AS (SELECT id, form, doc, {AT} AS at FROM item WHERE {base}),
         cand(item, sender) AS (
           SELECT id, CAST(substr(json_extract(doc, '$.counterparty.sender'), 3) AS INTEGER)
             FROM g WHERE form = 'commitment' AND json_extract(doc, '$.direction') = 'you_owe'
           UNION
           SELECT id, CAST(substr(json_extract(doc, '$.owner.sender'), 3) AS INTEGER)
             FROM g WHERE form = 'commitment' AND json_extract(doc, '$.direction') = 'owed_to_you'
           UNION
           SELECT g.id, CAST(substr(json_extract(p.value, '$.sender'), 3) AS INTEGER)
             FROM g, json_each(g.doc, '$.participants') p
            WHERE g.form = 'conversation' AND json_extract(p.value, '$.kind') = 'sender'),
         party(item, sender) AS (
           SELECT cand.item, cand.sender FROM cand JOIN sender s ON s.id = cand.sender
            WHERE coalesce(s.kind, 'stranger') = 'stranger' AND s.declined_at IS {state})"
    )
}

/// The count of senders of the gate (or of the declined senders).
pub(crate) fn count_gate(c: &Connection, now: i64, declined: bool) -> rusqlite::Result<i64> {
    c.query_row(
        &format!(
            "WITH {} SELECT count(DISTINCT sender) FROM party",
            parties(now, declined)
        ),
        [],
        |r| r.get(0),
    )
}

/// One page of `limit` senders after the sort keys `after`: newest first by
/// the newest item of each sender.
pub(crate) fn read_gate(
    c: &Connection,
    now: i64,
    declined: bool,
    limit: u32,
    after: Option<&[SqlValue]>,
) -> rusqlite::Result<GatePage> {
    let with = parties(now, declined);
    let seek = if after.is_some() {
        "(at, id) < (?1, ?2)"
    } else {
        "1"
    };
    let mut st = c.prepare(&format!(
        "WITH {with},
         s AS (SELECT sender.id, sender.address, sender.name, count(*) AS n, max(g.at) AS at
                 FROM party JOIN sender ON sender.id = party.sender JOIN g ON g.id = party.item
                GROUP BY sender.id)
         SELECT id, address, name, n, at FROM s WHERE {seek}
         ORDER BY at DESC, id DESC LIMIT {}",
        limit + 1
    ))?;
    let mut rows: Vec<(i64, String, Option<String>, i64, i64)> = st
        .query_map(rusqlite::params_from_iter(after.unwrap_or_default()), |r| {
            Ok((r.get(0)?, r.get(1)?, r.get(2)?, r.get(3)?, r.get(4)?))
        })?
        .collect::<rusqlite::Result<_>>()?;
    let more = rows.len() > limit as usize;
    rows.truncate(limit as usize);
    let name = if declined { "declined" } else { "gate" };
    let next = more
        .then(|| rows.last())
        .flatten()
        .map(|r| encode_cursor(name, &[SqlValue::Integer(r.4), SqlValue::Integer(r.0)]));

    // The newest items of each sender of the page, in one query.
    let ids = rows
        .iter()
        .map(|r| r.0.to_string())
        .collect::<Vec<_>>()
        .join(",");
    let mut items: std::collections::HashMap<i64, Vec<Value>> = Default::default();
    if !ids.is_empty() {
        let mut st = c.prepare(&format!(
            "WITH {with},
             r AS (SELECT party.sender AS sid, g.id, g.at,
                          row_number() OVER (PARTITION BY party.sender ORDER BY g.at DESC, g.id DESC) AS k
                     FROM party JOIN g ON g.id = party.item WHERE party.sender IN ({ids}))
             SELECT item.id, item.form, item.status, item.doc, r.sid FROM r JOIN item ON item.id = r.id
              WHERE r.k <= {GATE_ITEMS} ORDER BY r.sid, r.at DESC, r.id DESC"
        ))?;
        for row in st.query_map([], |r| Ok((r.get::<_, i64>(4)?, doc_of(r)?)))? {
            let (sid, doc) = row?;
            items.entry(sid).or_default().push(doc);
        }
    }
    Ok(GatePage {
        count: count_gate(c, now, declined)?,
        senders: rows
            .into_iter()
            .map(|(id, address, name, count, _)| GateSender {
                id: format_id(Kind::Sender, id),
                address,
                name,
                count,
                items: items.remove(&id).unwrap_or_default(),
            })
            .collect(),
        next,
    })
}

/// One page of the senders that wait at the gate, or (with `declined`) of the
/// declined senders, at the time `now`. `after` is the `next` value of the
/// page before.
pub fn gate_page(
    store: &Store,
    now: i64,
    declined: bool,
    limit: u32,
    after: Option<&str>,
) -> Result<GatePage> {
    if !(1..=MAX_LIMIT).contains(&limit) {
        return Err(Error::Query(format!("limit must be 1 to {MAX_LIMIT}")));
    }
    let name = if declined { "declined" } else { "gate" };
    let after = after.map(|t| decode_cursor(t, name, 2)).transpose()?;
    Ok(store.read(|c| Ok(read_gate(c, now, declined, limit, after.as_deref())?))?)
}

/// The answer `answer` (`accept`, `decline` or `reopen`) of the person to the
/// sender `id`. Gives the sender row after the answer, or `None` when no
/// sender has the ID or the answer is not known. A sender that is not a
/// stranger now is [`Error::Conflict`].
///
/// The answer and the understanding again of the mail of each sender whose
/// hostile state changed are one transaction (specification 1, section 6.7).
pub fn answer(store: &Store, id: &str, answer: &str, now_ms: i64) -> Result<Option<Value>> {
    type Gate = fn(&Connection, i64, i64) -> rusqlite::Result<Answer>;
    let f: Gate = match answer {
        "accept" => gate::accept,
        "decline" => gate::decline,
        "reopen" => gate::reopen,
        _ => return Ok(None),
    };
    let row = row_of(id, Kind::Sender)?;
    let result = store.write(|tx| {
        Ok(match f(tx, row, now_ms)? {
            Answer::Done { rerun } => {
                iris_understand::store::rerun_senders(tx, &rerun, now_ms)?;
                Ok(Some(sender(tx, row)?))
            }
            Answer::NoSender => Ok(None),
            Answer::NotStranger(kind) => Err(kind),
        })
    })?;
    result.map_err(|kind| {
        Error::Conflict(
            match kind {
                Some(iris_senders::Kind::Hostile) => {
                    "Iris marks this sender as hostile. Its mail stays in quarantine."
                }
                _ => "This sender is not a stranger now. Look at the brief again.",
            }
            .into(),
        )
    })
}

/// The sender row `row` as the answer of the API.
fn sender(c: &Connection, row: i64) -> rusqlite::Result<Value> {
    c.query_row(
        "SELECT address, name, coalesce(kind, 'stranger'), decided_by, declined_at IS NOT NULL
           FROM sender WHERE id = ?1",
        [row],
        |r| {
            Ok(json!({
                "id": format_id(Kind::Sender, row),
                "address": r.get::<_, String>(0)?,
                "name": r.get::<_, Option<String>>(1)?,
                "kind": r.get::<_, String>(2)?,
                "decided_by": r.get::<_, Option<String>>(3)?,
                "declined": r.get::<_, bool>(4)?,
            }))
        },
    )
}
