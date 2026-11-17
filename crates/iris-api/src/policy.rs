//! The policy view (spec 2, section 6.3): the active rules, the file on
//! disk and its difference to the active policy, and the changes that the
//! person makes in the web client.

use crate::{Error, Result};
use iris_policy::{Change, Confirm, Discarded, Edited, FileState, Mode, Policy, Rule};
use iris_store::Store;
use serde_json::{json, Value};

/// The active policy and the state of the file, as `GET /api/policy` gives
/// them. `crates/iris-api/README.md` gives the format.
pub fn policy(store: &Store) -> Result<Value> {
    let active = store.read(iris_policy::active)?;
    let file = match iris_policy::file_state(store)? {
        FileState::Same => json!({ "state": "same" }),
        FileState::Missing => json!({ "state": "missing" }),
        FileState::Invalid(e) => json!({ "state": "invalid", "error": error(&e) }),
        FileState::Changed(l) => {
            let mut v = difference(&active.policy, &l.policy);
            v["state"] = json!("changed");
            v["sha256"] = json!(l.sha256);
            v["text"] = json!(l.text);
            v
        }
    };
    Ok(json!({
        "mode": active.policy.mode.name(),
        "version": active.version,
        "next_id": store.read(iris_policy::next_rule_id)?,
        "rules": active.policy.rules.iter().map(rule).collect::<Vec<_>>(),
        "proposals": proposals(store)?,
        "error": active.error.as_ref().map(error),
        "file": file,
    }))
}

/// Makes the file on disk the active policy, if its hash is `sha256`.
pub fn confirm(store: &Store, sha256: &str, now: i64) -> Result<Value> {
    match iris_policy::confirm(store, sha256, now)? {
        Confirm::Done { .. } => policy(store),
        Confirm::Stale => Err(conflict(
            "The file changed after you saw it. Look at the new difference.",
        )),
        Confirm::Invalid(e) => Err(conflict(&e.to_string())),
        Confirm::Missing => Err(conflict("There is no policy file to confirm.")),
    }
}

/// Writes the active policy to the file. The engine keeps the old file as
/// a copy; `saved` is its name.
pub fn discard(store: &Store) -> Result<Value> {
    let saved = match iris_policy::discard(store)? {
        Discarded::Nothing => None,
        Discarded::Done { saved } => saved,
        Discarded::FileChanged => {
            return Err(conflict(
                "The policy file changed again. Look at the new change.",
            ))
        }
    };
    let mut v = policy(store)?;
    v["saved"] = json!(saved
        .as_deref()
        .and_then(|p| p.file_name())
        .map(|n| n.to_string_lossy()));
    Ok(v)
}

/// Changes the mode, or removes a rule, if the active version is `seen`.
pub fn change(store: &Store, seen: Option<i64>, change: Change, now: i64) -> Result<Value> {
    match iris_policy::change(store, seen, change, now)? {
        Edited::Done { .. } => policy(store),
        Edited::Stale => Err(conflict(
            "The policy changed after you saw it. Look at it again.",
        )),
        Edited::FileChanged => Err(conflict(
            "The policy file changed on disk. Confirm or discard that change first.",
        )),
        Edited::NoRule => Err(conflict(
            "The policy has no such rule, or the proposal is not open any more.",
        )),
        Edited::Invalid(e) => Err(conflict(&e.to_string())),
    }
}

/// The open proposals (spec 2, section 6.6), each with the rule that an
/// accept adds. `rule.id` and `rule.added` are the values of now: the
/// accept gives the next free ID and the date of the accept.
fn proposals(store: &Store) -> Result<Vec<Value>> {
    let (open, next) = store.read(|c| {
        Ok((
            iris_policy::propose::open(c)?,
            iris_policy::next_rule_id(c)?,
        ))
    })?;
    let today = iris_policy::propose::date(crate::now_s());
    Ok(open
        .iter()
        .map(|p| {
            json!({
                "id": p.id,
                // `approvals` (R5-01) or `choices` (a hide rule, R5-02).
                "kind": if p.verb.is_some() { "approvals" } else { "choices" },
                "approvals": p.approvals,
                "created_at": p.created_at,
                "rule": rule(&p.rule(next, &today)),
            })
        })
        .collect())
}

/// Dismisses an open proposal. Iris does not propose the same rule again.
pub fn dismiss(store: &Store, id: i64, now: i64) -> Result<Value> {
    if !iris_policy::propose::dismiss(store, id, now)? {
        return Err(conflict("The proposal is not open any more."));
    }
    policy(store)
}

/// Reads a mode name of a request.
pub fn mode(name: &str) -> Result<Mode> {
    Mode::from_name(name).ok_or_else(|| {
        Error::Query(format!(
            "mode \"{name}\" is not known; use observe or apply"
        ))
    })
}

fn conflict(why: &str) -> Error {
    Error::Conflict(why.to_string())
}

fn error(e: &iris_policy::Error) -> Value {
    json!({ "line": e.line, "message": e.message })
}

fn rule(r: &Rule) -> Value {
    let w = &r.when;
    let mut when = serde_json::Map::new();
    let mut put = |k: &str, v: Option<Value>| {
        if let Some(v) = v {
            when.insert(k.into(), v);
        }
    };
    put("form", w.form.map(|v| json!(v.name())));
    put("sender", w.sender.as_ref().map(|v| json!(v)));
    put("sender_kind", w.sender_kind.map(|v| json!(v.name())));
    put("label", w.label.as_ref().map(|v| json!(v)));
    put("record_type", w.record_type.map(|v| json!(v.name())));
    put(
        "commitment_kind",
        w.commitment_kind.map(|v| json!(v.name())),
    );
    put("author", w.author.map(|v| json!(v.name())));
    put("older_than_days", w.older_than_days.map(|v| json!(v)));
    json!({
        "id": r.id,
        "name": r.name,
        "effect": r.effect.name(),
        "verbs": r.verbs.iter().map(|v| v.name()).collect::<Vec<_>>(),
        "when": when,
        "by": r.by.name(),
        "added": r.added,
        "job": r.job,
        "line": r.line,
    })
}

fn find(p: &Policy, id: u32) -> Option<&Rule> {
    p.rules.iter().find(|r| r.id == id)
}

/// The difference of the file to the active policy, rule by rule. A rule
/// that keeps its ID and changes any field other than its line is changed.
fn difference(active: &Policy, file: &Policy) -> Value {
    let same = |a: &Rule, b: &Rule| {
        Rule {
            line: 0,
            ..a.clone()
        } == Rule {
            line: 0,
            ..b.clone()
        }
    };
    let added: Vec<Value> = file
        .rules
        .iter()
        .filter(|r| find(active, r.id).is_none())
        .map(rule)
        .collect();
    let removed: Vec<Value> = active
        .rules
        .iter()
        .filter(|r| find(file, r.id).is_none())
        .map(rule)
        .collect();
    let changed: Vec<Value> = file
        .rules
        .iter()
        .filter_map(|r| {
            let old = find(active, r.id)?;
            (!same(old, r)).then(|| json!({ "before": rule(old), "after": rule(r) }))
        })
        .collect();
    json!({
        "mode": file.mode.name(),
        "added": added,
        "removed": removed,
        "changed": changed,
    })
}

/// A grant inside one open job (`docs/specs/2026-11-17-start-job.md`,
/// section 2): one `allow` rule for the agent of `job`, for `verbs`. Gives
/// `None` when no job has the ID. `seen` is the active version that the
/// person saw.
pub fn grant(
    store: &Store,
    job: &str,
    verbs: &[String],
    seen: Option<i64>,
    now: i64,
) -> Result<Option<Value>> {
    let Some(row) = iris_job::parse_job_id(job) else {
        return Ok(None);
    };
    let mut list: Vec<iris_policy::Verb> = Vec::new();
    for v in verbs {
        match iris_policy::Verb::from_name(v) {
            Some(v) if !list.contains(&v) => list.push(v),
            Some(_) => return Err(Error::Query(format!("the verb {v:?} is in the list twice"))),
            None => {
                return Err(Error::Query(format!(
                    "not a verb that a rule can allow: {v:?}"
                )))
            }
        }
    }
    if list.is_empty() {
        return Err(Error::Query("a grant needs one or more verbs".into()));
    }
    let state = match iris_job::get(store, row) {
        Ok(j) => j.state,
        Err(iris_job::Error::NotFound) => return Ok(None),
        Err(iris_job::Error::Store(e)) => return Err(Error::Store(e)),
        Err(e) => return Err(Error::Conflict(e.to_string())),
    };
    // A job that ends after this check keeps a rule that matches nothing.
    if state == iris_job::State::Done {
        return Err(conflict("The job ended."));
    }
    let change = Change::Grant {
        job: iris_job::format_job_id(row),
        verbs: list,
    };
    self::change(store, seen, change, now).map(Some)
}
