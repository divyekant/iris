//! Triage (release 2): the engine proposes mailbox changes as a stored
//! changeset (specification 2, section 5.4).
//!
//! The rules come from observe mode (`iris-observe`): each rule reads the
//! forms that the model made, and gives `archive` actions with the reason of
//! the rule. [`run`] stores the proposal of one account as a changeset of the
//! author `triage`, and submits it. Submit gives each action `proposed`, so
//! nothing applies without a rule or an approval.
//!
//! Each account has one open triage changeset. A run adds a new revision
//! only when the proposal changed, so an action that a person reviews does
//! not change while nothing changed in the mailbox.
//!
//! One run is one write transaction (specification 1, section 8): a crash
//! leaves no draft, and two runs cannot both make a changeset.
//!
//! Memory: a rule gives one action for each [`CHUNK`] messages, and the run
//! reads the IDs of one action at a time (specification 1, section 9).
//!
//! In the policy mode `apply`, the active policy then decides each action
//! (spec 2, section 5.4, step 5), with the reason of the decision.
//!
//! This crate has no provider client and makes no model call.

use iris_changeset::{
    add_action_in, create_in, decide_in, revise_in, submit_in, withdraw_in, ActionId, Author,
    AuthorKind, By, Error, State, Verb,
};
use iris_observe::Rule;
use iris_policy::{Facts, Mode, Outcome as Policy};
use iris_store::{format_id, Kind, Store};
use rusqlite::{params, Connection, OptionalExtension};
use serde_json::{json, Map, Value};

/// The reason of each triage revision.
pub const WHY: &str =
    "Triage proposes these mailbox changes. Nothing applies without a rule or your approval.";

/// The most messages in one triage action.
pub const CHUNK: usize = 500;

/// The result of a run.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Outcome {
    /// The run submitted this revision of the changeset.
    Submitted { changeset: i64, revision: i64 },
    /// The newest revision already has this proposal, or the proposal is
    /// empty and an approved action keeps the changeset open.
    Same { changeset: i64 },
    /// The proposal is empty: the run withdrew the open changeset, so its
    /// proposed and in_review actions are cancelled.
    Withdrawn { changeset: i64 },
    /// No rule matches a message, and no triage changeset is open.
    Nothing,
}

/// The author of the triage changeset of `account`. The session names the
/// account, because one action changes one account (spec 2, section 5.6).
pub fn author(account: i64) -> Author {
    Author {
        kind: AuthorKind::Triage,
        name: Some("Iris".into()),
        session: Some(format!("account-{account}")),
        job: None,
    }
}

/// Store the triage proposal of `account` (time `now_ms`), in one write
/// transaction.
pub fn run(store: &Store, account: i64, now_ms: i64) -> Result<Outcome, Error> {
    iris_changeset::transaction(store, |c| run_in(c, account, now_ms))
}

fn run_in(c: &Connection, account: i64, now_ms: i64) -> Result<Outcome, Error> {
    let who = author(account);
    // Only a run makes a triage changeset, in one transaction, so it is
    // never a draft after a commit.
    let open: Option<(i64, i64)> = c
        .query_row(
            "SELECT id, revision FROM changeset
             WHERE author_kind = 'triage' AND author_name IS ?1 AND author_session IS ?2
               AND state = 'open'
             ORDER BY id DESC LIMIT 1",
            params![who.name, who.session],
            |r| Ok((r.get(0)?, r.get(1)?)),
        )
        .optional()?;
    // Pass 1: compare the proposal with the actions of the newest revision
    // that wait for a decision, one action at a time. The proposal leaves
    // out the messages of the other actions.
    let mut actions = 0i64;
    let mut same = open.is_some();
    each_action(c, account, |rule, ids| {
        actions += 1;
        if let (true, Some((id, revision))) = (same, &open) {
            let old: Option<(String, Option<String>, String)> = c
                .query_row(
                    "SELECT verb, why, targets FROM changeset_action
                     WHERE changeset_id = ?1 AND revision = ?2
                       AND state IN ('proposed', 'in_review')
                     ORDER BY n LIMIT 1 OFFSET ?3",
                    params![id, revision, actions - 1],
                    |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?)),
                )
                .optional()?;
            same = old.is_some_and(|(verb, why, targets)| {
                verb == rule.verb()
                    && why.as_deref() == Some(rule.why())
                    && serde_json::from_str::<Value>(&targets).ok()
                        == Some(json!({ "messages": ids }))
            });
        }
        Ok(())
    })?;
    let changeset = match open {
        None if actions == 0 => return Ok(Outcome::Nothing),
        // An empty proposal withdraws the changeset (a revision needs an
        // action). The proposal leaves out the messages of an approved
        // action, and a withdraw would cancel it, so such an action keeps
        // the changeset open.
        // shortcut: the proposed and in_review actions then wait for a
        // person or the expiry (issue #220); a cancel of one action needs
        // a new move in the trigger action_state.
        Some((id, ..)) if actions == 0 => {
            let approved: bool = c.query_row(
                "SELECT EXISTS (SELECT 1 FROM changeset_action
                 WHERE changeset_id = ?1 AND state IN ('approved', 'held'))",
                [id],
                |r| r.get(0),
            )?;
            if approved {
                return Ok(Outcome::Same { changeset: id });
            }
            withdraw_in(c, id, Some(&who))?;
            return Ok(Outcome::Withdrawn { changeset: id });
        }
        Some((id, revision)) if same => {
            let more: bool = c.query_row(
                "SELECT count(*) > ?3 FROM changeset_action
                 WHERE changeset_id = ?1 AND revision = ?2
                   AND state IN ('proposed', 'in_review')",
                params![id, revision, actions],
                |r| r.get(0),
            )?;
            if !more {
                return Ok(Outcome::Same { changeset: id });
            }
            revise_in(c, id, &who)?;
            id
        }
        Some((id, ..)) => {
            revise_in(c, id, &who)?;
            id
        }
        None => create_in(c, &who, now_ms)?,
    };
    // Pass 2: the same snapshot, so the same actions as pass 1.
    each_action(c, account, |rule, ids| {
        let ids: Vec<&str> = ids.iter().map(String::as_str).collect();
        let verb = Verb::parse(rule.verb()).expect("a verb of section 4");
        add_action_in(
            c,
            changeset,
            &who,
            verb,
            &ids,
            &Map::new(),
            Some(rule.why()),
        )?;
        Ok(())
    })?;
    submit_in(c, changeset, &who, WHY, now_ms)?;
    let revision: i64 = c.query_row(
        "SELECT revision FROM changeset WHERE id = ?1",
        [changeset],
        |r| r.get(0),
    )?;
    decide_submitted(c, changeset, revision, iris_policy::Author::Triage, now_ms)?;
    Ok(Outcome::Submitted {
        changeset,
        revision,
    })
}

/// Spec 2 section 5.4, step 5, for each author: in the mode `apply`, the
/// active policy decides each `proposed` action of the revision, with its
/// reason. In the mode `observe`, a hard limit that denies (section 6.5)
/// denies the action, and each other action stays `proposed`. Call it in
/// the transaction of the submit, so a stop leaves no submitted revision
/// without its decisions.
pub fn decide_submitted(
    c: &Connection,
    changeset: i64,
    revision: i64,
    author: iris_policy::Author,
    now_ms: i64,
) -> Result<(), Error> {
    let active = iris_policy::active(c)?;
    let apply = active.policy.mode == Mode::Apply;
    let now = now_ms.div_euclid(1000);
    // A rule with `job` matches only the actions of that job.
    let job: Option<String> = c.query_row(
        "SELECT author_job FROM changeset WHERE id = ?1",
        [changeset],
        |r| r.get(0),
    )?;
    // One action at a time, by its number, so memory holds one action.
    let mut last = 0i64;
    while let Some((id, n, verb, flags)) = c
        .query_row(
            "SELECT id, n, verb, flags FROM changeset_action
             WHERE changeset_id = ?1 AND revision = ?2 AND state = 'proposed' AND n > ?3
             ORDER BY n LIMIT 1",
            params![changeset, revision, last],
            |r| {
                Ok((
                    r.get::<_, i64>(0)?,
                    r.get::<_, i64>(1)?,
                    r.get::<_, String>(2)?,
                    r.get::<_, String>(3)?,
                ))
            },
        )
        .optional()?
    {
        last = n;
        let id_n = ActionId {
            changeset,
            revision,
            n,
        };
        // No rule decides a verb that the policy does not know: `draft` and
        // `send` (spec R3-01, issue #239). Each goes to a person, in the mode
        // `apply`; in the mode `observe` it stays proposed, as other actions.
        let Some(verb) = iris_policy::Verb::from_name(&verb) else {
            if apply {
                decide_in(
                    c,
                    id_n,
                    State::InReview,
                    By::Policy,
                    Some("No rule can allow a draft or a send: a person decides."),
                    now_ms,
                )?;
            }
            continue;
        };
        let bulk = serde_json::from_str::<Vec<String>>(&flags)
            .map_err(|e| Error::Refused(format!("flags: {e}")))?
            .iter()
            .any(|f| f == "bulk");
        // At most CHUNK messages: the action holds their facts.
        let mut facts: Vec<Facts> = Vec::new();
        let ids: Vec<i64> = c
            .prepare("SELECT message_id FROM action_message WHERE action_id = ?1 ORDER BY 1")?
            .query_map([id], |r| r.get(0))?
            .collect::<rusqlite::Result<_>>()?;
        for m in ids {
            facts.extend(iris_policy::facts(c, m, now)?);
        }
        let d = active.policy.decide(
            &iris_policy::Action {
                verb,
                author,
                job: job.as_deref(),
                bulk,
                now,
            },
            facts.iter().map(Facts::message),
        );
        // A hard limit: a denial with no rule (spec 2 section 6.5).
        if !apply && !(d.outcome == Policy::Deny && d.rule.is_none()) {
            continue;
        }
        let to = match d.outcome {
            Policy::Allow => State::Approved,
            Policy::Review => State::InReview,
            Policy::Deny => State::Denied,
        };
        let by = match (d.rule, active.version) {
            (Some(rule), Some(policy)) => By::Rule {
                id: rule.into(),
                policy,
            },
            // A rule decision needs the confirmed version (spec 2, 5.1).
            (Some(_), None) => return Err(Error::Refused("a rule with no policy version".into())),
            // No rule: a hard limit (spec 2, section 6.5) or the default review.
            (None, _) if bulk || to == State::Denied => By::Limit,
            (None, _) => By::Policy,
        };
        decide_in(c, id_n, to, by, Some(&d.reason), now_ms)?;
    }
    Ok(())
}

/// Call `f` for each action of the proposal, in order: each rule, in groups
/// of at most [`CHUNK`] message IDs.
fn each_action(
    c: &Connection,
    account: i64,
    mut f: impl FnMut(Rule, &[String]) -> Result<(), Error>,
) -> Result<(), Error> {
    for rule in Rule::ALL {
        // A decision on a message holds: a message that a decided or an
        // approved triage action of the account named does not come back. A
        // replaced action was not decided, and a proposed or in_review
        // action is still in the proposal.
        let mut st = c.prepare(&format!(
            "SELECT id FROM ({}) WHERE id NOT IN (
               SELECT m.message_id FROM action_message m
               JOIN changeset_action a ON a.id = m.action_id
               JOIN changeset s ON s.id = a.changeset_id
               WHERE s.author_kind = 'triage' AND s.author_session = ?2
                 AND a.state NOT IN ('proposed', 'in_review', 'replaced'))
             ORDER BY 1",
            rule.query()
        ))?;
        let mut rows = st.query(params![account, author(account).session])?;
        let mut ids = Vec::with_capacity(CHUNK);
        while let Some(row) = rows.next()? {
            ids.push(format_id(Kind::Message, row.get(0)?));
            if ids.len() == CHUNK {
                f(rule, &ids)?;
                ids.clear();
            }
        }
        if !ids.is_empty() {
            f(rule, &ids)?;
        }
    }
    Ok(())
}
