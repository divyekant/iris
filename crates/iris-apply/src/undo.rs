//! The undo of a ledger entry (specification 2, section 7.4).

use iris_changeset::{ActionId, Author, AuthorKind, Verb};
use iris_store::Store;
use rusqlite::{params, Connection, OptionalExtension};

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
        // A change of a draft: a change back to the content of `pre`.
        "draft" => Verb::Draft,
        // A state entry: `restore` sets its `pre` values again.
        v if iris_changeset::STATE_VERBS.contains(&v) => Verb::Restore,
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
/// `archive` of each message that had no `INBOX` before the trash. The undo
/// of a state entry (`close`, `decline`, `defer`, `correct`) has one action,
/// `restore`: apply it with [`crate::apply_state`].
///
/// The target of each action is the entry (`{"ledger": ["l-N"]}`): submit
/// resolves the messages with `done` that are not gone. The apply
/// reads the labels at Gmail again, and skips each message that changed
/// after the entry. When each action of the undo applies and the undo
/// changed one or more messages or items, the entry becomes `undone`. An
/// undo that changed nothing keeps the entry open (decision D18).
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
        iris_changeset::Error::Refused(why) | iris_changeset::Error::Denied(why) => {
            Error::Undo(why)
        }
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
    // A state entry changes items: submit resolves them, and refuses an
    // entry with no item in the store.
    let restore = verb_back == Verb::Restore;
    let args = if verb == "draft" {
        change_args(c, ledger)?
    } else {
        if !restore && !done(c, ledger, false)? {
            return Err(refused(format!(
                "l-{ledger} changed no message that is in the mirror"
            )));
        }
        // The same arguments: none, or the label name. `restore` takes none.
        if restore {
            "{}".into()
        } else {
            args
        }
    };
    let why = format!("Undo of l-{ledger}");
    let cs = iris_changeset::create_in(c, author, now_ms)?;
    add(c, cs, 1, verb_back, &args, &why, ledger)?;
    if verb == "trash" && done(c, ledger, true)? {
        add(c, cs, 2, Verb::Archive, "{}", &why, ledger)?;
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

/// The draft change entry has a later entry of its draft, or its draft is
/// sent or has a claimed send (change spec, section 3).
pub const CHANGE_LATER: &str =
    "The draft changed or went out after this change. Iris cannot undo it.";

/// The arguments of the undo of the draft change entry `ledger`: the
/// content before the change, with no subject for a reply (the draft keeps
/// its subject). The same expression as the trigger `ledger_undo_insert`
/// (migration 57). Refuses an entry that is not the newest entry of its
/// draft (view `ledger_can_undo`).
fn change_args(c: &Connection, ledger: i64) -> iris_changeset::Result<String> {
    let args: Option<Option<String>> = c
        .query_row(
            "SELECT CASE WHEN EXISTS (SELECT 1 FROM ledger_can_undo v WHERE v.ledger_id = l.id)
                    THEN CASE WHEN json_extract(l.ops, '$.thread') IS NULL
                           THEN json(json_extract(l.pre, '$.draft.content'))
                         ELSE json_remove(json_extract(l.pre, '$.draft.content'), '$.subject') END
                    END
             FROM ledger l WHERE l.id = ?1 AND json_extract(l.ops, '$.call') = 'drafts.update'",
            [ledger],
            |r| r.get(0),
        )
        .optional()?;
    match args {
        None => Err(iris_changeset::Error::Refused(format!(
            "l-{ledger} has no undo: it made no change of a draft"
        ))),
        Some(None) => Err(iris_changeset::Error::Refused(CHANGE_LATER.into())),
        Some(Some(a)) => Ok(a),
    }
}

/// True if the entry has a message with `done` that is not gone. With
/// `no_inbox`, a message whose `pre` has no `INBOX`.
fn done(c: &Connection, ledger: i64, no_inbox: bool) -> rusqlite::Result<bool> {
    c.query_row(
        "SELECT EXISTS (SELECT 1 FROM ledger_message lm
           JOIN message m ON m.id = lm.message_id AND m.gone_at IS NULL
           WHERE lm.ledger_id = ?1 AND lm.done = 1
             AND (NOT ?2 OR NOT EXISTS (SELECT 1 FROM json_each(lm.pre) WHERE value = 'INBOX')))",
        params![ledger, no_inbox],
        |r| r.get(0),
    )
}

/// Add action `n` of the undo to the draft `cs`, with its link to the entry
/// (migration 23). The target is the entry: submit takes its messages from
/// the ledger, so no list of message IDs is in memory (section 9).
fn add(
    c: &Connection,
    cs: i64,
    n: i64,
    verb: Verb,
    args: &str,
    why: &str,
    ledger: i64,
) -> iris_changeset::Result<()> {
    let targets = serde_json::json!({ "ledger": [format!("l-{ledger}")] }).to_string();
    c.execute(
        "INSERT INTO changeset_action(changeset_id, revision, n, verb, targets, args, why)
         VALUES (?1, 1, ?2, ?3, ?4, ?5, ?6)",
        params![cs, n, verb.as_str(), targets, args, why],
    )?;
    c.execute(
        "INSERT INTO ledger_undo(action_id, ledger_id, step) VALUES (?1, ?2, ?3)",
        params![c.last_insert_rowid(), ledger, n],
    )?;
    Ok(())
}
