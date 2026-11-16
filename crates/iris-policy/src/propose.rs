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
    Ok(pass(tx, now)?.proposals.get(&key).map(|&n| {
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
    // The pass reads many facts: it runs on a read connection, so it holds
    // no write lock. A decision after the read gives a new mark, and the
    // next tick runs again; an accept checks its proposal again.
    let (mark, found) = store.read(|c| {
        let mark = mark(c, now)?;
        Ok((
            mark,
            (*last != Some(mark)).then(|| pass(c, now)).transpose()?,
        ))
    })?;
    let Some(found) = found else {
        return Ok(None);
    };
    let n = store.write(|tx| {
        let mut keep = BTreeSet::new();
        for ((verb, cond, value), approvals) in &found.proposals {
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
        Ok(n)
    })?;
    // Only after the commit: a pass that did not commit runs again.
    *last = Some(mark);
    Ok(Some(n))
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

type Key = (String, String, String);

/// The counts of one key in a pass.
#[derive(Default)]
struct Count {
    /// Approvals and refusals that the rule changes to approved.
    yes: i64,
    no: i64,
    /// An approved action with no hostile mail has the key.
    sourced: bool,
}

/// The result of a pass: the proposals of now (the key and the number of
/// approvals), and the most message facts that the pass held at one time.
pub(crate) struct Found {
    pub(crate) proposals: BTreeMap<Key, i64>,
    pub(crate) held: usize,
}

/// One pass over the newest [`MAX_ACTIONS`] actions of another author with
/// a verb of [`VERBS`] that the person approved, refused (`denied` by the
/// person) or that was undone. The pass holds the facts of one action at a
/// time. A bulk action stays in review with each rule (section 6.5), so the
/// pass skips it, and each other action has 500 messages or fewer.
pub(crate) fn pass(conn: &Connection, now: i64) -> Result<Found> {
    let mut found = Found {
        proposals: BTreeMap::new(),
        held: 0,
    };
    let active = crate::active(conn)?;
    if active.error.is_some() {
        // The active text is not valid; the person corrects it first.
        return Ok(found);
    }
    let mut policy = active.policy;
    policy.mode = crate::Mode::Apply;
    let mut st = conn.prepare(
        "SELECT a.id, a.verb, a.state, c.author_kind, c.author_job
         FROM changeset_action a JOIN changeset c ON c.id = a.changeset_id
         WHERE a.verb IN ('archive', 'read', 'unread', 'inbox', 'trash')
           AND c.author_kind <> 'person'
           AND ((a.decided_by = 'person'
                 AND a.state IN ('approved', 'held', 'applied', 'failed', 'denied'))
                OR a.state = 'undone')
           AND NOT EXISTS (SELECT 1 FROM json_each(a.flags) WHERE value = 'bulk')
         ORDER BY a.id DESC LIMIT ?1",
    )?;
    let rows: Vec<(i64, String, String, String, Option<String>)> = st
        .query_map([MAX_ACTIONS as i64], |r| {
            Ok((r.get(0)?, r.get(1)?, r.get(2)?, r.get(3)?, r.get(4)?))
        })?
        .collect::<rusqlite::Result<_>>()?;
    let mut counts: BTreeMap<(Verb, &'static str, String), Count> = BTreeMap::new();
    let mut blocked = BTreeSet::new();
    for (id, verb, state, author, job) in rows {
        let (Some(verb), Some(author)) = (Verb::from_name(&verb), Author::from_name(&author))
        else {
            continue;
        };
        let approved = !matches!(state.as_str(), "denied" | "undone");
        let Some(messages) = read(conn, id, now)? else {
            if !approved {
                // A refusal that cannot be read again blocks each rule of its verb.
                blocked.insert(verb);
            }
            continue;
        };
        found.held = found.held.max(messages.len());
        let action = Action {
            verb,
            author,
            job: job.as_deref(),
            bulk: false,
            now,
        };
        let base = allows(&policy, &action, &messages);
        let hostile = messages.iter().any(|m| kind(m) == SenderKind::Hostile);
        // An allow rule with one condition matches an action only when each
        // message has the value, so these two keys are the only rules that
        // can change this action.
        let keys = [
            same(&messages, |m| {
                crate::check_sender(m.sender.clone())
                    .ok()
                    .filter(|s| !s.starts_with('@'))
            })
            .map(|s| ("sender", s)),
            same(&messages, |m| Some(kind(m))).map(|k| ("sender_kind", k.name().to_string())),
        ];
        for (cond, value) in keys.into_iter().flatten() {
            // `when` refuses a value that a rule cannot have, and `hostile`.
            let Some(when) = when(cond, &value) else {
                continue;
            };
            let rule = Proposal {
                id: 0,
                verb,
                when,
                approvals: 0,
                created_at: 0,
            }
            .rule(u32::MAX, "");
            policy.rules.push(rule);
            let changed = !base && allows(&policy, &action, &messages);
            policy.rules.pop();
            let c = counts.entry((verb, cond, value)).or_default();
            // An action on hostile mail gives no candidate: Iris does not
            // propose a rule from it, even one that the hard limits allow.
            c.sourced |= approved && !hostile;
            // An approval on hostile mail does not count either; a refusal does.
            match (changed, approved) {
                (true, true) if !hostile => c.yes += 1,
                (true, false) => c.no += 1,
                _ => {}
            }
        }
    }
    for ((verb, cond, value), c) in counts {
        if c.sourced
            && c.no == 0
            && c.yes >= MIN_APPROVALS
            && !blocked.contains(&verb)
            && !decided_before(conn, verb, cond, &value)?
        {
            found
                .proposals
                .insert((verb.name().to_string(), cond.to_string(), value), c.yes);
        }
    }
    Ok(found)
}

/// True when `policy` approves the action.
fn allows(policy: &Policy, action: &Action, messages: &[Facts]) -> bool {
    policy
        .decide(action, messages.iter().map(Facts::message))
        .outcome
        == Outcome::Allow
}

/// The facts of the messages of action `id`, or `None` when one of them
/// cannot be read again. The facts are the facts of the decision only while
/// the header is there: a cleanup of a gone message deletes it (spec 1, 5.5).
fn read(conn: &Connection, id: i64, now: i64) -> Result<Option<Vec<Facts>>> {
    let mut header = conn.prepare_cached("SELECT 1 FROM header WHERE message_id = ?1")?;
    let ids: Vec<i64> = conn
        .prepare_cached("SELECT message_id FROM action_message WHERE action_id = ?1")?
        .query_map([id], |r| r.get(0))?
        .collect::<rusqlite::Result<_>>()?;
    let mut out = Vec::with_capacity(ids.len());
    for m in ids {
        match facts(conn, m, now)? {
            Some(f) if header.exists([m])? => out.push(f),
            _ => return Ok(None),
        }
    }
    Ok(Some(out))
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

    /// The pass holds the facts of one action at a time, and skips each bulk
    /// action, so its memory does not grow with the number of actions.
    #[test]
    fn the_pass_holds_one_action_at_a_time() {
        let dir = std::env::temp_dir().join(format!("iris-propose-held-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        let store = Store::open(&dir).unwrap();
        store
            .write(|tx| {
                tx.execute_batch(
                    "INSERT INTO account(id, provider, address) VALUES (1, 'gmail', 'me@x.example');
                     INSERT INTO thread(id, account_id, provider_id) VALUES (1, 1, 't');",
                )?;
                // 30 actions of 40 messages, and one bulk action of 600.
                for (a, n, flags) in (0..30).map(|a| (a, 40, "[]")).chain([(30, 600, "[\"bulk\"]")]) {
                    tx.execute("INSERT INTO changeset(author_kind, created_at) VALUES ('agent', 0)", [])?;
                    let cs = tx.last_insert_rowid();
                    tx.execute(
                        "INSERT INTO changeset_action(changeset_id, revision, n, verb, targets, args, hash)
                         VALUES (?1, 1, 1, 'archive', '{}', '{}', 'h')",
                        [cs],
                    )?;
                    let id = tx.last_insert_rowid();
                    for m in 0..n {
                        tx.execute(
                            "INSERT INTO message(account_id, thread_id, provider_id, internal_date)
                             VALUES (1, 1, ?1, 0)",
                            [format!("m{a}-{m}")],
                        )?;
                        let msg = tx.last_insert_rowid();
                        tx.execute(
                            "INSERT INTO header(message_id, from_addr) VALUES (?1, 'x@y.example')",
                            [msg],
                        )?;
                        tx.execute(
                            "INSERT INTO action_message(action_id, message_id) VALUES (?1, ?2)",
                            [id, msg],
                        )?;
                    }
                    tx.execute("UPDATE changeset SET why = 'w', submitted_at = 1 WHERE id = ?1", [cs])?;
                    tx.execute(
                        "UPDATE changeset_action SET flags = ?2, state = 'approved',
                         decided_by = 'person', decided_at = 2 WHERE id = ?1",
                        params![id, flags],
                    )?;
                }
                Ok(())
            })
            .unwrap();
        let found = store.read(|c| pass(c, 1_800_000_000)).unwrap();
        let _ = std::fs::remove_dir_all(&dir);
        assert_eq!(found.held, 40);
        let key = (
            "archive".to_string(),
            "sender".to_string(),
            "x@y.example".to_string(),
        );
        assert_eq!(
            found.proposals.get(&key),
            Some(&30),
            "the bulk action does not count"
        );
    }

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
