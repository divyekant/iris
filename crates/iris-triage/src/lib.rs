//! Triage (release 2): the engine proposes mailbox changes as a stored
//! changeset (specification 2, section 5.4).
//!
//! The rules come from observe mode (`iris-observe`): each rule reads the
//! forms that the model made, and gives one `archive` action with its
//! reason. [`run`] stores the proposal of one account as a changeset of the
//! author `triage`, and submits it. Submit gives each action `proposed`, so
//! nothing applies without a rule or an approval.
//!
//! Each account has one open triage changeset. A run adds a new revision
//! only when the proposal changed, so an action that a person reviews does
//! not change while nothing changed in the mailbox.
//!
//! This crate has no provider client and makes no model call.

use iris_changeset::{add_action, create, revise, submit, Author, AuthorKind, Error, Verb};
use iris_observe::Rule;
use iris_store::Store;
use rusqlite::{params, OptionalExtension};
use serde_json::{json, Map};

/// The reason of each triage revision.
pub const WHY: &str =
    "Triage proposes these mailbox changes. Nothing applies without a rule or your approval.";

/// The result of a run.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Outcome {
    /// The run submitted this revision of the changeset.
    Submitted { changeset: i64, revision: i64 },
    /// The newest revision already has this proposal.
    Same { changeset: i64 },
    /// No rule matches a message, and no triage changeset is open.
    Nothing,
}

/// The author of the triage changeset of `account`. The session names the
/// account, because one action changes one account (section 5.6).
pub fn author(account: i64) -> Author {
    Author {
        kind: AuthorKind::Triage,
        name: Some("Iris".into()),
        session: Some(format!("account-{account}")),
        job: None,
    }
}

/// Store the triage proposal of `account` (time `now_ms`).
pub fn run(store: &Store, account: i64, now_ms: i64) -> Result<Outcome, Error> {
    let who = author(account);
    // One read transaction, so no message is in two actions.
    // shortcut: the message IDs of each rule are in memory, because
    // add_action takes a list; target items or threads if an inbox has
    // more than about 100,000 messages that triage proposes to archive.
    let proposal: Vec<(Rule, Vec<String>)> = store.read(|c| {
        let tx = c.unchecked_transaction()?;
        let mut out = Vec::new();
        for rule in Rule::ALL {
            let ids = iris_observe::message_ids(&tx, rule, account)?;
            if !ids.is_empty() {
                out.push((rule, ids));
            }
        }
        Ok(out)
    })?;
    let open: Option<(i64, String, i64)> = store.read(|c| {
        Ok(c.query_row(
            "SELECT id, state, revision FROM changeset
             WHERE author_kind = 'triage' AND author_name IS ?1 AND author_session IS ?2
               AND state IN ('draft', 'open')
             ORDER BY id DESC LIMIT 1",
            params![who.name, who.session],
            |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?)),
        )
        .optional()?)
    })?;
    let changeset = match open {
        None if proposal.is_empty() => return Ok(Outcome::Nothing),
        None => create(store, &who, now_ms)?,
        Some((id, state, revision)) => {
            let newest = store.read(|c| {
                let mut st = c.prepare(
                    "SELECT verb, why, targets FROM changeset_action
                     WHERE changeset_id = ?1 AND revision = ?2 ORDER BY n",
                )?;
                let rows = st.query_map(params![id, revision], |r| {
                    Ok((
                        r.get::<_, String>(0)?,
                        r.get::<_, Option<String>>(1)?,
                        r.get::<_, String>(2)?,
                    ))
                })?;
                Ok(rows.collect::<rusqlite::Result<Vec<_>>>()?)
            })?;
            if state == "draft" {
                if !newest.is_empty() {
                    // A run stopped before its submit. The draft actions
                    // cannot go away, so submit them, and the new revision
                    // below replaces them.
                    submit(store, id, &who, WHY, now_ms)?;
                    revise(store, id, &who)?;
                }
            } else {
                let same = newest.len() == proposal.len()
                    && newest
                        .iter()
                        .zip(&proposal)
                        .all(|((verb, why, targets), (rule, t))| {
                            verb == rule.verb()
                                && why.as_deref() == Some(rule.why())
                                && serde_json::from_str::<serde_json::Value>(targets).ok()
                                    == Some(json!({ "messages": t }))
                        });
                if same {
                    return Ok(Outcome::Same { changeset: id });
                }
                // shortcut: an empty proposal leaves the open actions as
                // they are, because a revision needs an action; withdraw
                // them when withdraw is built (R2-01 part 3).
                if proposal.is_empty() {
                    return Ok(Outcome::Same { changeset: id });
                }
                revise(store, id, &who)?;
            }
            id
        }
    };
    for (rule, ids) in &proposal {
        let ids: Vec<&str> = ids.iter().map(String::as_str).collect();
        let verb = Verb::parse(rule.verb()).expect("a verb of section 4");
        add_action(
            store,
            changeset,
            &who,
            verb,
            &ids,
            &Map::new(),
            Some(rule.why()),
        )?;
    }
    submit(store, changeset, &who, WHY, now_ms)?;
    let revision = store.read(|c| {
        Ok(c.query_row(
            "SELECT revision FROM changeset WHERE id = ?1",
            [changeset],
            |r| r.get(0),
        )?)
    })?;
    Ok(Outcome::Submitted {
        changeset,
        revision,
    })
}
