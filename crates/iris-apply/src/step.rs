//! The apply step of the engine (docs/specs/2026-11-17-held-outbox.md,
//! section 4): after each sync of an account, the recovery, the approved
//! Gmail actions, the move of approved sends to the held outbox, and the
//! release of each held send whose hold ended.

use iris_provider::Provider;
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
    /// True if Gmail refused a write for a missing scope. The step ended
    /// there, and the engine sets the account to read only
    /// (docs/specs/2026-11-18-gmail-write-access.md).
    pub scope_refused: bool,
}

impl Step {
    /// True if the step must stop before its end: another apply holds the
    /// lock, Gmail refused the token, or Gmail refused a write for a
    /// missing scope (then no other action of the step calls Gmail).
    fn stop(&mut self, token: &Token<'_>) -> bool {
        if self
            .results
            .last()
            .is_some_and(|(_, r)| crate::scope_refused(r))
        {
            self.scope_refused = true;
        }
        self.scope_refused
            || token.refused
            || matches!(self.results.last(), Some((_, Err(Error::Busy))))
    }
}

/// Run the apply step for `account` at the time `now_ms`. Step 2a reads
/// the time again for each send: `now_ms` plus the time that the step ran
/// so far ([`step_clock`]).
///
/// 1. The recovery of each ledger row of the account in `writing`. It runs
///    in each turn, also while the switch is off or Iris is stopped, so a
///    send whose response was lost does not stay `held` (issue #400).
///    While stopped, the step ends here.
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
/// with the next action. [`Error::Busy`], a refused token and a scope
/// refusal end the step.
/// An error of the recovery or of the store ends the step with the error.
pub fn step(
    store: &Store,
    mail: &dyn Provider,
    token: &mut Token<'_>,
    account: i64,
    now_ms: i64,
) -> Result<Step> {
    let begun = std::time::Instant::now();
    let clock = move || now_ms + i64::try_from(begun.elapsed().as_millis()).unwrap_or(i64::MAX / 2);
    step_clock(store, mail, token, account, &clock)
}

/// The step of an account with read access only
/// (docs/specs/2026-11-18-gmail-write-access.md): only the recovery of
/// step 1 with no write call ([`crate::recover_read_only`]), so a write
/// whose result is not known still ends. A call that never started waits
/// for a write sign-in. No action applies, and no send leaves.
pub fn recover_step(
    store: &Store,
    mail: &dyn Provider,
    token: &mut Token<'_>,
    account: i64,
) -> Result<Step> {
    if writing_rows(store, account)? {
        crate::recover_read_only(store, mail, token, account)?;
    }
    Ok(Step::default())
}

/// [`step`] with a clock. Steps 1, 2, 3 and 4 use the time at the start of
/// the step: a later time only makes them wait longer. Step 2a reads the
/// clock again for each send, because the approval of the send starts its
/// hold (docs/specs/2026-11-17-write.md, section 3).
pub fn step_clock(
    store: &Store,
    mail: &dyn Provider,
    token: &mut Token<'_>,
    account: i64,
    clock: &dyn Fn() -> i64,
) -> Result<Step> {
    let now_ms = clock();
    let mut out = Step::default();
    if writing_rows(store, account)? {
        recover(store, mail, token, account)?;
    }
    // While stopped, only the recovery runs. The claims check the stop
    // again, so a stop after this read still holds.
    if store.read(iris_store::stopped)?.is_some() {
        return Ok(out);
    }
    for action in approved(store, account)? {
        let r = apply(store, mail, token, account, action);
        out.results.push((action, r));
        if out.stop(token) {
            return Ok(out);
        }
    }
    // Step 2a: the send of each draft that the write view marked
    // (docs/specs/2026-11-17-write.md, section 3).
    iris_changeset::send_written(store, account, clock)
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
        let r = release(store, mail, token, account, action);
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
