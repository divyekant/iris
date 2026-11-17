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
    /// Add the rule of the open proposal with this ID (spec 2, section 6.6),
    /// with `by = "proposal"` and the next rule ID.
    Accept(i64),
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
    /// The active policy has no rule with this ID, or there is no open
    /// proposal with this ID.
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
/// it the active policy in one transaction. The old file stays as a copy
/// (`policy.toml.saved-<seconds>`), because an editor can still save into it.
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
        let added = match change {
            Change::Accept(id) => match crate::propose::current(tx, id, now)? {
                Some(p) => Some((
                    id,
                    p.rule(crate::next_rule_id(tx)?, &crate::propose::date(now)),
                    p.approvals,
                )),
                None => return Ok(Edited::NoRule),
            },
            _ => None,
        };
        let edited = match &added {
            Some((_, rule, approvals)) => add_rule(&base, rule, *approvals).map(Some),
            None => edit_text(&base, change),
        };
        let text = match edited {
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
        // `record` marks the proposal accepted, from the rule in the text.
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
    let mut doc = parse_doc(text)?;
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
        Change::Accept(_) => return Ok(None),
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
                    next.decor_mut().set_prefix(join(keep, &old));
                } else {
                    let old = doc.trailing().as_str().unwrap_or("").to_string();
                    doc.set_trailing(join(keep, &old));
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

/// Adds `rule` at the end of the text, as one `[[rule]]` table with a
/// comment that says why Iris proposed it. Keeps all other text.
fn add_rule(text: &str, rule: &crate::Rule, approvals: i64) -> std::result::Result<String, Error> {
    let mut doc = parse_doc(text)?;
    let mut t = toml_edit::Table::new();
    t.insert("id", toml_edit::value(i64::from(rule.id)));
    t.insert("name", toml_edit::value(rule.name.as_str()));
    t.insert("effect", toml_edit::value(rule.effect.name()));
    let verbs: toml_edit::Array = rule.verbs.iter().map(|v| v.name()).collect();
    t.insert("verbs", toml_edit::value(verbs));
    let mut when = toml_edit::InlineTable::new();
    if let Some(s) = &rule.when.sender {
        when.insert("sender", s.as_str().into());
    }
    if let Some(k) = rule.when.sender_kind {
        when.insert("sender_kind", k.name().into());
    }
    t.insert("when", toml_edit::value(when));
    t.insert("by", toml_edit::value(rule.by.name()));
    let added: toml_edit::Datetime = rule.added.parse().map_err(|_| Error {
        line: 0,
        message: format!("the date {} is not valid", rule.added),
    })?;
    t.insert("added", toml_edit::value(added));
    let verb = rule.verbs.first().map_or("", |v| v.name());
    t.decor_mut().set_prefix(format!(
        "\n# Iris proposed this rule after you approved {approvals} of {approvals} {verb} actions.\n"
    ));
    if doc.get("rule").is_none() {
        doc.insert("rule", Item::ArrayOfTables(toml_edit::ArrayOfTables::new()));
    }
    match doc.get_mut("rule").and_then(Item::as_array_of_tables_mut) {
        Some(rules) => rules.push(t),
        None => {
            return Err(Error {
                line: crate::find_line(text, "rule"),
                message: "rule is not a list of tables".into(),
            })
        }
    }
    Ok(doc.to_string())
}

fn parse_doc(text: &str) -> std::result::Result<DocumentMut, Error> {
    text.parse().map_err(|e: toml_edit::TomlError| Error {
        line: e.span().map_or(0, |s| line_of(text, s.start)),
        message: e.message().trim().to_string(),
    })
}

/// The part of the text before a `[[rule]]` header that is not about that
/// rule: each line up to the last blank line, with that blank line, which
/// separates it from the next text. A comment directly above the header
/// belongs to the rule and goes with it. Blank lines alone are not shared.
fn shared_comments(prefix: &str) -> &str {
    let mut end = 0;
    let mut at = 0;
    for line in prefix.split_inclusive('\n') {
        at += line.len();
        if line.ends_with('\n') && line.trim().is_empty() {
            end = at;
        }
    }
    let shared = &prefix[..end];
    if shared.trim().is_empty() {
        ""
    } else {
        shared
    }
}

/// `shared` in front of `next`, with one blank line between them: the blank
/// line at the end of `shared` goes when `next` starts with one.
fn join(shared: &str, next: &str) -> String {
    let starts_blank = next
        .split_inclusive('\n')
        .next()
        .is_some_and(|l| l.ends_with('\n') && l.trim().is_empty());
    let shared = if starts_blank {
        let body = shared.trim_end_matches([' ', '\t']);
        let body = &body[..body.len() - 1];
        &shared[..body.rfind('\n').map_or(0, |n| n + 1)]
    } else {
        shared
    };
    format!("{shared}{next}")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn shared_comments_stop_at_the_last_blank_line() {
        assert_eq!(shared_comments(""), "");
        assert_eq!(shared_comments("\n"), "");
        assert_eq!(shared_comments("\n# about rule 1\n"), "");
        assert_eq!(shared_comments("\n\n"), "");
        assert_eq!(shared_comments("\n# all rules\n\n"), "\n# all rules\n\n");
        assert_eq!(
            shared_comments("\n# all rules\n \n# about rule 1\n"),
            "\n# all rules\n \n"
        );
    }

    #[test]
    fn a_join_keeps_one_blank_line() {
        assert_eq!(join("# a\n\n", "[x]"), "# a\n\n[x]");
        assert_eq!(join("# a\n\n", "\n# b\n"), "# a\n\n# b\n");
        assert_eq!(join("# a\n \n", " \n"), "# a\n \n");
    }
}
