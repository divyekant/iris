//! The undo of a ledger entry (specification 2, section 7.4).

use iris_changeset::{ActionId, Author, AuthorKind, Verb};
use iris_store::{format_id, Kind, Store};
use rusqlite::{params, Connection, OptionalExtension};
use serde_json::{Map, Value};

use crate::{Error, Result};

/// The verb that reverses `verb`. `label` and `unlabel` keep the label
/// name of the entry.
fn inverse(verb: &str) -> Option<Verb> {
    Some(match verb {
        "archive" => Verb::Inbox,
        "inbox" => Verb::Archive,
        "read" => Verb::Unread,
        "unread" => Verb::Read,
        "label" => Verb::Unlabel,
        "unlabel" => Verb::Label,
        "trash" => Verb::Untrash,
        "untrash" => Verb::Trash,
        _ => return None,
    })
}

/// Make the undo of the ledger entry `ledger` (the row ID of `l-N`): a
/// changeset of the person, submitted and approved by the person in one
/// transaction (section 7.4). The choice of the person is the approval, so
/// call this only for an undo that the person selected.
///
/// Gives the row IDs of the actions, in order. Apply each one with
/// [`crate::apply`]: the mode `observe` does not hold them, and step 2
/// waits for step 1. The undo of `trash` has two actions: `untrash`, then
/// `archive` of each message that had no `INBOX` before the trash.
///
/// The inverse names each message with `done` that is not gone. The apply
/// reads the labels at Gmail again, and skips each message that changed
/// after the entry. When step 1 applies, the entry becomes `undone`.
///
/// Gives [`Error::Undo`] if the entry has no undo now: it is in `writing`,
/// final or undone, it changed no message, or an undo of it is open.
pub fn undo(store: &Store, ledger: i64, now_ms: i64) -> Result<Vec<i64>> {
    let author = Author {
        kind: AuthorKind::Person,
        name: None,
        session: None,
        job: None,
    };
    iris_changeset::transaction(store, |c| make(c, &author, ledger, now_ms)).map_err(|e| match e {
        iris_changeset::Error::Store(e) => Error::Store(e),
        iris_changeset::Error::NotFound => Error::Undo(format!("l-{ledger} is not in the ledger")),
        iris_changeset::Error::Refused(why) => Error::Undo(why),
    })
}

fn make(
    c: &Connection,
    author: &Author,
    ledger: i64,
    now_ms: i64,
) -> iris_changeset::Result<Vec<i64>> {
    let refused = |why: String| iris_changeset::Error::Refused(why);
    let entry: Option<(String, String, String, String, bool)> = c
        .query_row(
            "SELECT a.verb, a.args, l.state, l.undo,
                    EXISTS (SELECT 1 FROM ledger_undo u JOIN changeset_action o ON o.id = u.action_id
                            WHERE u.ledger_id = l.id
                              AND coalesce(o.state, 'proposed')
                                  IN ('proposed', 'in_review', 'approved'))
             FROM ledger l JOIN changeset_action a ON a.id = l.action_id WHERE l.id = ?1",
            [ledger],
            |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?, r.get(3)?, r.get(4)?)),
        )
        .optional()?;
    let (verb, args, state, undo, open) = entry.ok_or(iris_changeset::Error::NotFound)?;
    if !matches!(state.as_str(), "applied" | "failed") || undo != "inverse" {
        return Err(refused(format!("l-{ledger} has no undo: it is {state}")));
    }
    if open {
        return Err(refused(format!("an undo of l-{ledger} is open")));
    }
    let verb_back = inverse(&verb).ok_or_else(|| refused(format!("{verb} has no undo")))?;
    let targets = done(c, ledger, false)?;
    if targets.is_empty() {
        return Err(refused(format!(
            "l-{ledger} changed no message that is in the mirror"
        )));
    }
    // The arguments of a mailbox verb: none, or the label name.
    let args: Map<String, Value> = serde_json::from_str(&args).unwrap_or_default();
    let why = format!("Undo of l-{ledger}");
    let cs = iris_changeset::create_in(c, author, now_ms)?;
    add(c, cs, author, verb_back, &targets, &args, &why, ledger, 1)?;
    if verb == "trash" {
        let back = done(c, ledger, true)?;
        if !back.is_empty() {
            add(
                c,
                cs,
                author,
                Verb::Archive,
                &back,
                &Map::new(),
                &why,
                ledger,
                2,
            )?;
        }
    }
    iris_changeset::submit_in(c, cs, author, &why, now_ms)?;
    #[allow(clippy::type_complexity)]
    let actions: Vec<(i64, i64, i64, String, Option<String>, Option<String>)> = {
        let mut st = c.prepare(
            "SELECT id, revision, n, state, hash, reason FROM changeset_action
             WHERE changeset_id = ?1 ORDER BY n",
        )?;
        let rows = st.query_map([cs], |r| {
            Ok((
                r.get(0)?,
                r.get(1)?,
                r.get(2)?,
                r.get(3)?,
                r.get(4)?,
                r.get(5)?,
            ))
        })?;
        rows.collect::<rusqlite::Result<_>>()?
    };
    let mut ids = Vec::with_capacity(actions.len());
    for (id, revision, n, state, hash, reason) in actions {
        // A denial of submit rolls the undo back.
        if state != "proposed" {
            return Err(refused(format!(
                "the undo is {state}: {}",
                reason.unwrap_or_default()
            )));
        }
        let action = ActionId {
            changeset: cs,
            revision,
            n,
        };
        iris_changeset::approve_in(c, action, &hash.unwrap_or_default(), now_ms)?;
        ids.push(id);
    }
    Ok(ids)
}

/// The short IDs of the messages of the entry with `done` that are not
/// gone. With `no_inbox`, only the messages whose `pre` has no `INBOX`.
fn done(c: &Connection, ledger: i64, no_inbox: bool) -> rusqlite::Result<Vec<String>> {
    let mut st = c.prepare(
        "SELECT lm.message_id FROM ledger_message lm
         JOIN message m ON m.id = lm.message_id AND m.gone_at IS NULL
         WHERE lm.ledger_id = ?1 AND lm.done = 1
           AND (NOT ?2 OR NOT EXISTS (SELECT 1 FROM json_each(lm.pre) WHERE value = 'INBOX'))
         ORDER BY 1",
    )?;
    let rows = st.query_map(params![ledger, no_inbox], |r| {
        Ok(format_id(Kind::Message, r.get(0)?))
    })?;
    rows.collect()
}

/// Add one action of the undo, with its link to the entry (migration 12).
#[allow(clippy::too_many_arguments)]
fn add(
    c: &Connection,
    cs: i64,
    author: &Author,
    verb: Verb,
    targets: &[String],
    args: &Map<String, Value>,
    why: &str,
    ledger: i64,
    step: i64,
) -> iris_changeset::Result<()> {
    let targets: Vec<&str> = targets.iter().map(String::as_str).collect();
    let n = iris_changeset::add_action_in(c, cs, author, verb, &targets, args, Some(why))?;
    c.execute(
        "INSERT INTO ledger_undo(action_id, ledger_id, step)
         SELECT id, ?3, ?4 FROM changeset_action WHERE changeset_id = ?1 AND n = ?2",
        params![cs, n, ledger, step],
    )?;
    Ok(())
}
