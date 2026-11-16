//! The stranger gate (specification 1, section 6.7): the person accepts a
//! sender of the kind `stranger`, declines it, or reopens the decision.
//!
//! - Accept makes the kind `person`, decided by the person. The engine
//!   never changes it.
//! - Decline sets `declined_at`. The kind stays with the engine, so a
//!   declined sender can still become `hostile`, and a declined sender that
//!   becomes known leaves the decline behind. The decline applies only while
//!   the kind is `stranger`.
//! - Reopen removes both decisions. The engine decides the kind again.
//!
//! Each function runs in the transaction of the caller, and gives the
//! `sender` rows whose kind became `hostile` or stopped being `hostile`
//! (as [`crate::refresh`] does). The caller runs understanding again on
//! the mail of these rows in the same transaction.

use rusqlite::{params, Connection, OptionalExtension};

use crate::{domain_of, recheck, rules, sweep, Changes, Kind};

/// The `basis` of a sender that the person accepted at the gate.
pub const ACCEPTED: &str = "accepted at the gate";

/// The result of one answer at the gate.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Answer {
    /// The row has the answer now. `rerun` as in [`crate::Decision::rerun`].
    Done { rerun: Vec<i64> },
    /// No `sender` row has the ID.
    NoSender,
    /// The sender is not at the gate: its kind is not `stranger`, or a
    /// person set its kind for another reason. Nothing changed.
    NotStranger(Option<Kind>),
}

/// The state of one row: (address, kind, decided_by).
type Row = (String, Option<String>, Option<String>);

fn row(conn: &Connection, id: i64) -> rusqlite::Result<Option<Row>> {
    conn.query_row(
        "SELECT address, kind, decided_by FROM sender WHERE id = ?1",
        [id],
        |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?)),
    )
    .optional()
}

/// `None` when the row waits at the gate or was declined there: the stored
/// kind is `stranger` (or no kind yet), and the engine rules give `stranger`
/// now, unless a person set the kind. Else the answer that refuses the
/// change. The rules run again because the stored kind can
/// be older than the evidence (it changes at the next refresh): a person
/// must never accept a sender that is `hostile` now. A refusal writes
/// nothing; the next refresh writes the new kind.
fn refuse(
    conn: &Connection,
    addr: &str,
    kind: Option<&str>,
    by: Option<&str>,
) -> rusqlite::Result<Option<Answer>> {
    // The stored kind is what the person saw: only a `stranger` is at the gate.
    let stored = kind.and_then(Kind::parse);
    if kind.is_some() && stored != Some(Kind::Stranger) {
        return Ok(Some(Answer::NotStranger(stored)));
    }
    if by == Some("person") {
        // A person set the kind: the engine rules do not apply.
        return Ok(None);
    }
    let (now, _) = rules(conn, addr)?;
    Ok((now != Kind::Stranger).then_some(Answer::NotStranger(Some(now))))
}

/// Accept the sender `id`: the kind becomes `person`, decided by the person.
/// An accepted sender gives `Done` again with no change. The domain of the
/// sender becomes known, so the near-copy rule decides its look-alikes again.
pub fn accept(conn: &Connection, id: i64, now_ms: i64) -> rusqlite::Result<Answer> {
    let Some((addr, kind, by)) = row(conn, id)? else {
        return Ok(Answer::NoSender);
    };
    if by.as_deref() == Some("person") && kind.as_deref() == Some(Kind::Person.name()) {
        return Ok(Answer::Done { rerun: Vec::new() });
    }
    if let Some(no) = refuse(conn, &addr, kind.as_deref(), by.as_deref())? {
        return Ok(no);
    }
    conn.execute(
        "UPDATE sender SET kind = 'person', basis = ?2, decided_by = 'person',
                decided_at = ?3, declined_at = NULL
          WHERE id = ?1",
        params![id, ACCEPTED, now_ms],
    )?;
    let mut ch = Changes {
        domains: vec![domain_of(&addr).to_string()],
        ..Changes::default()
    };
    sweep(conn, now_ms, &mut ch)?;
    Ok(Answer::Done { rerun: ch.rerun })
}

/// Decline the sender `id`. A declined sender gives `Done` again and keeps
/// the time of the first decline.
pub fn decline(conn: &Connection, id: i64, now_ms: i64) -> rusqlite::Result<Answer> {
    let Some((addr, kind, by)) = row(conn, id)? else {
        return Ok(Answer::NoSender);
    };
    if let Some(no) = refuse(conn, &addr, kind.as_deref(), by.as_deref())? {
        return Ok(no);
    }
    conn.execute(
        "UPDATE sender SET declined_at = coalesce(declined_at, ?2) WHERE id = ?1",
        params![id, now_ms],
    )?;
    Ok(Answer::Done { rerun: Vec::new() })
}

/// Remove the answer of the person for the sender `id`: the decline, and a
/// kind that the person set. The engine decides the kind again from the
/// evidence in the store, as for a row that it never saw. With no answer,
/// `Done` and no change.
pub fn reopen(conn: &Connection, id: i64, now_ms: i64) -> rusqlite::Result<Answer> {
    let Some((addr, _, by)) = row(conn, id)? else {
        return Ok(Answer::NoSender);
    };
    conn.execute("UPDATE sender SET declined_at = NULL WHERE id = ?1", [id])?;
    let mut ch = Changes::default();
    if by.as_deref() == Some("person") {
        // The engine owns the row again. `recheck` compares the old kind with
        // the new one, so a change of `hostile` goes to `rerun` and a change
        // of a known domain goes to the sweep.
        conn.execute(
            "UPDATE sender SET decided_by = 'engine' WHERE id = ?1",
            [id],
        )?;
        recheck(conn, &addr, now_ms, &mut ch)?;
        sweep(conn, now_ms, &mut ch)?;
    }
    Ok(Answer::Done { rerun: ch.rerun })
}
