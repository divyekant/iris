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

/// The most sends in one answer. The outbox holds only sends that a
/// person approved and that wait for their hold or the switch, so the
/// count stays small.
pub const MAX_SENDS: i64 = 200;

/// `GET /api/outbox`: `{"sending": {"on": bool, "since": time|null},
/// "sends": [...]}`. Each send has the draft ID, the state (`approved` or
/// `held`), the time of the approval, the time that it can leave (`null`
/// while the switch is off), the flags, and the recipients and subject of
/// its draft. The recipients and the subject come from the stored raw
/// bytes of the draft, the bytes that Gmail gets. A send whose bytes Iris
/// cannot read has `"readable": false`.
pub fn outbox(store: &Store) -> Result<Value> {
    Ok(store.read(|c| {
        let on: Option<i64> =
            c.query_row("SELECT on_at FROM send_switch WHERE id = 1", [], |r| {
                r.get(0)
            })?;
        let mut st = c.prepare(
            "SELECT a.id, ad.draft_id, a.state, a.decided_at, a.flags
             FROM changeset_action a JOIN action_draft ad ON ad.action_id = a.id
             WHERE a.verb = 'send' AND a.state IN ('approved', 'held')
             ORDER BY a.decided_at, a.id LIMIT ?1",
        )?;
        let rows: Vec<(i64, i64, String, i64, Option<String>)> = st
            .query_map(params![MAX_SENDS], |r| {
                Ok((r.get(0)?, r.get(1)?, r.get(2)?, r.get(3)?, r.get(4)?))
            })?
            .collect::<rusqlite::Result<_>>()?;
        let mut sends = Vec::with_capacity(rows.len());
        for (action, draft, state, decided_at, flags) in rows {
            sends.push(send(c, action, draft, &state, decided_at, flags, on)?);
        }
        Ok(json!({
            "sending": {"on": on.is_some(), "since": on.map(rfc3339)},
            "sends": sends,
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
            v["subject"] = content.subject.into();
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
