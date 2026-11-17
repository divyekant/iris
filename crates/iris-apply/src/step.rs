//! The apply step of the engine (docs/specs/2026-11-17-held-outbox.md,
//! section 4): after each sync of an account, the recovery, the approved
//! Gmail actions, the move of approved sends to the held outbox, and the
//! release of each held send whose hold ended.

use iris_gmail::Gmail;
use iris_store::Store;
use rusqlite::params;

use crate::{
    apply, recover, release, send::changeset_error, writing_rows, Error, Outcome, Result, Token,
    HOLD_MS,
};

/// The verbs that the step applies with [`apply`]. A `send` leaves only
/// through [`release`].
const GMAIL_VERBS: &str =
    "'archive', 'inbox', 'read', 'unread', 'trash', 'label', 'unlabel', 'draft'";

/// What one apply step did.
#[derive(Debug, Default)]
pub struct Step {
    /// The result of each action that the step applied or released, in
    /// the order of the calls.
    pub results: Vec<(i64, Result<Outcome>)>,
    /// The earliest end of a hold, in milliseconds since 1970, while the
    /// switch is on and a held send waits for it. The engine wakes then.
    pub next: Option<i64>,
}

impl Step {
    /// True if the step stopped before its end: another apply holds the
    /// lock, or Gmail refused the token. The next turn tries again.
    fn stop(&self, token: &Token<'_>) -> bool {
        token.refused || matches!(self.results.last(), Some((_, Err(Error::Busy))))
    }
}

/// Run the apply step for `account` at the time `now_ms`.
///
/// 1. The recovery of each ledger row of the account in `writing`. It runs
///    in each turn, also while the switch is off, so a send whose response
///    was lost does not stay `held` (issue #400).
/// 2. Each `approved` action of the account with a Gmail verb, oldest
///    first, with [`apply`]. The apply keeps all its checks.
///    2a. The send of each draft of the write view
///    ([`iris_changeset::send_written`]), approved by the press of Send.
/// 3. The move of each `approved` send of the account to `held`
///    ([`iris_changeset::hold`]). A send 14 days after its submit expires.
/// 4. If the switch is on, the release of each `held` send of the account
///    whose hold ended, oldest first.
///
/// An error of one action goes in [`Step::results`], and the step goes on
/// with the next action. [`Error::Busy`] and a refused token end the step.
/// An error of the recovery or of the store ends the step with the error.
pub fn step(
    store: &Store,
    gmail: &Gmail,
    token: &mut Token<'_>,
    account: i64,
    now_ms: i64,
) -> Result<Step> {
    let mut out = Step::default();
    if writing_rows(store, account)? {
        recover(store, gmail, token, account)?;
    }
    for action in approved(store, account)? {
        let r = apply(store, gmail, token, account, action);
        out.results.push((action, r));
        if out.stop(token) {
            return Ok(out);
        }
    }
    // Step 2a: the send of each draft that the write view marked
    // (docs/specs/2026-11-17-write.md, section 3).
    iris_changeset::send_written(store, account, now_ms)
        .map_err(|e| Error::Store(changeset_error(e)))?;
    iris_changeset::hold(store, account, now_ms).map_err(|e| Error::Store(changeset_error(e)))?;
    let Some(on_at) = crate::sending(store)? else {
        return Ok(out);
    };
    for (action, decided_at) in held(store, account)? {
        let until = decided_at.max(on_at) + HOLD_MS;
        if until > now_ms {
            out.next = Some(out.next.map_or(until, |n| n.min(until)));
            continue;
        }
        let r = release(store, gmail, token, account, action);
        if let Ok(Outcome::Hold(t)) = r {
            out.next = Some(out.next.map_or(t, |n| n.min(t)));
        }
        out.results.push((action, r));
        if out.stop(token) {
            return Ok(out);
        }
    }
    Ok(out)
}

/// The `approved` actions of the account with a Gmail verb, oldest
/// approval first. The account of an action is the account of its
/// messages, of its draft, or of its account target (a new mail). The
/// apply denies an action that names two accounts.
fn approved(store: &Store, account: i64) -> Result<Vec<i64>> {
    Ok(store.read(|c| {
        let mut st = c.prepare(&format!(
            "SELECT a.id FROM changeset_action a
             WHERE a.state = 'approved' AND a.verb IN ({GMAIL_VERBS})
               AND (EXISTS (SELECT 1 FROM action_message am JOIN message m ON m.id = am.message_id
                            WHERE am.action_id = a.id AND m.account_id = ?1)
                 OR EXISTS (SELECT 1 FROM action_draft ad JOIN draft d ON d.id = ad.draft_id
                            WHERE ad.action_id = a.id AND d.account_id = ?1)
                 OR EXISTS (SELECT 1 FROM json_each(a.targets, '$.accounts') t
                            WHERE t.value = 'a-' || ?1))
             ORDER BY a.decided_at, a.id"
        ))?;
        let rows = st.query_map(params![account], |r| r.get(0))?;
        Ok(rows.collect::<rusqlite::Result<_>>()?)
    })?)
}

/// The `held` sends of the account with their approval time, oldest
/// approval first.
fn held(store: &Store, account: i64) -> Result<Vec<(i64, i64)>> {
    Ok(store.read(|c| {
        let mut st = c.prepare(
            "SELECT a.id, a.decided_at FROM changeset_action a
             JOIN action_draft ad ON ad.action_id = a.id
             JOIN draft d ON d.id = ad.draft_id
             WHERE a.state = 'held' AND a.verb = 'send' AND d.account_id = ?1
             ORDER BY a.decided_at, a.id",
        )?;
        let rows = st.query_map(params![account], |r| Ok((r.get(0)?, r.get(1)?)))?;
        Ok(rows.collect::<rusqlite::Result<_>>()?)
    })?)
}
