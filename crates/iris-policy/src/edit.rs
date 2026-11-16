//! Changes of the policy from the web client (spec 2, section 6.3): a change
//! of `mode`, the removal of a rule, and the discard of a file that the
//! person did not confirm. The engine writes `policy.toml` and keeps all
//! other text and comments.

use crate::active::{active, check, record, Checked};
use crate::{line_of, parse, sha256_hex, Error, Mode, Policy};
use iris_store::{PolicyExpect, Result, Store};
use std::path::PathBuf;
use toml_edit::{DocumentMut, Item, Value};

/// The text of a new policy file: the policy of the first start.
pub const DEFAULT_TEXT: &str = "\
# The policy of Iris (spec 2, section 6). Iris keeps the comments in this file.
version = 0

# observe: Iris applies nothing. apply: rules decide, and the rest goes to review.
mode = \"observe\"
";

/// One change that the person makes in the web client.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Change {
    Mode(Mode),
    /// Remove the rule with this ID. Iris never uses the ID again.
    Remove(u32),
}

/// The result of a change.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Edited {
    /// The file and the active policy have this version now.
    Done { version: i64, policy: Policy },
    /// The active policy is not the version that the person saw.
    Stale,
    /// The file on disk is not the active policy, or there is no file after
    /// a confirmation. The person first confirms or discards it.
    FileChanged,
    /// The active policy has no rule with this ID.
    NoRule,
    /// The active text is not valid any more, so it cannot be the start of
    /// a change. The person corrects the file and confirms it.
    Invalid(Error),
}

/// The result of a discard.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Discarded {
    /// The file on disk is the active policy, or there is no file and no
    /// active version.
    Nothing,
    /// `policy.toml` holds the active text now, or no file at the first
    /// start. `saved` is the copy of the discarded file.
    Done { saved: Option<PathBuf> },
    /// A new file came during the discard. The person looks at it.
    FileChanged,
}

/// Makes `change` to the active policy, if the active version is `seen`
/// (`None` for the policy of the first start). The engine starts from the
/// text of the active policy, writes the new text to `policy.toml` and makes
/// it the active policy in one transaction.
pub fn change(store: &Store, seen: Option<i64>, change: Change, now: i64) -> Result<Edited> {
    // The lock order of the store: the backup lock, then the write transaction.
    let lock = store.backup_lock()?;
    store.write(|tx| {
        let active = active(tx)?;
        if active.version != seen {
            return Ok(Edited::Stale);
        }
        if let Some(e) = active.error {
            return Ok(Edited::Invalid(e));
        }
        let base = match active.version {
            Some(v) => crate::version_text(tx, v)?.unwrap_or_default(),
            None => DEFAULT_TEXT.to_string(),
        };
        // Spec 2 section 6.3: the file on disk must be the active policy.
        // At the first start there is no active text, so there is no file.
        let expect = match active.version {
            Some(_) => PolicyExpect::Text(base.as_bytes()),
            None => PolicyExpect::Missing,
        };
        match (check(tx, store.dir())?, expect) {
            (Checked::Missing, PolicyExpect::Missing) => {}
            (Checked::Ok(l), PolicyExpect::Text(_))
                if Some(&l.sha256) == active.sha256.as_ref() => {}
            _ => return Ok(Edited::FileChanged),
        }
        let text = match edit_text(&base, change) {
            Ok(Some(t)) => t,
            Ok(None) => return Ok(Edited::NoRule),
            Err(e) => return Ok(Edited::Invalid(e)),
        };
        let policy = match parse(&text) {
            Ok(p) => p,
            Err(e) => return Ok(Edited::Invalid(e)),
        };
        // The write checks the file again: a save since the check stays.
        if !store
            .write_policy(&lock, expect, Some(text.as_bytes()))?
            .done
        {
            return Ok(Edited::FileChanged);
        }
        // If the commit fails after the write, the file is a change that the
        // person confirms or discards; it is not active.
        let version = record(tx, &text, &sha256_hex(&text), &policy, now)?;
        Ok(Edited::Done { version, policy })
    })
}

/// Writes the text of the active policy to `policy.toml`, so that a change
/// on disk that the person did not confirm goes away. The file on disk stays
/// as a copy (`policy.toml.saved-<seconds>`), so that no edit is lost. At the
/// first start there is no active text, so no file stays.
pub fn discard(store: &Store) -> Result<Discarded> {
    let lock = store.backup_lock()?;
    // The write transaction keeps a confirmation or a change out.
    store.write(|tx| {
        let active = active(tx)?;
        match check(tx, store.dir())? {
            Checked::Ok(l) if Some(&l.sha256) == active.sha256.as_ref() => {
                return Ok(Discarded::Nothing)
            }
            Checked::Missing if active.version.is_none() => return Ok(Discarded::Nothing),
            _ => {}
        }
        let text = match active.version {
            Some(v) => crate::version_text(tx, v)?,
            None => None,
        };
        let w = store.write_policy(&lock, PolicyExpect::Any, text.as_deref().map(str::as_bytes))?;
        Ok(if w.done {
            Discarded::Done { saved: w.saved }
        } else {
            Discarded::FileChanged
        })
    })
}

/// Makes `change` to the text of a policy file, and keeps all other text
/// and comments. Gives `None` when the text has no rule with the ID to
/// remove.
pub fn edit_text(text: &str, change: Change) -> std::result::Result<Option<String>, Error> {
    let mut doc: DocumentMut = text.parse().map_err(|e: toml_edit::TomlError| Error {
        line: e.span().map_or(0, |s| line_of(text, s.start)),
        message: e.message().trim().to_string(),
    })?;
    match change {
        Change::Mode(mode) => {
            let Some(v) = doc.get_mut("mode").and_then(Item::as_value_mut) else {
                return Err(Error {
                    line: 0,
                    message: "the file has no mode".into(),
                });
            };
            // The decor holds the space after `=` and a comment at the end of the line.
            let decor = v.decor().clone();
            *v = Value::from(mode.name());
            *v.decor_mut() = decor;
        }
        Change::Remove(id) => {
            let Some(rules) = doc.get_mut("rule").and_then(Item::as_array_of_tables_mut) else {
                return Ok(None);
            };
            let Some(i) = rules
                .iter()
                .position(|t| t.get("id").and_then(Item::as_integer) == Some(i64::from(id)))
            else {
                return Ok(None);
            };
            let prefix = rules
                .get(i)
                .and_then(|t| t.decor().prefix())
                .and_then(|p| p.as_str())
                .unwrap_or("")
                .to_string();
            rules.remove(i);
            let keep = shared_comments(&prefix);
            if !keep.is_empty() {
                if let Some(next) = rules.get_mut(i) {
                    let old = next
                        .decor()
                        .prefix()
                        .and_then(|p| p.as_str())
                        .unwrap_or("")
                        .to_string();
                    next.decor_mut().set_prefix(format!("{keep}{old}"));
                } else {
                    let old = doc.trailing().as_str().unwrap_or("").to_string();
                    doc.set_trailing(format!("{keep}{old}"));
                }
            }
            if doc
                .get("rule")
                .and_then(Item::as_array_of_tables)
                .is_some_and(|r| r.is_empty())
            {
                doc.remove("rule");
            }
        }
    }
    Ok(Some(doc.to_string()))
}

/// The part of the text before a `[[rule]]` header that is not about that
/// rule: each line up to the last blank line. A comment directly above the
/// header belongs to the rule and goes with it.
fn shared_comments(prefix: &str) -> &str {
    let mut end = 0;
    let mut at = 0;
    for line in prefix.split_inclusive('\n') {
        if line.ends_with('\n') && line.trim().is_empty() {
            end = at;
        }
        at += line.len();
    }
    &prefix[..end]
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn shared_comments_stop_at_the_last_blank_line() {
        assert_eq!(shared_comments(""), "");
        assert_eq!(shared_comments("\n"), "");
        assert_eq!(shared_comments("\n# about rule 1\n"), "");
        assert_eq!(shared_comments("\n# all rules\n\n"), "\n# all rules\n");
        assert_eq!(
            shared_comments("\n# all rules\n\n# about rule 1\n"),
            "\n# all rules\n"
        );
    }
}
