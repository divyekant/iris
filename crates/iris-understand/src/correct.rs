//! A correction (specification 1, section 7.5, and specification 2,
//! section 4): change one field of an item, and keep the change through
//! each later sync and understanding run.
//!
//! The caller is the step that applies a `correct` action of a changeset
//! (specification 2, section 4), in the transaction of its ledger row
//! (`iris_apply::apply_state`). The command `iris correct` adds a changeset
//! action (specification 2, section 8.2).
//!
//! The `correction` row pins the item. Understanding never deletes a pinned
//! item, never changes its ID, and gives the model the corrected document as
//! an existing item (`store.rs`, `DROPPABLE` and `pinned`).

use rusqlite::{params, Connection, OptionalExtension, Transaction};
use serde_json::Value;

use crate::validate;

/// The common fields of section 6.1 of specification 1. A correction cannot
/// change them (specification 2, section 4). `status` changes through the
/// state verbs.
const COMMON: [&str; 13] = [
    "model",
    "id",
    "form",
    "status",
    "title",
    "sources",
    "created",
    "updated",
    "derived",
    "confidence",
    "matter",
    "ready",
    "attention",
];

/// Set the field `field` (a dotted path, for example `due.date`) of the item
/// with the ID `id` (for example `c-7`) to `value`, in the transaction `tx`.
/// `by` is the changeset ID of the action, or `person` (specification 1,
/// section 5.5).
///
/// Gives `Ok(Err(reason))` and writes nothing when the change is not
/// allowed: no such item, a common field, a field that the conversation
/// rule makes (`iris_talk::DERIVED`), a path that does
/// not fit the document (`iris_talk::set_field`), a path to the `sender` ID
/// of a party, or a document that is not valid against the schemas. Else
/// writes the `correction` row and the new document, with the sender ID of
/// each party from its address (a new address gets a sender row), and gives
/// the new document. The status follows the new document (section 6.8): a
/// record is `corrected`, and an active or expired signal follows its
/// `expires`. A gone item keeps `gone` and gets that status when it comes
/// back.
pub fn correct(
    tx: &Transaction,
    id: &str,
    field: &str,
    value: Value,
    by: &str,
    now_ms: i64,
) -> rusqlite::Result<Result<Value, String>> {
    let (item, mut doc, status) = match corrected(tx, id, field, value.clone(), now_ms)? {
        Ok(c) => c,
        Err(why) => return Ok(Err(why)),
    };
    let mut failed = None;
    crate::set_senders(&mut doc, &mut |addr, name| {
        crate::store::sender_row(tx, addr, name).unwrap_or_else(|e| {
            failed.get_or_insert(e);
            0
        })
    });
    if let Some(e) = failed {
        return Err(e);
    }
    tx.execute(
        "INSERT INTO correction(item_id, field, value, by, at) VALUES (?1, ?2, ?3, ?4, ?5)",
        params![item, field, value.to_string(), by, now_ms],
    )?;
    // A gone item keeps `gone`, and comes back with the new status.
    tx.execute(
        "UPDATE item SET doc = ?2, updated_at = ?3,
           status = CASE WHEN ?4 IS NULL OR status = 'gone' THEN status ELSE ?4 END,
           status_before_gone = CASE WHEN ?4 IS NOT NULL AND status = 'gone'
             THEN ?4 ELSE status_before_gone END
         WHERE id = ?1",
        params![item, doc.to_string(), now_ms, status],
    )?;
    Ok(Ok(doc))
}

/// Gives the reason why [`correct`] refuses the change, or `Ok(())`, and
/// writes nothing. The submit of a `correct` action (specification 2,
/// section 4) uses it, so the submit and the apply use one rule.
pub fn check(
    c: &Connection,
    id: &str,
    field: &str,
    value: Value,
    now_ms: i64,
) -> rusqlite::Result<Result<(), String>> {
    Ok(corrected(c, id, field, value, now_ms)?.map(|_| ()))
}

/// The checks of [`correct`]: the item row, the corrected document (with
/// the sender IDs still to set) and the new status, or the reason of the
/// refusal.
#[allow(clippy::type_complexity)]
fn corrected(
    tx: &Connection,
    id: &str,
    field: &str,
    value: Value,
    now_ms: i64,
) -> rusqlite::Result<Result<(i64, Value, Option<&'static str>), String>> {
    let top = field.split('.').next().unwrap_or("");
    if field.split('.').any(|s| s == "sender") {
        return Ok(Err(
            "the engine sets the sender ID of a party from its address".into(),
        ));
    }
    if COMMON.contains(&top) {
        return Ok(Err(format!(
            "the field {top} is a common field, and a correction cannot change it"
        )));
    }
    let no_item = || Ok(Err(format!("no item has the ID {id:?}")));
    let Some((_, item)) = iris_store::parse_id(id) else {
        return no_item();
    };
    let row: Option<(String, String, String)> = tx
        .query_row(
            "SELECT i.form, i.doc, a.address FROM item i JOIN account a ON a.id = i.account_id
             WHERE i.id = ?1",
            [item],
            |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?)),
        )
        .optional()?;
    let Some((form, doc, me)) = row else {
        return no_item();
    };
    let Ok(mut doc) = serde_json::from_str::<Value>(&doc) else {
        return Ok(Err("the saved document is not JSON".into()));
    };
    // The document holds its ID, so a prefix of another form (or an ID
    // that is not an item, such as `m-7`) finds no item.
    if doc["id"].as_str() != Some(id) {
        return no_item();
    }
    // The conversation rule makes these fields of a conversation from the
    // store (`iris-talk`), for example its thread.
    if form == "conversation" && iris_talk::DERIVED.contains(&top) {
        return Ok(Err(format!(
            "the conversation rule makes the field {top} of a conversation"
        )));
    }
    if !iris_talk::set_field(&mut doc, field, value.clone()) {
        return Ok(Err(format!("the field {field:?} does not fit the item")));
    }
    // The party rule of a model answer (section 7.4): the owner address
    // makes a `self` party with no other field, and a `self` party with the
    // address of another person is not valid.
    if crate::prompt::parties(&mut doc, &[&me]).is_none() {
        return Ok(Err(
            "a party is not valid: a self party with the address of another person, or a party with no address".into(),
        ));
    }
    doc["updated"] = iris_talk::rfc3339(now_ms).into();
    // The status that follows from the corrected document (section 6.8).
    // The status column holds `gone` while the item is gone, and the sync
    // does not change the document (`iris-sync`, section 4.5).
    let status = match (form.as_str(), doc["status"].as_str()) {
        ("record", _) => Some("corrected"),
        ("signal", Some("active" | "expired")) => {
            let expires = doc["expires"].as_str().and_then(crate::time::parse_rfc3339);
            Some(crate::signal_status(expires, now_ms))
        }
        _ => None,
    };
    if let Some(s) = status {
        doc["status"] = s.into();
    }
    // The engine links each sender party to the sender row of its address,
    // as for a new item, so an ID and an address cannot disagree. First the
    // check with a placeholder ID, so a refused correction writes no row.
    let mut check = doc.clone();
    crate::set_senders(&mut check, &mut |_, _| 1);
    if let Err(e) = validate(&check) {
        return Ok(Err(format!("the corrected item is not valid: {e}")));
    }
    Ok(Ok((item, doc, status)))
}
