//! The state verbs of the web client (specification 2, section 4): the
//! person closes, declines or defers one commitment, skips a feed item,
//! mutes a conversation or dismisses a signal. As for an undo
//! (section 7.4), the engine makes a changeset with the author `person` and
//! one action, and the choice of the person is the approval. The action
//! applies in the same request, through the one apply path
//! (`iris_apply::apply_state`) and its ledger entry.

use iris_changeset::{ActionId, Author, AuthorKind, Verb};
use rusqlite::OptionalExtension;
use serde::Deserialize;
use serde_json::{Map, Value};

use crate::{item, Error, Result};

/// The body of a verb request. `reason` is optional for each verb but
/// `defer`, which needs `date` (`YYYY-MM-DD`).
#[derive(Debug, Default, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct VerbBody {
    pub reason: Option<String>,
    pub date: Option<String>,
}

/// The most characters of a reason: the limit of submit.
pub use iris_changeset::MAX_REASON;

/// Apply the verb `verb` (`close`, `decline`, `defer`, `skip`, `mute` or `dismiss`) of the person to
/// the item `id`. Gives the item after the change, or `None` when no item
/// has the ID. A body that is not valid is [`Error::Query`]. An item that
/// the verb cannot change (for example, a commitment that is `done`) is
/// [`Error::Conflict`] with the reason of the failed ledger entry.
pub fn apply_verb(
    store: &iris_store::Store,
    id: &str,
    verb: &str,
    b: &VerbBody,
    now_ms: i64,
) -> Result<Option<Value>> {
    let (verb, args, why) = parse(verb, b)?;
    if item(store, id)?.is_none() {
        return Ok(None);
    }
    let person = Author {
        kind: AuthorKind::Person,
        name: None,
        session: None,
        job: None,
    };
    // One transaction: the changeset, the submit and the approval. If the
    // submit denies the action, nothing stays.
    let action = iris_changeset::transaction(store, |c| {
        let cs = iris_changeset::create_in(c, &person, now_ms)?;
        // `add_action_in` gives the action number in the changeset, not
        // the row ID.
        let n = iris_changeset::add_action_in(c, cs, &person, verb, &[id], &args, None)?;
        iris_changeset::submit_in(c, cs, &person, &why, now_ms)?;
        let (row, state, reason, revision): (i64, Option<String>, Option<String>, i64) = c
            .query_row(
                "SELECT id, state, reason, revision FROM changeset_action
                 WHERE changeset_id = ?1 AND n = ?2",
                [cs, n],
                |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?, r.get(3)?)),
            )?;
        if state.as_deref() != Some("proposed") {
            return Err(iris_changeset::Error::Refused(
                reason.unwrap_or_else(|| "the submit did not propose the action".into()),
            ));
        }
        let (_, hash) = iris_changeset::hash(c, row)?;
        let action = ActionId {
            changeset: cs,
            revision,
            n,
        };
        iris_changeset::approve_in(c, action, &hash, now_ms)?;
        Ok(row)
    })
    .map_err(|e| match e {
        iris_changeset::Error::Store(e) => Error::Store(e),
        other => Error::Query(other.to_string()),
    })?;
    // The tick can apply the action first: then this apply finds it not
    // approved. The ledger entry gives the result in both cases.
    match iris_apply::apply_state(store, action) {
        Ok(_) | Err(iris_apply::Error::NotApproved(_)) => {}
        Err(iris_apply::Error::Store(e)) => return Err(Error::Store(e)),
        Err(e) => return Err(Error::Store(iris_store::Error::Query(e.to_string()))),
    }
    let entry: Option<(String, Option<String>)> = store.read(|c| {
        Ok(c.query_row(
            "SELECT state, error FROM ledger WHERE action_id = ?1",
            [action],
            |r| Ok((r.get(0)?, r.get(1)?)),
        )
        .optional()?)
    })?;
    match entry {
        Some((state, _)) if state == "applied" => item(store, id),
        Some((_, error)) => Err(Error::Conflict(
            error.unwrap_or_else(|| "the change failed".into()),
        )),
        None => {
            let reason: Option<String> = store.read(|c| {
                Ok(c.query_row(
                    "SELECT reason FROM changeset_action WHERE id = ?1",
                    [action],
                    |r| r.get(0),
                )?)
            })?;
            Err(Error::Conflict(
                reason.unwrap_or_else(|| "the change did not apply".into()),
            ))
        }
    }
}

/// The verb, its arguments and the reason of the changeset.
fn parse(verb: &str, b: &VerbBody) -> Result<(Verb, Map<String, Value>, String)> {
    let mut args = Map::new();
    let reason = match b.reason.as_deref().map(str::trim) {
        Some("") => return Err(Error::Query("the reason is blank".into())),
        Some(r) if r.chars().count() > MAX_REASON => {
            return Err(Error::Query(format!(
                "the reason is longer than {MAX_REASON} characters"
            )))
        }
        r => r.map(str::to_string),
    };
    let (verb, why) = match verb {
        "close" | "decline" | "skip" | "mute" | "dismiss" => {
            if b.date.is_some() {
                return Err(Error::Query(format!("{verb} takes no date")));
            }
            let (v, default) = match verb {
                "close" => (Verb::Close, "Done."),
                "decline" => (Verb::Decline, "Declined."),
                "skip" => (Verb::Skip, "Skipped."),
                "mute" => (Verb::Mute, "Muted."),
                _ => (Verb::Dismiss, "Dismissed."),
            };
            let why = reason.unwrap_or_else(|| default.into());
            args.insert("reason".into(), why.clone().into());
            (v, why)
        }
        "defer" => {
            let Some(date) = &b.date else {
                return Err(Error::Query("defer needs a date (YYYY-MM-DD)".into()));
            };
            args.insert("date".into(), date.clone().into());
            (
                Verb::Defer,
                reason.unwrap_or_else(|| format!("Later, on {date}.")),
            )
        }
        other => return Err(Error::Query(format!("unknown verb {other}"))),
    };
    Ok((verb, args, why))
}
