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
    /// A valid file that is not the active policy. The person can confirm it.
    Changed(Loaded),
    /// The file cannot be read or is not valid. The active policy stays.
    Invalid(Error),
}

/// The result of a confirmation.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Confirm {
    /// The file is the active policy now.
    Done(Policy),
    /// The file on disk is not the text that the person saw.
    Stale,
    /// The file cannot be read or is not valid.
    Invalid(Error),
    /// There is no file to confirm.
    Missing,
}

/// Reads the active policy.
pub fn active(conn: &Connection) -> Result<Active> {
    let row: Option<(String, String)> = conn
        .query_row(
            "SELECT text, sha256 FROM policy_active WHERE id = 1",
            [],
            |r| Ok((r.get(0)?, r.get(1)?)),
        )
        .optional()?;
    Ok(match row {
        None => Active {
            policy: Policy::default(),
            sha256: None,
            error: None,
        },
        Some((text, sha256)) => match parse(&text) {
            Ok(policy) => Active {
                policy,
                sha256: Some(sha256),
                error: None,
            },
            Err(e) => Active {
                policy: Policy::default(),
                sha256: Some(sha256),
                error: Some(e),
            },
        },
    })
}

/// Reads `policy.toml` in the data folder and compares it to the active
/// policy. Changes nothing.
pub fn file_state(store: &Store) -> Result<FileState> {
    let loaded = match load(store.dir()) {
        Ok(Some(l)) => l,
        Ok(None) => return Ok(FileState::Missing),
        Err(e) => return Ok(FileState::Invalid(e)),
    };
    let active = store.read(active)?;
    Ok(
        if active.sha256.as_deref() == Some(loaded.sha256.as_str()) && active.error.is_none() {
            FileState::Same
        } else {
            FileState::Changed(loaded)
        },
    )
}

/// Makes the file on disk the active policy, if its hash is `sha256`: the
/// hash of the text that the person saw. The read of the file, the checks
/// and the write are in one transaction, so a crash leaves the old policy or
/// the new one.
pub fn confirm(store: &Store, sha256: &str, now: i64) -> Result<Confirm> {
    store.write(|tx| {
        let loaded = match load(store.dir()) {
            Ok(Some(l)) => l,
            Ok(None) => return Ok(Confirm::Missing),
            Err(e) => return Ok(Confirm::Invalid(e)),
        };
        if loaded.sha256 != sha256 {
            return Ok(Confirm::Stale);
        }
        // A rule ID that an earlier policy removed is never used again.
        let mut removed =
            tx.prepare("SELECT 1 FROM policy_rule_id WHERE id = ?1 AND removed_at IS NOT NULL")?;
        for rule in &loaded.policy.rules {
            if removed.exists([rule.id])? {
                return Ok(Confirm::Invalid(Error {
                    line: rule.line,
                    message: format!(
                        "the id {} was the id of a removed rule; give the rule a new id",
                        rule.id
                    ),
                }));
            }
        }
        drop(removed);

        tx.execute(
            "INSERT INTO policy_active (id, text, sha256, confirmed_at) VALUES (1, ?1, ?2, ?3)
             ON CONFLICT (id) DO UPDATE SET text = ?1, sha256 = ?2, confirmed_at = ?3",
            rusqlite::params![loaded.text, loaded.sha256, now],
        )?;
        let ids: Vec<u32> = loaded.policy.rules.iter().map(|r| r.id).collect();
        let mut used: Vec<u32> = tx
            .prepare("SELECT id FROM policy_rule_id WHERE removed_at IS NULL")?
            .query_map([], |r| r.get(0))?
            .collect::<rusqlite::Result<_>>()?;
        used.retain(|id| !ids.contains(id));
        for id in used {
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
        Ok(Confirm::Done(loaded.policy))
    })
}
