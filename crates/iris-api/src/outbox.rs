//! The outbox of the web client (docs/specs/2026-11-17-held-outbox.md,
//! section 6): each approved and held send, the state of the send switch,
//! and the cancel of the person. The API reads the switch, but it cannot
//! change it: only `iris sending on|off` at the terminal changes it.

use iris_store::{format_id, Kind, Store};
use rusqlite::{params, Connection};
use serde_json::{json, Value};

use crate::read::rfc3339;
use crate::review::changeset_error;
use crate::Result;

/// The most sends in one page. A next page gives the rest: each send that
/// waits is reachable (`next` and `count`), so no send leaves unseen.
pub const MAX_SENDS: u32 = 200;

/// `GET /api/outbox?limit=&after=`: `{"sending": {"on": bool, "since":
/// time|null}, "count": n, "sends": [...], "next": cursor|null}`. `count`
/// is the number of all sends that wait. The sends come in the order of
/// their action ID, `limit` (1 to 200, default 200) at a time; `next` is
/// the `after` of the next page. Each send has the draft ID, the state
/// (`approved` or `held`), the time of the approval, the time that it can
/// leave (`null` while the switch is off), the flags, and the recipients
/// and subject of its draft. The recipients and the subject come from the
/// stored raw bytes of the draft, the bytes that Gmail gets. A send whose
/// bytes Iris cannot read has `"readable": false`.
pub fn outbox(store: &Store, limit: u32, after: Option<&str>) -> Result<Value> {
    if !(1..=MAX_SENDS).contains(&limit) {
        return Err(crate::Error::Query(format!(
            "limit must be 1 to {MAX_SENDS}"
        )));
    }
    let after: i64 = match after {
        None | Some("") => 0,
        Some(t) => t.parse().ok().filter(|n: &i64| *n >= 0).ok_or_else(|| {
            crate::Error::Query(format!("after {t} is not the next value of a page"))
        })?,
    };
    // One read transaction: the switch, the count, the page and each draft
    // come from one snapshot, as the engine changes sends at the same time.
    Ok(store.read(|conn| {
        let tx = conn.unchecked_transaction()?;
        let c: &Connection = &tx;
        let on: Option<i64> =
            c.query_row("SELECT on_at FROM send_switch WHERE id = 1", [], |r| {
                r.get(0)
            })?;
        const OPEN: &str = "FROM changeset_action a JOIN action_draft ad ON ad.action_id = a.id
             WHERE a.verb = 'send' AND a.state IN ('approved', 'held')";
        let count: i64 = c.query_row(&format!("SELECT count(*) {OPEN}"), [], |r| r.get(0))?;
        let mut st = c.prepare(&format!(
            "SELECT a.id, ad.draft_id, a.state, a.decided_at, a.flags {OPEN}
               AND a.id > ?1 ORDER BY a.id LIMIT ?2"
        ))?;
        let rows: Vec<(i64, i64, String, i64, Option<String>)> = st
            .query_map(params![after, i64::from(limit) + 1], |r| {
                Ok((r.get(0)?, r.get(1)?, r.get(2)?, r.get(3)?, r.get(4)?))
            })?
            .collect::<rusqlite::Result<_>>()?;
        let more = rows.len() > limit as usize;
        let rows = &rows[..rows.len().min(limit as usize)];
        let next = if more {
            rows.last().map(|r| r.0.to_string())
        } else {
            None
        };
        let mut sends = Vec::with_capacity(rows.len());
        for (action, draft, state, decided_at, flags) in rows.iter().cloned() {
            sends.push(send(c, action, draft, &state, decided_at, flags, on)?);
        }
        drop(st);
        tx.finish()?;
        Ok(json!({
            "sending": {"on": on.is_some(), "since": on.map(rfc3339)},
            "count": count,
            "sends": sends,
            "next": next,
        }))
    })?)
}

fn send(
    c: &Connection,
    action: i64,
    draft: i64,
    state: &str,
    decided_at: i64,
    flags: Option<String>,
    on: Option<i64>,
) -> iris_store::Result<Value> {
    let flags: Vec<String> =
        serde_json::from_str(flags.as_deref().unwrap_or("[]")).unwrap_or_default();
    let leaves = on.map(|on| rfc3339(decided_at.max(on) + iris_apply::HOLD_MS));
    let mut v = json!({
        "draft": format_id(Kind::Draft, draft),
        "state": state,
        "approved": rfc3339(decided_at),
        "leaves": leaves,
        "flags": flags,
    });
    match iris_changeset::action_content(c, action).map_err(|e| match changeset_error(e) {
        crate::Error::Store(e) => e,
        other => iris_store::Error::Query(other.to_string()),
    })? {
        Some((_, content)) => {
            v["readable"] = true.into();
            v["to"] = content.to.into();
            v["cc"] = content.cc.into();
            v["bcc"] = content.bcc.into();
            v["subject"] = content.subject.into();
            v["attachments"] = crate::drafts::files(&content.attachments).into();
        }
        None => v["readable"] = false.into(),
    }
    Ok(v)
}

/// The answer of `POST /api/outbox/{d-N}/cancel`.
#[derive(Debug, PartialEq, Eq)]
pub enum Cancelled {
    /// The send is cancelled, and it does not leave.
    Done,
    /// The release claimed the send: it cannot be cancelled.
    Started,
    /// The draft has no send that waits.
    NotOpen,
}

/// The cancel of the person in the outbox: the open send of the draft
/// `draft` (a row ID) becomes `denied` by `person`.
pub fn cancel_send(store: &Store, draft: i64, now_ms: i64) -> Result<Cancelled> {
    Ok(
        match iris_changeset::cancel_draft(store, draft, now_ms).map_err(changeset_error)? {
            Some(iris_changeset::Cancel::Done) => Cancelled::Done,
            Some(iris_changeset::Cancel::Started) => Cancelled::Started,
            Some(iris_changeset::Cancel::NotOpen) | None => Cancelled::NotOpen,
        },
    )
}
