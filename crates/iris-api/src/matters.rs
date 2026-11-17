//! Matters in the local API (`docs/specs/2026-11-16-matters.md`, "Person
//! decisions"): the list, one matter with its members, and the confirm and
//! the decline of the person. The rules of the store (migration 16) and of
//! `iris_matters` hold each decision; this module maps IDs and errors.

use iris_matters::Error as MatterError;
use iris_store::{format_id, parse_id, Kind, Store};
use rusqlite::{params, Connection, OptionalExtension};
use serde_json::{json, Map, Value};

use crate::read::{doc_of, rfc3339, row_of, MATTER};
use crate::{Error, Result};

/// The forms of a member, with the ID kind of each.
const MEMBER_FORMS: [(&str, Kind); 3] = [
    ("commitment", Kind::Commitment),
    ("conversation", Kind::Conversation),
    ("record", Kind::Record),
];

/// The members of a matter that its document and its view show: the live
/// members of a proposal, the kept members of a confirmed matter, and each
/// member of a declined matter (the full grouping that the person declined).
const SHOWN: &str = "SELECT mi.item_id FROM matter_item mi JOIN matter m ON m.id = mi.matter_id
     WHERE mi.matter_id = ?1 AND CASE m.status
       WHEN 'proposed' THEN mi.item_id IN (SELECT item_id FROM matter_live WHERE matter_id = ?1)
       WHEN 'confirmed' THEN mi.kept = 1
       ELSE 1 END";

/// The valid proposals (the view `matter_valid`), newest first, and the
/// confirmed matters, by the last change, newest first.
pub fn matters(store: &Store) -> Result<Value> {
    Ok(store.read(|c| {
        let ids = |sql: &str| -> rusqlite::Result<Vec<i64>> {
            let mut st = c.prepare(sql)?;
            let v = st.query_map([], |r| r.get(0))?.collect();
            v
        };
        let proposed = ids(
            "SELECT m.id FROM matter m JOIN matter_valid v ON v.matter_id = m.id
             ORDER BY m.created_at DESC, m.id DESC",
        )?;
        let confirmed = ids(
            "SELECT id FROM matter WHERE status = 'confirmed' ORDER BY updated_at DESC, id DESC",
        )?;
        let docs = |list: Vec<i64>| -> rusqlite::Result<Vec<Value>> {
            list.into_iter()
                .filter_map(|m| matter_doc(c, m).transpose())
                .collect()
        };
        Ok(json!({ "proposed": docs(proposed)?, "confirmed": docs(confirmed)? }))
    })?)
}

/// One matter (`g-4`) with the documents of its members, by form. `None`
/// when no such matter exists, or for a proposal that is not valid now.
pub fn matter(store: &Store, id: &str) -> Result<Option<Value>> {
    let row = row_of(id, Kind::Matter)?;
    Ok(store.read(|c| Ok(view(c, row)?))?)
}

/// The person confirms the proposal `id` with a title and the item IDs to
/// keep. The answer is the matter after the change; `None` when no
/// such matter exists.
pub fn confirm_matter(
    store: &Store,
    id: &str,
    title: &str,
    items: &[String],
    now_ms: i64,
) -> Result<Option<Value>> {
    let row = row_of(id, Kind::Matter)?;
    let mut keep = Vec::with_capacity(items.len());
    for item in items {
        keep.push(member_row(item)?);
    }
    // The kind of each ID must agree with the form of its row.
    let forms_agree = store.read(|c| {
        for (item, row) in items.iter().zip(&keep) {
            let form: Option<String> = c
                .query_row("SELECT form FROM item WHERE id = ?1", [row], |r| r.get(0))
                .optional()?;
            if form.as_deref() != Some(form_of(item)) {
                return Ok(false);
            }
        }
        Ok(true)
    })?;
    if !forms_agree {
        return Err(Error::Query("an item to keep is not a member".into()));
    }
    decided(
        store,
        row,
        iris_matters::confirm(store, row, title, &keep, now_ms),
    )
}

/// The person declines the proposal `id`. The answer is the matter after
/// the change; `None` when no such matter exists.
pub fn decline_matter(store: &Store, id: &str, now_ms: i64) -> Result<Option<Value>> {
    let row = row_of(id, Kind::Matter)?;
    decided(store, row, iris_matters::decline(store, row, now_ms))
}

fn decided(
    store: &Store,
    row: i64,
    r: std::result::Result<(), MatterError>,
) -> Result<Option<Value>> {
    match r {
        Ok(()) => Ok(store.read(|c| Ok(view(c, row)?))?),
        Err(MatterError::NotFound) => Ok(None),
        Err(MatterError::Conflict(why)) => Err(Error::Conflict(why.into())),
        Err(MatterError::Bad(why)) => Err(Error::Query(why.into())),
        Err(MatterError::Store(e)) => Err(Error::Store(e)),
        Err(e) => Err(Error::Conflict(e.to_string())),
    }
}

/// The row of a member ID (`c-`, `v-` or `r-`).
fn member_row(id: &str) -> Result<i64> {
    match parse_id(id) {
        Some((k, row)) if MEMBER_FORMS.iter().any(|(_, m)| *m == k) => Ok(row),
        _ => Err(Error::Query(format!("not a valid ID: {id}"))),
    }
}

fn form_of(id: &str) -> &'static str {
    let kind = parse_id(id).map(|(k, _)| k);
    MEMBER_FORMS
        .iter()
        .find(|(_, k)| Some(*k) == kind)
        .map_or("", |(f, _)| *f)
}

/// The matter document with its member documents, or `None`.
fn view(c: &Connection, row: i64) -> rusqlite::Result<Option<Value>> {
    let Some(doc) = matter_doc(c, row)? else {
        return Ok(None);
    };
    let mut st = c.prepare(&format!(
        "SELECT id, form, status, doc, {MATTER} FROM item
         WHERE id IN ({SHOWN}) ORDER BY id"
    ))?;
    let mut by_form = Map::new();
    for (form, _) in MEMBER_FORMS {
        by_form.insert(form.into(), Value::Array(Vec::new()));
    }
    for d in st.query_map([row], |r| Ok((r.get::<_, String>(1)?, doc_of(r)?)))? {
        let (form, d) = d?;
        if let Some(Value::Array(list)) = by_form.get_mut(&form) {
            list.push(d);
        }
    }
    Ok(Some(json!({ "matter": doc, "items": by_form })))
}

/// The open-model document of a matter (`matter.schema.json`), or `None`
/// for no such row or for a proposal that is not valid now.
fn matter_doc(c: &Connection, row: i64) -> rusqlite::Result<Option<Value>> {
    let head = c
        .query_row(
            "SELECT title, status, model, prompt, created_at, updated_at, decided_at,
                    (status <> 'proposed' OR id IN (SELECT matter_id FROM matter_valid))
               FROM matter WHERE id = ?1",
            [row],
            |r| {
                Ok((
                    r.get::<_, String>(0)?,
                    r.get::<_, String>(1)?,
                    r.get::<_, String>(2)?,
                    r.get::<_, String>(3)?,
                    r.get::<_, i64>(4)?,
                    r.get::<_, i64>(5)?,
                    r.get::<_, Option<i64>>(6)?,
                    r.get::<_, bool>(7)?,
                ))
            },
        )
        .optional()?;
    let Some((title, status, model, prompt, created, updated, decided, shown)) = head else {
        return Ok(None);
    };
    if !shown {
        return Ok(None);
    }
    let mut st = c.prepare(&format!(
        "SELECT i.id, i.form FROM item i WHERE i.id IN ({SHOWN}) ORDER BY i.id"
    ))?;
    let mut items = Vec::new();
    let mut counts = Map::new();
    for (form, _) in MEMBER_FORMS {
        counts.insert(form.into(), 0.into());
    }
    for r in st.query_map(params![row], |r| {
        Ok((r.get::<_, i64>(0)?, r.get::<_, String>(1)?))
    })? {
        let (id, form) = r?;
        if let Some((f, kind)) = MEMBER_FORMS.iter().find(|(f, _)| *f == form) {
            items.push(Value::String(format_id(*kind, id)));
            let n = counts[*f].as_i64().unwrap_or(0) + 1;
            counts.insert((*f).into(), n.into());
        }
    }
    let mut doc = json!({
        "model": "iris/0",
        "id": format_id(Kind::Matter, row),
        "title": title,
        "status": status,
        "items": items,
        "counts": counts,
        "created": rfc3339(created),
        "updated": rfc3339(updated),
        "derived": { "by": "model", "model": model, "prompt": prompt },
    });
    if let Some(d) = decided {
        doc["decided"] = rfc3339(d).into();
    }
    Ok(Some(doc))
}
