//! The apply of an approved action of a state verb (`close`, `decline`,
//! `defer`, `correct`): Iris state only, no provider call (specification 2,
//! section 4).
//!
//! One SQLite transaction writes the new item documents, a `correction` row
//! for each changed field (with the changeset ID as `by`, so the item stays
//! pinned), and the ledger entry with the old values as `pre` and the new
//! values as `post`. So the entry never stays in `writing`.
//!
//! The checks before the write, in order:
//!
//! 1. The action expires 14 days after its submit.
//! 2. In the mode `observe`, an action that a person did not approve
//!    waits. The approval of a person applies, as an undo does: a state
//!    verb changes no mailbox (section 6.5).
//! 3. An action that a rule approved goes to review (issue #168).
//! 4. An action whose hash does not agree with its rows is denied.
//! 5. `close`, `decline` and `defer` change only a commitment that is `open`
//!    or `deferred`. If one item is not, or is gone, the entry is `failed`
//!    with the reason, and no item changes.

use iris_changeset::format_changeset_id;
use iris_store::Store;
use rusqlite::{params, OptionalExtension, Transaction};
use serde_json::{json, Map, Value};

use crate::{expire, now, Counts, Error, Outcome, Result};

/// The state verbs.
const VERBS: &str = "'close', 'decline', 'defer', 'correct'";

/// Apply each approved action of a state verb, oldest first. The engine
/// calls it at each tick. Gives the outcome of each action.
pub fn apply_states(store: &Store) -> Result<Vec<(i64, Outcome)>> {
    let actions: Vec<i64> = store.read(|c| {
        let mut st = c.prepare(&format!(
            "SELECT id FROM changeset_action WHERE state = 'approved' AND verb IN ({VERBS})
             ORDER BY id"
        ))?;
        let rows = st.query_map([], |r| r.get(0))?;
        Ok(rows.collect::<rusqlite::Result<_>>()?)
    })?;
    let mut out = Vec::with_capacity(actions.len());
    for action in actions {
        match apply_state(store, action) {
            Ok(o) => out.push((action, o)),
            // Another writer decided it first: nothing to apply.
            Err(Error::NotApproved(_)) => {}
            Err(e) => return Err(e),
        }
    }
    Ok(out)
}

/// Apply one approved action of a state verb, in one transaction. See the
/// module documentation for the checks.
pub fn apply_state(store: &Store, action: i64) -> Result<Outcome> {
    // A store error rolls back the whole transaction: no entry stays in
    // `writing`, and no item changes without its entry.
    store
        .write(|tx| match apply_in(tx, action, now()) {
            Err(Inner::Store(e)) => Err(e),
            other => Ok(other),
        })?
        .map_err(|e| match e {
            Inner::Store(e) => Error::Store(e),
            Inner::NotApproved => Error::NotApproved(action),
            Inner::Verb(v) => Error::Verb(v),
        })
}

enum Inner {
    Store(iris_store::Error),
    NotApproved,
    Verb(String),
}

impl From<rusqlite::Error> for Inner {
    fn from(e: rusqlite::Error) -> Inner {
        Inner::Store(e.into())
    }
}

impl From<iris_store::Error> for Inner {
    fn from(e: iris_store::Error) -> Inner {
        Inner::Store(e)
    }
}

fn apply_in(tx: &Transaction, action: i64, now_ms: i64) -> std::result::Result<Outcome, Inner> {
    #[allow(clippy::type_complexity)]
    let row: Option<(String, Option<String>, String, i64, String)> = tx
        .query_row(
            "SELECT verb, hash, decided_by, changeset_id, args FROM changeset_action
             WHERE id = ?1 AND state = 'approved'",
            [action],
            |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?, r.get(3)?, r.get(4)?)),
        )
        .optional()?;
    let (verb, hash, decided_by, changeset, args) = row.ok_or(Inner::NotApproved)?;
    if !matches!(verb.as_str(), "close" | "decline" | "defer" | "correct") {
        return Err(Inner::Verb(verb));
    }
    if expire(tx, action)? {
        return Ok(Outcome::Expired);
    }
    if decided_by != "person" && iris_policy::active(tx)?.policy.mode == iris_policy::Mode::Observe
    {
        return Ok(Outcome::Waiting);
    }
    // shortcut: each rule approval goes to review, as for a mailbox verb;
    // evaluate the rule again before the apply (issue #168).
    if decided_by == "rule" {
        let reason = "A person must approve the action: Iris does not check the rule again \
                      before the apply yet.";
        decide(tx, action, "in_review", reason, now_ms)?;
        return Ok(Outcome::Review(reason.into()));
    }
    let now_hash = iris_changeset::hash(tx, action).map_err(|e| match e {
        iris_changeset::Error::Store(e) => Inner::Store(e),
        other => Inner::Store(iris_store::Error::Query(other.to_string())),
    })?;
    if Some(now_hash.1) != hash {
        let reason = "The action changed after its approval.";
        decide(tx, action, "denied", reason, now_ms)?;
        return Ok(Outcome::Denied(reason.into()));
    }
    // Submit wrote the arguments, and checked them.
    let args: Map<String, Value> = serde_json::from_str(&args).unwrap_or_default();
    let fields: Vec<String> = match verb.as_str() {
        // A correction can change the status too (a record becomes
        // `corrected`, a signal follows `expires`).
        "correct" => vec![args["field"].as_str().unwrap_or("").into(), "status".into()],
        // `close` and `decline` remove `deferred_until`: the undo needs it.
        _ => vec!["status".into(), "deferred_until".into()],
    };
    let by = format_changeset_id(changeset);
    // The writes first, in a savepoint: a refusal leaves no item write of
    // the action. Then the entry, with `pre` and `post` read from the
    // documents before and after, so the entry holds each changed value.
    tx.execute_batch("SAVEPOINT state_write")?;
    let result = match plan(tx, action, &verb, &args, &fields, now_ms)? {
        Ok(items) => match write(tx, &verb, &args, &items, &by, now_ms)? {
            Ok(()) => {
                let mut pre = Map::new();
                let mut post = Map::new();
                for i in items {
                    let doc = item_doc(tx, i.item)?.unwrap_or(Value::Null);
                    post.insert(i.id.clone(), values(&doc, &fields));
                    pre.insert(i.id, i.pre);
                }
                Ok((Value::Object(pre), Value::Object(post)))
            }
            Err(e) => Err(e),
        },
        Err(e) => Err(e),
    };
    if result.is_err() {
        tx.execute_batch("ROLLBACK TO state_write")?;
    }
    tx.execute_batch("RELEASE state_write")?;
    let (pre, post) = match &result {
        Ok((pre, post)) => (Some(pre.to_string()), Some(post.to_string())),
        Err(_) => (None, None),
    };
    // The entry claims the action. `pre` and `post` do not change after it.
    tx.execute(
        "INSERT INTO ledger(action_id, ops, pre, post, state, undo, at)
         VALUES (?1, ?2, ?3, ?4, 'writing', ?5, ?6)",
        params![
            action,
            json!({ "fields": fields }).to_string(),
            pre,
            post,
            // A failed entry changed nothing, so it has no undo.
            if result.is_ok() { "inverse" } else { "none" },
            now_ms
        ],
    )?;
    let ledger = tx.last_insert_rowid();
    Ok(match result {
        Ok(_) => {
            // The trigger ledger_state moves the action to `applied`.
            tx.execute(
                "UPDATE ledger SET state = 'applied' WHERE id = ?1",
                [ledger],
            )?;
            Outcome::Applied(Counts::default())
        }
        Err(error) => {
            tx.execute(
                "UPDATE ledger SET state = 'failed', error = ?2 WHERE id = ?1",
                params![ledger, error],
            )?;
            Outcome::Failed {
                counts: Counts::default(),
                error,
            }
        }
    })
}

/// The document of an item row, or `None` if the row or its JSON is gone.
fn item_doc(tx: &Transaction, item: i64) -> rusqlite::Result<Option<Value>> {
    let doc: Option<String> = tx
        .query_row("SELECT doc FROM item WHERE id = ?1", [item], |r| r.get(0))
        .optional()?;
    Ok(doc.and_then(|d| serde_json::from_str(&d).ok()))
}

/// The value of each field of a document, `null` for a field it has not.
fn values(doc: &Value, fields: &[String]) -> Value {
    Value::Object(
        fields
            .iter()
            .map(|f| {
                (
                    f.clone(),
                    doc.pointer(&pointer(f)).cloned().unwrap_or(Value::Null),
                )
            })
            .collect(),
    )
}

/// One item of the action: its row, its ID, the old values of each field,
/// and the values that the verb sets.
struct Planned {
    item: i64,
    id: String,
    pre: Value,
    post: Value,
}

/// The old and the new values of each item, from the documents of now, or
/// the reason why the action cannot apply. Writes nothing.
fn plan(
    tx: &Transaction,
    action: i64,
    verb: &str,
    args: &Map<String, Value>,
    fields: &[String],
    now_ms: i64,
) -> std::result::Result<std::result::Result<Vec<Planned>, String>, Inner> {
    #[allow(clippy::type_complexity)]
    let rows: Vec<(i64, Option<String>, Option<String>, Option<String>)> = {
        let mut st = tx.prepare(
            "SELECT ai.item_id, i.form, i.status, i.doc FROM action_item ai
             LEFT JOIN item i ON i.id = ai.item_id
             WHERE ai.action_id = ?1 ORDER BY ai.item_id",
        )?;
        let rows = st.query_map([action], |r| {
            Ok((r.get(0)?, r.get(1)?, r.get(2)?, r.get(3)?))
        })?;
        rows.collect::<rusqlite::Result<_>>()?
    };
    let mut out = Vec::with_capacity(rows.len());
    for (item, form, status, doc) in rows {
        let doc: Option<Value> = doc.and_then(|d| serde_json::from_str(&d).ok());
        let (Some(form), Some(status), Some(doc)) = (form, status, doc) else {
            return Ok(Err(format!("item {item} is not in the store")));
        };
        let id = doc["id"].as_str().unwrap_or_default().to_string();
        let pre = values(&doc, fields);
        let post: Map<String, Value> = match verb {
            "correct" => {
                let field = &fields[0];
                if let Err(why) =
                    iris_understand::correct::check(tx, &id, field, args["value"].clone(), now_ms)?
                {
                    return Ok(Err(format!("{id}: {why}")));
                }
                [(field.clone(), args["value"].clone())]
                    .into_iter()
                    .collect()
            }
            _ => {
                // Specification 1, section 6.8: from `open` (or `deferred`) only.
                if form != "commitment" || !matches!(status.as_str(), "open" | "deferred") {
                    return Ok(Err(format!(
                        "{id} is {status}: {verb} changes an open or deferred commitment"
                    )));
                }
                let until = if verb == "defer" {
                    args["date"].clone()
                } else {
                    Value::Null
                };
                [
                    ("status".to_string(), json!(new_status(verb))),
                    ("deferred_until".to_string(), until),
                ]
                .into_iter()
                .collect()
            }
        };
        out.push(Planned {
            item,
            id,
            pre,
            post: Value::Object(post),
        });
    }
    Ok(Ok(out))
}

/// Write the planned values, with a `correction` row for each field. Gives
/// the reason if a correction refuses now. The caller then rolls back to
/// its savepoint, and fails the entry.
fn write(
    tx: &Transaction,
    verb: &str,
    args: &Map<String, Value>,
    items: &[Planned],
    by: &str,
    now_ms: i64,
) -> std::result::Result<std::result::Result<(), String>, Inner> {
    for p in items {
        if verb == "correct" {
            let field = args["field"].as_str().unwrap_or_default();
            if let Err(why) = iris_understand::correct::correct(
                tx,
                &p.id,
                field,
                args["value"].clone(),
                by,
                now_ms,
            )? {
                return Ok(Err(format!("{}: {why}", p.id)));
            }
            continue;
        }
        let status = new_status(verb);
        let until = p.post.get("deferred_until").cloned().unwrap_or(Value::Null);
        tx.execute(
            "UPDATE item SET status = ?2, updated_at = ?3,
               doc = CASE WHEN ?4 IS NULL
                 THEN json_remove(json_set(doc, '$.status', ?2, '$.updated', ?5), '$.deferred_until')
                 ELSE json_set(doc, '$.status', ?2, '$.updated', ?5, '$.deferred_until', ?4) END
             WHERE id = ?1",
            params![
                p.item,
                status,
                now_ms,
                until.as_str(),
                iris_talk::rfc3339(now_ms)
            ],
        )?;
        let mut add = tx.prepare_cached(
            "INSERT INTO correction(item_id, field, value, by, at) VALUES (?1, ?2, ?3, ?4, ?5)",
        )?;
        // A removed field (`deferred_until` of `close`) has no correction.
        for (field, value) in p.post.as_object().into_iter().flatten() {
            if !value.is_null() {
                add.execute(params![p.item, field, value.to_string(), by, now_ms])?;
            }
        }
    }
    Ok(Ok(()))
}

/// The status that a verb sets (specification 2, section 4).
fn new_status(verb: &str) -> &'static str {
    match verb {
        "close" => "done",
        "decline" => "declined",
        _ => "deferred",
    }
}

/// The JSON pointer of a dotted field path, for example `due.date`.
fn pointer(field: &str) -> String {
    field
        .split('.')
        .map(|s| format!("/{}", s.replace('~', "~0").replace('/', "~1")))
        .collect()
}

/// A decision of the engine on an approved action: `denied` or `in_review`.
fn decide(
    tx: &Transaction,
    action: i64,
    state: &str,
    reason: &str,
    now_ms: i64,
) -> rusqlite::Result<()> {
    tx.execute(
        "UPDATE changeset_action SET state = ?2, decided_by = 'engine', rule = NULL,
           policy = NULL, reason = ?3, decided_at = ?4
         WHERE id = ?1 AND state = 'approved'",
        params![action, state, reason, now_ms],
    )?;
    Ok(())
}
