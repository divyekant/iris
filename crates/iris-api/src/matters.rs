//! Matters in the local API (`docs/specs/2026-11-16-matters.md`, "Person
//! decisions"): the list, one matter with its members, and the confirm and
//! the decline of the person. The rules of the store (migration 16) and of
//! `iris_matters` hold each decision; this module maps IDs and errors.

use iris_matters::Error as MatterError;
use iris_store::{format_id, parse_id, Kind, Store};
use rusqlite::Connection;
use serde_json::{json, Map, Value};

use crate::read::{doc_at, rfc3339, row_of, MATTER};
use crate::{Error, Result};

/// The forms of a member, with the ID kind of each.
const MEMBER_FORMS: [(&str, Kind); 3] = [
    ("commitment", Kind::Commitment),
    ("conversation", Kind::Conversation),
    ("record", Kind::Record),
];

/// One statement reads each matter of a response with its members, so the
/// document and the member list come from one snapshot: a decision that
/// commits during the read cannot mix the two. Each matter row comes with
/// one row for each member that it shows (or one row of NULLs): the live
/// members of a proposal, the kept members of a confirmed matter, and each
/// member of a declined matter (the full grouping that the person
/// declined). A proposal that is not valid now is not read. A member that
/// comes from hostile mail is not read (issue 112), and [`load`] leaves out
/// a matter with too few members to show.
fn select(filter: &str, order: &str) -> String {
    format!(
        "SELECT m.id, m.title, m.status, m.model, m.prompt, m.created_at, m.updated_at,
                m.decided_at, item.id, item.form, item.status, item.doc, {MATTER}
           FROM matter m
           LEFT JOIN matter_item mi ON mi.matter_id = m.id AND CASE m.status
                WHEN 'proposed' THEN mi.item_id IN
                  (SELECT l.item_id FROM matter_live l WHERE l.matter_id = m.id)
                WHEN 'confirmed' THEN mi.kept = 1
                ELSE 1 END
           LEFT JOIN item ON item.id = mi.item_id AND NOT {from_hostile}
          WHERE (m.status <> 'proposed' OR m.id IN (SELECT matter_id FROM matter_valid))
            AND ({filter})
          ORDER BY {order}, m.id DESC, item.id",
        from_hostile = crate::read::FROM_HOSTILE
    )
}

/// The matters of one statement of [`select`], in its order: each matter
/// document with the documents of its members by form (when `members`).
fn load(
    c: &Connection,
    sql: &str,
    args: impl rusqlite::Params,
    members: bool,
) -> rusqlite::Result<Vec<(Value, Map<String, Value>)>> {
    let mut st = c.prepare(sql)?;
    let mut rows = st.query(args)?;
    let mut out: Vec<(Value, Map<String, Value>)> = Vec::new();
    let mut last = None;
    while let Some(r) = rows.next()? {
        let id: i64 = r.get(0)?;
        if last != Some(id) {
            last = Some(id);
            let mut doc = json!({
                "model": iris_store::MODEL,
                "id": format_id(Kind::Matter, id),
                "title": r.get::<_, String>(1)?,
                "status": r.get::<_, String>(2)?,
                "items": [],
                "counts": {"commitment": 0, "conversation": 0, "record": 0},
                "created": rfc3339(r.get(5)?),
                "updated": rfc3339(r.get(6)?),
                "derived": {"by": "model", "model": r.get::<_, String>(3)?,
                            "prompt": r.get::<_, String>(4)?},
            });
            if let Some(d) = r.get::<_, Option<i64>>(7)? {
                doc["decided"] = rfc3339(d).into();
            }
            let by_form = MEMBER_FORMS
                .iter()
                .map(|(f, _)| ((*f).to_string(), Value::Array(Vec::new())))
                .collect();
            out.push((doc, by_form));
        }
        let Some(item) = r.get::<_, Option<i64>>(8)? else {
            continue;
        };
        let form: String = r.get(9)?;
        let Some((f, kind)) = MEMBER_FORMS.iter().find(|(f, _)| *f == form) else {
            continue;
        };
        let (doc, by_form) = out.last_mut().expect("a matter row comes first");
        if let Some(Value::Array(list)) = doc.get_mut("items") {
            list.push(format_id(*kind, item).into());
        }
        let n = doc["counts"][*f].as_i64().unwrap_or(0) + 1;
        doc["counts"][*f] = n.into();
        if members {
            if let Some(Value::Array(list)) = by_form.get_mut(*f) {
                list.push(doc_at(r, 8)?);
            }
        }
    }
    // A matter with fewer members to show than its schema needs (one for a
    // confirmed matter, two for another) is not read, like a proposal that
    // is not valid. Only hidden members of hostile mail can cause it, and
    // the matter shows again when the state clears (issue 112).
    out.retain(|(doc, _)| {
        let min = if doc["status"] == "confirmed" { 1 } else { 2 };
        doc["items"].as_array().map_or(0, Vec::len) >= min
    });
    Ok(out)
}

/// The valid proposals (the view `matter_valid`), newest first, and the
/// confirmed matters, by the last change, newest first. One statement.
pub fn matters(store: &Store) -> Result<Value> {
    let sql = select(
        "m.status IN ('proposed', 'confirmed')",
        "m.status = 'confirmed',
         CASE m.status WHEN 'proposed' THEN m.created_at ELSE m.updated_at END DESC",
    );
    let all = store.read(|c| Ok(load(c, &sql, [], false)?))?;
    let (mut proposed, mut confirmed) = (Vec::new(), Vec::new());
    for (doc, _) in all {
        if doc["status"] == "proposed" {
            proposed.push(doc);
        } else {
            confirmed.push(doc);
        }
    }
    Ok(json!({ "proposed": proposed, "confirmed": confirmed }))
}

/// One matter (`g-4`) with the documents of its members, by form. `None`
/// when no such matter exists, for a proposal that is not valid now, or
/// for a matter with too few members that are not hostile mail.
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
        keep.push(member(item)?);
    }
    // The form of each ID is checked in the decision transaction, after the
    // check of the matter: a matter that is gone answers 404 first.
    decided(
        store,
        row,
        iris_matters::confirm_forms(store, row, title, &keep, now_ms),
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

/// The row and the form of a member ID (`c-`, `v-` or `r-`).
fn member(id: &str) -> Result<(i64, &'static str)> {
    parse_id(id)
        .and_then(|(k, row)| {
            MEMBER_FORMS
                .iter()
                .find(|(_, m)| *m == k)
                .map(|(f, _)| (row, *f))
        })
        .ok_or_else(|| Error::Query(format!("not a valid ID: {id}")))
}

/// The matter document with its member documents, or `None`. One
/// statement.
fn view(c: &Connection, row: i64) -> rusqlite::Result<Option<Value>> {
    let sql = select("m.id = ?1", "m.id");
    Ok(load(c, &sql, [row], true)?
        .pop()
        .map(|(doc, items)| json!({ "matter": doc, "items": items })))
}
