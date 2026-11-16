//! A correction (specification 1, section 7.5, and specification 2,
//! section 4): change one field of an item, and keep the change through
//! each later sync and understanding run.
//!
//! The caller is the step that applies a `correct` action of a changeset
//! (specification 2, section 4), in the transaction of its ledger row.
//! Release 1 has no such caller and no command for it: the command
//! `iris correct` adds a changeset action (specification 2, section 8.2).
//!
//! The `correction` row pins the item. Understanding never deletes a pinned
//! item, never changes its ID, and gives the model the corrected document as
//! an existing item (`store.rs`, `DROPPABLE` and `pinned`).

use rusqlite::{params, OptionalExtension, Transaction};
use serde_json::Value;

use crate::validate;

/// The common fields of section 6.1 of specification 1. A correction cannot
/// change them (specification 2, section 4). `status` changes through the
/// state verbs.
const COMMON: [&str; 12] = [
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
    "attention",
];

/// Set the field `field` (a dotted path, for example `due.date`) of the item
/// with the ID `id` (for example `c-7`) to `value`, in the transaction `tx`.
/// `by` is the changeset ID of the action, or `person` (specification 1,
/// section 5.5).
///
/// Gives `Ok(Err(reason))` and writes nothing when the change is not
/// allowed: no such item, a common field, or a document that is not valid
/// against the schemas. Else writes the `correction` row and the new
/// document, and gives the new document. A record gets the status
/// `corrected` (section 6.8). If the record is gone, `corrected` is its
/// status after it comes back.
pub fn correct(
    tx: &Transaction,
    id: &str,
    field: &str,
    value: Value,
    by: &str,
    now_ms: i64,
) -> rusqlite::Result<Result<Value, String>> {
    let top = field.split('.').next().unwrap_or("");
    if field.split('.').any(str::is_empty) {
        return Ok(Err(format!("the field {field:?} is not a dotted path")));
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
    let row: Option<(String, String)> = tx
        .query_row("SELECT form, doc FROM item WHERE id = ?1", [item], |r| {
            Ok((r.get(0)?, r.get(1)?))
        })
        .optional()?;
    let Some((form, doc)) = row else {
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
    iris_talk::set_field(&mut doc, field, value.clone());
    doc["updated"] = iris_talk::rfc3339(now_ms).into();
    // The status column holds `gone` while the record is gone, and the
    // sync does not change the document (`iris-sync`, section 4.5).
    if form == "record" {
        doc["status"] = "corrected".into();
    }
    if let Err(e) = validate(&doc) {
        return Ok(Err(format!("the corrected item is not valid: {e}")));
    }
    tx.execute(
        "INSERT INTO correction(item_id, field, value, by, at) VALUES (?1, ?2, ?3, ?4, ?5)",
        params![item, field, value.to_string(), by, now_ms],
    )?;
    // A gone record keeps `gone` and comes back as `corrected`.
    tx.execute(
        "UPDATE item SET doc = ?2, updated_at = ?3,
           status = CASE WHEN form = 'record' AND status <> 'gone' THEN 'corrected' ELSE status END,
           status_before_gone = CASE WHEN form = 'record' AND status = 'gone'
             THEN 'corrected' ELSE status_before_gone END
         WHERE id = ?1",
        params![item, doc.to_string(), now_ms],
    )?;
    Ok(Ok(doc))
}
