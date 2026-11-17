//! The active policy in `iris.db` (spec 2, section 6.3). The engine
//! evaluates only the active policy. A change of the file on disk becomes
//! active only when the person confirms the text that they saw.

use crate::{load, parse, Error, Loaded, Policy};
use iris_store::{Result, Store};
use rusqlite::{Connection, OptionalExtension};

/// The policy that the engine evaluates.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Active {
    pub policy: Policy,
    /// The confirmed version. `None` for the policy of the first start. A
    /// decision by a rule names this version and the rule ID, so that the
    /// ledger always points to one rule text.
    pub version: Option<i64>,
    /// The hash of the confirmed text. `None` for the policy of the first
    /// start.
    pub sha256: Option<String>,
    /// Set when the confirmed text is no longer valid, for example after a
    /// program update with a stricter check. `policy` is then the policy of
    /// the first start, which applies nothing.
    pub error: Option<Error>,
}

/// The file on disk, compared to the active policy.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum FileState {
    /// The file is the active policy.
    Same,
    /// There is no file. The active policy stays.
    Missing,
    /// A file that the person can confirm. It is not the active policy.
    Changed(Loaded),
    /// The file cannot be read, is not valid, or cannot be confirmed. The
    /// active policy stays.
    Invalid(Error),
}

/// The result of a confirmation.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Confirm {
    /// The file is the active policy now, with this version.
    Done { version: i64, policy: Policy },
    /// The file on disk is not the text that the person saw.
    Stale,
    /// The file cannot be read, is not valid, or cannot be confirmed.
    Invalid(Error),
    /// There is no file to confirm.
    Missing,
}

/// Reads the active policy.
pub fn active(conn: &Connection) -> Result<Active> {
    let row: Option<(i64, String, String)> = conn
        .query_row(
            "SELECT version, text, sha256 FROM policy_version ORDER BY version DESC LIMIT 1",
            [],
            |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?)),
        )
        .optional()?;
    let Some((version, text, sha256)) = row else {
        return Ok(Active {
            policy: Policy::default(),
            version: None,
            sha256: None,
            error: None,
        });
    };
    let (policy, error) = match parse(&text) {
        Ok(p) => (p, None),
        Err(e) => (Policy::default(), Some(e)),
    };
    Ok(Active {
        policy,
        version: Some(version),
        sha256: Some(sha256),
        error,
    })
}

/// The file on disk and the checks that a confirmation makes, other than
/// the hash. `file_state` and `confirm` both use this, so a file that
/// `file_state` calls `Changed` is a file that `confirm` accepts.
pub(crate) enum Checked {
    Missing,
    Invalid(Error),
    Ok(Loaded),
}

pub(crate) fn check(conn: &Connection, data_dir: &std::path::Path) -> Result<Checked> {
    let loaded = match load(data_dir) {
        Ok(Some(l)) => l,
        Ok(None) => return Ok(Checked::Missing),
        Err(e) => return Ok(Checked::Invalid(e)),
    };
    // A rule ID that an earlier policy removed is never used again.
    let mut removed =
        conn.prepare("SELECT 1 FROM policy_rule_id WHERE id = ?1 AND removed_at IS NOT NULL")?;
    for rule in &loaded.policy.rules {
        if removed.exists([rule.id])? {
            return Ok(Checked::Invalid(Error {
                line: rule.line,
                message: format!(
                    "the id {} was the id of a removed rule; give the rule a new id",
                    rule.id
                ),
            }));
        }
    }
    Ok(Checked::Ok(loaded))
}

/// Reads `policy.toml` in the data folder and compares it to the active
/// policy. Changes nothing.
pub fn file_state(store: &Store) -> Result<FileState> {
    store.read(|conn| {
        let loaded = match check(conn, store.dir())? {
            Checked::Missing => return Ok(FileState::Missing),
            Checked::Invalid(e) => return Ok(FileState::Invalid(e)),
            Checked::Ok(l) => l,
        };
        let active = active(conn)?;
        Ok(
            if active.sha256.as_deref() == Some(loaded.sha256.as_str()) && active.error.is_none() {
                FileState::Same
            } else {
                FileState::Changed(loaded)
            },
        )
    })
}

/// Makes the file on disk the active policy, if its hash is `sha256`: the
/// hash of the text that the person saw. The read of the file, the checks
/// and the write are in one transaction, so a crash leaves the old policy or
/// the new one.
pub fn confirm(store: &Store, sha256: &str, now: i64) -> Result<Confirm> {
    store.write(|tx| {
        let loaded = match check(tx, store.dir())? {
            Checked::Missing => return Ok(Confirm::Missing),
            Checked::Invalid(e) => return Ok(Confirm::Invalid(e)),
            Checked::Ok(l) => l,
        };
        if loaded.sha256 != sha256 {
            return Ok(Confirm::Stale);
        }
        let version = record(tx, &loaded.text, &loaded.sha256, &loaded.policy, now)?;
        Ok(Confirm::Done {
            version,
            policy: loaded.policy,
        })
    })
}

/// Stores `text` as the newest version of the active policy, and records
/// its rule IDs. A rule ID of the version before that `policy` does not have
/// is removed, and never used again. Then writes the attention of each item
/// again, in the same transaction.
pub(crate) fn record(
    tx: &Connection,
    text: &str,
    sha256: &str,
    policy: &Policy,
    now: i64,
) -> Result<i64> {
    tx.execute(
        "INSERT INTO policy_version (text, sha256, confirmed_at) VALUES (?1, ?2, ?3)",
        rusqlite::params![text, sha256, now],
    )?;
    let version = tx.last_insert_rowid();
    let ids: Vec<u32> = policy.rules.iter().map(|r| r.id).collect();
    let mut gone: Vec<u32> = tx
        .prepare("SELECT id FROM policy_rule_id WHERE removed_at IS NULL")?
        .query_map([], |r| r.get(0))?
        .collect::<rusqlite::Result<_>>()?;
    gone.retain(|id| !ids.contains(id));
    for id in gone {
        tx.execute(
            "UPDATE policy_rule_id SET removed_at = ?2 WHERE id = ?1",
            rusqlite::params![id, now],
        )?;
    }
    for id in &ids {
        tx.execute(
            "INSERT OR IGNORE INTO policy_rule_id (id) VALUES (?1)",
            [id],
        )?;
    }
    crate::attention::evaluate(tx, policy, now)?;
    crate::propose::accepted(tx, policy, now)?;
    Ok(version)
}

/// One more than the highest rule ID that a rule had: a new rule can use it,
/// because the ID of a removed rule is never used again.
pub fn next_rule_id(conn: &Connection) -> Result<u32> {
    Ok(conn.query_row(
        "SELECT coalesce(max(id), 0) + 1 FROM policy_rule_id",
        [],
        |r| r.get(0),
    )?)
}

/// The text of a confirmed version, so that the ledger and the review can
/// show the rule that decided. `None` if there is no such version.
pub fn version_text(conn: &Connection, version: i64) -> Result<Option<String>> {
    Ok(conn
        .query_row(
            "SELECT text FROM policy_version WHERE version = ?1",
            [version],
            |r| r.get(0),
        )
        .optional()?)
}
