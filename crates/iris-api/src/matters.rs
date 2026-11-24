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

/// The forms of a member, with the ID kind of each (signals since
/// `docs/specs/2026-11-24-matters-edit.md`).
const MEMBER_FORMS: [(&str, Kind); 4] = [
    ("commitment", Kind::Commitment),
    ("conversation", Kind::Conversation),
    ("record", Kind::Record),
    ("signal", Kind::Signal),
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
                m.decided_at, item.id, item.form, item.status, item.doc, {MATTER}, m.author
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
            let author: String = r.get(14)?;
            let derived = if author == "model" {
                json!({"by": "model", "model": r.get::<_, String>(3)?,
                       "prompt": r.get::<_, String>(4)?})
            } else {
                json!({ "by": author })
            };
            let mut doc = json!({
                "model": iris_store::MODEL,
                "id": format_id(Kind::Matter, id),
                "title": r.get::<_, String>(1)?,
                "status": r.get::<_, String>(2)?,
                "items": [],
                "counts": {"commitment": 0, "conversation": 0, "record": 0, "signal": 0},
                "created": rfc3339(r.get(5)?),
                "updated": rfc3339(r.get(6)?),
                "derived": derived,
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
    let all = store.read(|c| {
        let mut all = load(c, &sql, [], false)?;
        for (doc, _) in &mut all {
            additions(c, doc, None)?;
        }
        Ok(all)
    })?;
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
        Err(e) => Err(error(e)),
    }
}

fn error(e: MatterError) -> Error {
    match e {
        MatterError::Conflict(why) => Error::Conflict(why.into()),
        MatterError::Bad(why) => Error::Query(why.into()),
        MatterError::Store(e) => Error::Store(e),
        MatterError::NotFound => Error::Conflict("no such matter".into()),
        e => Error::Conflict(e.to_string()),
    }
}

/// The row and the form of a member ID (`c-`, `v-`, `r-` or `s-`).
fn member(id: &str) -> Result<(i64, &'static str)> {
    iris_matters::parse_ref(id).ok_or_else(|| Error::Query(format!("not a valid ID: {id}")))
}

/// The matter document with its member documents, or `None`. One
/// statement.
fn view(c: &Connection, row: i64) -> rusqlite::Result<Option<Value>> {
    let sql = select("m.id = ?1", "m.id");
    let Some((mut doc, items)) = load(c, &sql, [row], true)?.pop() else {
        return Ok(None);
    };
    let mut docs = Vec::new();
    additions(c, &mut doc, Some(&mut docs))?;
    Ok(Some(
        json!({ "matter": doc, "items": items, "additions": docs }),
    ))
}

/// Sets the field `additions` of a confirmed matter document: the item IDs
/// of its live additions (the view `matter_add_live`), newest first, with
/// no item of hostile mail. With `docs`, also the item documents.
fn additions(
    c: &Connection,
    doc: &mut Value,
    mut docs: Option<&mut Vec<Value>>,
) -> rusqlite::Result<()> {
    if doc["status"] != "confirmed" {
        return Ok(());
    }
    let Some((_, row)) = doc["id"].as_str().and_then(parse_id) else {
        return Ok(());
    };
    let mut st = c.prepare_cached(&format!(
        "SELECT item.id, item.form, item.status, item.doc, {MATTER}
           FROM matter_add_live a JOIN item ON item.id = a.item_id
          WHERE a.matter_id = ?1 AND NOT {from_hostile}
          ORDER BY a.created_at DESC, item.id DESC LIMIT 50",
        from_hostile = crate::read::FROM_HOSTILE
    ))?;
    let mut rows = st.query([row])?;
    let mut ids = Vec::new();
    while let Some(r) = rows.next()? {
        let form: String = r.get(1)?;
        let Some((_, kind)) = MEMBER_FORMS.iter().find(|(f, _)| *f == form) else {
            continue;
        };
        ids.push(Value::from(format_id(*kind, r.get(0)?)));
        if let Some(d) = docs.as_deref_mut() {
            d.push(doc_at(r, 0)?);
        }
    }
    if !ids.is_empty() {
        doc["additions"] = ids.into();
    }
    Ok(())
}

/// True when the item `id` can join a matter now (`iris_matters::joinable`):
/// the web client shows its matter picker only then.
pub fn joinable(store: &Store, id: &str) -> Result<bool> {
    iris_matters::joinable(store, member(id)?).map_err(error)
}

/// The person makes a matter by hand with a title and 1 to 50 item IDs.
/// The answer is the new matter view.
pub fn create_matter(store: &Store, title: &str, items: &[String], now_ms: i64) -> Result<Value> {
    let items = refs(items)?;
    let row = iris_matters::create(store, title, &items, now_ms).map_err(error)?;
    store
        .read(|c| Ok(view(c, row)?))?
        .ok_or_else(|| Error::Conflict("the new matter is not visible".into()))
}

/// The person edits the confirmed matter `id`: a new title, items to add,
/// items to remove. The answer is the matter view; `None` when no such
/// matter exists.
pub fn edit_matter(
    store: &Store,
    id: &str,
    title: Option<String>,
    add: &[String],
    remove: &[String],
    now_ms: i64,
) -> Result<Option<Value>> {
    let row = row_of(id, Kind::Matter)?;
    let e = iris_matters::Edit {
        title,
        add: refs(add)?,
        remove: refs(remove)?,
    };
    decided(store, row, iris_matters::edit(store, row, &e, now_ms))
}

/// The person deletes the confirmed matter `id`. `false` when no such
/// matter exists.
pub fn delete_matter(store: &Store, id: &str, now_ms: i64) -> Result<bool> {
    let row = row_of(id, Kind::Matter)?;
    match iris_matters::delete(store, row, now_ms) {
        Ok(()) => Ok(true),
        Err(MatterError::NotFound) => Ok(false),
        Err(e) => Err(error(e)),
    }
}

/// The row and the form of each member ID.
pub(crate) fn refs(items: &[String]) -> Result<Vec<(i64, &'static str)>> {
    items.iter().map(|i| member(i)).collect()
}
