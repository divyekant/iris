//! Changes of the policy from the web client (spec 2, section 6.3): a change
//! of `mode`, the removal of a rule, and the discard of a file that the
//! person did not confirm. The engine writes `policy.toml` and keeps all
//! other text and comments.

use crate::active::{active, check, record, Checked};
use crate::{
    line_of, parse, sha256_hex, Author, By, Effect, Error, Mode, Policy, Rule, Verb, When,
};
use iris_store::{BackupLock, PolicyExpect, Result, Store};
use std::path::PathBuf;
use toml_edit::{DocumentMut, Item, Value};

/// The text of a new policy file: the policy of the first start.
pub const DEFAULT_TEXT: &str = "\
# The policy of Iris (spec 2, section 6). Iris keeps the comments in this file.
version = 1

# observe: Iris applies nothing. apply: rules decide, and the rest goes to review.
mode = \"observe\"
";

/// One change that the person makes in the web client.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Change {
    Mode(Mode),
    /// Remove the rule with this ID. Iris never uses the ID again.
    Remove(u32),
    /// Add the rule of the open proposal with this ID (spec 2, section 6.6),
    /// with `by = "proposal"` and the next rule ID.
    Accept(i64),
    /// Add a grant inside one job (`docs/specs/2026-11-17-start-job.md`):
    /// an `allow` rule for `verbs` with `when = { author = "agent" }` and
    /// `job`, with `by = "person"` and the next rule ID. The caller checks
    /// that the job is open.
    Grant {
        job: String,
        verbs: Vec<Verb>,
    },
}

/// The rule of a grant inside `job`, with the ID `id`, added on `added`.
pub fn grant_rule(id: u32, job: &str, verbs: &[Verb], added: &str) -> Rule {
    let names: Vec<&str> = verbs.iter().map(|v| v.name()).collect();
    let list = match names.split_last() {
        Some((last, rest)) if !rest.is_empty() => format!("{} and {last}", rest.join(", ")),
        _ => names.join(""),
    };
    Rule {
        id,
        name: format!("In job {job}, allow {list} without review"),
        effect: Effect::Allow,
        verbs: verbs.to_vec(),
        when: When {
            author: Some(Author::Agent),
            ..When::default()
        },
        by: By::Person,
        added: added.to_string(),
        job: Some(job.to_string()),
        line: 0,
    }
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
    change_locked(store, &lock, seen, change, now)
}

/// Removes each rule of the active policy whose `job` names a job that
/// ended (issue #385): a grant inside a job matches nothing after the job
/// is done, and the move to done has more than one caller. Gives the count
/// of removed rules. Changes nothing while another holder has the backup
/// lock (a backup, or a policy writer), or while the file on disk is not
/// the active policy (the person has a change to confirm). The engine runs
/// it again later.
pub fn end_grants(store: &Store, now: i64) -> Result<usize> {
    let Some(lock) = store.try_backup_lock()? else {
        return Ok(0);
    };
    let mut removed = 0;
    loop {
        let (version, ended) = store.read(|c| {
            let active = active(c)?;
            let mut done = c.prepare("SELECT 1 FROM job WHERE id = ?1 AND state = 'done'")?;
            let mut ended = None;
            for rule in &active.policy.rules {
                let job = rule.job.as_deref().and_then(|j| j.strip_prefix("j-"));
                if let Some(id) = job.and_then(|j| j.parse::<i64>().ok()) {
                    if done.exists([id])? {
                        ended = Some(rule.id);
                        break;
                    }
                }
            }
            Ok((active.version, ended))
        })?;
        let Some(id) = ended else {
            return Ok(removed);
        };
        match change_locked(store, &lock, version, Change::Remove(id), now)? {
            Edited::Done { .. } => removed += 1,
            _ => return Ok(removed),
        }
    }
}

fn change_locked(
    store: &Store,
    lock: &BackupLock,
    seen: Option<i64>,
    change: Change,
    now: i64,
) -> Result<Edited> {
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
        let today = crate::propose::date(now);
        let added = match &change {
            Change::Accept(id) => match crate::propose::current(tx, *id, now)? {
                Some(p) => {
                    let n = p.approvals;
                    let why = match p.verb {
                        Some(verb) => format!(
                            "Iris proposed this rule after you approved {n} of {n} {} actions.",
                            verb.name()
                        ),
                        None => format!(
                            "Iris proposed this rule after you hid {n} items like this, and never undid one."
                        ),
                    };
                    Some((p.rule(crate::next_rule_id(tx)?, &today), why))
                }
                None => return Ok(Edited::NoRule),
            },
            Change::Grant { job, verbs } => Some((
                grant_rule(crate::next_rule_id(tx)?, job, verbs, &today),
                format!("You gave this grant inside job {job}, on the job page."),
            )),
            _ => None,
        };
        let edited = match &added {
            Some((rule, why)) => add_rule(&base, rule, why).map(Some),
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
            .write_policy(lock, expect, Some(text.as_bytes()))?
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
        Change::Accept(_) | Change::Grant { .. } => return Ok(None),
        Change::Remove(id) => {
            // The parser accepts the rules as an array of inline tables too.
            if let Some(rules) = doc.get_mut("rule").and_then(Item::as_array_mut) {
                let Some(i) = rules.iter().position(|v| {
                    v.as_inline_table()
                        .and_then(|t| t.get("id"))
                        .and_then(Value::as_integer)
                        == Some(i64::from(id))
                }) else {
                    return Ok(None);
                };
                // The comments directly above the rule go with it, as for a
                // table. A comment that a blank line separates stays.
                let prefix = rules
                    .get(i)
                    .and_then(|v| v.decor().prefix())
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
                        let old = rules.trailing().as_str().unwrap_or("").to_string();
                        rules.set_trailing(join(keep, &old));
                    }
                }
                // An empty array with a comment in it stays, so the comment stays.
                if rules.is_empty() && !rules.trailing().as_str().unwrap_or("").contains('#') {
                    doc.remove("rule");
                }
                return Ok(Some(doc.to_string()));
            }
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

/// Adds `rule` at the end of the text, as one `[[rule]]` table with the
/// comment `why` above it. Keeps all other text. It writes each condition
/// of `when`, so the file reads back the same rule.
fn add_rule(text: &str, rule: &crate::Rule, why: &str) -> std::result::Result<String, Error> {
    let mut doc = parse_doc(text)?;
    let mut t = toml_edit::Table::new();
    t.insert("id", toml_edit::value(i64::from(rule.id)));
    t.insert("name", toml_edit::value(rule.name.as_str()));
    t.insert("effect", toml_edit::value(rule.effect.name()));
    // A hide, bundle or notify rule has no `verbs` key (spec 2, section 6.2).
    if !rule.verbs.is_empty() {
        let verbs: toml_edit::Array = rule.verbs.iter().map(|v| v.name()).collect();
        t.insert("verbs", toml_edit::value(verbs));
    }
    let w = &rule.when;
    let mut when = toml_edit::InlineTable::new();
    let texts = [
        ("form", w.form.map(|f| f.name().to_string())),
        ("sender", w.sender.clone()),
        ("sender_kind", w.sender_kind.map(|k| k.name().to_string())),
        ("label", w.label.clone()),
        ("record_type", w.record_type.map(|r| r.name().to_string())),
        (
            "commitment_kind",
            w.commitment_kind.map(|c| c.name().to_string()),
        ),
        ("author", w.author.map(|a| a.name().to_string())),
    ];
    for (key, value) in texts {
        if let Some(v) = value {
            when.insert(key, v.as_str().into());
        }
    }
    if let Some(d) = w.older_than_days {
        when.insert("older_than_days", i64::from(d).into());
    }
    t.insert("when", toml_edit::value(when));
    t.insert("by", toml_edit::value(rule.by.name()));
    let added: toml_edit::Datetime = rule.added.parse().map_err(|_| Error {
        line: 0,
        message: format!("the date {} is not valid", rule.added),
    })?;
    t.insert("added", toml_edit::value(added));
    if let Some(job) = &rule.job {
        t.insert("job", toml_edit::value(job.as_str()));
    }
    t.decor_mut().set_prefix(format!("\n# {why}\n"));
    if doc.get("rule").is_none() {
        doc.insert("rule", Item::ArrayOfTables(toml_edit::ArrayOfTables::new()));
    }
    if let Some(rules) = doc.get_mut("rule").and_then(Item::as_array_mut) {
        // An array of inline tables: the rule goes in as one more entry, on
        // its own line, with the comment above it.
        let mut v = Value::InlineTable(t.into_inline_table());
        v.decor_mut().set_prefix(format!("\n  # {why}\n  "));
        rules.push_formatted(v);
        rules.set_trailing_comma(true);
        // The text before `]` (a comment there) stays; the `]` goes on a
        // line of its own.
        let trailing = rules.trailing().as_str().unwrap_or("").to_string();
        if !trailing.contains('\n') {
            rules.set_trailing(format!("\n{trailing}"));
        }
        return Ok(doc.to_string());
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

    /// Issue 144: an add to an inline array keeps each comment of the file,
    /// also one before the closing bracket, and adds exactly one rule. An
    /// accept and a grant both add through `add_rule`.
    #[test]
    fn an_add_to_an_inline_array_keeps_each_comment() {
        let mut seed = 0x9e37_79b9_u64;
        for case in 0..100 {
            let mut bit = || {
                seed ^= seed << 13;
                seed ^= seed >> 7;
                seed ^= seed << 17;
                seed.is_multiple_of(2)
            };
            let mut text = "version = 0\nmode = \"apply\"\nrule = [\n".to_string();
            for id in 1..=3 {
                if bit() {
                    text += &format!("  # all before {id}\n\n");
                }
                if bit() {
                    text += &format!("  # about {id}\n");
                }
                text += &format!("  {{ id = {id}, name = \"r{id}\", effect = \"hide\", when = {{ form = \"feed\" }}, by = \"person\", added = 2026-11-10 }}");
                text += if id < 3 || bit() { ",\n" } else { "\n" };
            }
            if bit() {
                text += "  # footer\n";
            }
            text += "]\n# tail\n";
            let mut rule = crate::parse(&text).unwrap().rules[0].clone();
            rule.id = 4;
            let out = add_rule(&text, &rule, "why 4").unwrap();
            let after = crate::parse(&out).unwrap_or_else(|e| panic!("case {case}: {e}\n{out}"));
            assert_eq!(after.rules.len(), 4, "case {case}\n{out}");
            assert_eq!(after.rules[3].id, 4);
            for line in text.lines().filter(|l| l.trim_start().starts_with('#')) {
                assert!(
                    out.contains(line),
                    "case {case}: {line}\n{text}\n---\n{out}"
                );
            }
            assert!(out.contains("# why 4"), "{out}");
        }
    }

    /// The file reads back each condition that `add_rule` gets.
    #[test]
    fn an_added_rule_reads_back_the_same() {
        use crate::{Author, By, CommitmentKind, Effect, Form, RecordType, Rule, SenderKind, Verb};
        let one = |f: fn(&mut When)| {
            let mut w = When::default();
            f(&mut w);
            w
        };
        let cases = [
            (
                Effect::Allow,
                one(|w| w.sender = Some("a@b.example".into())),
            ),
            (
                Effect::Allow,
                one(|w| w.sender_kind = Some(SenderKind::Service)),
            ),
            (Effect::Allow, one(|w| w.label = Some("Finance".into()))),
            (Effect::Allow, one(|w| w.author = Some(Author::Agent))),
            (Effect::Allow, one(|w| w.older_than_days = Some(30))),
            (
                Effect::Allow,
                one(|w| {
                    w.form = Some(Form::Record);
                    w.record_type = Some(RecordType::Receipt);
                }),
            ),
            (
                Effect::Allow,
                one(|w| {
                    w.form = Some(Form::Commitment);
                    w.commitment_kind = Some(CommitmentKind::Payment);
                }),
            ),
            (
                Effect::Hide,
                one(|w| {
                    w.sender = Some("news@shop.example".into());
                    w.form = Some(Form::Feed);
                }),
            ),
        ];
        for (effect, when) in cases {
            let rule = Rule {
                id: 1,
                name: "r".into(),
                effect,
                verbs: if effect == Effect::Allow {
                    vec![Verb::Archive]
                } else {
                    vec![]
                },
                when: when.clone(),
                by: By::Proposal,
                added: "2027-01-15".into(),
                job: None,
                line: 0,
            };
            let text = add_rule("version = 0\nmode = \"observe\"\n", &rule, "why").unwrap();
            let back = crate::parse(&text).unwrap_or_else(|e| panic!("{e:?}: {text}"));
            assert_eq!(back.rules[0].when, when, "{text}");
            assert_eq!(back.rules[0].verbs, rule.verbs, "{text}");
        }
    }

    #[test]
    fn a_join_keeps_one_blank_line() {
        assert_eq!(join("# a\n\n", "[x]"), "# a\n\n[x]");
        assert_eq!(join("# a\n\n", "\n# b\n"), "# a\n\n# b\n");
        assert_eq!(join("# a\n \n", " \n"), "# a\n \n");
    }
}
