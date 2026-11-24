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
//! 3. An action that a rule approved is evaluated again against the active
//!    policy (specification 2, section 5.4).
//! 4. An action whose hash does not agree with its rows is denied.
//! 5. `close`, `decline` and `defer` change only a commitment that is `open`
//!    or `deferred`. If one item is not, or is gone, the entry is `failed`
//!    with the reason, and no item changes.
//!
//! `restore` is the undo of a state entry (section 7.4): see [`restore`].

use iris_changeset::format_changeset_id;
use iris_store::Store;
use rusqlite::{params, OptionalExtension, Transaction};
use serde_json::{json, Map, Value};

use crate::{expire, now, Counts, Error, Outcome, Result};

/// The actions that [`apply_states`] reads in one page (specification 2,
/// section 9: no list of changesets in memory).
const PAGE: i64 = 100;

/// Apply each approved action of a state verb, oldest first, one page of
/// IDs at a time. The engine calls it at each tick. Gives the count of
/// actions that applied. An action that waits stays, and the next page
/// starts after it.
pub fn apply_states(store: &Store) -> Result<usize> {
    let (mut after, mut applied) = (0, 0);
    loop {
        let page: Vec<i64> = store.read(|c| {
            let mut st = c.prepare_cached(&format!(
                "SELECT id FROM changeset_action
                 WHERE state = 'approved' AND verb IN ({}) AND id > ?1
                 ORDER BY id LIMIT ?2",
                // `restore` is the undo of a state entry.
                iris_changeset::sql_list(iris_changeset::STATE_VERBS.iter().chain(&["restore"]))
            ))?;
            let rows = st.query_map([after, PAGE], |r| r.get(0))?;
            Ok(rows.collect::<rusqlite::Result<_>>()?)
        })?;
        for &action in &page {
            match apply_state(store, action) {
                Ok(Outcome::Applied(_)) => applied += 1,
                Ok(_) => {}
                // Another writer decided it first: nothing to apply.
                Err(Error::NotApproved(_)) => {}
                Err(e) => return Err(e),
            }
        }
        match page.last() {
            Some(&last) if page.len() as i64 == PAGE => after = last,
            _ => return Ok(applied),
        }
    }
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
    if !iris_changeset::STATE_VERBS.contains(&verb.as_str()) && verb != "restore" {
        return Err(Inner::Verb(verb));
    }
    // While stopped, only a changeset of the person applies: its state
    // verbs change only the items of the store (docs/specs/2026-11-17-stop.md).
    if iris_store::stopped(tx)?.is_some() {
        let author: String = tx.query_row(
            "SELECT author_kind FROM changeset WHERE id = ?1",
            [changeset],
            |r| r.get(0),
        )?;
        if author != "person" {
            return Ok(Outcome::Stopped);
        }
    }
    if expire(tx, action)? {
        return Ok(Outcome::Expired);
    }
    // In the transaction of the item writes: a revoke that commits first
    // stops the action (docs/specs/2026-11-17-job-verbs.md).
    if let Some(why) = crate::deny_outside_grant(tx, action)? {
        return Ok(Outcome::Denied(why));
    }
    if decided_by != "person" && iris_policy::active(tx)?.policy.mode == iris_policy::Mode::Observe
    {
        return Ok(Outcome::Waiting);
    }
    // Section 5.4: a rule approval is evaluated again before the apply.
    if let Some(o) = crate::recheck_rule(tx, action)? {
        return Ok(o);
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
    if verb == "restore" {
        return restore(tx, action, now_ms);
    }
    // Submit wrote the arguments, and checked them.
    let args: Map<String, Value> = serde_json::from_str(&args).unwrap_or_default();
    let fields: Vec<String> = match verb.as_str() {
        // A correction can change the status too (a record becomes
        // `corrected`, a signal follows `expires`).
        // The entry also holds the whole top-level value of the field: a
        // correction can change its siblings too (an address of the owner
        // makes the party `self`), and the undo sets them back.
        "correct" => {
            let field = args["field"].as_str().unwrap_or("");
            let top = field.split('.').next().unwrap_or(field);
            let mut f = vec![field.to_string()];
            if top != field {
                f.push(top.to_string());
            }
            f.push("status".into());
            f
        }
        // `skip`, `mute` and `dismiss` change the status only.
        "skip" | "mute" | "dismiss" => vec!["status".into()],
        // `close` and `decline` remove `deferred_until`: the undo needs it.
        _ => vec!["status".into(), "deferred_until".into()],
    };
    let by = format_changeset_id(changeset);
    // The writes first, in a savepoint: a refusal leaves no item write of
    // the action. Then the entry, with `pre` and `post` read from the
    // documents before and after, so the entry holds each changed value.
    // Each correction row after this ID is of this entry: the undo removes
    // only these rows, also when the changeset has more actions on the item.
    let last: i64 = tx.query_row("SELECT coalesce(max(id), 0) FROM correction", [], |r| {
        r.get(0)
    })?;
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
    let corrections: Vec<i64> = if result.is_ok() {
        let mut st = tx.prepare("SELECT id FROM correction WHERE id > ?1 ORDER BY id")?;
        let ids = st.query_map([last], |r| r.get(0))?;
        ids.collect::<rusqlite::Result<_>>()?
    } else {
        Vec::new()
    };
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
            json!({ "fields": fields, "corrections": corrections }).to_string(),
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

/// The apply of `restore`, the undo of a state entry (specification 2,
/// section 4): each item that still has the `post` values of the entry gets
/// its `pre` values again, and loses the correction rows of the entry. An
/// item that changed after the entry stays as it is, and counts as
/// unchanged. The undo has its own ledger entry, and the first entry
/// becomes `undone`.
fn restore(tx: &Transaction, action: i64, now_ms: i64) -> std::result::Result<Outcome, Inner> {
    #[allow(clippy::type_complexity)]
    let (entry, pre, post, ops): (i64, String, String, String) = tx.query_row(
        "SELECT l.id, l.pre, l.post, l.ops FROM ledger_undo u
         JOIN ledger l ON l.id = u.ledger_id
         WHERE u.action_id = ?1 AND u.step = 1",
        [action],
        |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?, r.get(3)?)),
    )?;
    let parse = |t: &str| serde_json::from_str::<Value>(t).unwrap_or(Value::Null);
    let (pre, post, ops) = (parse(&pre), parse(&post), parse(&ops));
    let fields: Vec<String> = ops["fields"]
        .as_array()
        .into_iter()
        .flatten()
        .filter_map(|f| f.as_str().map(str::to_string))
        .collect();
    let corrections = ops["corrections"].to_string();
    // `later`: the item is gone, or a state entry after this entry names it.
    // The value check alone misses a later action that set the same values.
    let rows: Vec<(i64, String, bool)> = {
        let mut st = tx.prepare(&format!(
            "SELECT i.id, i.doc, i.status = 'gone' OR EXISTS (
               SELECT 1 FROM action_item x
               JOIN changeset_action o ON o.id = x.action_id
               JOIN ledger l ON l.action_id = o.id
               WHERE x.item_id = i.id AND x.action_id != ?1 AND l.id > ?2
                 AND l.state != 'failed'
                 AND o.verb IN ({}))
             FROM action_item ai JOIN item i ON i.id = ai.item_id
             WHERE ai.action_id = ?1 ORDER BY i.id",
            iris_changeset::sql_list(iris_changeset::STATE_VERBS.iter().chain(&["restore"]))
        ))?;
        let rows = st.query_map(params![action, entry], |r| {
            Ok((r.get(0)?, r.get(1)?, r.get(2)?))
        })?;
        rows.collect::<rusqlite::Result<_>>()?
    };
    let (mut was, mut now) = (Map::new(), Map::new());
    let mut counts = Counts::default();
    for (item, doc, later) in rows {
        let Ok(doc) = serde_json::from_str::<Value>(&doc) else {
            counts.unchanged += 1;
            continue;
        };
        let id = doc["id"].as_str().unwrap_or_default().to_string();
        let current = values(&doc, &fields);
        // A later change (an action, understanding, the tick of a deferred
        // item, a gone item) wins: the item stays as it is.
        if later || post.get(&id) != Some(&current) || !pre[&id].is_object() {
            counts.unchanged += 1;
            continue;
        }
        // `set_field` is the path rule of a correction, with array indices.
        // A path that does not fit leaves the item as it is.
        let mut new = doc.clone();
        let fits = fields.iter().all(|f| match &pre[&id][f] {
            Value::Null => remove(&mut new, f),
            v => iris_talk::set_field(&mut new, f, v.clone()),
        });
        // The restored item must be valid against the schemas, as after a
        // correction (with a placeholder sender ID, so no sender row yet).
        let mut check = new.clone();
        iris_understand::set_senders(&mut check, &mut |_, _| 1);
        if !fits || iris_understand::validate(&check).is_err() {
            counts.unchanged += 1;
            continue;
        }
        let mut doc = new;
        // The sender ID of each party follows its address, as for a correction.
        let mut failed = None;
        iris_understand::set_senders(&mut doc, &mut |addr, name| {
            iris_understand::store::sender_row(tx, addr, name).unwrap_or_else(|e| {
                failed.get_or_insert(e);
                0
            })
        });
        if let Some(e) = failed {
            return Err(e.into());
        }
        doc["updated"] = json!(iris_talk::rfc3339(now_ms));
        tx.execute(
            "UPDATE item SET doc = ?2, updated_at = ?3, status = coalesce(?4, status) WHERE id = ?1",
            params![item, doc.to_string(), now_ms, doc["status"].as_str()],
        )?;
        tx.execute(
            "DELETE FROM correction WHERE item_id = ?1
               AND id IN (SELECT value FROM json_each(?2))",
            params![item, corrections],
        )?;
        now.insert(id.clone(), values(&doc, &fields));
        was.insert(id, current);
        counts.changed += 1;
    }
    tx.execute(
        "INSERT INTO ledger(action_id, ops, pre, post, state, undo, at)
         VALUES (?1, ?2, ?3, ?4, 'writing', 'none', ?5)",
        params![
            action,
            json!({ "fields": fields }).to_string(),
            Value::Object(was).to_string(),
            Value::Object(now).to_string(),
            now_ms
        ],
    )?;
    let undo = tx.last_insert_rowid();
    // The trigger ledger_state moves the action to `applied`, and the first
    // entry's action to `undone`. An undo that changed no item keeps the
    // entry open, so the person can undo it again (decision D18, issue #463).
    tx.execute("UPDATE ledger SET state = 'applied' WHERE id = ?1", [undo])?;
    if counts.changed > 0 {
        tx.execute(
            "UPDATE ledger SET state = 'undone', undone_by = ?2
             WHERE id = ?1 AND state IN ('applied', 'failed')",
            params![entry, undo],
        )?;
    }
    Ok(Outcome::Applied(counts))
}

/// Remove the field at a dotted path, and each parent object that becomes
/// empty: a correction makes the missing parents of its path (`due` of
/// `due.date`). Gives false, and changes nothing, if the field is an element
/// of an array: a removal would move the others.
fn remove(doc: &mut Value, field: &str) -> bool {
    if doc.pointer(&pointer(field)).is_none() {
        return true;
    }
    let mut path = field;
    loop {
        let (parent, last) = match path.rsplit_once('.') {
            Some((p, l)) => (p, l),
            None => ("", path),
        };
        let at = if parent.is_empty() {
            String::new()
        } else {
            pointer(parent)
        };
        match doc.pointer_mut(&at) {
            Some(Value::Object(map)) => {
                map.remove(last);
                if parent.is_empty() || !map.is_empty() {
                    return true;
                }
            }
            _ => return false,
        }
        path = parent;
    }
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
            "skip" | "mute" | "dismiss" => {
                // Specification 1, section 6.8: the form and the statuses of each verb.
                let (want, from): (&str, &[&str]) = match verb {
                    "skip" => ("feed", &["new", "read"]),
                    "mute" => ("conversation", &["active", "quiet"]),
                    _ => ("signal", &["active"]),
                };
                if form != want || !from.contains(&status.as_str()) {
                    return Ok(Err(format!(
                        "{id} is {status}: {verb} changes a {want} that is {}",
                        from.join(" or ")
                    )));
                }
                [("status".to_string(), json!(new_status(verb)))]
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
            let doc = match iris_understand::correct::correct(
                tx,
                &p.id,
                field,
                args["value"].clone(),
                by,
                now_ms,
            )? {
                Ok(doc) => doc,
                Err(why) => return Ok(Err(format!("{}: {why}", p.id))),
            };
            // The correction can change the status too (a record becomes
            // `corrected`, a signal follows `expires`). Spec 1 section 7.5
            // records each status change by an applied action as a correction.
            if doc["status"] != p.pre["status"] {
                tx.execute(
                    "INSERT INTO correction(item_id, field, value, by, at)
                     VALUES (?1, 'status', ?2, ?3, ?4)",
                    params![p.item, doc["status"].to_string(), by, now_ms],
                )?;
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
        "skip" => "skipped",
        "mute" => "muted",
        "dismiss" => "dismissed",
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
