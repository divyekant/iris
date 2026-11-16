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
//! This crate has no provider client and makes no model call.

use iris_changeset::{
    add_action_in, create_in, revise_in, submit_in, Author, AuthorKind, Error, Verb,
};
use iris_observe::Rule;
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
    /// empty and the open actions stay.
    Same { changeset: i64 },
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
    // Pass 1: compare the proposal with the newest revision, one action at
    // a time.
    let mut actions = 0i64;
    let mut same = open.is_some();
    each_action(c, account, |rule, ids| {
        actions += 1;
        if let (true, Some((id, revision))) = (same, &open) {
            let old: Option<(String, Option<String>, String)> = c
                .query_row(
                    "SELECT verb, why, targets FROM changeset_action
                     WHERE changeset_id = ?1 AND revision = ?2 AND n = ?3",
                    params![id, revision, actions],
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
        // shortcut: an empty proposal leaves the open actions as they are,
        // because a revision needs an action; withdraw them when withdraw is
        // built (R2-01 part 3).
        Some((id, ..)) if actions == 0 => return Ok(Outcome::Same { changeset: id }),
        Some((id, revision)) if same => {
            let more: bool = c.query_row(
                "SELECT EXISTS (SELECT 1 FROM changeset_action
                 WHERE changeset_id = ?1 AND revision = ?2 AND n > ?3)",
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
    let revision = c.query_row(
        "SELECT revision FROM changeset WHERE id = ?1",
        [changeset],
        |r| r.get(0),
    )?;
    Ok(Outcome::Submitted {
        changeset,
        revision,
    })
}

/// Call `f` for each action of the proposal, in order: each rule, in groups
/// of at most [`CHUNK`] message IDs.
fn each_action(
    c: &Connection,
    account: i64,
    mut f: impl FnMut(Rule, &[String]) -> Result<(), Error>,
) -> Result<(), Error> {
    for rule in Rule::ALL {
        // A decision on a message holds: a message that a decided triage
        // action of the account named does not come back. A replaced action
        // was not decided, and an open action is still in the proposal.
        let mut st = c.prepare(&format!(
            "SELECT id FROM ({}) WHERE id NOT IN (
               SELECT m.message_id FROM action_message m
               JOIN changeset_action a ON a.id = m.action_id
               JOIN changeset s ON s.id = a.changeset_id
               WHERE s.author_kind = 'triage' AND s.author_session = ?2
                 AND a.state NOT IN ('proposed', 'in_review', 'approved', 'held', 'replaced'))
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
