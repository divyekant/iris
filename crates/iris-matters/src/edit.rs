//! The writes of the person on a confirmed matter, the matter by hand, and
//! the proposal of an agent (`docs/specs/2026-11-24-matters-edit.md`,
//! decisions D19 and D22).
//!
//! Each write is one transaction. The checks here give a clear error; the
//! triggers of migration 46 hold the same rules for each other path.

use std::collections::HashSet;

use iris_store::Store;
use rusqlite::{params, OptionalExtension, Transaction};

use crate::{check_title, insert_by, status, write, Error, Result, MAX_ITEMS};

/// One item of a request: the row ID and the form that the ID names
/// (`c-` is a commitment). A row of another form is not the item.
pub type Ref<'a> = (i64, &'a str);

/// An edit of a confirmed matter: a new title, items to add, and items to
/// remove (a kept member, or a pending addition that the person refuses).
#[derive(Clone, Debug, Default)]
pub struct Edit<'a> {
    pub title: Option<String>,
    pub add: Vec<Ref<'a>>,
    pub remove: Vec<Ref<'a>>,
}

/// The rows of `items`, with a check of the count and of duplicates.
fn rows(items: &[Ref], min: usize, what: &'static str) -> Result<Vec<i64>> {
    let rows: Vec<i64> = items.iter().map(|(r, _)| *r).collect();
    if rows.len() < min || rows.len() > MAX_ITEMS {
        return Err(Error::Bad(what));
    }
    if rows.iter().collect::<HashSet<_>>().len() != rows.len() {
        return Err(Error::Bad("an item is two times in the list"));
    }
    Ok(rows)
}

fn title(t: &str) -> Result<String> {
    check_title(t).ok_or(Error::Bad("the title must be 1 to 80 characters"))
}

/// True when the item `row` of the form `form` is live: a member form, not
/// ended, not a duplicate, not hidden, and no hostile mail.
fn live(tx: &Transaction, (row, form): Ref) -> Result<bool> {
    Ok(tx.query_row(
        concat!(
            "SELECT EXISTS (SELECT 1 FROM item i WHERE i.id = ?1 AND i.form = ?2 AND ",
            crate::live_item!(),
            ")"
        ),
        params![row, form],
        |r| r.get(0),
    )?)
}

/// The open matter (`proposed` or `confirmed`) that has `row` as a kept
/// member, if one has.
fn open_matter(tx: &Transaction, row: i64) -> Result<Option<i64>> {
    Ok(tx
        .query_row(
            "SELECT mi.matter_id FROM matter_item mi JOIN matter m ON m.id = mi.matter_id
             WHERE mi.item_id = ?1 AND mi.kept = 1 AND m.status IN ('proposed', 'confirmed')",
            [row],
            |r| r.get(0),
        )
        .optional()?)
}

/// The person makes a matter by hand, with `title` and 1 to 50 live items
/// that are in no open matter. The matter is confirmed at once. Gives its
/// row ID.
pub fn create(store: &Store, title_text: &str, items: &[Ref], now: i64) -> Result<i64> {
    let t = title(title_text)?;
    let list = rows(items, 1, "give 1 to 50 items")?;
    write(store, |tx| {
        for item in items {
            if !live(tx, *item)? {
                return Err(Error::Conflict("an item is not live now"));
            }
            if open_matter(tx, item.0)?.is_some() {
                return Err(Error::Conflict("an item is in another matter"));
            }
        }
        let id = insert_by(tx, "person", &t, "", "", None, &list, now)?;
        let keep = serde_json::to_string(&list).expect("a list of integers");
        tx.execute(
            "UPDATE matter SET status = 'confirmed', keep = ?2, decided_at = ?3, updated_at = ?3
             WHERE id = ?1 AND status = 'proposed'",
            params![id, keep, now],
        )?;
        Ok(id)
    })
}

/// The number of kept members of `matter`.
fn kept(tx: &Transaction, matter: i64) -> Result<usize> {
    let n: i64 = tx.query_row(
        "SELECT count(*) FROM matter_item WHERE matter_id = ?1 AND kept = 1",
        [matter],
        |r| r.get(0),
    )?;
    Ok(n as usize)
}

/// The person edits the confirmed matter `matter`: one transaction for the
/// title, each add and each remove.
pub fn edit(store: &Store, matter: i64, e: &Edit, now: i64) -> Result<()> {
    let t = e.title.as_deref().map(title).transpose()?;
    let add = rows(&e.add, 0, "add 0 to 50 items")?;
    let remove = rows(&e.remove, 0, "remove 0 to 50 items")?;
    if t.is_none() && add.is_empty() && remove.is_empty() {
        return Err(Error::Bad("the edit is empty"));
    }
    if add.iter().any(|r| remove.contains(r)) {
        return Err(Error::Bad("an item is in add and in remove"));
    }
    write(store, |tx| {
        if status(tx, matter)? != "confirmed" {
            return Err(Error::Conflict("the matter is not confirmed"));
        }
        for item in &e.add {
            if !live(tx, *item)? {
                return Err(Error::Conflict("an item to add is not live now"));
            }
            if open_matter(tx, item.0)?.is_some() {
                return Err(Error::Conflict("an item to add is in a matter"));
            }
        }
        let mut n = kept(tx, matter)?;
        // Adds and removes alternate, so the count stays from 1 to 50 at
        // each step when the count after the edit is from 1 to 50.
        let (mut adds, mut removes) = (e.add.iter(), e.remove.iter());
        let (mut next_add, mut next_remove) = (adds.next(), removes.next());
        loop {
            if let Some(item) = next_add.filter(|_| n < MAX_ITEMS || next_remove.is_none()) {
                add_one(tx, matter, *item, now)?;
                n += 1;
                next_add = adds.next();
            } else if let Some(item) = next_remove {
                n -= remove_one(tx, matter, *item, now)?;
                next_remove = removes.next();
            } else {
                break;
            }
        }
        if let Some(t) = &t {
            tx.execute(
                "UPDATE matter SET title = ?2 WHERE id = ?1 AND status = 'confirmed'",
                params![matter, t],
            )?;
        }
        tx.execute(
            "UPDATE matter SET updated_at = ?2 WHERE id = ?1 AND status = 'confirmed'",
            params![matter, now],
        )?;
        Ok(())
    })
    .map_err(|e| match e {
        // A trigger refused the count (0, or more than 50 kept members).
        Error::Store(iris_store::Error::Db(rusqlite::Error::SqliteFailure(_, Some(m))))
            if m.contains("iris: not a valid") =>
        {
            Error::Conflict("a matter keeps 1 to 50 items")
        }
        e => e,
    })
}

/// Adds the live item `item` as a kept member: a new row, or `kept` from 0
/// to 1 for an item that the person removed or refused before.
fn add_one(tx: &Transaction, matter: i64, (row, _): Ref, _now: i64) -> Result<()> {
    if kept(tx, matter)? >= MAX_ITEMS {
        return Err(Error::Conflict("a matter keeps 1 to 50 items"));
    }
    let n = tx.execute(
        "UPDATE matter_item SET kept = 1 WHERE matter_id = ?1 AND item_id = ?2 AND kept = 0",
        params![matter, row],
    )?;
    if n == 0 {
        tx.execute(
            "INSERT INTO matter_item(matter_id, item_id, kept) VALUES (?1, ?2, 1)",
            params![matter, row],
        )?;
    }
    Ok(())
}

/// Removes `item`: a kept member gets `kept` = 0; a pending addition
/// becomes a refusal row. Gives the change of the kept count (1 or 0).
fn remove_one(tx: &Transaction, matter: i64, (row, form): Ref, _now: i64) -> Result<usize> {
    let of_form: bool = tx.query_row(
        "SELECT EXISTS (SELECT 1 FROM item WHERE id = ?1 AND form = ?2)",
        params![row, form],
        |r| r.get(0),
    )?;
    if of_form {
        if kept(tx, matter)? <= 1
            && tx
                .query_row(
                    "SELECT kept FROM matter_item WHERE matter_id = ?1 AND item_id = ?2",
                    params![matter, row],
                    |r| r.get::<_, i64>(0),
                )
                .optional()?
                == Some(1)
        {
            return Err(Error::Conflict("a matter keeps 1 to 50 items"));
        }
        let n = tx.execute(
            "UPDATE matter_item SET kept = 0 WHERE matter_id = ?1 AND item_id = ?2 AND kept = 1",
            params![matter, row],
        )?;
        if n == 1 {
            return Ok(1);
        }
        let pending: bool = tx.query_row(
            "SELECT EXISTS (SELECT 1 FROM matter_add WHERE matter_id = ?1 AND item_id = ?2)",
            params![matter, row],
            |r| r.get(0),
        )?;
        if pending {
            // The trigger `matter_item_kept_add` deletes the addition.
            tx.execute(
                "INSERT INTO matter_item(matter_id, item_id, kept) VALUES (?1, ?2, 0)",
                params![matter, row],
            )?;
            return Ok(0);
        }
    }
    Err(Error::Conflict(
        "an item to remove is not a member or an addition",
    ))
}

/// The person deletes the confirmed matter `matter`. With 2 or more member
/// rows the store keeps it as `declined`: its members stay pinned, and the
/// proposal step does not propose the same grouping again. With 1 member
/// row the row goes: one item is no grouping to remember.
pub fn delete(store: &Store, matter: i64, now: i64) -> Result<()> {
    write(store, |tx| {
        if status(tx, matter)? != "confirmed" {
            return Err(Error::Conflict("the matter is not confirmed"));
        }
        let rows: i64 = tx.query_row(
            "SELECT count(*) FROM matter_item WHERE matter_id = ?1",
            [matter],
            |r| r.get(0),
        )?;
        if rows < 2 {
            tx.execute("DELETE FROM matter WHERE id = ?1", [matter])?;
            return Ok(());
        }
        tx.execute(
            "UPDATE matter SET status = 'declined', keep = NULL, decided_at = ?2, updated_at = ?2
             WHERE id = ?1 AND status = 'confirmed'",
            params![matter, now],
        )?;
        Ok(())
    })
}

/// An agent proposes a matter (D22), in the transaction of the caller: 2
/// to 50 live, free items from 2 or more threads, not a subset of a
/// declined matter. The proposal waits for the person. Gives its row ID.
/// A refusal is `Bad` and writes nothing.
pub fn propose_by_agent(
    tx: &Transaction,
    title_text: &str,
    items: &[Ref],
    now: i64,
) -> Result<i64> {
    let t = title(title_text)?;
    let list = rows(items, 2, "give 2 to 50 items")?;
    let mut threads = HashSet::new();
    for (row, form) in items {
        let thread: Option<i64> = tx
            .query_row(
                concat!(
                    "SELECT msg.thread_id FROM item i
                     JOIN item_source s ON s.item_id = i.id AND s.role = 'origin'
                     JOIN message msg ON msg.id = s.message_id
                     WHERE i.id = ?1 AND i.form = ?2 AND ",
                    crate::live_item!(),
                    " AND ",
                    crate::free_item!()
                ),
                params![row, form],
                |r| r.get(0),
            )
            .optional()?;
        let Some(thread) = thread else {
            return Err(Error::Bad(
                "an item is not live, or is in a matter or an addition",
            ));
        };
        threads.insert(thread);
    }
    if threads.len() < 2 {
        return Err(Error::Bad("the items must come from 2 or more threads"));
    }
    if crate::propose::declined_subset(tx, &list)? {
        return Err(Error::Bad("the person declined these items as a matter"));
    }
    insert_by(tx, "agent", &t, "", "", None, &list, now)
}
