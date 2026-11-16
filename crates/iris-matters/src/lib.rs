//! Matters (card R6-01, `docs/specs/2026-11-16-matters.md`): a title and the
//! items that belong together. Iris proposes a matter; the person confirms
//! or declines it.
//!
//! The store holds the rules of the spec as triggers (migration 13), and
//! the one validity rule of a proposal as the view `matter_valid`. This
//! crate writes through these rules: it never decides validity in Rust.
//!
//! This part has the writes of the person ([`confirm`], [`decline`]), the
//! write of a proposal ([`insert_proposal`]) and the clean-up of step 0 of
//! the proposal step ([`clean`]). The model call comes in the next part.

use std::fmt;

use iris_store::Store;
use rusqlite::{params, OptionalExtension, Transaction};

/// The prompt version of the proposal step.
pub const PROMPT: &str = "matters/0.1";

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
}

impl fmt::Display for Error {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Error::Store(e) => write!(f, "store: {e}"),
            Error::NotFound => f.write_str("no such matter"),
            Error::Conflict(why) | Error::Bad(why) => f.write_str(why),
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
    tx.execute(
        "INSERT INTO matter(title, status, model, prompt, run_id, created_at, updated_at)
         VALUES (?1, 'proposed', ?2, ?3, ?4, ?5, ?5)",
        params![title, model, PROMPT, run, now],
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
    Ok(tx.execute(
        "DELETE FROM matter WHERE status = 'proposed'
           AND id NOT IN (SELECT matter_id FROM matter_valid)",
        [],
    )?)
}

/// The status of the matter, or `NotFound`.
fn status(tx: &Transaction, matter: i64) -> Result<String> {
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
    let title = check_title(title).ok_or(Error::Bad("the title must be 1 to 80 characters"))?;
    if keep.is_empty() || keep.len() > MAX_ITEMS {
        return Err(Error::Bad("keep 1 to 50 items"));
    }
    let mut sorted = keep.to_vec();
    sorted.sort_unstable();
    sorted.dedup();
    if sorted.len() != keep.len() {
        return Err(Error::Bad("an item is two times in the list"));
    }
    write(store, |tx| confirm_in(tx, matter, &title, &sorted, now))
}

fn confirm_in(tx: &Transaction, matter: i64, title: &str, keep: &[i64], now: i64) -> Result<()> {
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
    for item in keep {
        let member: bool = tx.query_row(
            "SELECT EXISTS (SELECT 1 FROM matter_item WHERE matter_id = ?1 AND item_id = ?2)",
            params![matter, item],
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
            .map(i64::to_string)
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
/// decline of a decided matter gets `Conflict`.
pub fn decline(store: &Store, matter: i64, now: i64) -> Result<()> {
    write(store, |tx| {
        status(tx, matter)?;
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
