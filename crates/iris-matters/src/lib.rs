//! Matters (card R6-01, `docs/specs/2026-11-16-matters.md`): a title and the
//! items that belong together. Iris proposes a matter; the person confirms
//! or declines it.
//!
//! The store holds the rules of the spec as triggers (migration 16), and
//! the one validity rule of a proposal as the view `matter_valid`. This
//! crate writes through these rules: it never decides validity in Rust.
//!
//! The crate has the writes of the person ([`confirm`], [`decline`]), and
//! the proposal step ([`propose`]) with its write ([`insert_proposal`]) and
//! its clean-up ([`clean`]).

use std::fmt;

/// The SQL condition "item `i` is live": a member form, a status that is
/// not ended, no duplicate, not hidden, and no hostile origin mail. The
/// same rule as the views `matter_live` and `matter_add_live` (migration
/// 46), for one item.
#[macro_export]
#[doc(hidden)]
macro_rules! live_item {
    () => {
        concat!(
            "NOT ",
            iris_store::from_hostile_mail!("i"),
            " AND i.form IN ('commitment', 'conversation', 'record', 'signal')
    AND i.status NOT IN ('gone', 'cancelled', 'declined')
    AND (i.form <> 'signal' OR i.status = 'active')
    AND i.duplicate_of IS NULL
    AND coalesce(json_extract(i.doc, '$.attention.state'), 'shown') <> 'hidden'"
        )
    };
}

/// The SQL condition "item `i` is free": no kept member of an open matter,
/// and no pending addition.
#[macro_export]
#[doc(hidden)]
macro_rules! free_item {
    () => {
        "NOT EXISTS (
      SELECT 1 FROM matter_item mi JOIN matter m ON m.id = mi.matter_id
      WHERE mi.item_id = i.id AND mi.kept = 1 AND m.status IN ('proposed', 'confirmed'))
    AND NOT EXISTS (SELECT 1 FROM matter_add ma WHERE ma.item_id = i.id)"
    };
}

pub mod edit;
pub mod propose;
pub use edit::{create, delete, edit, parse_ref, propose_by_agent, Edit};
pub use propose::{propose, Gate, Outcome, Skip};

use iris_store::Store;
use rusqlite::{params, OptionalExtension, Transaction};

/// The prompt version of the proposal step.
pub const PROMPT: &str = "matters/0.2";

/// The longest title, in characters (the schema `matter.schema.json`).
pub const MAX_TITLE: usize = 80;

/// The most members of one matter (the schema).
pub const MAX_ITEMS: usize = 50;

#[derive(Debug)]
pub enum Error {
    Store(iris_store::Error),
    /// No matter has this ID.
    NotFound,
    /// The matter is not a valid proposal now: it is decided, it lost
    /// members, or an item to keep is not live (HTTP 409).
    Conflict(&'static str),
    /// The request is not valid (HTTP 400).
    Bad(&'static str),
    /// Understanding, or another proposal step, holds the run lock.
    Busy,
    /// The program could not make the model from the model row, or
    /// another part of the program failed.
    Model(String),
    /// The caller may not use an item (a job session and its grant).
    Denied(String),
}

impl fmt::Display for Error {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Error::Store(e) => write!(f, "store: {e}"),
            Error::NotFound => f.write_str("no such matter"),
            Error::Conflict(why) | Error::Bad(why) => f.write_str(why),
            Error::Busy => f.write_str("a run is in progress"),
            Error::Model(e) => write!(f, "model: {e}"),
            Error::Denied(why) => f.write_str(why),
        }
    }
}

impl std::error::Error for Error {}

impl From<iris_store::Error> for Error {
    fn from(e: iris_store::Error) -> Self {
        Error::Store(e)
    }
}

impl From<rusqlite::Error> for Error {
    fn from(e: rusqlite::Error) -> Self {
        Error::Store(e.into())
    }
}

pub type Result<T> = std::result::Result<T, Error>;

/// Runs `f` in one write transaction. An error of `f` rolls the
/// transaction back and comes back as it is.
pub fn write<T>(store: &Store, f: impl FnOnce(&Transaction) -> Result<T>) -> Result<T> {
    let mut failed = None;
    let r = store.write(|tx| {
        f(tx).map_err(|e| {
            failed = Some(e);
            // Any store error rolls back; the real error is in `failed`.
            iris_store::Error::Db(rusqlite::Error::QueryReturnedNoRows)
        })
    });
    match (r, failed) {
        (Ok(v), _) => Ok(v),
        (Err(_), Some(e)) => Err(e),
        (Err(e), None) => Err(Error::Store(e)),
    }
}

/// The title after trim, if it is 1 to [`MAX_TITLE`] characters with no
/// control character. The model and the person give titles; both go
/// through this one check.
pub fn check_title(title: &str) -> Option<String> {
    let t = title.trim();
    let n = t.chars().count();
    ((1..=MAX_TITLE).contains(&n) && !t.chars().any(char::is_control)).then(|| t.to_string())
}

/// Writes a proposal with its members, in the transaction of the caller.
/// The triggers refuse a member that is not a commitment, a conversation or
/// a record, or that is a kept member of another open matter. The caller
/// checks the title and the rules of the answer first.
pub fn insert_proposal(
    tx: &Transaction,
    title: &str,
    model: &str,
    run: Option<i64>,
    items: &[i64],
    now: i64,
) -> Result<i64> {
    insert_by(tx, "model", title, model, PROMPT, run, items, now)
}

/// [`insert_proposal`] for the author `author` (`model`, `agent` or
/// `person`). An agent or the person gives no model and no prompt (`''`).
#[allow(clippy::too_many_arguments)]
pub(crate) fn insert_by(
    tx: &Transaction,
    author: &str,
    title: &str,
    model: &str,
    prompt: &str,
    run: Option<i64>,
    items: &[i64],
    now: i64,
) -> Result<i64> {
    tx.execute(
        "INSERT INTO matter(title, status, author, model, prompt, run_id, created_at, updated_at)
         VALUES (?1, 'proposed', ?2, ?3, ?4, ?5, ?6, ?6)",
        params![title, author, model, prompt, run, now],
    )?;
    let id = tx.last_insert_rowid();
    let mut add =
        tx.prepare_cached("INSERT INTO matter_item(matter_id, item_id) VALUES (?1, ?2)")?;
    for item in items {
        add.execute(params![id, item])?;
    }
    tx.execute("UPDATE matter SET sealed = 1 WHERE id = ?1", [id])?;
    Ok(id)
}

/// Step 0 of the proposal step: deletes each proposal that is not valid,
/// with its member rows (the cascade). Gives the number of proposals that
/// it deleted.
pub fn clean(tx: &Transaction) -> Result<usize> {
    // An addition that is not live (the item ended, or became hidden) goes
    // too: the person never sees it, and its item is a candidate again.
    tx.execute(
        "DELETE FROM matter_add WHERE item_id NOT IN (SELECT item_id FROM matter_add_live)",
        [],
    )?;
    Ok(tx.execute(
        "DELETE FROM matter WHERE status = 'proposed'
           AND id NOT IN (SELECT matter_id FROM matter_valid)",
        [],
    )?)
}

/// The status of the matter, or `NotFound`.
pub(crate) fn status(tx: &Transaction, matter: i64) -> Result<String> {
    tx.query_row("SELECT status FROM matter WHERE id = ?1", [matter], |r| {
        r.get(0)
    })
    .optional()?
    .ok_or(Error::NotFound)
}

/// The person confirms the proposal `matter`, with `title` and the members
/// to `keep` (item row IDs). Each other member stays as a row with `kept`
/// = 0. One transaction: a second confirm, or a confirm of a proposal that
/// is not valid now, gets `Conflict` and changes nothing.
pub fn confirm(store: &Store, matter: i64, title: &str, keep: &[i64], now: i64) -> Result<()> {
    let keep: Vec<_> = keep.iter().map(|row| (*row, None)).collect();
    confirm_with(store, matter, title, keep, now)
}

/// [`confirm`] with the form that the person names for each member (from the
/// kind of its ID). A member whose row has another form is not a member. The
/// check runs in the decision transaction, after the check of the matter.
pub fn confirm_forms(
    store: &Store,
    matter: i64,
    title: &str,
    keep: &[(i64, &str)],
    now: i64,
) -> Result<()> {
    let keep: Vec<_> = keep.iter().map(|(row, form)| (*row, Some(*form))).collect();
    confirm_with(store, matter, title, keep, now)
}

fn confirm_with(
    store: &Store,
    matter: i64,
    title: &str,
    mut keep: Vec<(i64, Option<&str>)>,
    now: i64,
) -> Result<()> {
    let title = check_title(title).ok_or(Error::Bad("the title must be 1 to 80 characters"))?;
    if keep.is_empty() || keep.len() > MAX_ITEMS {
        return Err(Error::Bad("keep 1 to 50 items"));
    }
    let n = keep.len();
    keep.sort_unstable();
    keep.dedup_by_key(|(row, _)| *row);
    if keep.len() != n {
        return Err(Error::Bad("an item is two times in the list"));
    }
    write(store, |tx| confirm_in(tx, matter, &title, &keep, now))
}

/// The check before each decision: `matter` is a proposal that is valid now.
/// The trigger `matter_decide` holds the same rule in the store.
fn decidable(tx: &Transaction, matter: i64) -> Result<()> {
    if status(tx, matter)? != "proposed" {
        return Err(Error::Conflict("the matter is decided"));
    }
    let valid: bool = tx.query_row(
        "SELECT EXISTS (SELECT 1 FROM matter_valid WHERE matter_id = ?1)",
        [matter],
        |r| r.get(0),
    )?;
    if !valid {
        return Err(Error::Conflict("the proposal is not valid now"));
    }
    Ok(())
}

fn confirm_in(
    tx: &Transaction,
    matter: i64,
    title: &str,
    keep: &[(i64, Option<&str>)],
    now: i64,
) -> Result<()> {
    decidable(tx, matter)?;
    for (item, form) in keep {
        let member: bool = tx.query_row(
            "SELECT EXISTS (SELECT 1 FROM matter_item mi JOIN item ON item.id = mi.item_id
                WHERE mi.matter_id = ?1 AND mi.item_id = ?2 AND (?3 IS NULL OR item.form = ?3))",
            params![matter, item, form],
            |r| r.get(0),
        )?;
        if !member {
            return Err(Error::Bad("an item to keep is not a member"));
        }
        let live: bool = tx.query_row(
            "SELECT EXISTS (SELECT 1 FROM matter_live WHERE matter_id = ?1 AND item_id = ?2)",
            params![matter, item],
            |r| r.get(0),
        )?;
        if !live {
            return Err(Error::Conflict("an item to keep is not live now"));
        }
    }
    // The trigger `matter_confirm` sets `kept` = 0 on each other member.
    let list = format!(
        "[{}]",
        keep.iter()
            .map(|(row, _)| row.to_string())
            .collect::<Vec<_>>()
            .join(",")
    );
    let n = tx.execute(
        "UPDATE matter SET title = ?2, status = 'confirmed', updated_at = ?3, decided_at = ?3,
                keep = ?4
         WHERE id = ?1 AND status = 'proposed'",
        params![matter, title, now, list],
    )?;
    if n != 1 {
        return Err(Error::Conflict("the matter is decided"));
    }
    Ok(())
}

/// The person declines the proposal `matter`. Its members stay as rows and
/// are pinned, so the same grouping does not come back after a rebuild. A
/// decline of a decided matter, or of a proposal that is not valid now, gets
/// `Conflict`.
pub fn decline(store: &Store, matter: i64, now: i64) -> Result<()> {
    write(store, |tx| {
        decidable(tx, matter)?;
        let n = tx.execute(
            "UPDATE matter SET status = 'declined', updated_at = ?2, decided_at = ?2
                 WHERE id = ?1 AND status = 'proposed'",
            params![matter, now],
        )?;
        if n != 1 {
            return Err(Error::Conflict("the matter is decided"));
        }
        Ok(())
    })
}
