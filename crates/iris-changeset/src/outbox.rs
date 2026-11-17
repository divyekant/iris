//! The held outbox (docs/specs/2026-11-17-held-outbox.md): the move of an
//! approved send to `held`, and the cancel of a person.

use iris_store::Store;
use rusqlite::params;

use crate::{expire_in, write, Result};

/// The reason of a cancel. The store accepts a send `denied` by `person`
/// only with this reason (migration 22).
pub const CANCELLED: &str = "Cancelled in the outbox.";

/// Move each `approved` send of `account` to `held`, in one transaction.
/// The expiry runs first in the same transaction, so a send 14 days after
/// its submit becomes `expired` and never `held`. Gives the count of sends
/// that moved.
pub fn hold(store: &Store, account: i64, now_ms: i64) -> Result<usize> {
    write(store, |c| {
        expire_in(c, now_ms)?;
        Ok(c.execute(
            "UPDATE changeset_action SET state = 'held'
             WHERE state = 'approved' AND verb = 'send'
               AND id IN (SELECT ad.action_id FROM action_draft ad
                          JOIN draft d ON d.id = ad.draft_id WHERE d.account_id = ?1)",
            params![account],
        )?)
    })
}

/// The result of a cancel.
#[derive(Debug, PartialEq, Eq)]
pub enum Cancel {
    /// The send is `denied` by `person`, with the reason [`CANCELLED`].
    Done,
    /// The release claimed the send: it cannot be cancelled.
    Started,
    /// The action is not an open send.
    NotOpen,
}

/// The cancel of a person (section 5): a `send` in `approved` or `held`
/// with no ledger row becomes `denied` by `person`. The release claims with
/// `WHERE state = 'held'` on the same write connection, so only one of the
/// two commits.
pub fn cancel(store: &Store, action: i64, now_ms: i64) -> Result<Cancel> {
    write(store, |c| {
        let n = c.execute(
            "UPDATE changeset_action SET state = 'denied', decided_by = 'person', rule = NULL,
               policy = NULL, reason = ?3, decided_at = ?2
             WHERE id = ?1 AND verb = 'send' AND state IN ('approved', 'held')
               AND NOT EXISTS (SELECT 1 FROM ledger l WHERE l.action_id = ?1)",
            params![action, now_ms, CANCELLED],
        )?;
        if n == 1 {
            return Ok(Cancel::Done);
        }
        let started: bool = c.query_row(
            "SELECT EXISTS (SELECT 1 FROM ledger WHERE action_id = ?1)
                    AND EXISTS (SELECT 1 FROM changeset_action WHERE id = ?1 AND verb = 'send')",
            [action],
            |r| r.get(0),
        )?;
        Ok(if started {
            Cancel::Started
        } else {
            Cancel::NotOpen
        })
    })
}
