//! The stranger gate (specification 1, section 6.7): the person accepts a
//! sender of the kind `stranger`, declines it, or reopens the decision.
//!
//! - Accept makes the kind `person`, decided by the person. The engine
//!   never changes it.
//! - Decline sets `declined_at`. The kind stays with the engine, so a
//!   declined sender can still become `hostile`, and a declined sender that
//!   becomes known leaves the decline behind. The decline applies only while
//!   the kind is `stranger`.
//! - Reopen removes an answer of the gate: a decline, and an accept. The
//!   engine decides the kind again. A kind that a person set for another
//!   reason stays.
//! - The caller says if the sender is at the gate now (`at_gate`): a rule
//!   decides before the gate, so a sender whose items a rule took out of
//!   the gate gets no new answer.
//!
//! Each function runs in the transaction of the caller, and gives the
//! `sender` rows whose kind became `hostile` or stopped being `hostile`
//! (as [`crate::refresh`] does). In the same transaction, the caller
//! marks the mail of these rows for understanding again.

use rusqlite::{params, Connection, OptionalExtension};

use crate::{domain_of, recheck, rules, sweep, Changes, Kind};

/// The `basis` of a sender that the person accepted at the gate.
pub const ACCEPTED: &str = "accepted at the gate";

/// The `basis` of a near-copy sender that the person trusted (issue #49).
pub const TRUSTED: &str = "trusted after a near-copy mark";

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
    /// The kind is `stranger`, but no item of the sender waits at the gate
    /// (or is on the list of declined senders): for example, a rule took
    /// its items out. Nothing changed.
    NotAtGate,
    /// [`trust`] only: the sender is not `hostile` by the engine now. Nothing
    /// changed.
    NotHostile(Option<Kind>),
}

/// The state of one row.
struct Row {
    addr: String,
    kind: Option<String>,
    by: Option<String>,
    basis: Option<String>,
    declined: bool,
}

impl Row {
    /// The person accepted the sender at the gate.
    fn accepted(&self) -> bool {
        self.by.as_deref() == Some("person")
            && self.kind.as_deref() == Some(Kind::Person.name())
            && matches!(self.basis.as_deref(), Some(ACCEPTED | TRUSTED))
    }
}

fn row(conn: &Connection, id: i64) -> rusqlite::Result<Option<Row>> {
    conn.query_row(
        "SELECT address, kind, decided_by, basis, declined_at IS NOT NULL
           FROM sender WHERE id = ?1",
        [id],
        |r| {
            Ok(Row {
                addr: r.get(0)?,
                kind: r.get(1)?,
                by: r.get(2)?,
                basis: r.get(3)?,
                declined: r.get(4)?,
            })
        },
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
fn refuse(conn: &Connection, row: &Row) -> rusqlite::Result<Option<Answer>> {
    let (kind, by) = (row.kind.as_deref(), row.by.as_deref());
    // The stored kind is what the person saw: only a `stranger` is at the gate.
    let stored = kind.and_then(Kind::parse);
    if kind.is_some() && stored != Some(Kind::Stranger) {
        return Ok(Some(Answer::NotStranger(stored)));
    }
    if by == Some("person") {
        // A person set the kind: the engine rules do not apply.
        return Ok(None);
    }
    let (now, _) = rules(conn, &row.addr)?;
    Ok((now != Kind::Stranger).then_some(Answer::NotStranger(Some(now))))
}

/// Accept the sender `id`: the kind becomes `person`, decided by the person.
/// `at_gate`: the sender waits at the gate, or is on the list of declined
/// senders. A sender accepted at the gate gives `Done` again with no change.
/// The domain of the sender becomes known, so the near-copy rule decides its
/// look-alikes again.
pub fn accept(conn: &Connection, id: i64, now_ms: i64, at_gate: bool) -> rusqlite::Result<Answer> {
    let Some(row) = row(conn, id)? else {
        return Ok(Answer::NoSender);
    };
    if row.accepted() {
        return Ok(Answer::Done { rerun: Vec::new() });
    }
    if let Some(no) = refuse(conn, &row)? {
        return Ok(no);
    }
    if !at_gate {
        return Ok(Answer::NotAtGate);
    }
    conn.execute(
        "UPDATE sender SET kind = 'person', basis = ?2, decided_by = 'person',
                decided_at = ?3, declined_at = NULL
          WHERE id = ?1",
        params![id, ACCEPTED, now_ms],
    )?;
    let mut ch = Changes {
        domains: vec![domain_of(&row.addr).to_string()],
        ..Changes::default()
    };
    // The name of the sender is now the name of a known person: the store
    // queues the mail under that name for the spoof check (`fixed_due`).
    sweep(conn, now_ms, &mut ch)?;
    Ok(Answer::Done { rerun: ch.rerun })
}

/// Decline the sender `id`. `at_gate`: the sender waits at the gate. A
/// declined sender gives `Done` again and keeps the time of the first
/// decline.
pub fn decline(conn: &Connection, id: i64, now_ms: i64, at_gate: bool) -> rusqlite::Result<Answer> {
    let Some(row) = row(conn, id)? else {
        return Ok(Answer::NoSender);
    };
    if let Some(no) = refuse(conn, &row)? {
        return Ok(no);
    }
    if !at_gate && !row.declined {
        return Ok(Answer::NotAtGate);
    }
    conn.execute(
        "UPDATE sender SET declined_at = coalesce(declined_at, ?2) WHERE id = ?1",
        params![id, now_ms],
    )?;
    Ok(Answer::Done { rerun: Vec::new() })
}

/// Trust the sender `id` that the near-copy rule marks `hostile` (issue #49):
/// its domain looks like the domain of a known sender, but the person knows
/// the sender. The kind becomes `person`, decided by the person, so the
/// engine never marks it again. The near-copy rule is the only rule that
/// gives the kind `hostile`. The hostile marks of each message (`dmarc`,
/// `spoof`, `link`, `spam`, `phishing`) do not change. [`reopen`] removes
/// the trust. A trusted sender gives `Done` again with no change.
pub fn trust(conn: &Connection, id: i64, now_ms: i64) -> rusqlite::Result<Answer> {
    let Some(row) = row(conn, id)? else {
        return Ok(Answer::NoSender);
    };
    if row.accepted() && row.basis.as_deref() == Some(TRUSTED) {
        return Ok(Answer::Done { rerun: Vec::new() });
    }
    // The stored kind is what the person saw. A person-set kind is not the
    // engine's mark.
    let stored = row.kind.as_deref().and_then(Kind::parse);
    if stored != Some(Kind::Hostile) || row.by.as_deref() == Some("person") {
        return Ok(Answer::NotHostile(stored));
    }
    conn.execute(
        "UPDATE sender SET kind = 'person', basis = ?2, decided_by = 'person',
                decided_at = ?3, declined_at = NULL
          WHERE id = ?1",
        params![id, TRUSTED, now_ms],
    )?;
    // The sender is no more hostile: its mail is understood again. Its
    // domain is no reference of the near-copy rule (`known_domains`), so no
    // other row changes.
    Ok(Answer::Done { rerun: vec![id] })
}

/// Remove the answer of the gate for the sender `id`: the decline, and the
/// accept. After an accept, the engine decides the kind again from the
/// evidence in the store, as for a row that it never saw. A kind that a
/// person set for another reason stays. With no answer of the gate, `Done`
/// and no change.
pub fn reopen(conn: &Connection, id: i64, now_ms: i64) -> rusqlite::Result<Answer> {
    let Some(row) = row(conn, id)? else {
        return Ok(Answer::NoSender);
    };
    conn.execute("UPDATE sender SET declined_at = NULL WHERE id = ?1", [id])?;
    let mut ch = Changes::default();
    if row.accepted() {
        // The engine owns the row again. `recheck` compares the old kind with
        // the new one, so a change of `hostile` goes to `rerun` and a change
        // of a known domain goes to the sweep.
        conn.execute(
            "UPDATE sender SET decided_by = 'engine' WHERE id = ?1",
            [id],
        )?;
        recheck(conn, &row.addr, now_ms, &mut ch)?;
        sweep(conn, now_ms, &mut ch)?;
    }
    Ok(Answer::Done { rerun: ch.rerun })
}
