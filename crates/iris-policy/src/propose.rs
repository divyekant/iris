//! Rules that Iris proposes from the approvals and the choices of the
//! person (spec 2, section 6.6). A proposal is a row of `rule_proposal`
//! (store migrations 15 and 24), never a line in the file. Only an accept by the person
//! ([`crate::Change::Accept`]) adds the rule to the file, with
//! `by = "proposal"`.
//!
//! The invariant: a proposal counts exactly the actions that its rule,
//! added to the active policy, would approve. Each count comes from
//! [`Policy::decide`] on that policy, so the proposal and the rule cannot
//! disagree. A `hide` proposal (card R5-02) counts exactly the choices
//! (`skip`, `mute`, `dismiss`) whose items its rule would hide, from
//! [`Policy::attention`].

use crate::attention::facts;
use crate::{
    Action, Attention, Author, By, Effect, Facts, Form, Outcome, Policy, Rule, SenderKind, Verb,
    When,
};
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

/// The forms of a `hide` proposal, with the verb of the person's choice
/// for each one.
pub const CHOICES: [(&str, Form); 3] = [
    ("skip", Form::Feed),
    ("mute", Form::Conversation),
    ("dismiss", Form::Signal),
];

/// One open proposal.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Proposal {
    pub id: i64,
    /// The verb of an `allow` proposal. `None` for a `hide` proposal, whose
    /// `when` has a `sender` and a `form`.
    pub verb: Option<Verb>,
    pub when: When,
    /// The number of approvals (or choices, for `hide`) that the rule would
    /// have decided.
    pub approvals: i64,
    /// Seconds since 1970-01-01 UTC.
    pub created_at: i64,
}

impl Proposal {
    /// The effect of the rule: `allow` or `hide`.
    pub fn effect(&self) -> Effect {
        if self.verb.is_some() {
            Effect::Allow
        } else {
            Effect::Hide
        }
    }

    /// The rule that an accept adds, with the ID `id` and the date `added`.
    pub fn rule(&self, id: u32, added: &str) -> Rule {
        Rule {
            id,
            name: name(self.verb, &self.when),
            effect: self.effect(),
            verbs: self.verb.into_iter().collect(),
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
        "SELECT id, verb, cond, value, form, approvals, created_at FROM rule_proposal
         WHERE state = 'open' ORDER BY id",
    )?;
    let rows = st.query_map([], |r| {
        Ok((
            r.get::<_, i64>(0)?,
            (
                r.get::<_, String>(1)?,
                r.get::<_, String>(2)?,
                r.get::<_, String>(3)?,
                r.get::<_, String>(4)?,
            ),
            r.get::<_, i64>(5)?,
            r.get::<_, i64>(6)?,
        ))
    })?;
    let mut out = Vec::new();
    for row in rows {
        let (id, key, approvals, created_at) = row?;
        // A row that this program cannot read is not shown, and cannot be accepted.
        if let Some((verb, when)) = parse_key(&key) {
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

/// The verb and the condition of a row key, or `None` if this program cannot
/// read it.
fn parse_key((verb, cond, value, form): &Key) -> Option<(Option<Verb>, When)> {
    let mut w = when(cond, value)?;
    if verb == "hide" {
        let form = CHOICES.iter().find(|(_, f)| f.name() == form)?.1;
        w.sender.as_ref()?;
        w.form = Some(form);
        Some((None, w))
    } else if form.is_empty() {
        Some((Some(Verb::from_name(verb)?), w))
    } else {
        None
    }
}

/// The row key of a proposal.
fn key_of(p: &Proposal) -> Option<Key> {
    let (cond, value) = match (&p.when.sender, p.when.sender_kind) {
        (Some(s), _) => ("sender", s.clone()),
        (None, Some(k)) => ("sender_kind", k.name().to_string()),
        (None, None) => return None,
    };
    let (verb, form) = match p.verb {
        Some(v) => (v.name(), ""),
        None => ("hide", p.when.form?.name()),
    };
    Some((verb.into(), cond.into(), value, form.into()))
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
    let Some(key) = key_of(&p) else {
        return Ok(None);
    };
    Ok(pass(tx, now)?.proposals.get(&key).map(|&n| {
        p.approvals = n;
        p
    }))
}

/// Marks as accepted each open proposal whose rule the policy has with
/// `by = "proposal"`. [`crate::active::record`] calls this for each policy
/// that becomes active, so an accept from the web client and a confirmed
/// file both mark it, also after a crash between the file write and the
/// commit of an accept.
pub(crate) fn accepted(tx: &Connection, policy: &Policy, now: i64) -> Result<()> {
    for r in &policy.rules {
        if r.by != By::Proposal || r.job.is_some() {
            continue;
        }
        let verb = match (r.effect, r.verbs.as_slice()) {
            (Effect::Allow, [v]) => Some(*v),
            (Effect::Hide, []) => None,
            _ => continue,
        };
        let p = Proposal {
            id: 0,
            verb,
            when: r.when.clone(),
            approvals: 0,
            created_at: 0,
        };
        // Only the exact rule of a proposal: the key gives the rule again.
        let Some(key) = key_of(&p) else { continue };
        if parse_key(&key).map(|(_, w)| w) != Some(r.when.clone()) {
            continue;
        }
        tx.execute(
            "UPDATE rule_proposal SET state = 'accepted', rule = ?5, updated_at = ?6
             WHERE state = 'open' AND verb = ?1 AND cond = ?2 AND value = ?3 AND form = ?4",
            params![key.0, key.1, key.2, key.3, r.id, now],
        )?;
    }
    Ok(())
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
pub struct Mark([i64; 8]);

/// Writes the open proposals again from the decided actions, if the
/// decisions, the active policy or the hour changed since `last`. Gives the
/// number of open proposals after a pass, or `None` when no pass ran or a
/// change came between the read and the write.
pub fn refresh(store: &Store, now: i64, last: &mut Option<Mark>) -> Result<Option<usize>> {
    match read_pass(store, now, last)? {
        Some(pass) => write_pass(store, pass, now, last),
        None => Ok(None),
    }
}

/// The result of the read half of [`refresh`], for [`write_pass`].
pub struct Pass {
    mark: Mark,
    found: Found,
}

/// The read half of [`refresh`]: a pass on a read connection, so it holds
/// no write lock. Gives `None` when nothing changed since `last`.
pub fn read_pass(store: &Store, now: i64, last: &Option<Mark>) -> Result<Option<Pass>> {
    store.read(|c| {
        let mark = mark(c, now)?;
        if *last == Some(mark) {
            return Ok(None);
        }
        Ok(Some(Pass {
            mark,
            found: pass(c, now)?,
        }))
    })
}

/// The write half of [`refresh`]. The pass is the truth only of the state
/// that it read: a decision or an accept (a new policy) after the read
/// changes the mark, and the write then drops the pass. The next tick
/// runs again, because `last` does not change.
pub fn write_pass(
    store: &Store,
    pass: Pass,
    now: i64,
    last: &mut Option<Mark>,
) -> Result<Option<usize>> {
    let n = store.write(|tx| {
        if mark(tx, now)? != pass.mark {
            return Ok(None);
        }
        write(tx, &pass.found, now).map(Some)
    })?;
    // Only after the commit: a pass that did not commit runs again.
    if n.is_some() {
        *last = Some(pass.mark);
    }
    Ok(n)
}

/// Runs a pass in `tx` and writes its proposals. [`crate::active::record`]
/// calls this for each policy that becomes active, so an accepted rule also
/// removes each other open proposal that it covers, in the same transaction.
pub(crate) fn reconcile(tx: &Connection, now: i64) -> Result<usize> {
    write(tx, &pass(tx, now)?, now)
}

/// Writes the proposals of `found`, and removes each open proposal that is
/// not in it. Gives the number of open proposals.
fn write(tx: &Connection, found: &Found, now: i64) -> Result<usize> {
    let mut keep = BTreeSet::new();
    for (key, approvals) in &found.proposals {
        let (verb, cond, value, form) = key;
        tx.execute(
            "INSERT INTO rule_proposal (verb, cond, value, form, approvals, created_at, updated_at)
             VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?6)
             ON CONFLICT (verb, cond, value, form) DO UPDATE
             SET approvals = excluded.approvals, updated_at = excluded.updated_at
             WHERE state = 'open' AND approvals <> excluded.approvals",
            params![verb, cond, value, form, approvals, now],
        )?;
        keep.insert(key.clone());
    }
    // A pattern that is gone (a refusal, an undo, a new rule) is no longer proposed.
    let open: Vec<(i64, Key)> = tx
        .prepare("SELECT id, verb, cond, value, form FROM rule_proposal WHERE state = 'open'")?
        .query_map([], |r| {
            Ok((r.get(0)?, (r.get(1)?, r.get(2)?, r.get(3)?, r.get(4)?)))
        })?
        .collect::<rusqlite::Result<_>>()?;
    let mut n = 0;
    for (id, key) in open {
        if keep.contains(&key) {
            n += 1;
        } else {
            tx.execute("DELETE FROM rule_proposal WHERE id = ?1", [id])?;
        }
    }
    Ok(n)
}

fn mark(tx: &Connection, now: i64) -> Result<Mark> {
    // The pass reads `applied` and `undone` apart from the other states, so
    // a move into either one (also `approved` to `applied`, with no new
    // decision time) changes the mark.
    let (count, at, undone, applied): (i64, i64, i64, i64) = tx.query_row(
        "SELECT count(*), coalesce(max(decided_at), 0), coalesce(sum(state = 'undone'), 0),
                coalesce(sum(state = 'applied'), 0)
         FROM changeset_action WHERE decided_by = 'person' OR state = 'undone'",
        [],
        |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?, r.get(3)?)),
    )?;
    let version = crate::active(tx)?.version.unwrap_or(0);
    // A new open of a source mail (decision D12). A row never goes.
    let opens: i64 = tx.query_row("SELECT count(*) FROM use_open", [], |r| r.get(0))?;
    // A dismiss: a dismissed proposal is never proposed again, and a
    // dismissed domain gives its address proposals back.
    let closed: i64 = tx.query_row(
        "SELECT count(*) FROM rule_proposal WHERE state <> 'open'",
        [],
        |r| r.get(0),
    )?;
    // Each hour too, for a change of a fact that a rule reads (a sender kind).
    Ok(Mark([
        count,
        at,
        undone,
        applied,
        version,
        opens,
        closed,
        now.div_euclid(3600),
    ]))
}

/// The key of a row: the verb (`hide` for a `hide` rule), the condition,
/// its value, and the form (`''` for an `allow` rule).
type Key = (String, String, String, String);

/// The counts of one key in a pass.
#[derive(Default)]
struct Count {
    /// Approvals and refusals that the rule changes to approved.
    yes: i64,
    no: i64,
    /// An approved action with no hostile mail has the key.
    sourced: bool,
    /// The addresses of the counted approvals of a domain key. A domain
    /// proposal needs two or more: with one, the address proposal covers it.
    from: BTreeSet<String>,
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
        // The window holds the bulk actions too (section 6.6): the loop
        // skips them after the LIMIT, so that they use up the window.
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
    type Row = (i64, String, String, String, Option<String>, bool);
    let rows: Vec<Row> = st
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
    let mut counts: BTreeMap<(Verb, &'static str, String), Count> = BTreeMap::new();
    let mut blocked = BTreeSet::new();
    for (id, verb, state, author, job, bulk) in rows {
        if bulk {
            continue;
        }
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
        let address = |m: &Facts| {
            crate::check_sender(m.sender.clone())
                .ok()
                .filter(|s| !s.starts_with('@'))
        };
        let keys = [
            same(&messages, address).map(|s| ("sender", s)),
            same(&messages, |m| address(m).and_then(|a| domain(&a))).map(|d| ("sender", d)),
            same(&messages, |m| Some(kind(m))).map(|k| ("sender_kind", k.name().to_string())),
        ];
        for (cond, value) in keys.into_iter().flatten() {
            // `when` refuses a value that a rule cannot have, and `hostile`.
            let Some(when) = when(cond, &value) else {
                continue;
            };
            let rule = Proposal {
                id: 0,
                verb: Some(verb),
                when,
                approvals: 0,
                created_at: 0,
            }
            .rule(u32::MAX, "");
            policy.rules.push(rule);
            let changed = !base && allows(&policy, &action, &messages);
            policy.rules.pop();
            let is_domain = value.starts_with('@');
            let c = counts.entry((verb, cond, value)).or_default();
            // An action on hostile mail gives no candidate: Iris does not
            // propose a rule from it, even one that the hard limits allow.
            c.sourced |= approved && !hostile;
            // An approval on hostile mail does not count either; a refusal does.
            match (changed, approved) {
                (true, true) if !hostile => {
                    c.yes += 1;
                    if is_domain && c.from.len() < 2 {
                        c.from.extend(messages.iter().filter_map(address));
                    }
                }
                (true, false) => c.no += 1,
                _ => {}
            }
        }
    }
    for ((verb, cond, value), c) in counts {
        if c.sourced
            && c.no == 0
            && c.yes >= MIN_APPROVALS
            && (!value.starts_with('@') || c.from.len() >= 2)
            && !blocked.contains(&verb)
            && !decided_before(
                conn,
                &(
                    verb.name().to_string(),
                    cond.to_string(),
                    value.clone(),
                    String::new(),
                ),
            )?
        {
            found.proposals.insert(
                (
                    verb.name().to_string(),
                    cond.to_string(),
                    value,
                    String::new(),
                ),
                c.yes,
            );
        }
    }
    choices(conn, &mut policy, now, &mut found)?;
    // One proposal for a domain, not one more for each of its addresses
    // (decision D11). A dismissed domain gives the addresses back.
    let covered: Vec<Key> = found
        .proposals
        .keys()
        .filter(|(verb, cond, value, form)| {
            cond == "sender"
                && !value.starts_with('@')
                && domain(value).is_some_and(|d| {
                    found
                        .proposals
                        .contains_key(&(verb.clone(), cond.clone(), d, form.clone()))
                })
        })
        .cloned()
        .collect();
    for k in covered {
        found.proposals.remove(&k);
    }
    Ok(found)
}

/// The domain of a checked address, as a `sender` value: `@` and the domain.
fn domain(address: &str) -> Option<String> {
    address.rsplit_once('@').map(|(_, d)| format!("@{d}"))
}

/// The `hide` proposals of card R5-02: one pass over the newest
/// [`MAX_ACTIONS`] actions of the person with a verb of [`CHOICES`], `applied`
/// (a choice) or `undone` (a refusal). The candidate of a choice is a `hide`
/// rule with the `sender` of the origin message of its item and the form of
/// the item. A choice counts only when the rule, added to the active policy,
/// changes the attention of the item from shown to hidden.
fn choices(conn: &Connection, policy: &mut Policy, now: i64, found: &mut Found) -> Result<()> {
    let mut st = conn.prepare(
        "SELECT a.id, a.verb, a.state FROM changeset_action a
         JOIN changeset c ON c.id = a.changeset_id
         WHERE a.verb IN ('skip', 'mute', 'dismiss') AND c.author_kind = 'person'
           AND a.state IN ('applied', 'undone')
         ORDER BY a.id DESC LIMIT ?1",
    )?;
    let rows: Vec<(i64, String, String)> = st
        .query_map([MAX_ACTIONS as i64], |r| {
            Ok((r.get(0)?, r.get(1)?, r.get(2)?))
        })?
        .collect::<rusqlite::Result<_>>()?;
    // Keyed by the index of the form in CHOICES.
    let mut counts: BTreeMap<(usize, String), Count> = BTreeMap::new();
    let mut blocked = BTreeSet::new();
    for (id, verb, state) in rows {
        let Some(f) = CHOICES.iter().position(|(v, _)| *v == verb) else {
            continue;
        };
        let form = CHOICES[f].1;
        let chosen = state == "applied";
        let Some((facts, at)) = chosen_item(conn, id, form, now)? else {
            if !chosen {
                // A refusal that cannot be read again blocks each rule of its form.
                blocked.insert(f);
            }
            continue;
        };
        let msg = facts.message();
        let item = &facts.items[at];
        // A conversation that the person started has the person's own
        // message as its origin: a rule from it would hide each such
        // conversation, not the mail of one correspondent.
        if facts.sender_kind == SenderKind::SelfOwner {
            continue;
        }
        // Hostile mail gives no candidate and no count: neither a choice
        // nor a refusal on it is evidence of a preference.
        if kind(&facts) == SenderKind::Hostile {
            continue;
        }
        let Ok(sender) = crate::check_sender(facts.sender.clone()) else {
            if !chosen {
                blocked.insert(f);
            }
            continue;
        };
        if sender.starts_with('@') {
            continue;
        }
        for value in candidates(&sender) {
            let changed = hides(policy, &value, form, item, &msg, now);
            let is_domain = value.starts_with('@');
            let c = counts.entry((f, value)).or_default();
            c.sourced |= chosen;
            match (changed, chosen) {
                (true, true) => {
                    c.yes += 1;
                    if is_domain && c.from.len() < 2 {
                        c.from.insert(sender.clone());
                    }
                }
                (true, false) => c.no += 1,
                _ => {}
            }
        }
    }
    opens(conn, policy, now, &mut counts)?;
    for ((f, value), c) in counts {
        let key = (
            "hide".to_string(),
            "sender".to_string(),
            value,
            CHOICES[f].1.name().to_string(),
        );
        if c.sourced
            && c.no == 0
            && c.yes >= MIN_APPROVALS
            && (!key.2.starts_with('@') || c.from.len() >= 2)
            && !blocked.contains(&f)
            && !decided_before(conn, &key)?
        {
            found.proposals.insert(key, c.yes);
        }
    }
    Ok(())
}

/// The opens of the source mail (decision D12): the rows of `use_open` of
/// the newest days that hold the newest [`MAX_ACTIONS`] opens, each day
/// whole. The measures page writes them, on this machine only.
/// An open of a mail is a refusal of each `hide` candidate that would hide
/// an item whose origin is that mail. An open of a mail that cannot be read
/// again (its header is gone) names no sender, so it refuses nothing.
fn opens(
    conn: &Connection,
    policy: &mut Policy,
    now: i64,
    counts: &mut BTreeMap<(usize, String), Count>,
) -> Result<()> {
    let mut header = conn.prepare_cached("SELECT 1 FROM header WHERE message_id = ?1")?;
    let mut st = conn
        // `use_open` has no order inside a day, so the window takes whole
        // days: each day from the day of the newest MAX_ACTIONS-th open.
        .prepare(
            "SELECT DISTINCT message_id FROM use_open WHERE day >= coalesce(
               (SELECT day FROM use_open ORDER BY day DESC LIMIT 1 OFFSET ?1 - 1), '')",
        )?;
    // Streamed: the cut-off day can hold more than MAX_ACTIONS opens.
    let ids = st.query_map([MAX_ACTIONS as i64], |r| r.get::<_, i64>(0))?;
    for m in ids {
        let m = m?;
        if !header.exists([m])? {
            continue;
        }
        let Some(facts) = facts(conn, m, now)? else {
            continue;
        };
        // The same mail that gives no candidate gives no refusal.
        if facts.sender_kind == SenderKind::SelfOwner || kind(&facts) == SenderKind::Hostile {
            continue;
        }
        let Ok(sender) = crate::check_sender(facts.sender.clone()) else {
            continue;
        };
        if sender.starts_with('@') {
            continue;
        }
        let msg = facts.message();
        for item in &facts.items {
            let Some(f) = CHOICES.iter().position(|(_, form)| *form == item.form) else {
                continue;
            };
            for value in candidates(&sender) {
                if hides(policy, &value, item.form, item, &msg, now) {
                    counts.entry((f, value)).or_default().no += 1;
                }
            }
        }
    }
    Ok(())
}

/// The `sender` values of a `hide` candidate for a checked address: the
/// address and its domain.
fn candidates(address: &str) -> impl Iterator<Item = String> {
    [Some(address.to_string()), domain(address)]
        .into_iter()
        .flatten()
}

/// True when the `hide` rule for `sender` and `form`, added to the active
/// policy, changes the attention of `item` from shown to hidden.
fn hides(
    policy: &mut Policy,
    sender: &str,
    form: Form,
    item: &crate::Item,
    msg: &crate::Message,
    now: i64,
) -> bool {
    let base = policy.attention(item, msg, now);
    policy.rules.push(Rule {
        id: u32::MAX,
        name: String::new(),
        effect: Effect::Hide,
        verbs: vec![],
        when: When {
            sender: Some(sender.to_string()),
            form: Some(form),
            ..When::default()
        },
        by: By::Proposal,
        added: String::new(),
        job: None,
        line: 0,
    });
    let with = policy.attention(item, msg, now);
    policy.rules.pop();
    base == Attention::Shown && with == Attention::Hidden { rule: u32::MAX }
}

/// The facts of the origin message of the one item of action `id`, with the
/// index of the item in them, or `None` when the action cannot be read
/// again: no single item of the form `form`, no single origin message, no
/// `header` row (a cleanup of a gone message deletes it, spec 1 section
/// 5.5) or no sender address.
fn chosen_item(conn: &Connection, id: i64, form: Form, now: i64) -> Result<Option<(Facts, usize)>> {
    let items: Vec<i64> = conn
        .prepare_cached("SELECT item_id FROM action_item WHERE action_id = ?1")?
        .query_map([id], |r| r.get(0))?
        .collect::<rusqlite::Result<_>>()?;
    let [item] = items[..] else {
        return Ok(None);
    };
    let origins: Vec<i64> = conn
        .prepare_cached(
            "SELECT s.message_id FROM item_source s JOIN header h ON h.message_id = s.message_id
             WHERE s.item_id = ?1 AND s.role = 'origin'",
        )?
        .query_map([item], |r| r.get(0))?
        .collect::<rusqlite::Result<_>>()?;
    let [origin] = origins[..] else {
        return Ok(None);
    };
    let Some(facts) = facts(conn, origin, now)? else {
        return Ok(None);
    };
    let at = facts.item_ids.iter().position(|&i| i == item);
    Ok(match at {
        Some(at) if facts.items[at].form == form && !facts.sender.is_empty() => Some((facts, at)),
        _ => None,
    })
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

fn decided_before(tx: &Connection, (verb, cond, value, form): &Key) -> Result<bool> {
    Ok(tx
        .query_row(
            "SELECT 1 FROM rule_proposal WHERE verb = ?1 AND cond = ?2 AND value = ?3
             AND form = ?4 AND state <> 'open'",
            params![verb, cond, value, form],
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
fn name(verb: Option<Verb>, when: &When) -> String {
    let who = match (&when.sender, when.sender_kind) {
        (Some(s), _) => match s.strip_prefix('@') {
            Some(d) => format!("anyone at {d}"),
            None => s.clone(),
        },
        (None, Some(SenderKind::SelfOwner)) => "you".into(),
        (None, Some(SenderKind::Person)) => "people".into(),
        (None, Some(SenderKind::Service)) => "services".into(),
        (None, Some(SenderKind::Stranger)) => "strangers".into(),
        _ => "this sender".into(),
    };
    let Some(verb) = verb else {
        return match when.form {
            Some(Form::Conversation) => format!("Hide conversations with {who}"),
            Some(Form::Signal) => format!("Hide signals from {who}"),
            _ => format!("Hide feed items from {who}"),
        };
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
            String::new(),
        );
        assert_eq!(
            found.proposals.get(&key),
            Some(&30),
            "the bulk action does not count"
        );
    }

    /// Spec 2 section 6.6: the window is the newest MAX_ACTIONS eligible
    /// actions, bulk ones too, and the pass then skips the bulk ones. An
    /// older action outside the window gives no proposal (issue 315).
    #[test]
    fn bulk_actions_use_up_the_window() {
        let dir = std::env::temp_dir().join(format!("iris-propose-window-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        let store = Store::open(&dir).unwrap();
        store
            .write(|tx| {
                tx.execute_batch(
                    "INSERT INTO account(id, provider, address) VALUES (1, 'gmail', 'me@x.example');
                     INSERT INTO thread(id, account_id, provider_id) VALUES (1, 1, 't');",
                )?;
                // MIN_APPROVALS old actions, then MAX_ACTIONS newer bulk actions.
                let old = std::iter::repeat_n(("old@y.example", "[]"), MIN_APPROVALS as usize);
                let bulk = std::iter::repeat_n(("new@y.example", "[\"bulk\"]"), MAX_ACTIONS);
                for (a, (from, flags)) in old.chain(bulk).enumerate() {
                    tx.execute("INSERT INTO changeset(author_kind, created_at) VALUES ('agent', 0)", [])?;
                    let cs = tx.last_insert_rowid();
                    tx.execute(
                        "INSERT INTO changeset_action(changeset_id, revision, n, verb, targets, args, hash)
                         VALUES (?1, 1, 1, 'archive', '{}', '{}', 'h')",
                        [cs],
                    )?;
                    let id = tx.last_insert_rowid();
                    tx.execute(
                        "INSERT INTO message(account_id, thread_id, provider_id, internal_date)
                         VALUES (1, 1, ?1, 0)",
                        [format!("m{a}")],
                    )?;
                    let msg = tx.last_insert_rowid();
                    tx.execute(
                        "INSERT INTO header(message_id, from_addr) VALUES (?1, ?2)",
                        params![msg, from],
                    )?;
                    tx.execute(
                        "INSERT INTO action_message(action_id, message_id) VALUES (?1, ?2)",
                        [id, msg],
                    )?;
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
        assert!(
            found.proposals.keys().all(|k| k.2 != "old@y.example"),
            "{:?}",
            found.proposals
        );
        assert!(found.proposals.is_empty(), "{:?}", found.proposals);
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
            name(Some(Verb::Archive), &w),
            "Archive mail from deals@acme.example"
        );
        let w = When {
            sender_kind: Some(SenderKind::Service),
            ..When::default()
        };
        assert_eq!(
            name(Some(Verb::Read), &w),
            "Mark mail from services as read"
        );
        let w = When {
            sender: Some("news@shop.example".into()),
            form: Some(Form::Feed),
            ..When::default()
        };
        assert_eq!(name(None, &w), "Hide feed items from news@shop.example");
        let w = When {
            sender: Some("@shop.example".into()),
            ..When::default()
        };
        assert_eq!(
            name(Some(Verb::Archive), &w),
            "Archive mail from anyone at shop.example"
        );
    }
}
