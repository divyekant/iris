//! Rules that Iris proposes from the approvals of the person (spec 2,
//! section 6.6). A proposal is a row of `rule_proposal` (store migration 12),
//! never a line in the file. Only an accept by the person
//! ([`crate::Change::Accept`]) adds the rule to the file, with
//! `by = "proposal"`.
//!
//! The invariant: a proposal counts exactly the actions that its rule,
//! added to the active policy, would approve. Each count comes from
//! [`Policy::decide`] on that policy, so the proposal and the rule cannot
//! disagree.

use crate::attention::facts;
use crate::{Action, Author, By, Effect, Facts, Outcome, Policy, Rule, SenderKind, Verb, When};
use iris_store::{Result, Store};
use rusqlite::{params, Connection, OptionalExtension};
use std::collections::{BTreeMap, BTreeSet};

/// The number of approvals, with no refusal, that a proposal needs.
pub const MIN_APPROVALS: i64 = 10;

/// The newest decided actions that a pass reads.
// shortcut: older decisions do not count, change to an incremental count if a person decides more than this often.
pub const MAX_ACTIONS: usize = 2000;

/// The verbs that a proposal can have. `label` and `unlabel` are not here,
/// because a rule cannot name the label, so it would allow each label. The
/// state verbs change items, and `correct` changes facts.
pub const VERBS: [Verb; 5] = [
    Verb::Archive,
    Verb::Read,
    Verb::Unread,
    Verb::Inbox,
    Verb::Trash,
];

/// One open proposal.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Proposal {
    pub id: i64,
    pub verb: Verb,
    pub when: When,
    /// The number of approvals that the rule would have decided.
    pub approvals: i64,
    /// Seconds since 1970-01-01 UTC.
    pub created_at: i64,
}

impl Proposal {
    /// The rule that an accept adds, with the ID `id` and the date `added`.
    pub fn rule(&self, id: u32, added: &str) -> Rule {
        Rule {
            id,
            name: name(self.verb, &self.when),
            effect: Effect::Allow,
            verbs: vec![self.verb],
            when: self.when.clone(),
            by: By::Proposal,
            added: added.to_string(),
            job: None,
            line: 0,
        }
    }
}

/// The open proposals, the oldest first.
pub fn open(conn: &Connection) -> Result<Vec<Proposal>> {
    let mut st = conn.prepare(
        "SELECT id, verb, cond, value, approvals, created_at FROM rule_proposal
         WHERE state = 'open' ORDER BY id",
    )?;
    let rows = st.query_map([], |r| {
        Ok((
            r.get::<_, i64>(0)?,
            r.get::<_, String>(1)?,
            r.get::<_, String>(2)?,
            r.get::<_, String>(3)?,
            r.get::<_, i64>(4)?,
            r.get::<_, i64>(5)?,
        ))
    })?;
    let mut out = Vec::new();
    for row in rows {
        let (id, verb, cond, value, approvals, created_at) = row?;
        // A row that this program cannot read is not shown, and cannot be accepted.
        if let (Some(verb), Some(when)) = (Verb::from_name(&verb), when(&cond, &value)) {
            out.push(Proposal {
                id,
                verb,
                when,
                approvals,
                created_at,
            });
        }
    }
    Ok(out)
}

/// The open proposal `id`, if there is one.
pub fn get(conn: &Connection, id: i64) -> Result<Option<Proposal>> {
    Ok(open(conn)?.into_iter().find(|p| p.id == id))
}

/// The open proposal `id`, only if the decisions of now still give it
/// (the same pass as [`refresh`]), with the count of now. A decision that
/// came after the last pass can make a proposal stale: the accept of
/// [`crate::change`] calls this in its transaction, so it never adds a rule
/// that a refusal contradicts.
pub(crate) fn current(tx: &Connection, id: i64, now: i64) -> Result<Option<Proposal>> {
    let Some(mut p) = get(tx, id)? else {
        return Ok(None);
    };
    let (cond, value) = match (&p.when.sender, p.when.sender_kind) {
        (Some(s), _) => ("sender", s.clone()),
        (None, Some(k)) => ("sender_kind", k.name().to_string()),
        (None, None) => return Ok(None),
    };
    let key = (p.verb.name().to_string(), cond.to_string(), value);
    Ok(pass(tx, now)?.get(&key).map(|&n| {
        p.approvals = n;
        p
    }))
}

/// Marks the open proposal `id` as accepted with the rule `rule`. Only the
/// accept of [`crate::change`] calls this, in its transaction.
pub(crate) fn accepted(tx: &Connection, id: i64, rule: u32, now: i64) -> Result<bool> {
    Ok(tx.execute(
        "UPDATE rule_proposal SET state = 'accepted', rule = ?2, updated_at = ?3
         WHERE id = ?1 AND state = 'open'",
        params![id, rule, now],
    )? == 1)
}

/// Dismisses the open proposal `id`. Iris does not propose the same rule
/// again. Gives `false` when there is no open proposal `id`.
pub fn dismiss(store: &Store, id: i64, now: i64) -> Result<bool> {
    store.write(|tx| {
        Ok(tx.execute(
            "UPDATE rule_proposal SET state = 'dismissed', updated_at = ?2
             WHERE id = ?1 AND state = 'open'",
            params![id, now],
        )? == 1)
    })
}

/// What a pass read last. A pass runs again only when it changes.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Mark([i64; 5]);

/// Writes the open proposals again from the decided actions, if the
/// decisions, the active policy or the hour changed since `last`. Gives the
/// number of open proposals after a pass, or `None` when no pass ran.
pub fn refresh(store: &Store, now: i64, last: &mut Option<Mark>) -> Result<Option<usize>> {
    store.write(|tx| {
        let mark = mark(tx, now)?;
        if *last == Some(mark) {
            return Ok(None);
        }
        let found = pass(tx, now)?;
        let mut keep = BTreeSet::new();
        for ((verb, cond, value), approvals) in &found {
            tx.execute(
                "INSERT INTO rule_proposal (verb, cond, value, approvals, created_at, updated_at)
                 VALUES (?1, ?2, ?3, ?4, ?5, ?5)
                 ON CONFLICT (verb, cond, value) DO UPDATE
                 SET approvals = excluded.approvals, updated_at = excluded.updated_at
                 WHERE state = 'open' AND approvals <> excluded.approvals",
                params![verb, cond, value, approvals, now],
            )?;
            keep.insert((verb.clone(), cond.clone(), value.clone()));
        }
        // A pattern that is gone (a refusal, an undo, a new rule) is no longer proposed.
        let open: Vec<(i64, String, String, String)> = tx
            .prepare("SELECT id, verb, cond, value FROM rule_proposal WHERE state = 'open'")?
            .query_map([], |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?, r.get(3)?)))?
            .collect::<rusqlite::Result<_>>()?;
        let mut n = 0;
        for (id, verb, cond, value) in open {
            if keep.contains(&(verb, cond, value)) {
                n += 1;
            } else {
                tx.execute("DELETE FROM rule_proposal WHERE id = ?1", [id])?;
            }
        }
        *last = Some(mark);
        Ok(Some(n))
    })
}

fn mark(tx: &Connection, now: i64) -> Result<Mark> {
    let (count, at, undone): (i64, i64, i64) = tx.query_row(
        "SELECT count(*), coalesce(max(decided_at), 0), coalesce(sum(state = 'undone'), 0)
         FROM changeset_action WHERE decided_by = 'person' OR state = 'undone'",
        [],
        |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?)),
    )?;
    let version = crate::active(tx)?.version.unwrap_or(0);
    // Each hour too, for a change of a fact that a rule reads (a sender kind).
    Ok(Mark([count, at, undone, version, now.div_euclid(3600)]))
}

/// One decided action, with the facts of its messages.
struct Decided {
    verb: Verb,
    author: Author,
    job: Option<String>,
    /// The flag `bulk` of the action (spec 2, section 5.3).
    bulk: bool,
    /// True for an approval by the person, false for a denial or an undo.
    approved: bool,
    messages: Vec<Facts>,
}

type Key = (String, String, String);

/// The proposals that the decided actions give now: the key and the number
/// of approvals.
fn pass(tx: &Connection, now: i64) -> Result<BTreeMap<Key, i64>> {
    let active = crate::active(tx)?;
    if active.error.is_some() {
        // The active text is not valid; the person corrects it first.
        return Ok(BTreeMap::new());
    }
    let actions = decided(tx, now)?;
    // The candidates: one value of a condition that each message of an
    // approved action has.
    let mut candidates: BTreeSet<(Verb, &'static str, String)> = BTreeSet::new();
    // An action on hostile mail gives no candidate: Iris does not propose
    // a rule from it, even one that the hard limits allow.
    for a in actions
        .iter()
        .filter(|a| a.approved && !a.messages.iter().any(|m| kind(m) == SenderKind::Hostile))
    {
        if let Some(s) = same(&a.messages, |m| {
            crate::check_sender(m.sender.clone())
                .ok()
                .filter(|s| !s.starts_with('@'))
        }) {
            candidates.insert((a.verb, "sender", s));
        }
        if let Some(k) = same(&a.messages, |m| Some(kind(m))) {
            candidates.insert((a.verb, "sender_kind", k.name().to_string()));
        }
    }
    let mut base = active.policy;
    base.mode = crate::Mode::Apply;
    let mut out = BTreeMap::new();
    for (verb, cond, value) in candidates {
        // `when` refuses a value that a rule cannot have, and `hostile`.
        let Some(when) = when(cond, &value) else {
            continue;
        };
        if decided_before(tx, verb, cond, &value)? {
            continue;
        }
        let mut policy = base.clone();
        let p = Proposal {
            id: 0,
            verb,
            when,
            approvals: 0,
            created_at: 0,
        };
        policy.rules.push(p.rule(u32::MAX, ""));
        let (mut yes, mut no) = (0i64, 0i64);
        for a in actions.iter().filter(|a| a.verb == verb) {
            let action = Action {
                verb,
                author: a.author,
                job: a.job.as_deref(),
                bulk: a.bulk,
                now,
            };
            let allow = |p: &Policy| {
                p.decide(&action, a.messages.iter().map(Facts::message))
                    .outcome
                    == Outcome::Allow
            };
            // Only an action that the new rule changes to approved counts.
            if !allow(&base) && allow(&policy) {
                if a.approved {
                    yes += 1;
                } else {
                    no += 1;
                }
            }
        }
        if no == 0 && yes >= MIN_APPROVALS {
            out.insert((verb.name().to_string(), cond.to_string(), value), yes);
        }
    }
    Ok(out)
}

/// The newest [`MAX_ACTIONS`] actions of another author with a verb of
/// [`VERBS`] that the person approved, denied or that was undone.
fn decided(tx: &Connection, now: i64) -> Result<Vec<Decided>> {
    let mut st = tx.prepare(
        "SELECT a.id, a.verb, a.state, c.author_kind, c.author_job,
                EXISTS (SELECT 1 FROM json_each(a.flags) WHERE value = 'bulk')
         FROM changeset_action a JOIN changeset c ON c.id = a.changeset_id
         WHERE a.verb IN ('archive', 'read', 'unread', 'inbox', 'trash')
           AND c.author_kind <> 'person'
           AND ((a.decided_by = 'person'
                 AND a.state IN ('approved', 'held', 'applied', 'failed', 'denied'))
                OR a.state = 'undone')
         ORDER BY a.id DESC LIMIT ?1",
    )?;
    #[allow(clippy::type_complexity)]
    let rows: Vec<(i64, String, String, String, Option<String>, bool)> = st
        .query_map([MAX_ACTIONS as i64], |r| {
            Ok((
                r.get(0)?,
                r.get(1)?,
                r.get(2)?,
                r.get(3)?,
                r.get(4)?,
                r.get(5)?,
            ))
        })?
        .collect::<rusqlite::Result<_>>()?;
    let mut ids =
        tx.prepare_cached("SELECT message_id FROM action_message WHERE action_id = ?1")?;
    let mut out = Vec::new();
    let mut blocked = BTreeSet::new();
    for (id, verb, state, author, job, bulk) in rows {
        let (Some(verb), Some(author)) = (Verb::from_name(&verb), Author::from_name(&author))
        else {
            continue;
        };
        let approved = !matches!(state.as_str(), "denied" | "undone");
        let msg_ids: Vec<i64> = ids
            .query_map([id], |r| r.get(0))?
            .collect::<rusqlite::Result<_>>()?;
        let mut messages = Vec::with_capacity(msg_ids.len());
        let mut whole = true;
        for m in msg_ids {
            match facts(tx, m, now)? {
                Some(f) => messages.push(f),
                None => whole = false,
            }
        }
        if !whole {
            if approved {
                // An approval that cannot be read again does not count.
                continue;
            }
            // A refusal that cannot be read again blocks each rule of its verb.
            blocked.insert(verb);
            continue;
        }
        out.push(Decided {
            verb,
            author,
            job,
            bulk,
            approved,
            messages,
        });
    }
    out.retain(|a| !blocked.contains(&a.verb));
    Ok(out)
}

fn decided_before(tx: &Connection, verb: Verb, cond: &str, value: &str) -> Result<bool> {
    Ok(tx
        .query_row(
            "SELECT 1 FROM rule_proposal WHERE verb = ?1 AND cond = ?2 AND value = ?3
             AND state <> 'open'",
            params![verb.name(), cond, value],
            |_| Ok(()),
        )
        .optional()?
        .is_some())
}

/// The one value that `f` gives for each message, or `None`.
fn same<T: PartialEq>(messages: &[Facts], f: impl Fn(&Facts) -> Option<T>) -> Option<T> {
    let mut it = messages.iter();
    let first = f(it.next()?)?;
    it.all(|m| f(m).as_ref() == Some(&first)).then_some(first)
}

/// The kind that a rule matches: a hostile mark gives `hostile`.
fn kind(m: &Facts) -> SenderKind {
    if m.hostile {
        SenderKind::Hostile
    } else {
        m.sender_kind
    }
}

fn when(cond: &str, value: &str) -> Option<When> {
    match cond {
        "sender" => crate::check_sender(value.to_string()).ok().map(|s| When {
            sender: Some(s),
            ..When::default()
        }),
        "sender_kind" => SenderKind::from_name(value)
            .filter(|k| *k != SenderKind::Hostile)
            .map(|k| When {
                sender_kind: Some(k),
                ..When::default()
            }),
        _ => None,
    }
}

/// The name of the rule: one line in the words of the brief.
fn name(verb: Verb, when: &When) -> String {
    let who = match (&when.sender, when.sender_kind) {
        (Some(s), _) => s.clone(),
        (None, Some(SenderKind::SelfOwner)) => "you".into(),
        (None, Some(SenderKind::Person)) => "people".into(),
        (None, Some(SenderKind::Service)) => "services".into(),
        (None, Some(SenderKind::Stranger)) => "strangers".into(),
        _ => "this sender".into(),
    };
    match verb {
        Verb::Read => format!("Mark mail from {who} as read"),
        Verb::Unread => format!("Mark mail from {who} as unread"),
        Verb::Inbox => format!("Move mail from {who} to the inbox"),
        Verb::Trash => format!("Trash mail from {who}"),
        _ => format!("Archive mail from {who}"),
    }
}

/// The date `YYYY-MM-DD` (UTC) of `secs` seconds since 1970-01-01.
pub fn date(secs: i64) -> String {
    // The civil date of a day number (H. Hinnant, "chrono-compatible
    // low-level date algorithms").
    let z = secs.div_euclid(86_400) + 719_468;
    let era = z.div_euclid(146_097);
    let doe = z - era * 146_097;
    let yoe = (doe - doe / 1460 + doe / 36_524 - doe / 146_096) / 365;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
    let mp = (5 * doy + 2) / 153;
    let d = doy - (153 * mp + 2) / 5 + 1;
    let m = if mp < 10 { mp + 3 } else { mp - 9 };
    let y = yoe + era * 400 + i64::from(m <= 2);
    format!("{y:04}-{m:02}-{d:02}")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn dates() {
        assert_eq!(date(0), "1970-01-01");
        assert_eq!(date(951_782_400), "2000-02-29");
        assert_eq!(date(1_794_787_200), "2026-11-16");
        assert_eq!(date(-86_400), "1969-12-31");
    }

    #[test]
    fn names_are_one_line() {
        let w = When {
            sender: Some("deals@acme.example".into()),
            ..When::default()
        };
        assert_eq!(
            name(Verb::Archive, &w),
            "Archive mail from deals@acme.example"
        );
        let w = When {
            sender_kind: Some(SenderKind::Service),
            ..When::default()
        };
        assert_eq!(name(Verb::Read, &w), "Mark mail from services as read");
    }
}
